import { type NextRequest, NextResponse } from "next/server";

export function GET(request: NextRequest) {
  return NextResponse.json({ hello: request.nextUrl.searchParams.get("name") ?? "world" });
}

export async function POST(request: Request) {
  const body = await request.json();
  return Response.json({ received: body, type: request.headers.get("content-type") }, { status: 201 });
}
