import { NextRequest, NextResponse } from "../server.js";
import { isNotFound, readRedirect } from "./errors.js";

const METHODS = ["GET", "HEAD", "POST", "PUT", "PATCH", "DELETE", "OPTIONS"];

/** Methods a route module answers: its exports, `HEAD` with `GET`, and `OPTIONS` always. */
function allowed(module) {
  const own = METHODS.filter((method) => typeof module[method] === "function");
  if (own.includes("GET") && !own.includes("HEAD")) own.push("HEAD");
  if (!own.includes("OPTIONS")) own.push("OPTIONS");
  return own;
}

/** Writes a `Response` to the engine sink: the head as JSON, then the body chunks as bytes. */
async function send(response, sink) {
  const headers = [...response.headers];
  if (!sink.shell(JSON.stringify({ status: response.status, headers }))) return "";
  const body = response.body;
  if (!body) return "";
  const encoder = new TextEncoder();
  for await (const chunk of body) {
    const bytes = typeof chunk === "string" ? encoder.encode(chunk) : chunk;
    if (bytes.byteLength && !sink.write(bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes))) break;
  }
  return "";
}

/** `redirect()` and `notFound()` thrown by a handler become their responses, as in Next.js. */
function thrown(error) {
  const redirect = readRedirect(error);
  if (redirect) return new Response(null, { status: redirect.status, headers: { location: redirect.location } });
  if (isNotFound(error)) return new Response("Not Found", { status: 404 });
  throw error;
}

/** The request of a handler or middleware call, from the JSON the server sends. */
function request(input, body) {
  const init = { method: input.method, headers: input.headers };
  if (input.method !== "GET" && input.method !== "HEAD" && body?.length) init.body = body;
  return new NextRequest(input.url, init);
}

/** Runs the `route.ts` handler of `input.route` for one request. */
export async function handle(handlers, payload, sink) {
  const input = JSON.parse(payload);
  const module = handlers[input.route];
  if (!module) throw new Error(`no route handler for ${input.route}`);
  const methods = allowed(module);
  const handler = module[input.method] ?? (input.method === "HEAD" ? module.GET : undefined);
  if (!handler) {
    const status = input.method === "OPTIONS" ? 204 : 405;
    return send(new Response(null, { status, headers: { allow: methods.join(", ") } }), sink);
  }
  const params = input.params ?? {};
  // Next.js 15 passes `params` as a promise; earlier versions as an object. This serves both.
  const context = { params: Object.assign(Promise.resolve(params), params) };
  let response;
  try {
    response = await handler(request(input, sink.body), context);
  } catch (error) {
    response = thrown(error);
  }
  if (!(response instanceof Response)) {
    throw new TypeError(`${input.method} ${input.route} did not return a Response`);
  }
  return send(response, sink);
}

/** Runs `middleware.ts` for one request; a matcher JavaScript must evaluate is tested first. */
export async function middleware(module, payload, sink) {
  const input = JSON.parse(payload);
  if (input.matchers?.length && !input.matchers.some((source) => new RegExp(source).test(input.path))) {
    return send(NextResponse.next(), sink);
  }
  const run = module.middleware ?? module.proxy ?? module.default;
  if (typeof run !== "function") throw new Error("middleware.ts must export a `middleware` function");
  let response;
  try {
    response = await run(request(input, sink.body), { waitUntil() {} });
  } catch (error) {
    response = thrown(error);
  }
  return send(response instanceof Response ? response : NextResponse.next(), sink);
}

/** Route segment config of every handler, read by `volt build`. */
export function configs(handlers) {
  return Object.entries(handlers).map(([id, module]) => ({
    id,
    methods: allowed(module),
    dynamic: typeof module.dynamic === "string" ? module.dynamic : "auto",
    revalidate: typeof module.revalidate === "number" ? module.revalidate : null,
  }));
}
