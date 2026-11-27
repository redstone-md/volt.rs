export async function POST(request: Request) {
  const text = await request.text();
  return new Response(`upstream got ${text}`, { headers: { "x-upstream": request.headers.get("x-token") ?? "none" } });
}
