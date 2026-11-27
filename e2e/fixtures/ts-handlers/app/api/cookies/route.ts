import { type NextRequest, NextResponse } from "next/server";

export function GET(request: NextRequest) {
  const response = NextResponse.json({ theme: request.cookies.get("theme")?.value ?? null });
  response.cookies.set("visited", "1", { httpOnly: true, sameSite: "lax" });
  return response;
}
