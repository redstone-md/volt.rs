import { type NextRequest, NextResponse } from "next/server";

// Written as for Next.js: `nextUrl.pathname` leaves the base path out, `clone()` keeps it.
export function middleware(request: NextRequest) {
  if (request.nextUrl.pathname.startsWith("/dashboard") && !request.cookies.get("session")) {
    const login = request.nextUrl.clone();
    login.pathname = "/login";
    return NextResponse.redirect(login);
  }
  const response = NextResponse.next();
  response.headers.set("x-seen-path", request.nextUrl.pathname);
  return response;
}

export const config = { matcher: ["/dashboard/:path*", "/api/:path*"] };
