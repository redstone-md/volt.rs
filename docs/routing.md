# Routing

volt uses the Next.js App Router file conventions. Every folder in `app/` is a URL segment and a `page.tsx` makes it routable. The Pages Router (`pages/`) is supported too, alone or next to `app/`: see [Pages Router](./pages-router.md).

| Path | URL |
| --- | --- |
| `app/page.tsx` | `/` |
| `app/about/page.tsx` | `/about` |
| `app/blog/[slug]/page.tsx` | `/blog/hello` |
| `app/docs/[...path]/page.tsx` | `/docs/a/b/c` |
| `app/shop/[[...rest]]/page.tsx` | `/shop` and `/shop/a/b` |
| `app/(marketing)/pricing/page.tsx` | `/pricing` (groups do not affect the URL) |
| `app/_components/x.tsx` | not routable (private folder) |

When several routes match, static segments win over dynamic ones, and dynamic ones over catch-alls.

## Special files

| File | Purpose |
| --- | --- |
| `layout.tsx` | Wraps the segment and its children. The root layout renders `<html>` and `<body>`. |
| `page.tsx` | The UI of a route. |
| `loading.tsx` | Suspense fallback for the segment. |
| `error.tsx` | Error boundary. Receives `{ error, reset }`. |
| `not-found.tsx` | Rendered for `notFound()` and, at the root, for unknown URLs. |
| `template.tsx` | Like a layout, but remounted on navigation. |

Files may use `.tsx`, `.ts`, `.jsx` or `.js`.

## Pages and layouts

```tsx
// app/blog/layout.tsx
import type { LayoutProps } from "volt-rs";

export default function BlogLayout({ children }: LayoutProps) {
  return <section className="mx-auto max-w-2xl">{children}</section>;
}
```

```tsx
// app/blog/[slug]/page.tsx
import type { PageProps } from "volt-rs";

export default function Post({ params, searchParams }: PageProps<undefined, { slug: string }>) {
  return <p>{params.slug} {searchParams.q}</p>;
}
```

`params` and `searchParams` are plain objects that can also be awaited, so code written for Next.js 15 (`const { slug } = await params`) keeps working.

Pages with a Rust loader get its result as `data`; see [Data](./data.md).

## Linking and navigation

```tsx
import Link from "volt-rs/link";
import { useRouter, usePathname, useSearchParams, useParams } from "volt-rs/navigation";

<Link href="/blog/hello">Read</Link>
<Link href={{ pathname: "/search", query: { q: "rust" } }} replace scroll={false}>Search</Link>

const router = useRouter();
router.push("/dashboard");
router.replace("/login");
router.refresh();
router.prefetch("/settings");
```

`<Link>` renders a plain `<a>`, so pages work without JavaScript. With JavaScript, navigation is client-side: volt fetches the target route's loader data from `/_volt/data` and its code chunk, then swaps the page without a full reload. Links prefetch when they enter the viewport and on hover. Scroll position is restored on back and forward.

`next/link`, `next/navigation`, `next/image`, `next/script`, `next/head`, `next/form` and `next/dynamic` resolve to their volt equivalents, so existing components and shadcn blocks work unchanged.

## Not found and redirects

```tsx
import { notFound, redirect } from "volt-rs/navigation";

if (!post) notFound();
if (!user) redirect("/login");
```

Both work during server rendering (the response becomes a 404 or a 307) and on the client. From Rust, return `Err(not_found())` or `Err(redirect("/login"))` from a loader or action.

## Client components

`"use client"` is accepted and ignored: every component is rendered on the server and hydrated in the browser. Access `window`, `document` or `localStorage` only inside effects and event handlers.
