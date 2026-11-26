# Architecture

```
            ┌────────────────────────── volt build / volt dev ───────────────────────────┐
            │  app/**  ──►  route scanner  ──►  rolldown (oxc)  ──►  client chunks (ESM)   │
            │                                   │                  server bundle (IIFE) │
            │  app/globals.css  ──►  Tailwind CLI  ──►  app-[hash].css                    │
            │  server/main.rs  ──►  cargo  ──►  binary  ──►  --emit-sdk  ──►  @volt/api     │
            └────────────────────────────────────────────────────────────────────────────┘

 request ──► hyper ──► middleware ──► static / action / data / image / API / page
                                                                     │
                                                    Rust loader ──► QuickJS worker pool
                                                                     │  React renderToString
                                                                     ▼
                                                         HTML + state + modulepreload
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

The server bundle is evaluated once per worker in an isolated QuickJS runtime with a memory limit and a 10-second watchdog. A render call passes the page state as JSON; JavaScript resolves metadata, renders the layout tree with `renderToString` and returns HTML. Rust then inserts the stylesheet, `modulepreload` hints, the serialized state and the client entry. React 19 hoists `<title>`, `<meta>` and `<link>` into `<head>`.

Workers are plain OS threads pulling jobs from a shared queue, so a slow render never blocks the async HTTP runtime. In development the pool reloads the bundle when the manifest changes.

## Client

The client entry hydrates the document, then owns navigation: links fetch `/_volt/data?path=...` (the route's loader result as JSON), import the route's chunk, resolve metadata and commit the new tree in a transition. Both server and client build the tree with the same code (`volt-rs/internal/router.js`), which is what makes hydration exact.

## Type bridge

`#[model]` and `#[derive(Type)]` implement `voltrs::Type`, which describes a Rust type as TypeScript. `#[action]` and `#[loader]` register themselves with `inventory`. Running the server binary with `--emit-sdk <dir>` walks the registry and writes `api.ts` (actions, models, `Loaders`, `PageProps`) and `server.json` (used by the CLI to decide which pages can be prerendered).

## Builds

rolldown bundles twice from the same virtual modules: one ESM build for browsers with a lazy chunk per route, and one single-file IIFE build for QuickJS (`react-dom/server.browser`). Imported CSS is collected in execution order and handed to Tailwind. Static routes without loaders are rendered in-process and written to `.volt/prerender`. Static assets are precompressed with Brotli (quality 11) and gzip.
