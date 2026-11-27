import assert from "node:assert/strict";
import { setup } from "./harness.js";

const { browser, check, done, base } = await setup();
const page = await browser.newPage();
await page.goto(base, { waitUntil: "networkidle" });
const style = (selector, property) =>
  page.$eval(selector, (el, property) => getComputedStyle(el).getPropertyValue(property), property);

await check("css module classes are scoped and composed", async () => {
  const className = await page.getAttribute("#title", "class");
  assert.match(className, /^page-module__title__\S+ page-module__base__\S+$/);
  assert.equal(await style("#title", "color"), "rgb(255, 0, 0)");
  assert.equal(await style("#title", "letter-spacing"), "2px");
  assert.notEqual(await style("#global", "color"), "rgb(255, 0, 0)");
});

await check("scss variables, partials and nesting compile", async () => {
  assert.match(await page.getAttribute("#card", "class"), /^card-module__card__/);
  assert.equal(await style("#card", "padding-top"), "20px");
  assert.equal(await style("#card", "border-top-color"), "rgb(10, 20, 30)");
  assert.equal(await style("#dotted", "padding-top"), "12px");
  assert.equal(await style("#dotted", "color"), "rgb(10, 20, 30)");
  assert.equal(await style("#dotted .inner", "user-select"), "none");
});

await check("local font is served, preloaded and applied", async () => {
  const family = await style("body", "font-family");
  assert.match(family, /^"?__Geist_\w+"?, "Helvetica Neue", sans-serif$/);
  assert.equal(await style("#mono", "font-family"), family);
  const preload = await page.getAttribute("link[rel=preload][as=font]", "href");
  assert.match(preload, /^\/_volt\/static\/media\/Geist-\w+\.woff2$/);
  const response = await fetch(base + preload);
  assert.equal(response.status, 200);
  const loaded = await page.evaluate(async () => {
    await document.fonts.ready;
    return [...document.fonts].filter((f) => f.family.includes("__Geist_")).map((f) => f.status);
  });
  assert.deepEqual(loaded, ["loaded"]);
});

await check("url() assets in css are copied and load", async () => {
  for (const selector of ["#dotted", "#icon"]) {
    const image = await style(selector, "background-image");
    const url = image.match(/url\("?([^")]+)"?\)/)?.[1];
    assert.match(url ?? "", /\/_volt\/static\/media\/dot-\w+\.png$/);
    const response = await fetch(new URL(url, base));
    assert.equal(response.status, 200);
    assert.equal(response.headers.get("content-type"), "image/png");
  }
});

await check("production css is one minified, prefixed stylesheet", async () => {
  const links = await page.$$eval("link[rel=stylesheet]", (l) => l.map((x) => x.getAttribute("href")));
  assert.equal(links.length, 1);
  const css = await (await fetch(base + links[0])).text();
  assert.ok(!css.includes("\n  "), "css is not minified");
  assert.ok(css.includes("-webkit-user-select:none"), "missing vendor prefix");
  assert.ok(css.includes("@font-face"), "missing @font-face");
  assert.ok(css.indexOf("__Geist_") < css.indexOf("page-module__title__"), "layout css comes first");
});

await done();
