import { type NextRequest, NextResponse } from "next/server";

export function GET(request: NextRequest) {
  const { pathname, basePath, href } = request.nextUrl;
  const moved = request.nextUrl.clone();
  moved.pathname = "/elsewhere";
  return NextResponse.json({ pathname, basePath, href, moved: moved.href, url: request.url });
}
