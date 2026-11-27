# Configuration

All settings live in an optional `volt.toml` at the project root.

```toml
[site]
name = "Acme"                     # default title and og:site_name
url = "https://acme.dev"          # canonical origin for sitemap, robots and Open Graph
language = "en"

[server]
host = "0.0.0.0"
port = 3000
workers = 0                       # QuickJS render workers, 0 = number of CPUs
memory_limit_mb = 256             # heap limit per render worker
compress = true                   # gzip dynamic responses
page_cache = 1000                 # pages rendered at runtime kept in memory (prerendered pages always are)

[build]
minify = true
sourcemap = false                 # client source maps in production
prerender = true                  # prerender static routes and generateStaticParams paths
aliases = { "@ui" = "./components/ui" }
define = { "__API_VERSION__" = "\"2\"" }
base_path = "/docs"               # serve the app under a path prefix
trailing_slash = false            # true: /about/ is canonical, the other form redirects with 308
export_dir = "out"                # output of `volt export`

[seo]
sitemap = true
robots = true
llms = true
disallow = ["/admin"]

[images]
optimize = true
concurrency = 0                   # parallel resize jobs, 0 = number of CPUs

[headers]
"Strict-Transport-Security" = "max-age=63072000; includeSubDomains; preload"

[[redirects]]
source = "/old-blog/[slug]"
destination = "/blog/[slug]"
permanent = true

[[rewrites]]
source = "/docs"
destination = "/docs/introduction"
```

Unknown keys are rejected, so typos fail loudly.

`base_path` and `trailing_slash` are applied to routing, links, assets, redirects, the sitemap and `volt export`; see [static generation](static-generation.md#base-path-and-trailing-slashes). Redirect and rewrite sources and destinations are written without the base path.

## Environment variables

| Variable | Effect |
| --- | --- |
| `PORT`, `HOST` | override `server.port` and `server.host` |
| `VOLT_SITE_URL` | overrides `site.url` |
| `VOLT_ROOT` | project directory for the server binary |
| `VOLT_OUT_DIR` | build directory, default `.volt` |
| `VOLT_PUBLIC_*`, `NEXT_PUBLIC_*` | inlined into client code as `process.env.NAME` |

`.env`, `.env.local`, `.env.development`, `.env.production` and their `.local` variants are loaded in the Next.js order. Variables that are already set win.

## TypeScript

`tsconfig.json` in the templates maps `@/*` to the project root and `@volt/api` to the generated client, and loads `volt-rs/env` for asset import types. The bundler reads the same `paths`.

## Default response headers

Every response carries `X-Content-Type-Options: nosniff`, `Referrer-Policy: strict-origin-when-cross-origin` and `X-Frame-Options: SAMEORIGIN`. Headers in `[headers]` are added on top.
