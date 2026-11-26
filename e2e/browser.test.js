// Browser checks against a running `default` template app: BASE_URL=http://localhost:3000 node browser.test.js
import assert from "node:assert/strict";
import { createRequire } from "node:module";

const { chromium } = createRequire(import.meta.url)("playwright");

const base = process.env.BASE_URL ?? "http://localhost:3000";
const browser = await chromium.launch();
const failures = [];

async function check(name, run) {
  try {
    await run();
    console.log(`ok   ${name}`);
  } catch (error) {
    failures.push(name);
    console.log(`FAIL ${name}\n     ${error.message.split("\n")[0]}`);
  }
}

const page = await browser.newPage();
const errors = [];
page.on("pageerror", (error) => errors.push(error.message));
const documents = [];
page.on("request", (request) => request.resourceType() === "document" && documents.push(request.url()));

await check("server-rendered page hydrates", async () => {
  await page.goto(base, { waitUntil: "networkidle" });
  assert.equal(await page.title(), "Todos · app");
  assert.equal(await page.locator('meta[name="description"]').count(), 1);
});

await check("action from a form updates the page in place", async () => {
  await page.fill("input[name=title]", "Written by Playwright");
  await page.click('button[type=submit]:has-text("Add")');
  await page.getByText("Written by Playwright").waitFor({ timeout: 5000 });
  assert.equal(await page.inputValue("input[name=title]"), "");
});

await check("links navigate on the client", async () => {
  const before = documents.length;
  await page.click('a[href="/about"]');
  await page.locator("h1", { hasText: "About" }).waitFor();
  assert.equal(await page.title(), "About · app");
  assert.equal(documents.length, before, "no full document load");
});

await check("back button restores the previous page", async () => {
  await page.goBack();
  await page.getByText("Written by Playwright").waitFor();
  assert.equal(await page.title(), "Todos · app");
});

await check("unknown routes render not-found with 404", async () => {
  const response = await page.goto(`${base}/does-not-exist`);
  assert.equal(response.status(), 404);
  assert.equal(await page.title(), "Not found · app");
});

await check("no runtime errors in the browser", async () => {
  assert.deepEqual(errors, []);
});

await check("forms work without JavaScript", async () => {
  const context = await browser.newContext({ javaScriptEnabled: false });
  const noJs = await context.newPage();
  await noJs.goto(base);
  await noJs.fill("input[name=title]", "Posted without JS");
  await Promise.all([noJs.waitForURL(`${base}/`), noJs.click('button[type=submit]:has-text("Add")')]);
  await noJs.getByText("Posted without JS").waitFor();
  await context.close();
});

await browser.close();
if (failures.length) {
  console.log(`\n${failures.length} failed`);
  process.exit(1);
}
