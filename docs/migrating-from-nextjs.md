# Coming from Next.js

volt keeps the App Router and Pages Router file layouts, the component APIs and the metadata API. What changes is where server code lives: instead of server components and `"use server"` functions in TypeScript, the server is Rust.

## What carries over unchanged

| Next.js | volt |
| --- | --- |
| `app/**/page.tsx`, `layout.tsx`, `loading.tsx`, `error.tsx`, `global-error.tsx`, `not-found.tsx`, `template.tsx` | same |
| `favicon.ico`, `icon.*`, `apple-icon.*`, `opengraph-image.*`, `twitter-image.*`, `manifest.json`, `robots.txt`, `sitemap.xml` files | same (image files only: no `opengraph-image.tsx`) |
| `app/sitemap.ts`, `app/robots.ts`, `app/manifest.ts` | same, plus Rust `#[sitemap]` providers |
| `[slug]`, `[...slug]`, `[[...slug]]`, `(group)`, `_private` | same |
| `@slot` parallel routes with `default.tsx`, `(.)` / `(..)` / `(...)` intercepting routes | same, see [Routing](./routing.md#parallel-routes) |
| `next/link`, `next/navigation`, `next/router`, `next/document`, `next/app`, `next/image`, `next/script`, `next/head`, `next/form`, `next/dynamic`, `next/cache`, `next/server` | aliased to `volt-rs/*` (`next/cache` and `next/server` only export `unstable_noStore` and `connection`) |
| `pages/**`, `_app`, `_document`, `404`, `500`, `getServerSideProps`, `getStaticProps`, `getStaticPaths` | same, see [Pages Router](./pages-router.md) |
| `export const metadata`, `generateMetadata`, `export const viewport` | same |
| `params` / `searchParams` props (sync or awaited) | same |
| `export const revalidate`, `export const dynamic` (`force-dynamic`, `force-static`, `error`) | same, with stale-while-revalidate |
| `<Suspense>`, `use(promise)`, `loading.tsx`, `next/dynamic` | streamed from the server; `loading.tsx` shows at once on navigation, but on a first request to a page with a Rust loader only after the loader returns |
| `generateStaticParams`, `dynamicParams`, `getStaticPaths` | same, evaluated at build time for routes without a Rust loader |
| `output: "export"`, `basePath`, `trailingSlash` | `volt export`, `[build] base_path`, `[build] trailing_slash` |
| Tailwind CSS, shadcn/ui, CSS imports, `@/*` alias | same |
| `NEXT_PUBLIC_*`, `.env*` files | same (`VOLT_PUBLIC_*` also works) |
| `public/` | same |
| `next.config.js` redirects, rewrites, headers | `volt.toml` |

## What changes

| Next.js | volt |
| --- | --- |
| async server component that queries data | Rust `#[loader("/route")]`, page receives `data` |
| `"use server"` action | Rust `#[action]`, imported from `@volt/api` |
| `app/api/**/route.ts`, `pages/api/**` | Rust `#[get]`, `#[post]`, ... |
| `middleware.ts` | `App::new().middleware(...)` |
| no `app/sitemap.ts` | automatic sitemap of every known path |
| `revalidatePath()` after an action | `ctx.revalidate_path()` / `voltrs::revalidate_path()` for cached pages and client route data; `<Form>` refreshes the current page automatically, or `router.refresh()` |
| `useActionState`, `useFormStatus` | same; `volt-rs/actions` also exports a `useActionState` that returns action errors |
| `formData.get("file")` in an action | a `File` field in the action input, or `ctx.multipart()` |
| `cookies()`, `headers()` | `ctx.cookie()`, `ctx.header()`, `ctx.set_cookie()` |

## Example

Next.js:

```tsx
// app/posts/[id]/page.tsx
export default async function Post({ params }) {
  const { id } = await params;
  const post = await db.post.findUnique({ where: { id } });
  if (!post) notFound();
  return <Article post={post} />;
}
```

volt:

```rust
// server/main.rs
#[loader("/posts/[id]")]
async fn post(ctx: Ctx) -> Result<Post> {
    let id: i64 = ctx.param_as("id")?;
    ctx.state::<Db>().post(id).await?.ok_or_else(not_found)
}
```

```tsx
// app/posts/[id]/page.tsx
import type { PageProps } from "@volt/api";

export default function Post({ data }: PageProps<"/posts/[id]">) {
  return <Article post={data} />;
}
```

## Not supported yet

- React Server Components. Every component renders on the server (streaming Suspense boundaries) and hydrates in the browser; data that needs the server comes from Rust loaders, which run before rendering starts.
- Intercepting routes in `volt export` builds (client navigations render the original route) and `metadata` from slot pages.
- `next/og`, edge runtime.
- `next/font` metric-adjusted fallback fonts (`adjustFontFallback`) and the Google `axes` option.
- Pages Router `getInitialProps`, i18n locale domains and `req`/`res` in `getServerSideProps` (see [Pages Router](./pages-router.md#not-supported)).
