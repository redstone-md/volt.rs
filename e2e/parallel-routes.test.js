import assert from "node:assert/strict";
import { setup } from "./harness.js";

const { browser, check, done, base } = await setup();
const page = await browser.newPage();
const errors = [];
page.on("pageerror", (error) => errors.push(error.message));
page.on("console", (message) => {
  if (message.type() === "error") errors.push(message.text());
});

const text = (selector) => page.locator(selector).innerText();
const path = () => new URL(page.url()).pathname;

await check("photos open in a modal over the gallery on client navigation", async () => {
  await page.goto(`${base}/photos`, { waitUntil: "networkidle" });
  assert.equal(await page.locator("#modal").count(), 0);
  await page.click("#like");
  await page.click("#photo-2");
  await page.locator("#modal").waitFor();
  assert.equal(path(), "/photos/2");
  assert.equal(await text("#modal-title"), "Photo 2");
  assert.equal(await text("#modal-source"), "modal");
  assert.equal(await page.locator("h1").innerText(), "Photos");
  assert.equal(await text("#like"), "Likes 1", "the gallery stays mounted");
  assert.equal(await page.locator("#photo-page").count(), 0);
  assert.equal(await page.title(), "Photos · parallel-routes");
});

await check("links inside the modal stay intercepted", async () => {
  await page.click("#modal-next");
  await page.waitForURL(`${base}/photos/3`);
  await page.locator("#modal-title", { hasText: "Photo 3" }).waitFor();
  assert.equal(await page.locator("h1").innerText(), "Photos");
});

await check("back closes the modal and forward reopens it", async () => {
  await page.goBack();
  await page.waitForURL(`${base}/photos/2`);
  await page.locator("#modal-title", { hasText: "Photo 2" }).waitFor();
  await page.click("#modal-close");
  await page.waitForURL(`${base}/photos`);
  await page.locator("#modal").waitFor({ state: "detached" });
  assert.equal(await page.locator("h1").innerText(), "Photos");
  await page.goForward();
  await page.waitForURL(`${base}/photos/2`);
  await page.locator("#modal-title", { hasText: "Photo 2" }).waitFor();
  assert.equal(await page.locator("h1").innerText(), "Photos");
});

await check("navigating elsewhere closes the modal through the catch-all slot page", async () => {
  await page.click("#modal-home");
  await page.waitForURL(`${base}/dashboard`);
  await page.locator("h1", { hasText: "Dashboard home" }).waitFor();
  assert.equal(await page.locator("#modal").count(), 0);
});

await check("a refresh renders the full photo page", async () => {
  await page.goto(`${base}/photos`, { waitUntil: "networkidle" });
  await page.click("#photo-4");
  await page.locator("#modal").waitFor();
  await page.reload({ waitUntil: "networkidle" });
  assert.equal(await text("#photo-page"), "Photo 4");
  assert.equal(await text("#photo-source"), "page");
  assert.equal(await page.locator("#modal").count(), 0);
  assert.equal(await page.title(), "Photo 4 · parallel-routes");
});

await check("the modal also intercepts from other pages and from the full photo page", async () => {
  await page.goto(`${base}/photos/4`, { waitUntil: "networkidle" });
  await page.click("a[href='/photos']");
  await page.locator("h1", { hasText: "Photos" }).waitFor();
  assert.equal(await page.locator("#modal").count(), 0);
  await page.click("#photo-5");
  await page.locator("#modal-title", { hasText: "Photo 5" }).waitFor();
});

await check("the data endpoint resolves interceptions only for client navigations", async () => {
  const soft = await page.evaluate(async () => {
    const response = await fetch("/_volt/data?path=%2Fphotos%2F1", { headers: { "x-volt-from": "/photos" } });
    return response.json();
  });
  assert.equal(soft.retain, true);
  assert.equal(soft.slots["/@modal"].route, "/@modal/(.)photos/[id]");
  assert.equal(soft.slots["/@modal"].data.source, "modal");
  const plain = await (await fetch(`${base}/_volt/data?path=%2Fphotos%2F1`)).json();
  assert.equal(plain.route, "/photos/[id]");
  assert.equal(plain.retain, undefined);
  assert.equal(plain.slots["/@modal"].route, "/@modal/[...catchAll]");
});

await check("dashboard layout renders its slots", async () => {
  await page.goto(`${base}/dashboard`, { waitUntil: "networkidle" });
  assert.equal(await text("#children"), "Dashboard home");
  assert.equal(await text("#analytics"), "Analytics overview");
  assert.equal(await text("#team"), "Team overview");
});

await check("slots without a match keep their state on client navigation", async () => {
  await page.click("#to-settings");
  await page.locator("#children", { hasText: "Settings" }).waitFor();
  assert.equal(await text("#team"), "Team settings");
  assert.equal(await text("#analytics"), "Analytics overview");
});

await check("slots without a match render default.tsx on a hard load", async () => {
  await page.reload({ waitUntil: "networkidle" });
  assert.equal(await text("#children"), "Settings");
  assert.equal(await text("#team"), "Team settings");
  assert.equal(await text("#analytics"), "Analytics default");
});

await check("a URL that only a slot matches keeps the page on client navigation", async () => {
  await page.click("#to-overview");
  await page.locator("#children", { hasText: "Dashboard home" }).waitFor();
  await page.click("#to-views");
  await page.locator("#analytics", { hasText: "Analytics views" }).waitFor();
  assert.equal(path(), "/dashboard/views");
  assert.equal(await text("#children"), "Dashboard home");
  assert.equal(await text("#team"), "Team overview");
  await page.reload({ waitUntil: "networkidle" });
  assert.equal(await text("#children"), "Dashboard default");
  assert.equal(await text("#analytics"), "Analytics views");
  assert.equal(await text("#team"), "Team default");
});

await check("a slot without default.tsx makes unmatched hard loads 404", async () => {
  const response = await fetch(`${base}/strict/other`);
  assert.equal(response.status, 404);
  assert.equal((await fetch(`${base}/strict`)).status, 200);
  await page.goto(`${base}/strict`, { waitUntil: "networkidle" });
  assert.equal(await text("#side"), "Side panel");
  await page.click("#to-other");
  await page.locator("#strict-children", { hasText: "Strict other" }).waitFor();
  assert.equal(await text("#side"), "Side panel");
});

await check("an intercepting route outside a slot replaces the page from its segment only", async () => {
  await page.goto(`${base}/feed`, { waitUntil: "networkidle" });
  await page.click("#item-7");
  await page.waitForURL(`${base}/items/7`);
  assert.equal(await text("#item"), "Feed preview of item 7");
  await page.reload({ waitUntil: "networkidle" });
  assert.equal(await text("#item"), "Item 7");
  const direct = await page.evaluate(async () => {
    const response = await fetch("/_volt/data?path=%2Fitems%2F7", { headers: { "x-volt-from": "/" } });
    return response.json();
  });
  assert.equal(direct.route, "/items/[id]");
});

const data = (path, from) =>
  fetch(`${base}/_volt/data?path=${encodeURIComponent(path)}`, { headers: from ? { "x-volt-from": from } : {} }).then(
    (response) => response.json(),
  );

await check("useParams inside an intercepted modal reads the modal's params", async () => {
  await page.goto(`${base}/photos`, { waitUntil: "networkidle" });
  await page.click("#photo-3");
  await page.locator("#modal-title", { hasText: "Photo 3" }).waitFor();
  assert.equal(await text("#modal-param"), "3");
});

await check("back restores an intercepted view after a reload", async () => {
  await page.click("#modal-home");
  await page.waitForURL(`${base}/dashboard`);
  await page.locator("h1", { hasText: "Dashboard home" }).waitFor();
  await page.reload({ waitUntil: "networkidle" });
  await page.goBack();
  await page.waitForURL(`${base}/photos/3`);
  await page.locator("#modal-title", { hasText: "Photo 3" }).waitFor();
  assert.equal(await page.locator("h1").innerText(), "Photos");
  assert.equal(await page.locator("#photo-page").count(), 0);
});

await check("a page wins over a slot page with the same pattern", async () => {
  await page.goto(`${base}/shop/foo`, { waitUntil: "networkidle" });
  assert.equal(await text("#product"), "Product foo");
  assert.equal(await text("#shop-side-item"), "Side for foo");
  await page.click("#to-shop-a");
  await page.locator("#product", { hasText: "Product a" }).waitFor();
  assert.equal(await text("#shop-side-item"), "Side for a");
  await page.click("#to-shop-b");
  await page.locator("#product", { hasText: "Product b" }).waitFor();
  assert.equal(await text("#shop-side-item"), "Side for b");
  const soft = await data("/shop/foo", "/shop/[slug]");
  assert.equal(soft.route, "/shop/[slug]");
  assert.equal(soft.retain, undefined);
});

await check("slot loader failures render the slot's not-found and error files", async () => {
  assert.equal((await fetch(`${base}/err`)).status, 200);
  await page.goto(`${base}/err`, { waitUntil: "networkidle" });
  assert.equal(await text("#err-children"), "Err home");
  assert.equal(await text("#err-side"), "Side missing");
  assert.equal(await text("#err-boom"), "Boom failed: boom");
  const state = await data("/err");
  assert.equal(state.route, "/err");
  assert.equal(state.slots["/err/@side"].error.notFound, true);
  assert.equal(state.slots["/err/@boom"].error.status, 409);
});

await check("a URL that only a slot matches needs no default.tsx on client navigation", async () => {
  assert.equal((await fetch(`${base}/strict/extra`)).status, 404);
  await page.goto(`${base}/strict`, { waitUntil: "networkidle" });
  await page.click("#to-extra");
  await page.locator("#side", { hasText: "Side extra" }).waitFor();
  assert.equal(path(), "/strict/extra");
  assert.equal(await text("#strict-children"), "Strict home");
});

await check("slots of layouts the client does not show yet resolve as on a hard load", async () => {
  assert.equal((await data("/strict/other", "/photos")).route, "/_not-found");
  assert.equal((await data("/strict/extra", "/photos")).route, "/_not-found");
  assert.equal((await data("/strict/other", "/strict")).route, "/strict/other");
  assert.equal((await data("/no/such/page", "/photos")).route, "/_not-found", "catch-all slot pages create no URLs");
});

await check("no client errors", async () => {
  assert.deepEqual(errors, []);
});

await done();
