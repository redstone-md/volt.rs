import { type NextRequest, NextResponse } from "next/server";

export function middleware(request: NextRequest) {
  const { pathname } = request.nextUrl;
  if (pathname === "/old-home") return NextResponse.redirect(new URL("/", request.url));
  if (pathname.startsWith("/dashboard") && !request.cookies.get("session")) {
    return NextResponse.redirect(new URL("/login", request.url));
  }
  if (pathname === "/rewritten") return NextResponse.rewrite(new URL("/about?from=rewrite", request.url));
  if (pathname === "/blocked") return new Response("blocked by middleware", { status: 403 });
  const headers = new Headers(request.headers);
  headers.set("x-user", request.cookies.get("session")?.value ?? "guest");
  const response = NextResponse.next({ request: { headers } });
  response.headers.set("x-middleware-ran", "1");
  response.cookies.set("seen", "yes", { path: "/" });
  return response;
}

export const config = {
  // The last pattern needs a lookahead, which QuickJS evaluates; the others match in Rust.
  matcher: ["/dashboard/:path*", "/old-home", "/rewritten", "/blocked", "/((?!api|login).*)"],
};
