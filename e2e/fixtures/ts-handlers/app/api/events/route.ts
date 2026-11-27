// Server-sent events: each chunk must reach the client when it is written, not when the stream ends.
export function GET(request: Request) {
  const query = new URL(request.url).searchParams;
  const count = Number(query.get("count") ?? 3);
  const every = Number(query.get("every") ?? 300);
  const encoder = new TextEncoder();
  let sent = 0;
  const stream = new ReadableStream({
    start(controller) {
      const tick = () => {
        controller.enqueue(encoder.encode(`data: ${sent++}\n\n`));
        if (sent === count) controller.close();
        else setTimeout(tick, every);
      };
      tick();
    },
  });
  return new Response(stream, { headers: { "content-type": "text/event-stream", "cache-control": "no-cache" } });
}
