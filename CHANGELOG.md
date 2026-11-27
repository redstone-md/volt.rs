# Changelog

## 0.0.2

- Streaming SSR: dynamic pages render with `renderToReadableStream` in QuickJS and stream the shell, then every Suspense boundary as it resolves (`use(promise)`, `React.lazy`, `dynamic()`), with gzip flushed per chunk. `notFound()` and `redirect()` raised before the shell set the status code. Cached and prerendered pages and crawlers (`[server] bots`) get the complete document with every boundary resolved; `[server] streaming = false` keeps `renderToString`.
- A render worker interleaves up to 8 renders that wait on timers, and server timers follow the wall clock. A render that passes its 10-second deadline or waits on a promise nothing resolves is aborted: the document is completed with the pending boundaries rendered in the browser, and it is not cached.
- `TextEncoder` and `TextDecoder` in the server runtime are native, and complete documents are collected in Rust instead of being decoded in JavaScript.
- `dynamic()` server-renders its content in place and honours `ssr: false`.
- Client navigation to a route with `loading.tsx` shows the target layouts and loading UI at once while the loader data and page chunk load.
- Parallel routes: `@slot` folders render as layout props next to `children`, each with its own pages, boundaries and nested segments; unmatched slots keep their state on client navigation and render `default.tsx` on hard loads (404 without one). URLs that only a slot matches render the segment's `default.tsx`. Slot pages can have Rust loaders.
- Intercepting routes: `(.)`, `(..)`, `(..)(..)` and `(...)` folders render instead of the target route on client navigation (into a slot, keeping the current page, or as the page), while hard loads render the original route; back and forward restore how each entry was resolved. The client names its current route in an `x-volt-from` header on `/_volt/data`.
- Pages Router next to the App Router: `pages/` routes, `_app`, `_document` (`next/document`), `404`, `500` / `_error`, `getServerSideProps`, `getStaticProps` with `revalidate`, `getStaticPaths` prerendering, Rust loader props merged into page props, and `next/router` (`volt-rs/router`) with `useRouter`, `withRouter`, the `Router` singleton and router events.
- `next` and `next/*` imports type-check through `volt-rs/env`.
- `generateStaticParams` (pages and layouts), `dynamicParams` and Pages Router `getStaticPaths`, evaluated at build time; generated paths are prerendered and listed in the sitemap.
- Stale-while-revalidate for `revalidate` pages with a single background regeneration.
- On-demand revalidation with `ctx.revalidate_path`, `ctx.revalidate_prefix`, `voltrs::revalidate_path` and `voltrs::revalidate_prefix`.
- `volt export` writes a static site with client navigation from exported route data.
- `[build] base_path`, `trailing_slash` (308 normalisation) and `export_dir`.
- `<Image>` serves original files when `[images] optimize = false`.
- `revalidate = 0` renders on every request; the page cache keeps on-demand pages within `[server] page_cache_mb` (LRU).
- `volt dev` no longer rebuilds on events of the `.volt` directory itself.
- CSS Modules: `*.module.css`, `*.module.scss` and `*.module.sass` export scoped class names, including `composes`.
- Sass and SCSS stylesheets compiled with grass; `@use` and `@import` resolve relative to the file and `node_modules`.
- Lightning CSS processing of non-Tailwind CSS: `@import` inlining, vendor prefixes and syntax lowering for the `browserslist` targets, minification in production.
- Relative `url()` references in CSS (including Tailwind output) are copied to the build with content hashes.
- `next/font/google` and `next/font/local` (also `volt-rs/font/*`): build-time `@font-face` generation, self-hosted Google Fonts cached in `.volt/cache/fonts`, font preloading for the root layout (or `pages/_app` in the Pages Router).
- Remote `@import`s are moved to the top of the combined stylesheet, `@import` of plain CSS from Sass and CSS modules is inlined, and relative `url()`s in Sass partials resolve relative to the partial.
- Sass and CSS module compile errors show their message and location; importing a CSS module always includes its CSS.
- Google fonts without `weight` fail with a clear error when the family catalog cannot be read; the cached catalog is refreshed weekly.
- PostCSS: projects with a `postcss.config.*` and `postcss` installed run their plugins; Tailwind also compiles through `@tailwindcss/postcss` when its CLI is not installed.
- `volt start` hands its process over to the Rust server, so stopping it also stops the server.
- Fractional numbers in page data keep their exact value between the HTML and `/_volt/data`.
- File uploads: `multipart/form-data` bodies for actions and API routes, a `voltrs::File` type for `#[model]` fields (`File`, `Option<File>`, `Vec<File>`), `ctx.multipart()` and `[server] body_limit_mb`.
- `createAction` sends files as multipart, `<Form>` posts `multipart/form-data` without JavaScript, and `<form action={action}>` works without JavaScript too.
- React 19 action hooks: actions work with React's `useActionState` and `useFormStatus`; `volt-rs/actions` adds a `useActionState` that returns action errors.
- `Error::validation` answers 422 with `fieldErrors`, exposed as `ActionError.fieldErrors`, `useAction().fieldErrors` and the `useActionState` error.
- `ctx.revalidate()`, `ctx.revalidate_path()` and `ctx.revalidate_prefix()` also drop matching route data on the client, refreshing the current route when it matches. Actions that set cookies refresh the client's route data automatically (the shared server cache is left alone).
- Without JavaScript, `useActionState` forms from `volt-rs/actions` get their returned state or field errors back on the redirected page, and a `permalink` posts the form to that page.
- Nested objects next to files in a multipart action input are decoded from JSON fields.
- Uploaded file names drop control characters, and `.`/`..` become empty. No-JavaScript submissions return only to local paths.
- Form field grouping is linear in the number of fields.
- Metadata files in `app/`: `favicon.ico`, `icon.*`, `apple-icon.*`, `opengraph-image.*` (with `.alt.txt`), `twitter-image.*`, `manifest.json` / `manifest.webmanifest`, `robots.txt` and `sitemap.xml` are served at their Next.js URLs (hashed URLs cached forever, plain ones with an ETag) and added to the metadata of every route below their segment, with sizes and types.
- `app/sitemap.ts`, `app/robots.ts` and `app/manifest.ts` metadata routes, evaluated in QuickJS (`__VOLT__.metadataRoute`); `sitemap.ts` replaces the automatic sitemap and is merged with `#[sitemap]` providers. `MetadataRoute` types.
- `app/global-error.tsx` replaces the root layout when it throws in the browser or while rendering the error page on the server, and is the server error page when there is no root `error.tsx`.
- Nested `not-found.tsx` files handle `notFound()` during server rendering too, inside their layouts, with status 404 and `noindex`.
- `useSelectedLayoutSegment` and `useSelectedLayoutSegments`, `useServerInsertedHTML` (injected into the head of server-rendered pages), and `unstable_noStore` / `connection` no-ops; `next/cache` and `next/server` resolve to `volt-rs/cache` and `volt-rs/server`.
- `export const dynamic = "force-static"` caches one render per pathname without the query (also for routes with a Rust loader), `"error"` fails the build for routes with a Rust loader, and `dynamic` is read from layouts too. Other segment config exports (`fetchCache`, `runtime`, ...) are ignored.
- Error and not-found boundaries reset on navigation instead of remounting, so layouts nested in them keep their state; templates remount when the segment below them changes, as in Next.js.
- The subtree below a layout remounts when one of its segments changes (`/items/1` to `/items/2`), as in Next.js, while the layout keeps its state.
- `force-static` routes run their Rust loader with a request without query, cookies or headers, so no visitor's data is shared through the cache; client navigations get the same data.
- Metadata files under catch-all segments (`app/docs/[...slug]/icon.png`) are served; route patterns accept segments after a catch-all.
- Two metadata files resolving to the same URL fail the build; `volt export` copies metadata files of dynamic segments under each exported path.
- `sitemap.ts`, `robots.ts` and `manifest.ts` output is cached, honouring `revalidate` and `dynamic = "force-dynamic"` exports.

## 0.0.1

First release.

- App Router file conventions: pages, nested layouts, loading, error, not-found and template files, dynamic, catch-all, optional catch-all segments and route groups.
- Server-side rendering of React 19 in a pool of QuickJS workers, hydration and client-side navigation with prefetching and scroll restoration.
- Rust loaders, server actions and API routes with a generated, typed TypeScript client (`@volt/api`).
- `<Form>` with progressive enhancement, `useAction`, `ActionError`, CSRF protection for actions.
- Metadata API, JSON-LD, automatic `sitemap.xml`, `robots.txt` and `llms.txt`.
- `Link`, `Image` with on-demand resizing, `Script`, `Head`, `dynamic` and `next/*` import compatibility.
- rolldown bundling with per-route code splitting, Tailwind CSS v4, shadcn/ui-ready templates.
- Static prerendering, `revalidate` caching, Brotli and gzip precompression, security headers, redirects, rewrites.
- `volt` CLI: `new`, `dev` with live reload and error overlay, `build`, `start`, `sdk`, `routes`.
