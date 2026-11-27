import { type NextRequest, NextResponse } from "next/server";

export function middleware(request: NextRequest) {
  const { pathname } = request.nextUrl;
  if (pathname === "/old-home") return NextResponse.redirect(new URL("/", request.url));
  if (pathname.startsWith("/dashboard") && !request.cookies.get("session")) {
    return NextResponse.redirect(new URL("/login", request.url));
  }
  if (pathname === "/rewritten") return NextResponse.rewrite(new URL("/about?from=rewrite", request.url));
  if (pathname === "/blocked") return new Response("blocked by middleware", { status: 403 });
  if (pathname === "/api/flagged") return new Response("flagged", { status: 418 });
  if (pathname === "/api/guarded") return new Response("no pass", { status: 401 });
  const headers = new Headers(request.headers);
  headers.set("x-user", request.cookies.get("session")?.value ?? "guest");
  const response = NextResponse.next({ request: { headers } });
  response.headers.set("x-middleware-ran", "1");
  response.cookies.set("seen", "yes", { path: "/" });
  return response;
}

export const config = {
  // The lookahead pattern is evaluated by QuickJS; the others match in Rust, conditions included.
  matcher: [
    "/dashboard/:path*",
    "/old-home",
    "/rewritten",
    "/blocked",
    "/((?!api|login).*)",
    { source: "/api/flagged", has: [{ type: "header", key: "x-flag", value: "on|yes" }] },
    { source: "/api/guarded", missing: [{ type: "cookie", key: "pass" }] },
  ],
};
