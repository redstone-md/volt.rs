# Coming from Next.js

volt keeps the App Router file layout, the component APIs and the metadata API. What changes is where server code lives: instead of server components and `"use server"` functions in TypeScript, the server is Rust.

## What carries over unchanged

| Next.js | volt |
| --- | --- |
| `app/**/page.tsx`, `layout.tsx`, `loading.tsx`, `error.tsx`, `not-found.tsx`, `template.tsx` | same |
| `[slug]`, `[...slug]`, `[[...slug]]`, `(group)`, `_private` | same |
| `next/link`, `next/navigation`, `next/image`, `next/script`, `next/head`, `next/form`, `next/dynamic` | aliased to `volt-rs/*` |
| `export const metadata`, `generateMetadata`, `export const viewport` | same |
| `params` / `searchParams` props (sync or awaited) | same |
| `export const revalidate`, `export const dynamic = "force-dynamic"` | same |
| Tailwind CSS, shadcn/ui, CSS imports, `@/*` alias | same |
| `NEXT_PUBLIC_*`, `.env*` files | same (`VOLT_PUBLIC_*` also works) |
| `public/` | same |
| `next.config.js` redirects, rewrites, headers | `volt.toml` |

## What changes

| Next.js | volt |
| --- | --- |
| async server component that queries data | Rust `#[loader("/route")]`, page receives `data` |
| `"use server"` action | Rust `#[action]`, imported from `@volt/api` |
| `app/api/**/route.ts` | Rust `#[get]`, `#[post]`, ... |
| `middleware.ts` | `App::new().middleware(...)` |
| `app/sitemap.ts` | automatic, plus Rust `#[sitemap]` providers |
| `revalidatePath()` after an action | `<Form>` refreshes automatically, or `router.refresh()` |
| `cookies()`, `headers()` | `ctx.cookie()`, `ctx.header()`, `ctx.set_cookie()` |
| `next/font` | `@fontsource/*` packages or `@font-face` in CSS |

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

- React Server Components and streaming. Every component renders on the server and hydrates in the browser.
- Parallel (`@slot`) and intercepting (`(.)`) routes.
- `next/font`, `next/og`, edge runtime, Pages Router.
- File uploads in actions (multipart bodies).
