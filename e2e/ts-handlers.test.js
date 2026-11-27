import assert from "node:assert/strict";
import http from "node:http";
import { setup } from "./harness.js";

const { browser, check, done, base } = await setup();
const get = (path, init) => fetch(base + path, { redirect: "manual", ...init });
// Sends the path as written: `fetch` would resolve dot segments itself.
const raw = (path, method = "GET", body) =>
  new Promise((resolve, reject) => {
    const url = new URL(base);
    const headers = body ? { "content-type": "application/json" } : {};
    const request = http.request({ host: url.hostname, port: url.port, path, method, headers }, (response) => {
      response.resume();
      response.on("end", () => resolve({ status: response.statusCode, headers: response.headers }));
    });
    request.on("error", reject);
    request.end(body);
  });

await check("GET and POST handlers answer JSON", async () => {
  const hello = await get("/api/hello?name=volt");
  assert.equal(hello.status, 200);
  assert.match(hello.headers.get("content-type"), /^application\/json/);
  assert.deepEqual(await hello.json(), { hello: "volt" });
  const posted = await get("/api/hello", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ n: 1, text: "é" }),
  });
  assert.equal(posted.status, 201);
  assert.deepEqual(await posted.json(), { received: { n: 1, text: "é" }, type: "application/json" });
});

await check("handlers receive params and answer every method they export", async () => {
  assert.deepEqual(await (await get("/api/items/21")).json(), { id: "21", double: 42 });
  const deleted = await get("/api/items/21", { method: "DELETE" });
  assert.equal(deleted.status, 204);
  assert.equal(deleted.headers.get("x-deleted"), "1");
  const head = await get("/api/items/3", { method: "HEAD" });
  assert.equal(head.status, 200);
  const put = await get("/api/items/3", { method: "PUT" });
  assert.equal(put.status, 405);
  assert.equal(put.headers.get("allow"), "GET, DELETE, HEAD, OPTIONS");
  const options = await get("/api/items/3", { method: "OPTIONS" });
  assert.equal(options.status, 204);
});

await check("Rust API routes answer before route handlers", async () => {
  assert.deepEqual(await (await get("/api/rust")).json(), { from: "rust" });
  assert.deepEqual(await (await get("/api/other")).json(), { from: "typescript", name: "other" });
});

await check("NextResponse.redirect and cookies", async () => {
  const old = await get("/api/old");
  assert.equal(old.status, 307);
  assert.equal(old.headers.get("location"), `${base}/api/hello?name=redirected`);
  const cookies = await get("/api/cookies", { headers: { cookie: "theme=dark" } });
  assert.deepEqual(await cookies.json(), { theme: "dark" });
  assert.equal(cookies.headers.get("set-cookie"), "visited=1; Path=/; HttpOnly; SameSite=Lax");
});

await check("binary bodies keep every byte", async () => {
  const pixel = await get("/api/pixel");
  assert.equal(pixel.headers.get("content-type"), "image/gif");
  assert.equal(pixel.headers.get("cache-control"), "public, max-age=60");
  const bytes = new Uint8Array(await pixel.arrayBuffer());
  assert.equal(bytes.length, 43 + 256);
  assert.deepEqual([...bytes.slice(0, 6)], [0x47, 0x49, 0x46, 0x38, 0x39, 0x61]);
  assert.deepEqual([...bytes.slice(43)], [...Array(256).keys()]);
});

await check("urlencoded forms parse with request.formData()", async () => {
  const response = await get("/api/form", { method: "POST", body: new URLSearchParams("name=Ada+L&tag=a&tag=b") });
  assert.deepEqual(await response.json(), { name: "Ada L", tags: ["a", "b"] });
});

await check("fetch in a handler reaches another endpoint", async () => {
  const relay = await get("/api/relay");
  assert.deepEqual(await relay.json(), { status: 200, body: "upstream got hello", header: "secret" });
});

await check("force-static handlers are built once", async () => {
  const first = await get("/feed.xml");
  const second = await get("/feed.xml");
  assert.equal(first.headers.get("content-type"), "application/xml");
  const body = await first.text();
  assert.match(body, /<built>\d+-/);
  assert.equal(await second.text(), body);
});

await check("middleware redirects, rewrites, answers and injects headers", async () => {
  const old = await get("/old-home");
  assert.equal(old.status, 307);
  assert.equal(old.headers.get("location"), `${base}/`);
  const guarded = await get("/dashboard");
  assert.equal(guarded.headers.get("location"), `${base}/login`);
  const blocked = await get("/blocked");
  assert.equal(blocked.status, 403);
  assert.equal(await blocked.text(), "blocked by middleware");
  const rewritten = await get("/rewritten");
  assert.equal(rewritten.status, 200);
  assert.match(await rewritten.text(), /<h1>About<\/h1><p id="from">rewrite<\/p>/);
  const about = await get("/about");
  assert.equal(about.headers.get("x-middleware-ran"), "1");
  assert.equal(about.headers.get("x-rust-middleware"), "1");
  assert.match(about.headers.get("set-cookie"), /^seen=yes; Path=\//);
  const dashboard = await get("/dashboard", { headers: { cookie: "session=ada" } });
  assert.match(await dashboard.text(), /<p id="user">ada<\/p>/);
});

await check("the matcher skips paths it excludes", async () => {
  const api = await get("/api/hello");
  assert.equal(api.headers.get("x-middleware-ran"), null);
  assert.equal(api.headers.get("x-rust-middleware"), "1");
  const login = await get("/login");
  assert.equal(login.headers.get("x-middleware-ran"), null);
});

await check("encoded and dotted paths cannot slip past the matcher", async () => {
  for (const path of ["/dash%62oard", "/DASH/../dashboard", "/x/%2e%2e/dashboard"]) {
    const response = await raw(path);
    assert.equal(response.status, 307, path);
    assert.equal(response.headers.location, `${base}/login`, path);
  }
  const doubled = await get("//dashboard");
  assert.equal(doubled.status, 308);
  assert.equal(doubled.headers.get("location"), "/dashboard");
  const posted = await raw("//api/hello", "POST", JSON.stringify({ n: 2 }));
  assert.equal(posted.status, 201);
  for (const path of ["/dash%62oard", "//dashboard", "/x/../dashboard"]) {
    const data = await get(`/_volt/data?path=${encodeURIComponent(path)}`);
    assert.deepEqual(await data.json(), { redirect: "/login" }, path);
  }
  const relative = await get("/_volt/data?path=dashboard");
  assert.equal(relative.status, 400);
});

await check("matcher has and missing conditions", async () => {
  assert.equal((await get("/api/flagged", { headers: { "x-flag": "yes" } })).status, 418);
  const plain = await get("/api/flagged", { headers: { "x-flag": "nope" } });
  assert.deepEqual(await plain.json(), { from: "typescript", name: "flagged" });
  assert.equal((await get("/api/guarded")).status, 401);
  const passed = await get("/api/guarded", { headers: { cookie: "pass=1" } });
  assert.deepEqual(await passed.json(), { from: "typescript", name: "guarded" });
});

await check("streamed handler bodies reach the client chunk by chunk", async () => {
  const response = await get("/api/events?count=3&every=300");
  assert.equal(response.headers.get("content-type"), "text/event-stream");
  assert.equal(response.headers.get("content-encoding"), null);
  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  const arrivals = [];
  let text = "";
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    text += decoder.decode(value);
    arrivals.push(performance.now());
  }
  assert.equal(text, "data: 0\n\ndata: 1\n\ndata: 2\n\n");
  assert.ok(arrivals.length >= 3, `${arrivals.length} reads`);
  assert.ok(arrivals.at(-1) - arrivals[0] > 400, `chunks arrived within ${arrivals.at(-1) - arrivals[0]} ms`);
  const sized = await get("/api/hello");
  assert.ok(Number(sized.headers.get("content-length")) > 0);
});

await check("client navigations go through middleware", async () => {
  const context = await browser.newContext();
  const page = await context.newPage();
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.goto(base, { waitUntil: "networkidle" });
  await page.click("a[href='/dashboard']");
  await page.locator("h1", { hasText: "Login" }).waitFor();
  assert.equal(new URL(page.url()).pathname, "/login");
  await page.click("a[href='/rewritten']");
  await page.locator("#from", { hasText: "rewrite" }).waitFor();
  assert.equal(new URL(page.url()).pathname, "/rewritten");
  await context.addCookies([{ name: "session", value: "grace", url: base }]);
  // Route data prefetched before the session existed still holds the redirect.
  await page.reload({ waitUntil: "networkidle" });
  await page.click("a[href='/dashboard']");
  await page.locator("#user", { hasText: "grace" }).waitFor();
  await page.click("a[href='/old-home']");
  await page.locator("h1", { hasText: "Home" }).waitFor();
  assert.equal(new URL(page.url()).pathname, "/");
  assert.deepEqual(errors, []);
  await context.close();
});

await done();
