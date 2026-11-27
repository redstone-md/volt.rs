import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createServer } from "node:http";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { setup } from "./harness.js";

const { browser, check, done, base } = await setup();
const here = dirname(fileURLToPath(import.meta.url));
const png = readFileSync(join(here, "fixtures", "images", "remote.png"));

// Stand-in for an image CDN; `[images] remote_patterns` allows http://127.0.0.1/allowed/** only.
const hits = [];
const cdn = createServer((request, response) => {
  hits.push(request.url);
  if (request.url === "/allowed/redirect") {
    response.writeHead(302, { location: "/blocked/pic.png" }).end();
  } else if (request.url === "/allowed/page.html") {
    response.writeHead(200, { "content-type": "text/html" }).end("<p>not an image</p>");
  } else {
    response.writeHead(200, { "content-type": "image/png" }).end(png);
  }
});
await new Promise((resolve) => cdn.listen(0, "127.0.0.1", resolve));
const origin = `http://127.0.0.1:${cdn.address().port}`;

const image = (url, w, accept = "", extra = {}) =>
  fetch(`${base}/_volt/image?url=${encodeURIComponent(url)}&w=${w}&q=75`, { headers: { accept, ...extra } });
const html = await (await fetch(base)).text();
const tag = (id) => html.match(new RegExp(`<img[^>]*id="${id}"[^>]*>`))?.[0] ?? "";
const page = await browser.newPage();
await page.goto(base, { waitUntil: "networkidle" });
const attr = (id, name) => page.locator(`#${id}`).getAttribute(name);

await check("local images get a srcset from the configured sizes", async () => {
  assert.equal(
    await attr("fixed", "srcset"),
    "/_volt/image?url=%2Fphoto.jpg&w=640&q=75 1x, /_volt/image?url=%2Fphoto.jpg&w=828&q=75 2x",
  );
  assert.equal(await attr("fixed", "src"), "/_volt/image?url=%2Fphoto.jpg&w=828&q=75");
  assert.equal(await attr("fixed", "loading"), "lazy");
  const responsive = await attr("responsive", "srcset");
  assert.match(responsive, /^\/_volt\/image\?url=%2Fphoto\.jpg&w=640&q=75 640w, .* 3840w$/);
  assert.ok(!responsive.includes("w=256"), "sizes=50vw drops widths below half the smallest device size");
  assert.equal(await attr("responsive", "sizes"), "50vw");
  assert.match(await attr("quality", "srcset"), /w=64&q=40 1x, .*w=128&q=40 2x/);
});

await check("props: fill, overrideSrc, unoptimized, loader, svg", async () => {
  assert.equal(await attr("fill", "sizes"), "100vw");
  assert.equal(await attr("fill", "data-nimg"), "fill");
  const box = await page.locator("#fill").boundingBox();
  assert.deepEqual([box.width, box.height], [300, 200]);
  assert.equal(await attr("override", "src"), "/photo.jpg");
  assert.match(await attr("override", "srcset"), /_volt\/image/);
  assert.equal(await attr("unoptimized", "src"), "/photo.jpg");
  assert.equal(await attr("unoptimized", "srcset"), null);
  assert.equal(await attr("custom", "src"), "https://cdn.test/photo.jpg?w=128");
  assert.equal(await attr("svg", "src"), "/logo.svg");
  assert.equal(await attr("svg", "srcset"), null);
});

await check("AVIF and WebP are negotiated by Accept", async () => {
  const avif = await image("/graphic.png", 256, "image/avif,image/webp,*/*");
  assert.equal(avif.status, 200);
  assert.equal(avif.headers.get("content-type"), "image/avif");
  assert.match(avif.headers.get("vary") ?? "", /Accept/);
  const webp = await image("/graphic.png", 256, "image/webp,*/*");
  assert.equal(webp.headers.get("content-type"), "image/webp");
  const plain = await image("/graphic.png", 256, "*/*");
  assert.equal(plain.headers.get("content-type"), "image/png");
  const photo = await image("/photo.jpg", 256, "image/avif");
  assert.equal(photo.headers.get("content-type"), "image/avif");
  const bytes = Buffer.from(await photo.arrayBuffer());
  assert.equal(bytes.subarray(4, 12).toString(), "ftypavif");
  assert.ok(bytes.length < readFileSync(join(here, "fixtures", "images", "public", "photo.jpg")).length);
});

await check("variants are cached per format and revalidate with ETag", async () => {
  const again = await image("/graphic.png", 256, "image/avif,image/webp");
  assert.equal(again.headers.get("x-volt-cache"), "HIT");
  assert.equal(again.headers.get("content-type"), "image/avif");
  const etag = again.headers.get("etag");
  const webp = await image("/graphic.png", 256, "image/webp");
  assert.notEqual(webp.headers.get("etag"), etag);
  const fresh = await image("/graphic.png", 256, "image/avif", { "if-none-match": etag });
  assert.equal(fresh.status, 304);
  assert.equal(again.headers.get("content-security-policy"), "script-src 'none'; frame-src 'none'; sandbox;");
  assert.match(again.headers.get("content-disposition"), /^inline; filename="graphic\.avif"$/);
});

await check("rejects unknown widths, SVG and decompression bombs", async () => {
  assert.equal((await image("/graphic.png", 100)).status, 400);
  const svg = await image("/logo.svg", 64);
  assert.equal(svg.status, 400);
  const huge = await image("/huge.png", 64);
  assert.equal(huge.status, 400);
  assert.match(await huge.text(), /max_pixels/);
});

await check("remote images: allowed patterns are fetched, others refused", async () => {
  assert.match(await attr("remote-allowed", "srcset"), /_volt\/image\?url=http%3A%2F%2F127\.0\.0\.1%3A9%2Fallowed%2Fpic\.png/);
  assert.equal(await attr("remote-blocked", "src"), "http://127.0.0.1:9/blocked/pic.png");
  assert.equal(await attr("remote-blocked", "srcset"), null);

  const allowed = await image(`${origin}/allowed/pic.png`, 64, "image/avif");
  assert.equal(allowed.status, 200);
  assert.equal(allowed.headers.get("content-type"), "image/avif");
  assert.equal(allowed.headers.get("cache-control"), "public, max-age=14400, stale-while-revalidate=604800");
  const fetched = hits.length;
  const cached = await image(`${origin}/allowed/pic.png`, 128, "image/avif");
  assert.equal(cached.status, 200);
  assert.equal(hits.length, fetched, "the original is reused from the disk cache");

  const blocked = await image(`${origin}/blocked/pic.png`, 64);
  assert.equal(blocked.status, 400);
  const otherHost = await image(`http://localhost:${cdn.address().port}/allowed/pic.png`, 64);
  assert.equal(otherHost.status, 400);
  const redirect = await image(`${origin}/allowed/redirect`, 64);
  assert.equal(redirect.status, 400);
  assert.match(await redirect.text(), /remote_patterns/);
  const notImage = await image(`${origin}/allowed/page.html`, 64);
  assert.equal(notImage.status, 400);
  assert.ok(!hits.includes("/blocked/pic.png"), "blocked URLs are never requested");
});

await check("static imports carry a blur placeholder", async () => {
  const hero = tag("hero");
  assert.match(hero, /width="320"/);
  assert.match(hero, /height="180"/);
  assert.match(hero, /background-image:url\(&quot;data:image\/svg\+xml;charset=utf-8,.*data:image\/png;base64,/);
  assert.match(tag("dataurl"), /background-image:url\(&quot;data:image\/gif;base64,R0lGOD/);
  // The placeholder is removed once the image has loaded.
  await page.waitForFunction(() => document.getElementById("hero").style.backgroundImage === "");
});

await check("priority images are preloaded from the head", async () => {
  const preload = page.locator("head link[rel=preload][as=image]");
  assert.equal(await preload.count(), 1);
  assert.match(await preload.getAttribute("imagesrcset"), /url=%2Fgraphic\.png&w=640&q=75 1x/);
  assert.equal(await preload.getAttribute("fetchpriority"), "high");
  assert.equal(await attr("priority", "loading"), null);
  assert.equal(await attr("priority", "fetchpriority"), "high");
});

await check("onLoad and onError fire", async () => {
  await page.locator("#loaded", { hasText: "loaded" }).waitFor();
  await page.locator("#failed", { hasText: "failed" }).waitFor();
});

cdn.close();
await done();
