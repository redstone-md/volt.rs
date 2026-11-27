import assert from "node:assert/strict";
import { setup } from "./harness.js";

const { browser, check, done, base } = await setup();
const page = await browser.newPage();
const errors = [];
page.on("pageerror", (error) => errors.push(error.message));
page.on("console", (message) => message.type() === "error" && !message.text().startsWith("Failed to load resource") && errors.push(message.text()));
// A hydration mismatch makes React discard the server HTML, which shows up as removed nodes.
await page.addInitScript(() => {
  window.__removed = 0;
  new MutationObserver((records) => {
    for (const record of records) window.__removed += record.removedNodes.length;
  }).observe(document, { childList: true, subtree: true });
});
const documents = [];
page.on("request", (request) => request.resourceType() === "document" && documents.push(request.url()));

await check("custom _document sets html attributes and head tags", async () => {
  await page.goto(base, { waitUntil: "networkidle" });
  assert.equal(await page.getAttribute("html", "lang"), "fr");
  assert.equal(await page.getAttribute("html", "data-theme"), "dark");
  assert.equal(await page.getAttribute("body", "class"), "pages-body");
  assert.equal(await page.locator('head meta[name="generator"]').getAttribute("content"), "pages-router-fixture");
  assert.equal(await page.locator("#__next").count(), 1);
});

await check("_app wraps the page and next/head sets the title", async () => {
  assert.equal(await page.title(), "Home | pages");
  await page.locator('[data-testid="app-shell"] h1', { hasText: "Pages home" }).waitFor();
  assert.equal(await page.textContent('[data-testid="pathname"]'), "/");
});

await check("the page hydrates the server HTML in place", async () => {
  await page.waitForFunction(() => window.__routerEvents !== undefined);
  assert.equal(await page.evaluate(() => window.__removed), 0);
});

await check("client navigation fires router events without a reload", async () => {
  const before = documents.length;
  await page.click('nav a[href="/blog/hello-world"]');
  await page.locator("h1", { hasText: "Post hello-world" }).waitFor();
  assert.equal(documents.length, before, "no full document load");
  assert.equal(await page.title(), "Post hello-world");
  const events = await page.evaluate(() => window.__routerEvents);
  assert.deepEqual(events, ["start /blog/hello-world", "history /blog/hello-world", "complete /blog/hello-world"]);
});

await check("dynamic params via useRouter().query and getServerSideProps", async () => {
  assert.equal(await page.textContent('[data-testid="shout"]'), "HELLO-WORLD");
  assert.equal(await page.textContent('[data-testid="pattern"]'), "/blog/[slug]");
  assert.equal(await page.textContent('[data-testid="resolved"]'), "/blog/hello-world");
});

await check("data functions stay out of browser chunks", async () => {
  const html = await fetch(`${base}/blog/chunks`).then((r) => r.text());
  const chunks = [...html.matchAll(/rel="modulepreload" href="([^"]+)"/g)].map((m) => m[1]);
  assert.ok(chunks.length > 0);
  for (const chunk of chunks) {
    const code = await fetch(new URL(chunk, base)).then((r) => r.text());
    assert.ok(!code.includes("server-only-marker"), `${chunk} contains getServerSideProps`);
  }
});

await check("router.push with a url object interpolates the pattern", async () => {
  await page.goto(base, { waitUntil: "networkidle" });
  const before = documents.length;
  await page.click("button:has-text('Push')");
  await page.locator("h1", { hasText: "Post pushed" }).waitFor();
  assert.equal(documents.length, before);
  assert.equal(new URL(page.url()).pathname + new URL(page.url()).search, "/blog/pushed?ref=button");
  assert.equal(await page.textContent('[data-testid="ref"]'), "button");
  await page.goBack();
  await page.locator("h1", { hasText: "Pages home" }).waitFor();
});

await check("navigating to an app router page loads a full document", async () => {
  const before = documents.length;
  await page.click('a[href="/modern"]');
  await page.locator("h1", { hasText: "App Router page" }).waitFor();
  assert.equal(documents.length, before + 1);
  assert.equal(await page.title(), "Modern · app router");
  assert.equal(await page.getAttribute("html", "lang"), "en");
  await page.click('a[href="/"]');
  await page.locator("h1", { hasText: "Pages home" }).waitFor();
  assert.equal(documents.length, before + 2);
});

await check("rust loader props merge with getServerSideProps", async () => {
  await page.click('nav a[href="/profile"]');
  await page.locator("h1", { hasText: "Profile" }).waitFor();
  assert.equal(await page.textContent('[data-testid="loader"]'), "Ferris writes Rust");
  assert.equal(await page.textContent('[data-testid="greeting"]'), "Bonjour");
  const fresh = await browser.newPage();
  await fresh.goto(`${base}/profile`);
  assert.equal(await fresh.textContent('[data-testid="loader"]'), "Ferris writes Rust");
  assert.equal(await fresh.textContent('[data-testid="greeting"]'), "Bonjour");
  await fresh.close();
});

await check("getStaticProps page is prerendered at build", async () => {
  const first = await fetch(`${base}/static`).then((r) => r.text());
  const second = await fetch(`${base}/static`).then((r) => r.text());
  assert.match(first, /built ahead of time/);
  const stamp = (html) => /data-testid="rendered-at">(\d+)</.exec(html)?.[1];
  assert.ok(stamp(first));
  assert.equal(stamp(first), stamp(second));
  await page.click('nav a[href="/static"]');
  await page.locator('[data-testid="message"]', { hasText: "built ahead of time" }).waitFor();
});

await check("getStaticPaths prerenders listed paths and 404s the rest", async () => {
  const listed = await fetch(`${base}/posts/2`);
  assert.equal(listed.status, 200);
  assert.match(await listed.text(), /Static post 2/);
  const unlisted = await fetch(`${base}/posts/3`);
  assert.equal(unlisted.status, 404);
  assert.match(await unlisted.text(), /Lost in pages/);
});

await check("getServerSideProps redirect and notFound", async () => {
  const redirected = await fetch(`${base}/blog/old`, { redirect: "manual" });
  assert.equal(redirected.status, 307);
  assert.equal(redirected.headers.get("location"), "/blog/new");
  const missing = await fetch(`${base}/blog/missing`);
  assert.equal(missing.status, 404);
});

await check("custom 404 page", async () => {
  const response = await page.goto(`${base}/nope`);
  assert.equal(response.status(), 404);
  assert.equal(await page.title(), "Lost | pages");
  await page.locator('[data-testid="app-shell"] h1', { hasText: "Lost in pages" }).waitFor();
});

await check("pages/api is not served", async () => {
  const response = await fetch(`${base}/api/hello`);
  assert.equal(response.status, 404);
});

const stamp = (html) => /data-testid="rendered-at">([\d.]+)</.exec(html)?.[1];
const data = (path) => fetch(`${base}/_volt/data?path=${encodeURIComponent(path)}`).then((r) => r.json());

await check("client navigation reuses the props of the static HTML", async () => {
  const html = await fetch(`${base}/static`).then((r) => r.text());
  const state = await data("/static");
  assert.equal(String(state.data.renderedAt), stamp(html));
  const fresh = await data("/items/nav");
  const later = await fetch(`${base}/items/nav`).then((r) => r.text());
  assert.equal(String(fresh.data.renderedAt), stamp(later));
});

await check("getStaticProps with a request-dependent loader is never shared", async () => {
  const as = (user) => fetch(`${base}/who`, { headers: { cookie: `user=${user}` } });
  const alice = await as("alice");
  assert.match(alice.headers.get("cache-control"), /private/);
  assert.match(await alice.text(), /alice/);
  const bob = await as("bob");
  assert.match(bob.headers.get("cache-control"), /private/);
  const body = await bob.text();
  assert.match(body, /bob/);
  assert.doesNotMatch(body, /alice/);
});

await check("static pages are cached per pathname", async () => {
  const get = (path) => fetch(`${base}${path}`).then((r) => r.text()).then(stamp);
  const first = await get("/items/a");
  assert.equal(await get("/items/a?x=1"), first);
  assert.equal(await get("/items/a?x=2"), first);
  assert.notEqual(await get("/items/b"), first);
});

await check("an app router page with an #__next element hydrates in place", async () => {
  await page.goto(`${base}/hydrate`, { waitUntil: "networkidle" });
  assert.equal(await page.locator("#__next").count(), 1);
  assert.equal(await page.evaluate(() => document.querySelector("#__next").parentElement.tagName), "BODY");
  await page.click("button:has-text('count 0')");
  await page.locator("button", { hasText: "count 1" }).waitFor();
});

await check("no runtime or hydration errors", async () => {
  assert.deepEqual(errors, []);
});

await done();
