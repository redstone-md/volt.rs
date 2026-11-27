import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { request } from "node:http";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { setup } from "./harness.js";
import { serveStatic } from "./static.js";

const { browser, check, done, base } = await setup();
const app = process.env.APP_DIR;
const volt = resolve(process.env.VOLT_BIN ?? join(dirname(fileURLToPath(import.meta.url)), "..", "target", "debug", "volt"));
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const get = async (path) => {
  const started = performance.now();
  const response = await fetch(base + path, { redirect: "manual" });
  return { response, body: await response.text(), ms: performance.now() - started };
};
const stamp = (body) => Number(body.match(/id="time">(\d+)/)?.[1]);
// fetch() normalises `\` and `//` in paths, so unsafe paths go out as raw request lines.
const rawGet = (path) =>
  new Promise((resolve, reject) => {
    const { hostname, port } = new URL(base);
    request({ hostname, port, path, method: "GET" }, (response) => {
      response.resume();
      resolve({ status: response.statusCode, location: response.headers.location });
    })
      .on("error", reject)
      .end();
  });
const exportWith = (toml) => {
  const config = join(app, "volt.toml");
  const original = readFileSync(config, "utf8");
  writeFileSync(config, original + toml);
  try {
    return spawnSync(volt, ["export"], { cwd: app, encoding: "utf8" });
  } finally {
    writeFileSync(config, original);
  }
};

await check("generateStaticParams paths are prerendered at build time", async () => {
  for (const file of ["blog/hello", "blog/world", "docs/guide/intro", "lang/en/about", "lang/fr/about", "index"]) {
    assert.ok(existsSync(join(app, ".volt", "prerender", `${file}.html`)), `${file}.html`);
    assert.ok(existsSync(join(app, ".volt", "prerender", `${file}.json`)), `${file}.json`);
  }
  const manifest = JSON.parse(readFileSync(join(app, ".volt", "manifest.json"), "utf8"));
  const blog = manifest.routes.find((r) => r.id === "/blog/[slug]");
  assert.deepEqual(blog.prerendered, ["/blog/hello", "/blog/world"]);
  assert.equal(blog.dynamicParams, false);
});

await check("prerendered pages are served from the cache", async () => {
  const first = await get("/blog/hello");
  assert.equal(first.response.status, 200);
  assert.match(first.body, /<h1>Post <!-- -->hello<\/h1>|<h1>Post hello<\/h1>/);
  assert.match(first.response.headers.get("cache-control"), /s-maxage=31536000/);
  const html = readFileSync(join(app, ".volt", "prerender", "blog", "hello.html"), "utf8");
  const again = await get("/blog/hello");
  assert.equal(again.body, html, "served the build-time HTML");
  assert.ok(again.ms < 200, `took ${again.ms}ms`);
});

await check("dynamicParams = false answers 404 for unknown params", async () => {
  const { response, body } = await get("/blog/unknown");
  assert.equal(response.status, 404);
  assert.match(body, /404|not found/i);
});

await check("other params of a dynamic route render on demand", async () => {
  const { response, body } = await get("/docs/other/page");
  assert.equal(response.status, 200);
  assert.match(body, /Doc <!-- -->other\/page|Doc other\/page/);
});

await check("layout generateStaticParams feeds nested segments", async () => {
  const { body } = await get("/lang/fr/about");
  assert.match(body, /About <!-- -->fr|About fr/);
});

await check("revalidate serves stale pages and regenerates in the background", async () => {
  await get("/time");
  await sleep(600);
  await get("/time");
  await sleep(2300);
  const before = Date.now();
  const stale = await get("/time");
  assert.equal(stale.response.status, 200);
  assert.ok(stamp(stale.body) < before, `stale copy served while regenerating (${stamp(stale.body)} vs ${before})`);
  assert.match(stale.response.headers.get("cache-control"), /s-maxage=2/);
  await sleep(400);
  const fresh = await get("/time");
  assert.ok(stamp(fresh.body) >= before, "regenerated after the stale hit");
  const cached = await get("/time");
  assert.equal(stamp(cached.body), stamp(fresh.body));
});

await check("getStaticPaths paths are prerendered and fallback: false answers 404", async () => {
  const manifest = JSON.parse(readFileSync(join(app, ".volt", "manifest.json"), "utf8"));
  const posts = manifest.routes.find((r) => r.id === "/posts/[id]");
  assert.deepEqual(posts.prerendered, ["/posts/1", "/posts/2"]);
  for (const id of ["1", "2"]) {
    const { response, body } = await get(`/posts/${id}`);
    assert.equal(response.status, 200);
    assert.match(body, new RegExp(`Pages post (<!-- -->)?${id}`));
  }
  assert.equal((await get("/posts/3")).response.status, 404);
});

await check("revalidate = 0 renders on every request", async () => {
  assert.ok(!existsSync(join(app, ".volt", "prerender", "now.html")), "not prerendered");
  for (let i = 0; i < 3; i++) {
    const before = Date.now();
    const { response, body } = await get("/now");
    assert.ok(stamp(body) >= before, `request ${i} rendered fresh (${stamp(body)} vs ${before})`);
    assert.match(response.headers.get("cache-control"), /no-store/);
    await sleep(20);
  }
});

await check("trailing slashes redirect to the canonical URL", async () => {
  const { response } = await get("/blog/hello/?x=1");
  assert.equal(response.status, 308);
  assert.equal(response.headers.get("location"), "/blog/hello?x=1");
});

await check("slash normalisation never redirects to another host", async () => {
  for (const path of ["//evil.com/", "/\\evil.com/", "///evil.com/", "/\\/evil.com"]) {
    const { status, location } = await rawGet(path);
    assert.equal(status, 308, path);
    assert.equal(location, "/evil.com", path);
  }
});

await check("sitemap lists generated paths", async () => {
  const { body } = await get("/sitemap.xml");
  assert.match(body, /https:\/\/static\.example\/blog\/world/);
  assert.match(body, /https:\/\/static\.example\/lang\/en\/about/);
});

await check("client navigation between prerendered pages", async () => {
  const page = await browser.newPage();
  await page.goto(base, { waitUntil: "networkidle" });
  await page.evaluate(() => (window.__marker = 1));
  await page.click("a[href='/blog/hello']");
  await page.locator("h1", { hasText: "Post hello" }).waitFor();
  assert.equal(await page.evaluate(() => window.__marker), 1);
  assert.equal(await page.title(), "Post hello · static-generation");
  await page.close();
});

await check("volt export refuses an export_dir that holds project files", async () => {
  for (const dir of ["app", "../..", "/tmp"]) {
    const result = exportWith(`\n[build]\nexport_dir = ${JSON.stringify(dir)}\n`);
    assert.notEqual(result.status, 0, dir);
    assert.match(result.stderr + result.stdout, /export_dir/, dir);
    assert.ok(existsSync(join(app, "app", "page.tsx")), `sources survive ${dir}`);
    assert.ok(existsSync(join(app, "volt.toml")), `project survives ${dir}`);
  }
});

await check("volt export lists pages that need the server", async () => {
  const result = spawnSync(volt, ["export"], { cwd: app, encoding: "utf8" });
  assert.notEqual(result.status, 0);
  assert.match(result.stderr + result.stdout, /\/now: renders on every request/);
  rmSync(join(app, "app", "now"), { recursive: true });
});

await check("volt export writes a static site", async () => {
  const result = spawnSync(volt, ["export"], { cwd: app, encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr + result.stdout);
  const out = join(app, "out");
  for (const file of [
    "index.html",
    "blog/hello.html",
    "lang/fr/about.html",
    "404.html",
    "sitemap.xml",
    "robots.txt",
    "hello.txt",
    "_volt/data/blog/hello.json",
    "_volt/data/index.json",
  ]) {
    assert.ok(existsSync(join(out, file)), file);
  }
  assert.ok(!existsSync(join(out, "blog/unknown.html")));
  assert.match(readFileSync(join(out, "blog/hello.html"), "utf8"), /"export":true/);
});

await check("exported site navigates on the client without a server", async () => {
  const port = Number(process.env.PORT ?? 3900) + 1;
  const site = await serveStatic(join(app, "out"), port);
  const page = await browser.newPage();
  try {
    await page.goto(site.url, { waitUntil: "networkidle" });
    await page.evaluate(() => (window.__marker = 1));
    await page.click("a[href='/blog/world']");
    await page.locator("h1", { hasText: "Post world" }).waitFor();
    assert.equal(await page.evaluate(() => window.__marker), 1, "no full reload");
    assert.ok(site.requests.includes("/_volt/data/blog/world.json"));
    await page.click("a[href='/']");
    await page.click("a[href='/lang/fr/about']");
    await page.locator("h1", { hasText: "About fr" }).waitFor();
    const missing = await fetch(`${site.url}/blog/unknown`);
    assert.equal(missing.status, 404);
    assert.match(await missing.text(), /404|not found/i);
  } finally {
    await page.close();
    site.close();
  }
});

await done();
