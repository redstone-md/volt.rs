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
page_cache_mb = 50                # memory for pages cached on demand (LRU); build-time pages are not counted
body_limit_mb = 8                 # largest request body (actions, API routes, uploads), 413 above
streaming = true                  # stream dynamic pages with Suspense; false renders with renderToString
# user agents (regex, case-insensitive) that get complete pages; the default:
bots = '(bot|crawler|spider|crawling)\b|slurp|mediapartners|facebookexternalhit|facebookcatalog|embedly|quora link preview|outbrain|pinterest|vkshare|w3c_validator|whatsapp|skypeuripreview|bingpreview|ia_archiver|yandex|baiduspider|lighthouse|google-inspectiontool'

[build]
minify = true
sourcemap = false                 # client source maps in production
prerender = true                  # prerender static routes and generateStaticParams paths
aliases = { "@ui" = "./components/ui" }
define = { "__API_VERSION__" = "\"2\"" }
base_path = "/docs"               # serve the app under a path prefix
trailing_slash = false            # true: /about/ is canonical, the other form redirects with 308
export_dir = "out"                # output of `volt export`, a relative path inside the project

[seo]
sitemap = true
robots = true
llms = true
disallow = ["/admin"]

[images]
optimize = true
concurrency = 0                   # parallel resize jobs, 0 = number of CPUs

[i18n]
locales = ["en", "de"]            # locale-prefixed routing; empty or absent disables it
default_locale = "en"             # served without a prefix; the first locale when omitted
locale_detection = true           # redirect `/` by the locale cookie or Accept-Language

[headers]
"Strict-Transport-Security" = "max-age=63072000; includeSubDomains; preload"

[[redirects]]
source = "/old-blog/[slug]"
destination = "/blog/[slug]"
permanent = true
locale = true                     # with [i18n]: also match below locale prefixes (false: full path only)

[[rewrites]]
source = "/docs"
destination = "/docs/introduction"
```

Unknown keys are rejected, so typos fail loudly.

`[server] streaming` and `bots` are described in [Loading UI and streaming](routing.md#loading-ui-and-streaming). The default `bots` pattern above covers the common search engine crawlers and link-preview fetchers (Googlebot, Bingbot, Slurp, facebookexternalhit, Twitterbot, Slackbot, WhatsApp, Lighthouse and others). Setting `bots` replaces it rather than adding to it, so extend the default when you only need one more agent; an empty string streams to every client. An invalid pattern fails `volt build` and the server start.

`base_path` and `trailing_slash` are applied to routing, links, assets, redirects, the sitemap and `volt export`; see [static generation](static-generation.md#base-path-and-trailing-slashes). Redirect and rewrite sources and destinations are written without the base path.

`[i18n]` serves the default locale unprefixed and every other locale under `/<locale>/...`; see [internationalized routing](i18n.md). `default_locale` must be one of `locales`, and no locale may be listed twice (case-insensitively). Rewrites apply to the path below the locale prefix. Redirects match below every locale prefix and keep the locale in the destination; `locale = false` on a rule matches the full path instead. Without `[i18n]`, `[site] language` is the locale reported by `useLocale()` and `ctx.locale()`.

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
