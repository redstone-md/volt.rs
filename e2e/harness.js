// Shared helpers for fixture tests: `const { browser, check, done, base } = await setup();`
import { createRequire } from "node:module";

export async function setup() {
  const { chromium } = createRequire(import.meta.url)("playwright");
  const browser = await chromium.launch();
  const failures = [];
  const base = process.env.BASE_URL ?? "http://localhost:3900";
  async function check(name, body) {
    try {
      await body();
      console.log(`ok   ${name}`);
    } catch (error) {
      failures.push(name);
      console.log(`FAIL ${name}\n     ${String(error?.message ?? error).split("\n")[0]}`);
    }
  }
  async function done() {
    await browser.close();
    if (failures.length) {
      console.log(`\n${failures.length} failed`);
      process.exit(1);
    }
  }
  return { browser, check, done, base };
}
