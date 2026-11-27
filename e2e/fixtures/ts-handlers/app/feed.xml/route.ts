export const dynamic = "force-static";

/** Built once by `volt build`: every request gets the same timestamp. */
export function GET() {
  const xml = `<?xml version="1.0"?><feed><built>${Date.now()}-${Math.random()}</built></feed>`;
  return new Response(xml, { headers: { "content-type": "application/xml" } });
}
