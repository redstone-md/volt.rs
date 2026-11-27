# Architecture

```
            ┌────────────────────────── volt build / volt dev ───────────────────────────┐
            │  app/**  ──►  route scanner  ──►  rolldown (oxc)  ──►  client chunks (ESM)   │
            │                                   │                  server bundle (IIFE) │
            │  *.css / *.scss / fonts  ──►  Tailwind or Lightning CSS  ──►  app-[hash].css │
            │  server/main.rs  ──►  cargo  ──►  binary  ──►  --emit-sdk  ──►  @volt/api     │
            └────────────────────────────────────────────────────────────────────────────┘

 request ──► hyper ──► middleware ──► static / action / data / image / API / route.ts / page
                                                                     │
                                                    Rust loader ──► QuickJS worker pool
                                                                     │  React renderToReadableStream
                                                                     ▼
                                                shell + Suspense chunks, streamed with state + modulepreload
```

## Crates and packages

| Path | Role |
| --- | --- |
| `crates/voltrs` | Runtime linked into every server: HTTP, routing, SSR engine, actions, loaders, SEO, images. |
| `crates/voltrs-macros` | `#[action]`, `#[loader]`, `#[get]`..., `#[sitemap]`, `#[model]`, `#[derive(Type)]`. |
| `crates/voltrs-cli` | The `volt` command: scaffolding, bundling, CSS, dev server, builds. |
| `packages/volt-rs` | Browser and SSR runtime: router, `Link`, hooks, metadata, `Form`, `Image`, actions client, `volt` bin shim. |
| `packages/create-volt-rs` | `npm create volt-rs`. |

## Rendering

The server bundle is evaluated once per worker in an isolated QuickJS runtime with a memory limit and a 10-second watchdog per render. A render call passes the page state as JSON; JavaScript resolves metadata and renders the layout tree with `react-dom/server`'s `renderToReadableStream`. The polyfills give QuickJS what it needs for that: a WHATWG `ReadableStream`, UTF-8 `TextEncoder`/`TextDecoder`, `AbortController` and wall-clock timers. React 19 hoists `<title>`, `<meta>` and `<link>` into `<head>`.

Three render methods share that code:

- `stream(state, sink)` reports the shell to Rust (`sink.shell`, with the status code) as soon as it is ready, then hands every chunk React flushes to `sink.write`. Rust answers with the status and headers at the shell and sends the chunks as a chunked body backed by a channel from the worker thread. A splicer inserts the stylesheet and `modulepreload` hints before `</head>` and the serialized state and client entry before the final `</body>`, holding back a few bytes so that markers split across chunks are still found. When the client disconnects, writes fail and the render is aborted. Dynamic pages use it, and gzip is applied per chunk with a sync flush.
- `document(state, sink)` waits for `allReady`, then writes the whole document to the sink the same way; Rust collects the chunks into one response. It serves cached and prerendered pages, background regeneration and user agents matching `[server] bots`.
- `render(state)` uses `renderToString`, with `[server] streaming = false`.

`notFound()` and `redirect()` raised while the shell renders are recorded by `onError` and turned into the not-found page or a redirect before anything is sent, so the status code stays correct.

A worker is an event loop: it calls the render function, runs QuickJS jobs and the earliest due timer, and while every render it holds is waiting on a timer it takes further jobs from the queue, so a Suspense boundary waiting on a slow promise does not hold the worker. It holds at most 8 renders: whatever they do after their timers runs on that one thread, so further jobs stay in the shared queue, where idle workers take them, and a render's 10-second deadline starts when a worker takes it.

When a render passes its deadline, or waits on a promise nothing can resolve (no timer left), the worker calls `sink.abort`, which aborts React's render: the document is completed with the boundaries still pending marked for client rendering, so the page stays well-formed and hydration renders them in the browser. Such a page is never cached and is not prerendered. A render that does not finish within 2 seconds of the abort fails: before the shell it answers with the error page, after it the streamed document is closed as it is.

`TextEncoder` and `TextDecoder` call into Rust for the UTF-8 conversion, since React encodes every piece of markup it writes.

Pages Router routes run `getServerSideProps` / `getStaticProps` in the same render call, merge their props over the loader data and render the `_document` around the page tree, which goes into `<div id="__next">`; the props are returned to Rust so the serialized state matches. Client navigations call `__VOLT__.props` from `/_volt/data` for those routes.

With `[i18n]`, the server strips the locale prefix of a page or data path before route matching and puts the locale into the page state (`locale`) and `Ctx`; the page cache keys entries by route path, query and locale. The client reads the configured locales from build-time defines and adds the prefix back when it builds hrefs.

Error boundaries do not catch on the server, so the render resolves them itself: a `notFound()` that reaches the shell is rendered again with the innermost `not-found.tsx` in place of its segment's children (`state.notFound`, which the client hydrates the same way), and when the error page fails too, `global-error.tsx` replaces the document (`state.globalError`). Elements returned by `useServerInsertedHTML` callbacks are rendered once the shell (or, with `renderToString`, the tree) is complete and returned to Rust as extra head HTML. `__VOLT__.metadataRoute(name)` evaluates `app/sitemap.ts`, `app/robots.ts` and `app/manifest.ts`; Rust turns the result into XML, text or JSON.

Route handlers (`route.ts`) and `middleware.ts` are bundled into the server bundle. `__VOLT__.handle(request, sink)` and `__VOLT__.middleware(request, sink)` take the request as JSON (method, absolute URL, app path, headers, route and params) with the body as `sink.body`, a `Uint8Array`, and answer like a streamed render: the status and headers through `sink.shell`, then the body bytes through `sink.write`, so binary bodies cross without encoding. The head says whether the body is a `ReadableStream`: such a handler body is forwarded to hyper chunk by chunk and may outlive the render deadline (the worker drops the job once the client is gone), while a body known up front is collected and sent with a length. Before middleware or routing, the request path is normalized once (`router::canonical_path`: slashes collapsed, dot segments resolved, each segment decoded and re-encoded), and the request continues with that URI, so matchers, `nextUrl` and the router agree on the path. Middleware speaks Next.js's marker headers (`x-middleware-next`, `x-middleware-rewrite`, `x-middleware-override-headers`, `x-middleware-request-*`), which Rust strips and applies. The matcher is read from the source at build time with oxc, compiled from path-to-regexp syntax to one regular expression per pattern and matched in Rust, so unmatched requests never reach QuickJS; patterns the `regex` crate cannot run (lookarounds) are passed to the call and tested by QuickJS. `has` / `missing` conditions are checked in Rust against the request's headers, cookies, query and host.

`fetch` calls the host's `fetch(request, body)`, which starts the request on a small Tokio runtime of its own (hyper HTTP/1.1 and rustls with the platform verifier) and returns an id; the worker delivers the response to `__volt_fetched(id, ...)` when it arrives. While a job waits on a `fetch`, the worker waits on both the job queue and the responses, so it keeps taking renders; `[server] fetch_allow` and the timeout are checked in Rust, and unlisted hosts must resolve to public addresses.

Workers are plain OS threads pulling jobs from a shared queue, so a slow render never blocks the async HTTP runtime. In development the pool reloads the bundle when the manifest changes, once the renders in flight have finished.

## Client

The client entry hydrates the document, then owns navigation: links fetch `/_volt/data?path=...` (the route's loader result as JSON), import the route's chunk, resolve metadata and commit the new tree in a transition. Both server and client build the tree with the same code (`volt-rs/internal/router.js`), which is what makes hydration exact.

Routes with a `loading` file in any segment also get a small shell chunk with their segments but no page. On navigation the client matches the URL against the route patterns, and if the data has not arrived when the shell chunk has, it commits the target layouts around a page that suspends (an urgent update, not a transition), so the nearest `loading` boundary shows; the real view replaces it in a transition once the data and the page chunk are there.

Parallel route slots are resolved by the server with the same router as pages: the manifest lists every slot with its URL patterns, and the page state carries the page each slot renders (`state.slots`). On client navigations the runtime sends the current route in `x-volt-from`; the server uses it to apply intercepting routes and answers `retain: true` when only slots change, and the client merges the result over the current view, keeping slots that did not match.

## Type bridge

`#[model]` and `#[derive(Type)]` implement `voltrs::Type`, which describes a Rust type as TypeScript. `#[action]` and `#[loader]` register themselves with `inventory`. Running the server binary with `--emit-sdk <dir>` walks the registry and writes `api.ts` (actions, models, `Loaders`, `PageProps`) and `server.json` (used by the CLI to decide which pages can be prerendered).

## Builds

rolldown bundles twice from the same virtual modules: one ESM build for browsers with a lazy chunk per route (plus a shell chunk per route with `loading` UI), and one single-file IIFE build for QuickJS (`react-dom/server.browser`). The volt plugin compiles CSS Modules and Sass with Lightning CSS and grass, rewrites `next/font` calls (parsed with oxc) into class names plus generated `@font-face` stylesheets, and collects every stylesheet in module evaluation order. They become one file: through Tailwind when a stylesheet imports it, otherwise through Lightning CSS (prefixes, minification), with optional PostCSS plugins, and relative `url()` assets copied to `static/media`. After bundling, `__VOLT__.routes()` reports route exports and `__VOLT__.staticParams(id)` evaluates `generateStaticParams` / `getStaticPaths`; static routes, `getStaticProps` pages and generated paths without loaders are rendered in-process and written to `.volt/prerender` with their route data (Pages Router props included). Static assets are precompressed with Brotli (quality 11) and gzip. Metadata files of `app/` (icons, Open Graph images, ...) are copied to `.volt/metadata` and listed in the manifest with their route pattern and content hash; the route modules carry the head entries (URL, type, sizes, alt) of each segment's files, which `resolveMetadata` merges per request.

## Development

`volt dev` builds the browser bundle with `preserveModules`: one file per module under a stable name, with the React Refresh transform applied to app modules by the volt plugin (oxc, refresh pass only, TypeScript and JSX left to rolldown). Each instrumented module registers its components under its path and reports itself as evaluated to `volt-rs/internal/refresh`, which the client entry imports first so that `react-refresh` hooks into React DOM before it loads. After each rebuild the CLI compares the output files with the previous build (installed packages by their sources, since their output also varies with rolldown's interop details); when only instrumented app modules changed, it writes them into the manifest (`hot`: each module's URL and the revision it last changed in, the importers of every app module and the revision range covered) and keeps the build id, so client navigations stay soft. The dev server polls the manifest and broadcasts an `update`, `reload` or `failed` event over `/_volt/hmr`. The page walks from the changed modules it has loaded, newer than its copy, up through their importers to the nearest refresh boundaries (modules exporting components and plain data), fetches their code, rewrites their import specifiers to the instances it holds or to the other new instances, evaluates them from blob URLs, checks that the boundaries kept their exports and calls `performReactRefresh()`. In development the root error boundary reports to the overlay instead of rethrowing, so the root stays mounted and Fast Refresh can retry it. A failed rebuild leaves the previous build in memory, so the open page keeps working under the error overlay.
