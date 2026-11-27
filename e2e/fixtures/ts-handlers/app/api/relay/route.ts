import type { NextRequest } from "next/server";

/** Calls another handler of this app over HTTP with the server-side `fetch`. */
export async function GET(request: NextRequest) {
  const response = await fetch(new URL("/api/upstream", request.nextUrl.origin), {
    method: "POST",
    body: "hello",
    headers: { "x-token": "secret" },
  });
  return Response.json({
    status: response.status,
    body: await response.text(),
    header: response.headers.get("x-upstream"),
  });
}
