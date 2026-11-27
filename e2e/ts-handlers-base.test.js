import assert from "node:assert/strict";
import { setup } from "./harness.js";

const { check, done, base } = await setup();
const get = (path, init) => fetch(base + path, { redirect: "manual", ...init });

await check("nextUrl.pathname leaves the base path out, href keeps it", async () => {
  const response = await get("/app/api/url?x=1");
  assert.equal(response.headers.get("x-seen-path"), "/api/url");
  assert.deepEqual(await response.json(), {
    pathname: "/api/url",
    basePath: "/app",
    href: `${base}/app/api/url?x=1`,
    moved: `${base}/app/elsewhere?x=1`,
    url: `${base}/app/api/url?x=1`,
  });
});

await check("middleware written for Next.js guards pages under the base path", async () => {
  const guarded = await get("/app/dashboard");
  assert.equal(guarded.status, 307);
  assert.equal(guarded.headers.get("location"), `${base}/app/login`);
  const encoded = await get("/app/dash%62oard");
  assert.equal(encoded.status, 307);
  const allowed = await get("/app/dashboard", { headers: { cookie: "session=1" } });
  assert.equal(allowed.status, 200);
  assert.match(await allowed.text(), /<h1>Dashboard<\/h1>/);
  const data = await get("/app/_volt/data?path=/dashboard");
  assert.deepEqual(await data.json(), { redirect: "/login" });
});

await done();
