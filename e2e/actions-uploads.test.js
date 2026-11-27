import assert from "node:assert/strict";
import { setup } from "./harness.js";

const { browser, check, done, base } = await setup();
const page = await browser.newPage();
const errors = [];
page.on("pageerror", (error) => errors.push(error.message));

const png = { name: "pixel.png", mimeType: "image/png", buffer: Buffer.alloc(1234, 7) };
const text = (selector) => page.locator(selector).textContent();

await check("uploads a file through <Form> with JavaScript and reports useFormStatus pending", async () => {
  await page.goto(base, { waitUntil: "networkidle" });
  await page.setInputFiles("input[name=file]", png);
  await page.setInputFiles("input[name=extras]", [
    { name: "a.txt", mimeType: "text/plain", buffer: Buffer.from("a") },
    { name: "b.txt", mimeType: "text/plain", buffer: Buffer.from("bb") },
  ]);
  const request = page.waitForRequest((r) => r.url().includes("/_volt/action/upload"));
  await page.click("button[type=submit]");
  await page.locator("button", { hasText: "Uploading (volt)" }).waitFor({ timeout: 2000 });
  const sent = await request;
  assert.match(sent.headers()["content-type"], /^multipart\/form-data; boundary=/);
  await page.locator("#result", { hasText: "pixel.png:1234:2" }).waitFor();
  await page.locator("#last", { hasText: "Report|pixel.png|1234|image/png|2" }).waitFor();
  assert.equal(await text("button[type=submit]"), "Upload");
});

await check("uploads a file without JavaScript", async () => {
  const context = await browser.newContext({ javaScriptEnabled: false });
  const nojs = await context.newPage();
  await nojs.goto(base);
  assert.equal(await nojs.getAttribute("form", "enctype"), "multipart/form-data");
  await nojs.fill("input[name=title]", "Plain");
  await nojs.setInputFiles("input[name=file]", { name: "doc.pdf", mimeType: "application/pdf", buffer: Buffer.alloc(42) });
  await Promise.all([nojs.waitForNavigation(), nojs.click("button[type=submit]")]);
  assert.equal(new URL(nojs.url()).pathname, "/");
  assert.equal(await nojs.locator("#last").textContent(), "Plain|doc.pdf|42|application/pdf|0");
  await context.close();
});

await check("useActionState shows returned state and validation errors", async () => {
  await page.goto(`${base}/state`, { waitUntil: "networkidle" });
  await page.click("#volt-form button");
  await page.locator("#name-error", { hasText: "Name is required" }).waitFor();
  assert.equal(await text("#status"), "422");
  await page.fill("#volt-form input[name=name]", "Ada");
  await page.fill("#volt-form input[name=age]", "12");
  await page.click("#volt-form button");
  await page.locator("#age-error", { hasText: "Must be 18 or older" }).waitFor();
  assert.equal(await page.locator("#name-error").count(), 0);
  await page.fill("#volt-form input[name=name]", "Ada");
  await page.fill("#volt-form input[name=age]", "30");
  await page.click("#volt-form button");
  await page.locator("#greeting", { hasText: /^Hello, Ada! #\d+$/ }).waitFor();
  assert.equal(await text("#status"), "ok");
});

await check("actions work with React's own useActionState", async () => {
  await page.click("#react-form button");
  await page.locator("#react-greeting", { hasText: "Hello, React!" }).waitFor();
});

await check("useActionState forms post without JavaScript and get their state and field errors back", async () => {
  const context = await browser.newContext({ javaScriptEnabled: false });
  const nojs = await context.newPage();
  await nojs.goto(`${base}/state`);
  assert.equal(await nojs.getAttribute("#volt-form", "action"), "/_volt/action/greet");
  await Promise.all([nojs.waitForNavigation(), nojs.click("#volt-form button")]);
  assert.equal(new URL(nojs.url()).pathname, "/state");
  assert.equal(await nojs.locator("#name-error").textContent(), "Name is required");
  assert.equal(await nojs.locator("#status").textContent(), "422");
  await nojs.fill("#volt-form input[name=name]", "Grace");
  await Promise.all([nojs.waitForNavigation(), nojs.click("#volt-form button")]);
  assert.equal(new URL(nojs.url()).pathname, "/state");
  assert.match(await nojs.locator("#greeting").textContent(), /^Hello, Grace! #\d+$/);
  assert.equal(await nojs.locator("#name-error").count(), 0);
  await nojs.reload();
  assert.equal(await nojs.locator("#greeting").textContent(), "none");
  await context.close();
});

await check("a useActionState permalink posts to its page without JavaScript", async () => {
  const context = await browser.newContext({ javaScriptEnabled: false });
  const nojs = await context.newPage();
  await nojs.goto(`${base}/state`);
  assert.equal(await nojs.getAttribute("#permalink-form", "action"), "/state?via=permalink");
  await Promise.all([nojs.waitForNavigation(), nojs.click("#permalink-form button")]);
  const url = new URL(nojs.url());
  assert.equal(url.pathname + url.search, "/state?via=permalink");
  assert.equal(await nojs.locator("#permalink-greeting").textContent(), "Hello, Permalink!");
  await context.close();
});

await check("no-JavaScript submissions only return to local paths", async () => {
  const isolated = await browser.newContext();
  const post = (form) => isolated.request.post(`${base}/_volt/action/greet`, { form, maxRedirects: 0 });
  for (const redirect of ["https://evil.example", "//evil.example", "javascript:alert(1)"]) {
    const ok = await post({ name: "a", redirect });
    assert.equal(ok.status(), 303);
    assert.equal(ok.headers().location, "/");
    const failed = await post({ name: "", redirect });
    assert.equal(failed.status(), 422);
    assert.doesNotMatch(await failed.text(), /evil|javascript:/);
  }
  const local = await post({ name: "a", redirect: "/items?x=1" });
  assert.equal(local.headers().location, "/items?x=1");
  await isolated.close();
});

await check("action inputs with files and nested objects decode on the server", async () => {
  await page.goto(base, { waitUntil: "networkidle" });
  await page.click("#tag");
  await page.locator("#tagged", { hasText: "tags.txt|3|x,y|4" }).waitFor();
});

await check("ctx.form reads urlencoded bodies sent without a form content type", async () => {
  const result = await page.evaluate(() => fetch("/api/form", { method: "POST", body: "name=zed" }).then((r) => r.json()));
  assert.deepEqual(result, { name: "zed" });
});

await check("a cookie-setting action refreshes the client without purging the server cache", async () => {
  const isolated = await browser.newContext();
  const renders = async () => {
    const html = await isolated.request.get(`${base}/cached`).then((r) => r.text());
    return html.match(/id="renders">(\d+)</)[1];
  };
  const before = await renders();
  assert.equal(await renders(), before);
  const response = await isolated.request.post(`${base}/_volt/action/login`, {
    data: { name: "cache" },
    headers: { "x-volt-action": "1" },
  });
  assert.equal(response.headers()["x-volt-revalidate"], "*");
  assert.equal(await renders(), before);
  await isolated.close();
});

await check("revalidate_path refreshes the list without an explicit refresh", async () => {
  await page.goto(`${base}/items`, { waitUntil: "networkidle" });
  await page.fill("#add input[name=title]", "first");
  await page.click("#add button");
  await page.locator("#items li", { hasText: "first" }).waitFor();
  await page.click("#add-code");
  await page.locator("#items li", { hasText: "from code" }).waitFor();
  assert.equal(new URL(page.url()).pathname, "/items");
});

await check("cookies set by an action are visible on the next navigation", async () => {
  await page.goto(`${base}/items`, { waitUntil: "networkidle" });
  await page.click("#me-link");
  await page.locator("#user", { hasText: "anonymous" }).waitFor();
  await page.click("a[href='/items']");
  await page.locator("h1", { hasText: "Items" }).waitFor();
  const prefetched = page.waitForResponse((r) => r.url().includes("/_volt/data?path=%2Fme"), { timeout: 1500 });
  await page.hover("#me-link");
  await prefetched.catch(() => {});
  await page.click("#login button");
  await page.waitForResponse((r) => r.url().includes("/_volt/action/login"));
  await page.waitForLoadState("networkidle");
  await page.click("#me-link");
  await page.locator("#user", { hasText: "ada" }).waitFor();
});

await check("API routes read multipart bodies and enforce the body limit", async () => {
  const result = await page.evaluate(async () => {
    const form = new FormData();
    form.append("note", "hi");
    form.append("file", new Blob(["hello"], { type: "text/plain" }), "hello.txt");
    const ok = await fetch("/api/upload", { method: "POST", body: form }).then((r) => r.json());
    const big = new FormData();
    big.append("file", new Blob([new Uint8Array(2 * 1024 * 1024)]), "big.bin");
    const tooLarge = await fetch("/api/upload", { method: "POST", body: big }).then((r) => r.status);
    return { ok, tooLarge };
  });
  assert.deepEqual(result.ok, { note: "hi", filename: "hello.txt", size: 5 });
  assert.equal(result.tooLarge, 413);
});

await check("no client errors", async () => {
  assert.deepEqual(errors, []);
});

await done();
