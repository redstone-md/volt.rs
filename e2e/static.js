// Tiny static file server for exported sites: tries `path`, `path.html` and `path/index.html`, then 404.html.
import { existsSync, readFileSync, statSync } from "node:fs";
import { createServer } from "node:http";
import { extname, join, normalize } from "node:path";

const TYPES = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".json": "application/json",
  ".xml": "application/xml",
  ".txt": "text/plain; charset=utf-8",
};

/** Serves `root` under `basePath` and resolves with `{ url, close, requests }`. */
export function serveStatic(root, port, basePath = "") {
  const requests = [];
  const server = createServer((request, response) => {
    const pathname = decodeURIComponent(new URL(request.url, "http://x").pathname);
    requests.push(pathname);
    const send = (status, file) => {
      response.writeHead(status, { "content-type": TYPES[extname(file)] ?? "application/octet-stream" });
      response.end(readFileSync(file));
    };
    if (pathname !== basePath && !pathname.startsWith(`${basePath}/`)) return send(404, join(root, "404.html"));
    const relative = normalize(pathname.slice(basePath.length)).replace(/^(\.\.[/\\])+/, "");
    for (const candidate of [relative, `${relative}.html`, join(relative, "index.html")]) {
      const file = join(root, candidate);
      if (existsSync(file) && statSync(file).isFile()) return send(200, file);
    }
    send(404, join(root, "404.html"));
  });
  return new Promise((resolve) =>
    server.listen(port, () => resolve({ url: `http://localhost:${port}${basePath}`, close: () => server.close(), requests })),
  );
}
