export { connection } from "./navigation";

/** A `URL` with `clone()`, as `request.nextUrl`. */
export declare class NextURL extends URL {
  clone(): NextURL;
}

export interface RequestCookie {
  name: string;
  value: string;
}

export interface ResponseCookie extends RequestCookie {
  domain?: string;
  path?: string;
  expires?: Date | number;
  maxAge?: number;
  httpOnly?: boolean;
  secure?: boolean;
  partitioned?: boolean;
  sameSite?: boolean | "lax" | "strict" | "none";
  priority?: "low" | "medium" | "high";
}

/** `request.cookies`: the `Cookie` header of the request. */
export declare class RequestCookies implements Iterable<[string, RequestCookie]> {
  constructor(headers: Headers);
  readonly size: number;
  get(name: string | RequestCookie): RequestCookie | undefined;
  getAll(name?: string | RequestCookie): RequestCookie[];
  has(name: string): boolean;
  set(name: string, value: string): this;
  set(cookie: RequestCookie): this;
  delete(name: string): boolean;
  delete(names: string[]): boolean[];
  clear(): this;
  [Symbol.iterator](): Iterator<[string, RequestCookie]>;
  toString(): string;
}

/** `response.cookies`: the `Set-Cookie` headers of the response. */
export declare class ResponseCookies {
  constructor(headers: Headers);
  get(name: string | ResponseCookie): ResponseCookie | undefined;
  getAll(name?: string | ResponseCookie): ResponseCookie[];
  has(name: string): boolean;
  set(name: string, value: string, options?: Omit<ResponseCookie, "name" | "value">): this;
  set(cookie: ResponseCookie): this;
  delete(name: string | Omit<ResponseCookie, "value">): this;
  toString(): string;
}

/** The request of route handlers and middleware. */
export declare class NextRequest extends Request {
  constructor(input: URL | RequestInfo, init?: RequestInit);
  readonly nextUrl: NextURL;
  readonly cookies: RequestCookies;
}

export interface MiddlewareResponseInit extends ResponseInit {
  /** Replaces the headers of the request the rest of the app sees. */
  request?: { headers?: HeadersInit };
}

/** A `Response` with `cookies`, plus the middleware results `next()` and `rewrite()`. */
export declare class NextResponse<Body = unknown> extends Response {
  constructor(body?: BodyInit | null, init?: ResponseInit);
  readonly cookies: ResponseCookies;
  static json<JsonBody>(body: JsonBody, init?: ResponseInit): NextResponse<JsonBody>;
  /** Redirects with 307 unless another status is given. */
  static redirect(url: string | URL, init?: number | ResponseInit): NextResponse;
  /** Serves another path of the app (or proxies another origin) under the requested URL. */
  static rewrite(destination: string | URL, init?: MiddlewareResponseInit): NextResponse;
  /** Lets the request continue, with the response headers and cookies set on the result. */
  static next(init?: MiddlewareResponseInit): NextResponse;
}

export interface UserAgent {
  ua: string;
  isBot: boolean;
  browser: { name?: string; version?: string };
  os: { name?: string; version?: string };
  device: { type?: string; model?: string; vendor?: string };
  engine: { name?: string; version?: string };
  cpu: { architecture?: string };
}

export declare function userAgent(request: { headers: Headers }): UserAgent;

/** What middleware may return; nothing continues the request. */
export type NextMiddlewareResult = Response | NextResponse | null | undefined | void;

export interface NextFetchEvent {
  /** Accepted for compatibility; the promise is not awaited after the response. */
  waitUntil(promise: Promise<unknown>): void;
}

export type NextMiddleware = (
  request: NextRequest,
  event: NextFetchEvent,
) => NextMiddlewareResult | Promise<NextMiddlewareResult>;

/** `export const config` of `middleware.ts`. */
export interface MiddlewareConfig {
  matcher?: string | Array<string | { source: string }>;
}
