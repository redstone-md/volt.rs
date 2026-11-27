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
- `next/font/google` and `next/font/local` (also `volt-rs/font/*`): build-time `@font-face` generation, self-hosted Google Fonts cached in `.volt/cache/fonts`, font preloading for the root layout.
- PostCSS: projects with a `postcss.config.*` and `postcss` installed run their plugins; Tailwind also compiles through `@tailwindcss/postcss` when its CLI is not installed.

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
