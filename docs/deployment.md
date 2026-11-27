# Deployment

`volt build` produces everything the server needs:

```
.volt/
├── manifest.json        routes, chunks, build id
├── server/server-*.js   React render bundle for QuickJS
├── static/              hashed JS, CSS and media (+ .br and .gz)
├── prerender/           pages rendered at build time (.html) and their route data (.json)
└── types/               generated TypeScript client
target/release/<app>     the Rust server binary (default template)
```

The server is a single native binary. It needs the `.volt/` directory and `public/`, nothing else: no Node.js at runtime.

## Static hosting

Apps without Rust loaders, actions or API routes can run without any server: `volt export` writes plain files to `out/` (see [static generation](static-generation.md#static-export)). Upload the folder to any static host (S3, GitHub Pages, Netlify, nginx), serve `404.html` for missing files and, with the default `trailing_slash = false`, map `/about` to `about.html` (most hosts do this as "clean URLs"). With `[build] base_path = "/docs"`, upload the contents of `out/` to `/docs`.

## Docker

```dockerfile
FROM rust:1-bookworm AS build
RUN curl -fsSL https://deb.nodesource.com/setup_22.x | bash - && apt-get install -y nodejs
WORKDIR /app
COPY . .
RUN npm ci && npx volt build

FROM debian:bookworm-slim
WORKDIR /app
COPY --from=build /app/target/release/my-app /app/server
COPY --from=build /app/.volt /app/.volt
COPY --from=build /app/public /app/public
COPY --from=build /app/volt.toml /app/volt.toml
ENV PORT=3000 VOLT_ROOT=/app
EXPOSE 3000
CMD ["/app/server"]
```

For a `web` (React-only) app, copy the `volt` binary from `node_modules/@volt-rs/cli-linux-x64/bin/volt` and run `volt start`.

## systemd

```ini
[Service]
WorkingDirectory=/srv/my-app
Environment=PORT=3000
ExecStart=/srv/my-app/server
Restart=always
```

## Behind a reverse proxy

volt speaks HTTP/1.1 and HTTP/2 (h2c). Put TLS in front with nginx, Caddy or a load balancer. Forward `Host` (or `X-Forwarded-Host`): server actions compare it with the request `Origin` to block cross-site calls.

Dynamic pages stream with chunked transfer encoding. Proxies that buffer responses hold the shell back until the page is complete: streamed pages carry `X-Accel-Buffering: no`, which nginx honours; turn buffering off in other proxies.

## Caching

| Response | Cache-Control |
| --- | --- |
| `/_volt/static/*` | `public, max-age=31536000, immutable` |
| prerendered and cached pages, `getStaticProps` pages without a Rust loader | `public, max-age=0, s-maxage=31536000, stale-while-revalidate` |
| pages with `revalidate = N` | `public, max-age=0, s-maxage=N, stale-while-revalidate` |
| dynamic pages | `private, no-cache, no-store, max-age=0, must-revalidate` |
| `public/*` | `public, max-age=0, must-revalidate` with ETag |
| `/_volt/image` | `public, max-age=86400, stale-while-revalidate=604800` with ETag |

Pages that export `revalidate = <seconds>` are cached in memory (pages rendered on demand within `[server] page_cache_mb`); once expired they are served stale while one background render refreshes them. `ctx.revalidate_path` purges them on demand. See [static generation](static-generation.md#incremental-static-regeneration).

The server shuts down gracefully on SIGTERM and SIGINT, finishing in-flight requests for up to 10 seconds.
