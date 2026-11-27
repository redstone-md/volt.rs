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
| `loading.tsx` | Suspense fallback for the segment, shown at once on client navigation. See [Loading UI and streaming](#loading-ui-and-streaming). |
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

## Loading UI and streaming

> **Rust loaders run before rendering starts.** On the first request, a page with a `#[loader]` sends nothing, not even its `loading.tsx`, until the loader has returned; Next.js would stream the loading UI at once. Client navigations to such a page do show `loading.tsx` immediately (see below). To stream slow data on the first request, keep the loader fast and suspend on the slow part inside the page.

Dynamic pages stream. The server sends the shell (the layouts and everything outside pending `<Suspense>` boundaries, with their fallbacks) as soon as it is ready, then each boundary's HTML as it resolves, and React swaps it in before the page hydrates. Components can suspend with `use(promise)`, `React.lazy` or `dynamic()`:

```tsx
// app/dashboard/page.tsx
import { Suspense, use } from "react";

export const dynamic = "force-dynamic";

function Stats({ stats }: { stats: Promise<string> }) {
  return <p>{use(stats)}</p>;
}

export default function Dashboard() {
  const stats = new Promise<string>((resolve) => setTimeout(() => resolve("42 orders"), 500));
  return (
    <Suspense fallback={<p>Loading stats…</p>}>
      <Stats stats={stats} />
    </Suspense>
  );
}
```

- `loading.tsx` wraps the segment's page in a Suspense boundary, so a page that suspends streams the loading UI first.
- The status code comes from the shell: `notFound()` or `redirect()` raised before the shell is sent answers 404 or a redirect, even inside a boundary. Raised later, inside a boundary that already streamed its fallback, the page keeps its 200 and the browser shows the not-found page or follows the redirect.
- The state and the client entry are written before `</body>`, after the last boundary, so the page hydrates once the stream is complete; `modulepreload` hints in the shell let the browser fetch the code meanwhile.
- Crawlers and link-preview bots (`[server] bots`, a user-agent regex) get the complete document in one response, as do cached and prerendered pages, which wait for every boundary. `[server] streaming = false` renders every page with `renderToString` instead, where Suspense boundaries render their fallbacks.
- A render has 10 seconds. A boundary still pending then, or waiting on a promise that can never resolve, is left to the browser: the server completes the document with that boundary marked for client rendering, and React renders it after hydration. Such a page is never cached.

`dynamic(() => import("./chart"))` from `volt-rs/dynamic` (or `next/dynamic`) starts loading when it is defined, so its content is server-rendered in place, or streamed once it resolves. `{ ssr: false }` renders `loading` on the server and loads the component in the browser after hydration.

On client navigation (`<Link>`, `router.push` and `router.replace`) to a route that has a `loading.tsx` in any segment, the target's layouts and the nearest loading UI appear immediately while the loader data and the page chunk load, then the page replaces it. Navigations to routes without `loading.tsx` keep the current page on screen until the next one is ready (a React transition), and prefetched routes swap in at once.

## Not found and redirects

```tsx
import { notFound, redirect } from "volt-rs/navigation";

if (!post) notFound();
if (!user) redirect("/login");
```

Both work during server rendering (the response becomes a 404 or a 307) and on the client. From Rust, return `Err(not_found())` or `Err(redirect("/login"))` from a loader or action.

## Client components

`"use client"` is accepted and ignored: every component is rendered on the server and hydrated in the browser. Access `window`, `document` or `localStorage` only inside effects and event handlers.
