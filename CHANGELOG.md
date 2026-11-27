# Changelog

## 0.0.2

- Pages Router next to the App Router: `pages/` routes, `_app`, `_document` (`next/document`), `404`, `500` / `_error`, `getServerSideProps`, `getStaticProps` with `revalidate`, `getStaticPaths` prerendering, Rust loader props merged into page props, and `next/router` (`volt-rs/router`) with `useRouter`, `withRouter`, the `Router` singleton and router events.
- `next` and `next/*` imports type-check through `volt-rs/env`.
- `[server] page_cache` bounds the in-memory page cache.

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
