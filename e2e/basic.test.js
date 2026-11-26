import assert from "node:assert/strict";
import { setup } from "./harness.js";

const { browser, check, done, base } = await setup();
const page = await browser.newPage();

await check("renders and navigates on the client", async () => {
  await page.goto(base, { waitUntil: "networkidle" });
  assert.equal(await page.title(), "basic");
  await page.click("a[href='/second']");
  await page.locator("h1", { hasText: "Second" }).waitFor();
  assert.equal(await page.title(), "Second · basic");
});

await done();
