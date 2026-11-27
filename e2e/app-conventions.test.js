import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { setup } from "./harness.js";

const { browser, check, done, base } = await setup();
const app = join(process.env.APP_DIR, "app");
const page = await browser.newPage();
const errors = [];
page.on("pageerror", (error) => errors.push(error.message));

const attr = (selector, name) => page.locator(selector).first().getAttribute(name);
const text = (selector) => page.locator(selector).first().textContent();
const metaContent = (key, value) => attr(`meta[${key}="${value}"]`, "content");
const html = async (path) => {
  const response = await fetch(base + path);
  return { status: response.status, body: await response.text(), headers: response.headers };
};

await check("metadata files are linked from the head", async () => {
  await page.goto(base, { waitUntil: "networkidle" });
  const icons = await page.locator("link[rel=icon]").evaluateAll((links) =>
    links.map((l) => [l.getAttribute("href"), l.getAttribute("type"), l.getAttribute("sizes")]),
  );
  assert.deepEqual(icons[0], ["/favicon.ico", "image/x-icon", "16x16"]);
  assert.match(icons[1][0], /^\/icon\.png\?[0-9a-z]+$/);
  assert.deepEqual(icons[1].slice(1), ["image/png", "32x32"]);
  assert.match(await attr("link[rel=apple-touch-icon]", "href"), /^\/apple-icon\.png\?/);
  assert.equal(await attr("link[rel=apple-touch-icon]", "sizes"), "60x60");
  assert.equal(await attr("link[rel=manifest]", "href"), "/manifest.webmanifest");
  assert.match(await metaContent("property", "og:image"), /^https:\/\/conventions\.test\/opengraph-image\.png\?/);
  assert.equal(await metaContent("property", "og:image:width"), "120");
  assert.equal(await metaContent("property", "og:image:height"), "63");
  assert.equal(await metaContent("property", "og:image:type"), "image/png");
  assert.equal(await metaContent("property", "og:image:alt"), "Conventions cover");
  assert.match(await metaContent("name", "twitter:image"), /^https:\/\/conventions\.test\/twitter-image\.png\?/);
  assert.equal(await metaContent("name", "twitter:image:width"), "100");
});

await check("metadata files are served with their bytes and cache headers", async () => {
  const href = await attr("link[rel=icon][type='image/png']", "href");
  const icon = await fetch(base + href);
  assert.equal(icon.status, 200);
  assert.equal(icon.headers.get("content-type"), "image/png");
  assert.match(icon.headers.get("cache-control"), /immutable/);
  assert.deepEqual(Buffer.from(await icon.arrayBuffer()), readFileSync(join(app, "icon.png")));
  const favicon = await fetch(`${base}/favicon.ico`);
  assert.equal(favicon.headers.get("content-type"), "image/x-icon");
  assert.match(favicon.headers.get("cache-control"), /must-revalidate/);
  assert.deepEqual(Buffer.from(await favicon.arrayBuffer()), readFileSync(join(app, "favicon.ico")));
  const etag = favicon.headers.get("etag");
  const again = await fetch(`${base}/favicon.ico`, { headers: { "if-none-match": etag } });
  assert.equal(again.status, 304);
  const og = await fetch(`${base}/blog/opengraph-image.png`);
  assert.deepEqual(Buffer.from(await og.arrayBuffer()), readFileSync(join(app, "blog", "opengraph-image.png")));
});

await check("nested metadata files override the parent's", async () => {
  const { body } = await html("/blog/hello");
  assert.match(body, /property="og:image" content="https:\/\/conventions\.test\/blog\/opengraph-image\.png\?[0-9a-z]+"/);
  assert.match(body, /property="og:image:width" content="64"/);
  assert.match(body, /property="og:image:alt" content="The blog"/);
  assert.match(body, /property="og:title" content="Blog"/);
  assert.match(body, /rel="icon" href="\/favicon\.ico"/);
});

await check("useServerInsertedHTML adds to the head", async () => {
  const { body } = await html("/blog/hello");
  const head = body.slice(0, body.indexOf("</head>"));
  assert.match(head, /<meta name="inserted" content="server \/blog\/hello"\/?>/);
});

await check("sitemap.ts, robots.ts and manifest.ts are served", async () => {
  const sitemap = await html("/sitemap.xml");
  assert.equal(sitemap.headers.get("content-type"), "application/xml");
  assert.match(sitemap.body, /<loc>https:\/\/conventions\.test\/<\/loc>\s*<lastmod>2024-01-02T03:04:05\.000Z<\/lastmod>\s*<changefreq>weekly<\/changefreq>\s*<priority>1\.0<\/priority>/);
  assert.match(sitemap.body, /<loc>https:\/\/conventions\.test\/blog\/hello<\/loc>/);
  assert.doesNotMatch(sitemap.body, /\/cart</);
  const robots = await html("/robots.txt");
  assert.equal(
    robots.body,
    "User-Agent: *\nAllow: /\nDisallow: /private\nDisallow: /admin\n\nUser-Agent: BadBot\nDisallow: /\n\nSitemap: https://conventions.test/sitemap.xml\n",
  );
  const manifest = await fetch(`${base}/manifest.webmanifest`);
  assert.equal(manifest.headers.get("content-type"), "application/manifest+json");
  assert.deepEqual(await manifest.json(), {
    name: "Conventions",
    short_name: "Conv",
    start_url: "/",
    display: "standalone",
    theme_color: "#000000",
  });
});

await check("useSelectedLayoutSegment follows client navigation", async () => {
  await page.goto(base, { waitUntil: "networkidle" });
  assert.equal(await text("#root-segment"), "null");
  await page.click("a[href='/blog']");
  await page.locator("h1", { hasText: "Blog index" }).waitFor();
  assert.equal(await text("#root-segment"), "blog");
  assert.equal(await text("#blog-segment"), "null");
  await page.click("a[href='/blog/hello']");
  await page.locator("#post", { hasText: "Post hello" }).waitFor();
  assert.equal(await text("#root-segments"), "blog|hello");
  assert.equal(await text("#blog-segment"), "hello");
  await page.click("a[href='/cart']");
  await page.locator("h1", { hasText: "Cart" }).waitFor();
  assert.equal(await text("#root-segment"), "(shop)");
  assert.equal(await text("#root-segments"), "(shop)|cart");
  assert.equal(await page.evaluate(() => window.rootMounts), 1);
});

await check("templates remount on navigation while layouts persist", async () => {
  await page.goto(`${base}/blog/hello`, { waitUntil: "networkidle" });
  assert.equal(await page.evaluate(() => window.blogTemplateMounts), 1);
  await page.click("a[href='/blog/world']");
  await page.locator("#post", { hasText: "Post world" }).waitFor();
  assert.equal(await page.evaluate(() => window.blogTemplateMounts), 2);
  assert.equal(await page.evaluate(() => window.blogLayoutMounts), 1);
});

await check("a nested not-found renders inside its layout on the server", async () => {
  const { status, body } = await html("/blog/missing");
  assert.equal(status, 404);
  assert.match(body, /id="blog-layout"/);
  assert.match(body, /Blog post not found/);
  assert.doesNotMatch(body, /Root not found/);
  assert.match(body, /<meta name="robots" content="noindex"/);
  await page.goto(`${base}/blog/missing`, { waitUntil: "networkidle" });
  await page.locator("#blog-not-found").waitFor();
  assert.equal(await text("#blog-segment"), "missing");
  const unknown = await html("/nowhere");
  assert.equal(unknown.status, 404);
  assert.match(unknown.body, /Root not found/);
});

await check("a nested not-found catches notFound() after client navigation", async () => {
  await page.goto(`${base}/blog`, { waitUntil: "networkidle" });
  await page.click("a[href='/blog/missing']");
  await page.locator("#blog-not-found").waitFor();
  assert.equal(await page.locator("#blog-layout").count(), 1);
  await page.click("a[href='/blog/hello']");
  await page.locator("#post", { hasText: "Post hello" }).waitFor();
});

await check("global-error replaces the root layout when it throws on the client", async () => {
  await page.goto(base, { waitUntil: "networkidle" });
  await page.click("#crash");
  await page.locator("#global-error").waitFor();
  assert.equal(await text("#global-message"), "the root layout crashed");
  assert.equal(await page.locator("nav").count(), 0);
});

await check("global-error renders when the root layout fails on the server", async () => {
  const { status, body } = await html("/layout-crash");
  assert.equal(status, 500);
  assert.match(body, /id="global-error"/);
  assert.doesNotMatch(body, /<nav>/);
  await page.goto(`${base}/layout-crash`, { waitUntil: "networkidle" });
  await page.locator("#global-error").waitFor();
});

await check("dynamic = force-static ignores the query and caches publicly", async () => {
  const { body, headers } = await html("/dynamic?q=hello");
  assert.match(body, /id="search">q=(<!-- -->)?<\/h1>/);
  assert.match(headers.get("cache-control"), /public/);
});

await check("force-static runs a Rust loader without the visitor's cookies or query", async () => {
  const alice = await fetch(`${base}/who?q=1`, { headers: { cookie: "user=alice" } });
  const aliceBody = await alice.text();
  assert.match(alice.headers.get("cache-control"), /public/);
  assert.match(aliceBody, /id="who">anonymous</);
  assert.match(aliceBody, /id="who-query">none</);
  const bob = await fetch(`${base}/who`, { headers: { cookie: "user=bob" } });
  assert.match(await bob.text(), /id="who">anonymous</);
  const data = await fetch(`${base}/_volt/data?path=${encodeURIComponent("/who?q=2")}`, {
    headers: { cookie: "user=bob" },
  });
  assert.deepEqual((await data.json()).data, { user: null, query: null });
});

await check("a page remounts when a dynamic param changes", async () => {
  await page.goto(`${base}/items/1`, { waitUntil: "networkidle" });
  await page.click("#inc");
  await page.click("#inc");
  assert.equal(await text("#count"), "2");
  await page.click("a[href='/items/2']");
  await page.locator("#item", { hasText: "Item 2" }).waitFor();
  assert.equal(await text("#count"), "0");
  assert.equal(await page.evaluate(() => window.rootMounts), 1);
});

await check("metadata files below a catch-all segment are served", async () => {
  const { body } = await html("/docs/a/b");
  const href = /rel="icon" href="(\/docs\/a\/b\/icon\.png\?[0-9a-z]+)"/.exec(body)?.[1];
  assert.ok(href, "icon link");
  const icon = await fetch(base + href);
  assert.equal(icon.headers.get("content-type"), "image/png");
  assert.deepEqual(Buffer.from(await icon.arrayBuffer()), readFileSync(join(app, "docs", "[...slug]", "icon.png")));
});

await check("code metadata routes are rendered once and cached", async () => {
  const first = await html("/sitemap.xml");
  const second = await html("/sitemap.xml");
  assert.match(first.body, /\/render\/[0-9a-z]+</);
  assert.equal(second.body, first.body);
  assert.equal(first.headers.get("cache-control"), "public, max-age=0, s-maxage=3600, stale-while-revalidate");
});

await check("no uncaught page errors besides the intentional crash", async () => {
  assert.deepEqual(
    errors.filter((message) => !message.includes("root layout crashed")),
    [],
  );
});

await done();
