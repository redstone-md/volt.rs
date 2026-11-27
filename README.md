<p align="center">
  <img src="assets/banner.jpg" alt="volt.rs: React on a Rust server" width="100%">
</p>

<p align="center">
  <a href="https://github.com/redstone-md/volt.rs/actions/workflows/ci.yml"><img src="https://github.com/redstone-md/volt.rs/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://crates.io/crates/voltrs"><img src="https://img.shields.io/crates/v/voltrs.svg" alt="crates.io"></a>
  <a href="https://www.npmjs.com/package/volt-rs"><img src="https://img.shields.io/npm/v/volt-rs.svg" alt="npm"></a>
  <a href="./LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="MIT"></a>
</p>

**volt.rs** is a React framework with a Rust server. You write pages exactly like in the Next.js App Router: `app/` directory, layouts, `Link`, `useRouter`, the metadata API, Tailwind CSS and shadcn/ui, or with the Pages Router (`pages/`, `getServerSideProps`, `next/router`). The server, the loaders and the server actions are Rust. React is rendered on the server inside an embedded JavaScript engine, so production needs a single native binary and no Node.js.

```bash
npm create volt-rs@latest my-app
cd my-app && npm run dev
```

## Why

Same app (shadcn/ui + Tailwind todo list with server actions), same machine (4 vCPU), `oha -c 64 -z 10s`:

| | **volt.rs 0.0.1** | Next.js 16.4 |
| --- | ---: | ---: |
| Dynamic SSR page | **1,977 req/s** · p50 32 ms | 329 req/s · p50 186 ms |
| Static page | **36,352 req/s** · p50 1.7 ms | 1,711 req/s · p50 34 ms |
| Memory, idle | **19 MB** | 85 MB |
| Memory, under load | **49 MB** | 268 MB |
| Cold start to first response | **73 ms** | 375 ms |
| Production build | **1.1 s** | 5.9 s |
| Runtime to deploy | one 4 MB binary + assets | Node.js + `node_modules` |

`next start` runs as a single Node.js process with default settings; volt renders on a pool of QuickJS workers, one per core. Benchmarks are in [`docs/benchmarks.md`](./docs/benchmarks.md) with the exact setup so you can reproduce them.

## What it looks like

```rust
// server/main.rs
use voltrs::prelude::*;

#[model]
struct Todo { id: u32, title: String, done: bool }

#[model]
struct NewTodo { title: String }

#[loader("/")]
async fn home(ctx: Ctx) -> Result<Vec<Todo>> {
    ctx.state::<Db>().todos().await
}

#[action]
async fn add_todo(ctx: Ctx, input: NewTodo) -> Result<Todo> {
    if input.title.trim().is_empty() {
        return Err(Error::bad_request("title is required"));
    }
    ctx.state::<Db>().insert(input.title).await
}

fn main() {
    App::new().state(Db::open()).run();
}
```

```tsx
// app/page.tsx
import { addTodo, type PageProps } from "@volt/api";
import Form from "volt-rs/form";

export const metadata = { title: "Todos" };

export default function Home({ data }: PageProps<"/">) {
  return (
    <main>
      <Form action={addTodo}>
        <input name="title" required />
        <button>Add</button>
      </Form>
      <ul>{data.map((todo) => <li key={todo.id}>{todo.title}</li>)}</ul>
    </main>
  );
}
```

`@volt/api` is generated from the Rust code: `Todo` and `NewTodo` become TypeScript interfaces, `add_todo` becomes a typed `addTodo` function, and `PageProps<"/">` knows the loader returns `Todo[]`. Change a Rust struct and TypeScript tells you what broke.

## Features

- **App Router conventions**: `page`, `layout`, `loading`, `error`, `global-error`, `not-found` (nested too), `template`; `[slug]`, `[...slug]`, `[[...slug]]`, `(groups)`, `_private` folders; parallel routes (`@slot`, `default`) and intercepting routes (`(.)`, `(..)`, `(...)`) for modals; `export const dynamic` (`force-static`, `force-dynamic`, `error`); `useSelectedLayoutSegment(s)` and `useServerInsertedHTML`.
- **Pages Router** next to it: `pages/`, `_app`, `_document`, `404`, `getServerSideProps`, `getStaticProps`, `getStaticPaths` and `next/router` with router events.
- **Server-side rendering** of React 19 with hydration, then client-side navigation with prefetching, scroll restoration and per-route code splitting.
- **Streaming SSR**: the shell goes out first and each `<Suspense>` boundary (`use(promise)`, `lazy`, `next/dynamic`) streams in as it resolves; crawlers get the complete page. `loading.tsx` shows instantly on client navigation.
- **Rust loaders, server actions and API routes** with a generated, fully typed TypeScript client.
- **Forms that work without JavaScript**: `<Form action={...}>` posts natively (multipart, so file uploads too) and upgrades to in-place submission with pending state, `useFormStatus` and `useActionState`.
- **File uploads and validation**: `File` fields in `#[model]` inputs, `ctx.multipart()` in API routes, `Error::validation` field errors, and `ctx.revalidate_path` to refresh cached pages and client route data after a mutation.
- **Metadata API** (`metadata`, `generateMetadata`, `viewport`, Open Graph, Twitter, robots, alternates, verification), JSON-LD, automatic `sitemap.xml`, `robots.txt` and `llms.txt`.
- **Metadata files**: `favicon.ico`, `icon.*`, `apple-icon.*`, `opengraph-image.*` (with `.alt.txt`), `twitter-image.*`, `manifest.json`, `robots.txt` and `sitemap.xml` in any segment are served and linked from the head with sizes; `app/sitemap.ts`, `app/robots.ts` and `app/manifest.ts` generate them in code, cached with `revalidate`.
- **Next.js imports work**: `next/link`, `next/navigation`, `next/router`, `next/document`, `next/app`, `next/image`, `next/script`, `next/head`, `next/form`, `next/dynamic`, `next/cache`, `next/server` (`connection`), `next/font/google`, `next/font/local`.
- **Tailwind CSS v4 and shadcn/ui** out of the box; `npx shadcn add` works.
- **CSS Modules, Sass/SCSS and PostCSS**: `*.module.css` / `*.module.scss` with scoped class names, Sass compiled in Rust, Lightning CSS for `@import` inlining, `url()` assets, vendor prefixes and minification.
- **Fonts**: `next/font/google` and `next/font/local` compatible loaders; Google Fonts are downloaded at build time and self-hosted, with preloading.
- **Images**: `<Image>` with `srcset`, lazy loading, priority preloading and on-demand resizing.
- **Static prerendering** with `generateStaticParams`, `dynamicParams` and `getStaticPaths`, incremental static regeneration (stale-while-revalidate), on-demand `revalidate_path`, and **static export** (`volt export`) for any static host.
- **Internationalized routing**: `[i18n] locales` with unprefixed default locale, `/de/...` prefixes, `Accept-Language` and cookie detection, `useLocale()`, `<Link locale>`, `ctx.locale()` in Rust, automatic `hreflang` tags and localized sitemaps.
- `base_path` and `trailing_slash`, Brotli and gzip precompression, immutable asset caching, security headers, redirects and rewrites.
- **Dev server** with live reload, CSS hot swap, Rust recompilation and an in-browser error overlay.
- **Instructions for coding agents** in every generated project (`AGENTS.md`) and [`llms.txt`](./llms.txt) for the framework itself.

## How it works

`volt build` bundles the app twice with [rolldown](https://rolldown.rs): an ESM build for browsers split per route, and a single-file build that the Rust server evaluates in [QuickJS](https://github.com/quickjs-ng/quickjs) workers. A request is routed by [hyper](https://hyper.rs), the Rust loader runs, React renders the page to HTML in a worker, and the browser hydrates the same tree. Navigations fetch only the loader's JSON and the route's chunk. Details in [docs/architecture.md](./docs/architecture.md).

## Documentation

- [Getting started](./docs/getting-started.md)
- [Routing](./docs/routing.md)
- [Pages Router](./docs/pages-router.md)
- [Data: loaders, actions, API routes](./docs/data.md)
- [Metadata and SEO](./docs/metadata-seo.md)
- [Styling and assets](./docs/styling.md)
- [Static generation, ISR and export](./docs/static-generation.md)
- [Internationalized routing](./docs/i18n.md)
- [Configuration](./docs/configuration.md)
- [Deployment](./docs/deployment.md)
- [Coming from Next.js](./docs/migrating-from-nextjs.md)
- [CLI](./docs/cli.md)
- [Architecture](./docs/architecture.md)

## Status

volt.rs is at 0.0.1. The core is usable: routing, SSR, hydration, navigation, actions, metadata, styling and builds are covered by end-to-end tests. Not there yet: React Server Components and React Fast Refresh (dev uses full reloads). See [CHANGELOG.md](./CHANGELOG.md).

## Contributing

Issues and pull requests are welcome. Start with [CONTRIBUTING.md](./CONTRIBUTING.md) and [AGENTS.md](./AGENTS.md).

## License

[MIT](./LICENSE)
