import assert from "node:assert/strict";
import http from "node:http";
import { setup } from "./harness.js";

const { browser, check, done, base } = await setup();

/** Fetches a page over raw HTTP and records when each body chunk arrived. */
function chunks(path, agent = "Mozilla/5.0 (X11; Linux x86_64) volt-e2e") {
  return new Promise((resolve, reject) => {
    const started = Date.now();
    const request = http.get(`${base}${path}`, { headers: { "user-agent": agent } }, (response) => {
      const received = [];
      response.setEncoding("utf8");
      response.on("data", (text) => received.push({ at: Date.now() - started, text }));
      response.on("end", () =>
        resolve({ status: response.statusCode, headers: response.headers, received, body: received.map((c) => c.text).join("") }),
      );
    });
    request.on("error", reject);
  });
}

const arrival = (received, text) => received.find((chunk) => chunk.text.includes(text))?.at;

await check("streams the shell before a delayed Suspense boundary", async () => {
  const { status, headers, received, body } = await chunks("/stream");
  assert.equal(status, 200);
  assert.equal(headers["content-length"], undefined);
  assert.equal(headers["x-accel-buffering"], "no");
  const fallback = arrival(received, 'id="fallback"');
  const late = arrival(received, "Late content");
  assert.ok(fallback !== undefined, "the fallback is in the stream");
  assert.ok(late !== undefined, "the late content is in the stream");
  assert.ok(!received[0].text.includes("Late content"), "the first chunk holds only the shell");
  assert.ok(late - fallback >= 800, `the boundary arrived ${late - fallback}ms after the shell`);
  assert.ok(fallback < 800, `the shell arrived after ${fallback}ms`);
  assert.ok(body.includes("Widget rendered") && !body.includes('id="widget-loading"'), "dynamic() renders in place");
  assert.ok(body.includes('id="browser-loading"') && !body.includes("Rendered in"), "ssr: false renders its loading state");
  assert.ok(body.indexOf("__VOLT_DATA__") > body.indexOf("Late content"), "state follows the streamed boundaries");
  assert.ok(body.trimEnd().endsWith("</html>"));
  assert.equal(body.match(/<\/body>/g).length, 1);
  assert.ok(body.indexOf("modulepreload") < body.indexOf("</head>"), "head additions are in the shell");
});

await check("crawlers get the complete document", async () => {
  const { status, received, body } = await chunks("/stream", "Mozilla/5.0 (compatible; Googlebot/2.1; +http://www.google.com/bot.html)");
  assert.equal(status, 200);
  assert.ok(body.includes("Late content"));
  assert.ok(body.includes("Widget rendered"));
  assert.ok(!body.includes('id="fallback"'), "no fallback for crawlers");
  assert.ok(!body.includes("$RC"), "no streaming scripts for crawlers");
  assert.ok(received.length >= 1);
});

await check("notFound() before the shell sets the status", async () => {
  assert.equal((await chunks("/gone")).status, 404);
  assert.equal((await chunks("/gone", "Googlebot/2.1")).status, 404);
});

await check("a boundary that never settles still ends a well-formed document", async () => {
  const streamed = await chunks("/never");
  const crawled = await chunks("/never", "Googlebot/2.1");
  for (const { status, body } of [streamed, crawled]) {
    assert.equal(status, 200);
    assert.ok(body.trimEnd().endsWith("</html>"), "the document is closed");
    assert.equal(body.match(/<\/body>/g).length, 1);
    assert.ok(body.includes("__VOLT_DATA__"));
  }
  assert.equal(crawled.headers["cache-control"], streamed.headers["cache-control"]);
});

await check("concurrent streams do not wait for each other", async () => {
  const started = Date.now();
  const all = await Promise.all(Array.from({ length: 12 }, () => chunks("/stream")));
  assert.ok(all.every((r) => r.body.includes("Late content")));
  assert.ok(Date.now() - started < 6000, `12 streams took ${Date.now() - started}ms`);
});

const page = await browser.newPage();
const errors = [];
page.on("pageerror", (error) => errors.push(String(error)));
page.on("console", (message) => message.type() === "error" && errors.push(message.text()));

await check("hydrates a streamed page", async () => {
  await page.goto(`${base}/stream`);
  await page.locator("#late", { hasText: "Late content" }).waitFor();
  await page.locator("#browser", { hasText: "Rendered in the browser" }).waitFor();
  await page.locator("#widget").waitFor();
  assert.equal(await page.title(), "Stream · streaming");
  assert.deepEqual(errors, []);
});

await check("shows loading.tsx at once while a slow loader runs", async () => {
  await page.goto(base, { waitUntil: "networkidle" });
  const clicked = Date.now();
  await page.click("a[href='/slow']");
  await page.locator("#slow-loading").waitFor({ timeout: 1000 });
  const shown = Date.now() - clicked;
  assert.ok(shown < 1000, `loading UI after ${shown}ms`);
  assert.equal(await page.locator("#slow-title").count(), 0);
  assert.equal(await page.locator("nav").count(), 1, "the layout stays");
  assert.equal(new URL(page.url()).pathname, "/slow");
  await page.locator("#slow-title", { hasText: "Slow report" }).waitFor();
  assert.equal(await page.locator("#slow-loading").count(), 0);
  assert.ok(Date.now() - clicked >= 1400, "the page waited for the loader");
});

await check("Back during the loading UI keeps the URL and the content together", async () => {
  await page.goto(base, { waitUntil: "networkidle" });
  await page.click("a[href='/slow']");
  await page.locator("#slow-loading").waitFor({ timeout: 1000 });
  await page.goBack();
  await page.waitForTimeout(2500);
  assert.equal(new URL(page.url()).pathname, "/");
  assert.equal(await page.locator("main h1").textContent(), "Home");
  assert.equal(await page.locator("#slow-title").count(), 0);
});

await check("keeps transitions for routes without loading.tsx", async () => {
  await page.click("a[href='/quick']");
  await page.locator("#quick-title").waitFor();
  await page.click("a[href='/stream']");
  await page.locator("#late", { hasText: "Late content" }).waitFor();
  await page.goBack();
  await page.locator("#quick-title").waitFor();
  assert.deepEqual(errors, []);
});

await check("hydrates a page whose boundary the server gave up on", async () => {
  const other = await browser.newPage();
  await other.goto(`${base}/never`);
  await other.locator("#mounted").waitFor();
  await other.locator("#never", { hasText: "Rendered on the client" }).waitFor();
  await other.close();
});

await done();
