// Runs under `volt dev` (see the fixture's package.json) and edits the app while the page is open.
import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { setup } from "./harness.js";

const { browser, check, done, base } = await setup();
const page = await browser.newPage();
const dir = process.env.APP_DIR;
const edit = (file, from, to) => {
  const path = join(dir, file);
  const source = readFileSync(path, "utf8");
  assert.ok(source.includes(from), `${file} has no ${from}`);
  writeFileSync(path, source.replace(from, to));
};
const text = (selector) => page.locator(selector).textContent();
const overlay = page.locator("volt-dev-overlay");
const marked = () => page.evaluate(() => window.__marker === true);

await page.goto(base, { waitUntil: "networkidle" });
// Lost on any full reload.
await page.evaluate(() => (window.__marker = true));

await check("development builds install the refresh runtime", async () => {
  assert.equal(await page.evaluate(() => typeof window.__VOLT_REFRESH__?.apply), "function");
  const [cache, revalidated] = await page.evaluate(async () => {
    const script = document.querySelector('script[type="module"][src]');
    const response = await fetch(script.src);
    const etag = response.headers.get("etag");
    const again = await fetch(script.src, { headers: { "if-none-match": etag } });
    return [response.headers.get("cache-control"), etag && again.status];
  });
  assert.equal(cache, "no-cache");
  assert.equal(revalidated, 304);
});

await check("component edits apply in place and keep state", async () => {
  await page.click("#increment");
  await page.click("#increment");
  await page.fill("#text", "kept");
  edit("app/counter.tsx", "Version one", "Version two");
  await page.locator("#message", { hasText: "Version two" }).waitFor({ timeout: 20_000 });
  assert.equal(await text("#increment"), "Count: 2");
  assert.equal(await page.inputValue("#text"), "kept");
  assert.ok(await marked(), "the page reloaded");
});

await check("page edits apply in place", async () => {
  edit("app/page.tsx", ">Home<", ">Home edited<");
  await page.locator("#title", { hasText: "Home edited" }).waitFor({ timeout: 20_000 });
  assert.equal(await page.inputValue("#text"), "kept");
  assert.ok(await marked(), "the page reloaded");
});

await check("build errors show an overlay and the fix clears it without losing state", async () => {
  edit("app/counter.tsx", "Version two", "Version {");
  await overlay.locator("h1", { hasText: "Build failed" }).waitFor({ timeout: 20_000 });
  assert.equal(await page.inputValue("#text"), "kept");
  edit("app/counter.tsx", "Version {", "Version three");
  await page.locator("#message", { hasText: "Version three" }).waitFor({ timeout: 20_000 });
  await overlay.waitFor({ state: "detached" });
  assert.equal(await text("#increment"), "Count: 2");
  assert.equal(await page.inputValue("#text"), "kept");
  assert.ok(await marked(), "the page reloaded");
});

await check("runtime errors show an overlay until the next update", async () => {
  edit("app/counter.tsx", '"use client";', '"use client";\nif (typeof window !== "undefined" && !window.__fixed) throw new Error("module exploded");');
  await overlay.locator("pre", { hasText: "module exploded" }).waitFor({ timeout: 20_000 });
  assert.equal(await page.inputValue("#text"), "kept");
  edit("app/counter.tsx", '\nif (typeof window !== "undefined" && !window.__fixed) throw new Error("module exploded");', "");
  await overlay.waitFor({ state: "detached", timeout: 20_000 });
  assert.equal(await page.inputValue("#text"), "kept");
  assert.ok(await marked(), "the page reloaded");
});

await check("css edits swap the stylesheet", async () => {
  edit("app/globals.css", "rgb(0, 0, 0)", "rgb(0, 128, 0)");
  await page.waitForFunction(() => getComputedStyle(document.body).color === "rgb(0, 128, 0)", null, {
    timeout: 20_000,
  });
  assert.ok(await marked(), "the page reloaded");
});

await check("constant edits reach their importers in place", async () => {
  edit("app/label.ts", "Label one", "Label two");
  await page.locator("#label", { hasText: "Label two" }).waitFor({ timeout: 20_000 });
  assert.equal(await text("#increment"), "Count: 2");
  assert.equal(await page.inputValue("#text"), "kept");
  assert.ok(await marked(), "the page reloaded");
});

await check("helpers that close over a changed constant update their callers", async () => {
  edit("app/helper.ts", "Helper one", "Helper two");
  await page.locator("#helper", { hasText: "Helper two" }).waitFor({ timeout: 20_000 });
  assert.equal(await text("#increment"), "Count: 2");
  assert.ok(await marked(), "the page reloaded");
});

await check("updates evaluate only the modules that changed", async () => {
  const evaluations = () => page.evaluate(() => window.__evaluations);
  const before = await evaluations();
  edit("app/page.tsx", ">Home edited<", ">Home again<");
  await page.locator("#title", { hasText: "Home again" }).waitFor({ timeout: 20_000 });
  edit("app/page.tsx", ">Home again<", ">Home edited<");
  await page.locator("#title", { hasText: "Home edited" }).waitFor({ timeout: 20_000 });
  assert.equal(await evaluations(), before);
  assert.ok(await marked(), "the page reloaded");
});

await check("render errors without an error boundary show an overlay and the fix renders again", async () => {
  const boom = '  if (count > 0) throw new Error("render boom");\n';
  edit("app/counter.tsx", "  return (\n", `${boom}  return (\n`);
  await overlay.locator("pre", { hasText: "render boom" }).waitFor({ timeout: 20_000 });
  edit("app/counter.tsx", boom, "");
  await page.locator("#message", { hasText: "Version three" }).waitFor({ timeout: 20_000 });
  await overlay.waitFor({ state: "detached" });
  await page.click("#increment");
  assert.equal(await text("#increment"), "Count: 1");
  assert.ok(await marked(), "the page reloaded");
});

await check("changed route config reloads the page", async () => {
  edit("app/layout.tsx", 'title: "fast-refresh"', 'title: "fast-refresh edited"');
  await page.waitForFunction(() => !window.__marker, null, { timeout: 20_000 });
  await page.waitForFunction(() => document.title === "fast-refresh edited");
  assert.equal(await text("#label"), "Label two");
});

await check("installed package changes reload the page on the next update", async () => {
  await page.evaluate(() => (window.__marker = true));
  const link = join(dir, "node_modules/volt-rs/src/link.js");
  writeFileSync(link, `${readFileSync(link, "utf8")}\n// upgraded\n`);
  edit("app/counter.tsx", "Version three", "Version 3");
  await page.waitForFunction(() => !window.__marker, null, { timeout: 20_000 });
  await page.locator("#message", { hasText: "Version 3" }).waitFor();
  edit("app/counter.tsx", "Version 3", "Version three");
  await page.locator("#message", { hasText: "Version three" }).waitFor({ timeout: 20_000 });
});

await check("client navigation stays soft after updates", async () => {
  await page.evaluate(() => (window.__marker = true));
  edit("app/counter.tsx", "Version three", "Version four");
  await page.locator("#message", { hasText: "Version four" }).waitFor({ timeout: 20_000 });
  await page.click("a[href='/other']");
  await page.locator("#title", { hasText: "Other" }).waitFor();
  assert.ok(await marked(), "navigation reloaded the page");
});

await done();
