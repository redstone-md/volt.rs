# Changelog

## 0.0.2

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
- `ctx.revalidate()`, `ctx.revalidate_path()` and `ctx.revalidate_prefix()` also drop matching route data on the client, refreshing the current route when it matches. Actions that set cookies revalidate automatically.

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
