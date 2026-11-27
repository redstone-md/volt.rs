# Static generation, ISR and static export

volt renders pages ahead of time whenever it can, serves them from memory, refreshes them in the background when they expire and can write the whole app out as plain files for any static host.

## What gets prerendered

`volt build` renders every page that does not need a request:

- static routes (`/`, `/about`) without a Rust `#[loader]`,
- every path returned by `generateStaticParams` (App Router) or `getStaticPaths` (Pages Router) of a dynamic route without a Rust loader.

The HTML lands in `.volt/prerender/<path>.html`, the route data used by client navigation in `.volt/prerender/<path>.json`, and the paths are recorded in `.volt/manifest.json` (`paths` and `prerendered` of each route). The server loads them into its page cache at startup, so they are answered from memory without touching QuickJS.

Rust loaders run inside your server binary and are not available while `volt build` renders, so routes with a loader are always rendered per request. Their `generateStaticParams` still feeds `dynamicParams = false` and the sitemap. Set `[build] prerender = false` to skip prerendering entirely.

## `generateStaticParams`

Export it from a dynamic page and return one params object per path:

```tsx
// app/blog/[slug]/page.tsx
export function generateStaticParams() {
  return [{ slug: "hello" }, { slug: "world" }];
}

export default function Post({ params }: PageProps<undefined, { slug: string }>) {
  return <h1>{params.slug}</h1>;
}
```

Catch-all segments take arrays (`{ path: ["guide", "intro"] }` for `/docs/[...path]`). Values are percent-encoded into the URL, numbers are converted to strings.

Layouts of nested dynamic segments can export it too. Generators run from the outermost layout to the page; each one is called once per result of its parents with `{ params }` and its results are merged into them:

```tsx
// app/[lang]/layout.tsx
export const generateStaticParams = () => [{ lang: "en" }, { lang: "fr" }];

// app/[lang]/docs/[slug]/page.tsx
export async function generateStaticParams({ params }: { params: { lang: string } }) {
  return (await listDocs(params.lang)).map((doc) => ({ slug: doc.slug }));
}
```

Generators run once in QuickJS during `volt build` (`__VOLT__.staticParams`), with the same runtime as server rendering: no Node.js APIs and no network access. Keep the data they need in the bundle (JSON imports, MDX, constants).

### `dynamicParams`

By default, params that were not generated are rendered on demand and cached like generated ones. `export const dynamicParams = false` (in the page or a layout) answers them with the not-found page instead:

```tsx
export const dynamicParams = false;
```

`volt dev` does not evaluate generators, so every param renders there.

## `getStaticPaths` (Pages Router)

Pages Router pages can export `getStaticPaths` with the Next.js shape. Each entry is `{ params }` or a path string, and `fallback: false` behaves like `dynamicParams = false` (`true` and `"blocking"` render other paths on demand):

```tsx
export async function getStaticPaths() {
  return { paths: [{ params: { id: "1" } }, "/posts/2"], fallback: false };
}
```

## Incremental static regeneration

`export const revalidate = 60` keeps a page in the cache for 60 seconds. After that the next request still gets the cached copy immediately while one background task renders a fresh one; concurrent requests keep getting the stale copy and do not start more renders. If the background render fails the stale copy stays and the next request retries; if it answers 404 the page is dropped. Prerendered pages with `revalidate` start their timer when the server starts.

`export const revalidate = 0` means the page is never cached: it is not prerendered and renders on every request, like `dynamic = "force-dynamic"`.

Pages rendered on demand (paths not known at build time, and every query string variant) share the `[server] page_cache_mb` budget (50 MB by default); the least recently used are evicted first. Build-time paths stay in memory and do not count against it. A purge also discards renders that were already in flight, so an older render never replaces a purged page.

Revalidated pages are sent with `Cache-Control: public, max-age=0, s-maxage=<seconds>, stale-while-revalidate` so a CDN in front behaves the same way.

## On-demand revalidation

Purge cached pages from any action, handler, loader or background task:

```rust
use voltrs::prelude::*;

#[action]
async fn publish(ctx: Ctx, post: Post) -> Result<()> {
    save(&post).await?;
    ctx.revalidate_path(&format!("/blog/{}", post.slug));
    ctx.revalidate_path("/blog/[slug]"); // every page of a route
    ctx.revalidate_prefix("/docs");      // /docs and everything below it
    Ok(())
}
```

`voltrs::revalidate_path` and `voltrs::revalidate_prefix` do the same without a `Ctx`, for every server running in the process. Paths are app paths without `base_path`; all query string variants of a path are purged. The next request renders the page again and caches the result.

## Static export

`volt export` builds the app and writes a static site to `out/` (`[build] export_dir`):

```
out/
├── index.html, about.html, blog/hello.html   every prerendered page
├── 404.html                                  the not-found page
├── _volt/static/                             hashed JS, CSS and media
├── _volt/data/<path>.json                    route data for client navigation
├── sitemap.xml, robots.txt, llms.txt         when enabled in [seo]
└── ...                                       files from public/
```

With `trailing_slash = true` pages are written as `about/index.html`. Exported pages fetch `/_volt/data/<path>.json` instead of calling the server when navigating, so `Link`, `useRouter` and prefetching keep working on any static host. `<Image>` falls back to the original files because `/_volt/image` needs the server.

The export fails with a list of everything that needs the server:

- routes with a Rust `#[loader]`,
- server actions and API routes,
- dynamic routes without `generateStaticParams` or `getStaticPaths`, and pages with `dynamic = "force-dynamic"` or `revalidate = 0`,
- paths that could not be prerendered.

`export_dir` is emptied before writing, so it must be a relative path inside the project without `..`. It cannot be or contain `app/`, `public/`, `.volt/`, `node_modules/`, `server/`, `src/` or `target/`, and an existing non-empty directory is only replaced when a previous export wrote it.

`[[redirects]]`, `[[rewrites]]`, `[headers]` and `revalidate` are reported as ignored. Configure redirects and headers on the host, and point its 404 page to `404.html`.

## Base path and trailing slashes

```toml
[build]
base_path = "/docs"     # the app lives under https://example.com/docs
trailing_slash = true   # canonical URLs end with a slash
```

`base_path` is applied to routing (requests outside it answer 404), `Link` and router hrefs, asset and media URLs, actions, `/_volt/data`, `<Image>`, the dev live reload, redirects (configured ones and those raised by loaders and actions), the sitemap, `robots.txt` and the export. Write hrefs, redirects and `<Image src>` without it: `<Link href="/about">` renders `/docs/about/`, and `usePathname()` returns `/about`. Plain `<img>` and `<a>` tags pointing at `public/` files need the prefix written out.

With `trailing_slash = false` (the default) `/about/` redirects to `/about` with 308; with `true`, `/about` redirects to `/about/`. Paths whose last segment contains a dot (files) and `/_volt/*` endpoints are left alone. Repeated leading slashes and backslashes collapse to one `/`, so `//evil.com/` redirects to `/evil.com` on the same host.
