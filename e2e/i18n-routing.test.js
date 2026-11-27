import assert from "node:assert/strict";
import { setup } from "./harness.js";

const { browser, check, done, base } = await setup();
const errors = [];
const get = (path, headers = {}) => fetch(`${base}${path}`, { redirect: "manual", headers });
const watch = (page) => {
  page.on("pageerror", (error) => errors.push(error.message));
  const documents = [];
  page.on("request", (request) => request.resourceType() === "document" && documents.push(request.url()));
  return documents;
};

await check("the root redirects to the Accept-Language locale", async () => {
  const response = await get("/", { "accept-language": "fr;q=0.9, de-AT;q=0.8, en;q=0.1" });
  assert.equal(response.status, 307);
  assert.equal(response.headers.get("location"), "/de");
  assert.match(response.headers.get("vary") ?? "", /Accept-Language/);
  assert.equal((await get("/", { "accept-language": "en-US,en;q=0.9" })).status, 200);
  assert.equal((await get("/about", { "accept-language": "de" })).status, 200, "only the root detects");
});

await check("a locale cookie overrides Accept-Language", async () => {
  assert.equal((await get("/", { "accept-language": "de", cookie: "VOLT_LOCALE=en" })).status, 200);
  const next = await get("/", { "accept-language": "en", cookie: "NEXT_LOCALE=de" });
  assert.equal(next.status, 307);
  assert.equal(next.headers.get("location"), "/de");
});

await check("the default locale prefix redirects to the unprefixed URL", async () => {
  const response = await get("/en/about?x=1");
  assert.equal(response.status, 308);
  assert.equal(response.headers.get("location"), "/about?x=1");
  assert.equal((await get("/DE/about")).headers.get("location"), "/de/about");
});

await check("the default locale prefix never redirects to another host", async () => {
  for (const [path, location] of [
    ["/en//evil.com", "/evil.com"],
    ["/EN//evil.com/x?y=1", "/evil.com/x?y=1"],
    ["/en/\\evil.com", "/evil.com"],
    ["/de//evil.com", "/de/evil.com"],
  ]) {
    const response = await get(path);
    assert.equal(response.status, 308, path);
    assert.equal(response.headers.get("location"), location, path);
  }
});

await check("redirects to API routes and public files keep their path", async () => {
  for (const prefix of ["", "/de"]) {
    assert.equal((await get(`${prefix}/to-file`)).headers.get("location"), "/file.txt", prefix);
    assert.equal((await get(`${prefix}/to-api`)).headers.get("location"), "/api/ping?x=1", prefix);
  }
  assert.equal(await (await get("/file.txt")).text(), "a public file\n");
  assert.equal(await (await get("/de/file.txt")).text(), "a public file\n", "localized links still resolve");
  assert.equal(await (await get("/de/api/ping")).text(), "pong:de");
  assert.equal(await (await get("/api/ping")).text(), "pong:en");
});

await check("[[redirects]] apply below every locale prefix unless locale = false", async () => {
  const plain = await get("/legacy-old");
  assert.equal(plain.status, 307);
  assert.equal(plain.headers.get("location"), "/about");
  assert.equal((await get("/de/legacy-old")).headers.get("location"), "/de/about");
  assert.equal((await get("/en/legacy-old")).headers.get("location"), "/about");
  assert.equal((await get("/de/full-path")).headers.get("location"), "/blog/hello");
  assert.equal((await get("/full-path")).status, 404);
});

await check("prefixed routes render with the locale on the server", async () => {
  const html = await (await get("/de/about")).text();
  assert.match(html, /<html lang="de"/);
  assert.match(html, /Hallo aus Rust/);
  assert.match(html, /id="loader-locale">de</);
  const en = await (await get("/about")).text();
  assert.match(en, /<html lang="en"/);
  assert.match(en, /Hello from Rust/);
  const post = await (await get("/de/blog/hello")).text();
  assert.match(post, /id="locale">de</);
  assert.equal((await get("/de/missing")).status, 404);
});

await check("loader redirects keep the locale", async () => {
  const response = await get("/de/old");
  assert.ok(response.status >= 300 && response.status < 400, `status ${response.status}`);
  assert.equal(response.headers.get("location"), "/de/about");
});

await check("the data endpoint accepts prefixed paths", async () => {
  const state = await (await get(`/_volt/data?path=${encodeURIComponent("/de/about")}`)).json();
  assert.equal(state.locale, "de");
  assert.equal(state.pathname, "/about");
  assert.equal(state.data.greeting, "Hallo aus Rust");
  const redirect = await (await get(`/_volt/data?path=${encodeURIComponent("/de/old")}`)).json();
  assert.equal(redirect.redirect, "/de/about");
});

await check("hreflang alternates for every locale, x-default and metadata languages", async () => {
  const page = await browser.newPage();
  watch(page);
  await page.goto(`${base}/de/about`, { waitUntil: "networkidle" });
  const links = await page.$$eval('head link[rel="alternate"][hreflang]', (nodes) =>
    Object.fromEntries(nodes.map((n) => [n.getAttribute("hreflang"), n.getAttribute("href")])),
  );
  assert.deepEqual(links, {
    en: "https://example.com/about",
    de: "https://example.com/de/about",
    "x-default": "https://example.com/about",
    fr: "https://fr.example.com/",
  });
  await page.close();
});

await check("Link keeps the current locale and switches locales on the client", async () => {
  const context = await browser.newContext({ locale: "en-US" });
  const page = await context.newPage();
  const documents = watch(page);
  await page.goto(`${base}/about`, { waitUntil: "networkidle" });
  assert.equal(await page.getAttribute("#to-de", "href"), "/de/about");
  assert.equal(await page.getAttribute("#raw", "href"), "/about");
  const loads = documents.length;
  await page.click("#to-de");
  await page.locator("h1", { hasText: "Hallo aus Rust" }).waitFor();
  assert.equal(new URL(page.url()).pathname, "/de/about");
  assert.equal(await page.getAttribute("html", "lang"), "de");
  assert.equal(await page.textContent("#locale"), "de");
  assert.equal(await page.getAttribute("#home", "href"), "/de");
  assert.equal(await page.getAttribute("#raw", "href"), "/about");
  await page.waitForFunction(() =>
    document.querySelector('link[rel="alternate"][hreflang="de"]')?.getAttribute("href") === "https://example.com/de/about",
  );
  await page.click("#home");
  await page.locator("h1", { hasText: "Willkommen" }).waitFor();
  assert.equal(new URL(page.url()).pathname, "/de");
  await page.click("#to-en");
  await page.locator("h1", { hasText: "Welcome" }).waitFor();
  assert.equal(new URL(page.url()).pathname, "/");
  assert.equal(await page.getAttribute("html", "lang"), "en");
  assert.equal(documents.length, loads, "no full document load");
  const cookies = await context.cookies();
  assert.equal(cookies.find((c) => c.name === "VOLT_LOCALE")?.value, "en");
  await context.close();
});

await check("router.push takes a locale and actions see the caller's locale", async () => {
  const page = await browser.newPage();
  watch(page);
  await page.goto(`${base}/de`, { waitUntil: "networkidle" });
  assert.equal(await page.textContent("#locales"), "en,de|en");
  await page.click("#echo");
  await page.locator("#echoed", { hasText: "de:hi" }).waitFor();
  await page.goto(`${base}/`, { waitUntil: "networkidle" });
  await page.click("#echo");
  await page.locator("#echoed", { hasText: "en:hi" }).waitFor();
  await page.click("#push-de");
  await page.locator("h1", { hasText: "Hallo aus Rust" }).waitFor();
  assert.equal(new URL(page.url()).pathname, "/de/about");
  await page.close();
});

await check("the browser lands on its language until a locale is chosen", async () => {
  const context = await browser.newContext({ locale: "de-DE" });
  const page = await context.newPage();
  watch(page);
  await page.goto(base, { waitUntil: "networkidle" });
  assert.equal(new URL(page.url()).pathname, "/de");
  await page.locator("h1", { hasText: "Willkommen" }).waitFor();
  await page.click("#to-en");
  await page.locator("h1", { hasText: "Welcome" }).waitFor();
  await page.goto(base, { waitUntil: "networkidle" });
  assert.equal(new URL(page.url()).pathname, "/");
  assert.equal(await page.textContent("h1"), "Welcome");
  await context.close();
});

await check("Pages Router pages get the locale in data functions and useRouter", async () => {
  const page = await browser.newPage();
  watch(page);
  await page.goto(`${base}/de/legacy`, { waitUntil: "networkidle" });
  assert.equal(await page.textContent("#server-locale"), "de");
  assert.equal(await page.textContent("#router-locale"), "de|en,de|en");
  assert.equal(await page.getAttribute("html", "lang"), "de");
  assert.equal(await page.getAttribute('link[rel="alternate"][hreflang="en"]', "href"), "https://example.com/legacy");
  await page.close();
});

await check("sitemap.xml lists every locale variant with alternates", async () => {
  const xml = await (await get("/sitemap.xml")).text();
  assert.match(xml, /xmlns:xhtml="http:\/\/www\.w3\.org\/1999\/xhtml"/);
  for (const loc of ["https://example.com/", "https://example.com/de", "https://example.com/about", "https://example.com/de/about", "https://example.com/de/blog/hello", "https://example.com/de/blog/from-rust"]) {
    assert.ok(xml.includes(`<loc>${loc}</loc>`), loc);
  }
  assert.ok(xml.includes('<xhtml:link rel="alternate" hreflang="de" href="https://example.com/de/about"/>'));
  assert.ok(xml.includes('<xhtml:link rel="alternate" hreflang="x-default" href="https://example.com/about"/>'));
});

await check("no page errors", async () => {
  assert.deepEqual(errors, []);
});

await done();
