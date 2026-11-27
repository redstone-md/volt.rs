import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { setup } from "./harness.js";
import { serveStatic } from "./static.js";

const { browser, check, done, base } = await setup();
const app = process.env.APP_DIR;
const volt = resolve(process.env.VOLT_BIN ?? join(dirname(fileURLToPath(import.meta.url)), "..", "target", "debug", "volt"));
const get = async (path) => {
  const response = await fetch(base + path, { redirect: "manual" });
  return { response, body: await response.text() };
};

await check("pages are served under base_path only", async () => {
  assert.equal((await get("/")).response.status, 404);
  assert.equal((await get("/about/")).response.status, 404);
  const home = await get("/docs/");
  assert.equal(home.response.status, 200);
  assert.match(home.body, /href="\/docs\/about\/"/);
  assert.match(home.body, /src="\/docs\/_volt\/static\/js\/main-[^"]+\.js"/);
  const entry = home.body.match(/src="(\/docs\/_volt\/static\/js\/main-[^"]+\.js)"/)[1];
  assert.equal((await get(entry)).response.status, 200);
});

await check("trailing_slash redirects with 308", async () => {
  const bare = await get("/docs");
  assert.equal(bare.response.status, 308);
  assert.equal(bare.response.headers.get("location"), "/docs/");
  const about = await get("/docs/about?x=1");
  assert.equal(about.response.status, 308);
  assert.equal(about.response.headers.get("location"), "/docs/about/?x=1");
  assert.equal((await get("/docs/hello.txt")).response.status, 404);
});

await check("configured redirects resolve against the base path", async () => {
  const { response } = await get("/docs/old/");
  assert.equal(response.status, 308);
  assert.equal(response.headers.get("location"), "/docs/about/");
});

await check("generateStaticParams pages are prerendered and others answer 404", async () => {
  assert.ok(existsSync(join(app, ".volt", "prerender", "posts", "3.html")));
  assert.equal((await get("/docs/posts/3/")).response.status, 200);
  assert.equal((await get("/docs/posts/4/")).response.status, 404);
});

await check("client navigation keeps the base path", async () => {
  const page = await browser.newPage();
  await page.goto(`${base}/docs/`, { waitUntil: "networkidle" });
  await page.evaluate(() => (window.__marker = 1));
  await page.click("a[href='/docs/about/']");
  await page.locator("h1", { hasText: "About" }).waitFor();
  assert.equal(new URL(page.url()).pathname, "/docs/about/");
  assert.equal(await page.textContent("#pathname"), "/about");
  assert.equal(await page.evaluate(() => window.__marker), 1);
  await page.goBack();
  await page.locator("h1", { hasText: "Home" }).waitFor();
  await page.close();
});

await check("volt export honours base_path and trailing_slash", async () => {
  const result = spawnSync(volt, ["export"], { cwd: app, encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr + result.stdout);
  const out = join(app, "out");
  for (const file of ["index.html", "about/index.html", "posts/1/index.html", "_volt/data/about.json", "404.html"]) {
    assert.ok(existsSync(join(out, file)), file);
  }
  assert.match(readFileSync(join(out, "sitemap.xml"), "utf8"), /https:\/\/base\.example\/docs\/posts\/2\//);

  const port = Number(process.env.PORT ?? 3900) + 1;
  const site = await serveStatic(out, port, "/docs");
  const page = await browser.newPage();
  try {
    await page.goto(`${site.url}/`, { waitUntil: "networkidle" });
    await page.evaluate(() => (window.__marker = 1));
    await page.click("a[href='/docs/posts/2/']");
    await page.locator("h1", { hasText: "Post 2" }).waitFor();
    assert.equal(await page.evaluate(() => window.__marker), 1, "no full reload");
    assert.ok(site.requests.includes("/docs/_volt/data/posts/2.json"));
  } finally {
    await page.close();
    site.close();
  }
});

await done();
