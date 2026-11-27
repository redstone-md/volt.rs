// `next/server`: the request and response types of route handlers and middleware.
export { connection } from "./navigation.js";

/**
 * `request.nextUrl`: a `URL` whose `pathname` leaves out the `basePath`, which `href` and `toString()` keep, as in
 * Next.js. Setting `pathname` keeps the base path; setting `basePath` moves the path below another one.
 */
export class NextURL extends URL {
  constructor(input, base, options) {
    const named = base !== undefined && typeof base === "object" && !(base instanceof URL);
    super(input, named ? undefined : base);
    const basePath = ((named ? base : options)?.basePath ?? "").replace(/\/$/, "");
    const full = super.pathname;
    this._basePath = basePath && (full === basePath || full.startsWith(`${basePath}/`)) ? basePath : "";
  }
  get basePath() {
    return this._basePath;
  }
  set basePath(value) {
    const pathname = this.pathname;
    this._basePath = String(value).replace(/\/$/, "");
    this.pathname = pathname;
  }
  get pathname() {
    const full = super.pathname;
    return this._basePath ? full.slice(this._basePath.length) || "/" : full;
  }
  set pathname(value) {
    const path = String(value);
    const rest = path === "/" && this._basePath ? "" : path.startsWith("/") ? path : `/${path}`;
    super.pathname = `${this._basePath}${rest}`;
  }
  clone() {
    return new NextURL(this.href, { basePath: this._basePath });
  }
}

const parseCookies = (header) => {
  const cookies = new Map();
  for (const pair of (header ?? "").split(";")) {
    const index = pair.indexOf("=");
    if (index < 0) continue;
    const name = pair.slice(0, index).trim();
    if (!name || cookies.has(name)) continue;
    let value = pair.slice(index + 1).trim();
    try {
      value = decodeURIComponent(value);
    } catch {}
    cookies.set(name, { name, value });
  }
  return cookies;
};

/** `request.cookies`: the `Cookie` header, kept in sync when changed. */
export class RequestCookies {
  constructor(headers) {
    this._headers = headers;
    this._cookies = parseCookies(headers.get("cookie"));
  }
  _write() {
    const pairs = [...this._cookies.values()].map(({ name, value }) => `${name}=${encodeURIComponent(value)}`);
    if (pairs.length) this._headers.set("cookie", pairs.join("; "));
    else this._headers.delete("cookie");
  }
  get size() {
    return this._cookies.size;
  }
  get(name) {
    return this._cookies.get(typeof name === "string" ? name : name?.name);
  }
  getAll(name) {
    const all = [...this._cookies.values()];
    return name === undefined ? all : all.filter((cookie) => cookie.name === (name?.name ?? name));
  }
  has(name) {
    return this._cookies.has(name);
  }
  set(name, value) {
    const cookie = typeof name === "object" ? name : { name, value };
    this._cookies.set(cookie.name, { name: cookie.name, value: String(cookie.value) });
    this._write();
    return this;
  }
  delete(names) {
    const list = Array.isArray(names) ? names : [names];
    const removed = list.map((name) => this._cookies.delete(name));
    this._write();
    return Array.isArray(names) ? removed : removed[0];
  }
  clear() {
    this._cookies.clear();
    this._write();
    return this;
  }
  [Symbol.iterator]() {
    return [...this._cookies.entries()][Symbol.iterator]();
  }
  toString() {
    return this._headers.get("cookie") ?? "";
  }
}

const serializeCookie = ({ name, value, ...options }) => {
  let text = `${name}=${encodeURIComponent(value ?? "")}`;
  if (options.domain) text += `; Domain=${options.domain}`;
  text += `; Path=${options.path ?? "/"}`;
  if (options.expires !== undefined) {
    const expires = options.expires instanceof Date ? options.expires : new Date(options.expires);
    text += `; Expires=${expires.toUTCString()}`;
  }
  if (typeof options.maxAge === "number") text += `; Max-Age=${Math.floor(options.maxAge)}`;
  if (options.httpOnly) text += "; HttpOnly";
  if (options.secure) text += "; Secure";
  if (options.partitioned) text += "; Partitioned";
  if (options.sameSite) {
    const sameSite = options.sameSite === true ? "strict" : String(options.sameSite).toLowerCase();
    text += `; SameSite=${sameSite[0].toUpperCase()}${sameSite.slice(1)}`;
  }
  if (options.priority) text += `; Priority=${options.priority}`;
  return text;
};

/** `response.cookies`: `Set-Cookie` headers of the response. */
export class ResponseCookies {
  constructor(headers) {
    this._headers = headers;
    this._cookies = new Map();
  }
  _write() {
    this._headers.delete("set-cookie");
    for (const cookie of this._cookies.values()) this._headers.append("set-cookie", serializeCookie(cookie));
  }
  get(name) {
    return this._cookies.get(typeof name === "string" ? name : name?.name);
  }
  getAll(name) {
    const all = [...this._cookies.values()];
    return name === undefined ? all : all.filter((cookie) => cookie.name === (name?.name ?? name));
  }
  has(name) {
    return this._cookies.has(name);
  }
  set(name, value, options) {
    const cookie = typeof name === "object" ? { ...name } : { ...options, name, value };
    cookie.value = String(cookie.value ?? "");
    this._cookies.set(cookie.name, cookie);
    this._write();
    return this;
  }
  delete(name) {
    const options = typeof name === "object" ? name : { name };
    return this.set({ ...options, value: "", expires: new Date(0) });
  }
  toString() {
    return this._headers.getSetCookie().join("; ");
  }
}

/** The request of route handlers and middleware, with `nextUrl` and `cookies`. */
export class NextRequest extends Request {
  constructor(input, init = {}) {
    super(input, init);
    this.nextUrl = new NextURL(this.url, { basePath: init.nextConfig?.basePath });
    this.cookies = new RequestCookies(this.headers);
  }
}

/** Request header overrides of `NextResponse.next({ request: { headers } })`, as Next.js encodes them. */
function overrideRequest(headers, request) {
  if (!request?.headers) return;
  const replaced = new Headers(request.headers);
  const names = [];
  for (const [name, value] of replaced) {
    headers.set(`x-middleware-request-${name}`, value);
    names.push(name);
  }
  headers.set("x-middleware-override-headers", names.join(","));
}

/** A `Response` with `cookies`, and the middleware results `next()` and `rewrite()`. */
export class NextResponse extends Response {
  constructor(body, init = {}) {
    super(body, init);
    this.cookies = new ResponseCookies(this.headers);
  }
  static json(body, init = {}) {
    const headers = new Headers(init.headers);
    if (!headers.has("content-type")) headers.set("content-type", "application/json");
    // A string body, unlike `Response.json(...).body`, is known up front, so the server sends it with a length.
    return new NextResponse(JSON.stringify(body), { ...init, headers });
  }
  static redirect(url, init) {
    const status = typeof init === "number" ? init : (init?.status ?? 307);
    const headers = new Headers(typeof init === "object" ? init.headers : undefined);
    headers.set("location", String(url));
    return new NextResponse(null, { ...(typeof init === "object" ? init : {}), status, headers });
  }
  static rewrite(destination, init = {}) {
    const headers = new Headers(init.headers);
    headers.set("x-middleware-rewrite", String(destination));
    overrideRequest(headers, init.request);
    return new NextResponse(null, { ...init, headers });
  }
  static next(init = {}) {
    const headers = new Headers(init.headers);
    headers.set("x-middleware-next", "1");
    overrideRequest(headers, init.request);
    return new NextResponse(null, { ...init, headers });
  }
}

/** Parses a `User-Agent` header into the fields Next.js reports. */
export function userAgent({ headers }) {
  const ua = headers.get("user-agent") ?? "";
  const pick = (pairs) => pairs.find(([pattern]) => pattern.test(ua))?.[1];
  return {
    ua,
    isBot: /bot|crawler|spider|crawling/i.test(ua),
    browser: { name: pick([[/edg\//i, "Edge"], [/chrome\//i, "Chrome"], [/firefox\//i, "Firefox"], [/safari\//i, "Safari"]]) },
    os: { name: pick([[/windows/i, "Windows"], [/android/i, "Android"], [/iphone|ipad/i, "iOS"], [/mac os/i, "Mac OS"], [/linux/i, "Linux"]]) },
    device: { type: /mobile/i.test(ua) ? "mobile" : /tablet|ipad/i.test(ua) ? "tablet" : undefined },
    engine: {},
    cpu: {},
  };
}
