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
| `app/@modal/page.tsx` | a [slot](#parallel-routes) of the root layout, not a URL segment |
| `app/@modal/(.)photos/[id]/page.tsx` | [intercepts](#intercepting-routes) `/photos/1` on client navigation |

When several routes match, static segments win over dynamic ones, and dynamic ones over catch-alls.

## Special files

| File | Purpose |
| --- | --- |
| `layout.tsx` | Wraps the segment and its children. The root layout renders `<html>` and `<body>`. |
| `page.tsx` | The UI of a route. |
| `loading.tsx` | Suspense fallback for the segment, shown at once on client navigation. See [Loading UI and streaming](#loading-ui-and-streaming). |
| `error.tsx` | Error boundary. Receives `{ error, reset }`. |
| `global-error.tsx` | At the app root only: replaces the root layout when it fails. Renders its own `<html>` and `<body>`. |
| `not-found.tsx` | Rendered for `notFound()` thrown below its segment and, at the root, for unknown URLs. |
| `template.tsx` | Like a layout, but remounted when the segment below it changes. |
| `default.tsx` | Rendered by a [parallel route](#parallel-routes) slot that has no page for the URL. |
| `route.ts` | A [route handler](./data.md#route-handlers): `GET`, `POST`, ... returning a `Response`, instead of a page (`.ts` or `.js` only). |

Files may use `.tsx`, `.ts`, `.jsx` or `.js`. Metadata files (`icon.png`, `opengraph-image.jpg`, `sitemap.ts`, ...) are described in [Metadata and SEO](./metadata-seo.md).

### Errors and not found

`notFound()` renders the closest `not-found.tsx` above the segment that called it, inside the layouts above that file, with status 404 and `<meta name="robots" content="noindex">`. That works during server rendering and after client navigation. A `notFound()` thrown by a layout is handled by the `not-found.tsx` of a parent segment, and without any nested file the root `not-found.tsx` (or the built-in page) is shown. A Rust loader returning `not_found()` always renders the root not-found page.

`error.tsx` catches errors of its segment on the client. On the server, a failing render falls back to the root `error.tsx` inside the root layout with status 500. When the root layout itself throws (on the server, or later in the browser), `app/global-error.tsx` is rendered instead of the whole document; without a root `error.tsx` it is also the server error page. `global-error.tsx` receives `{ error, reset }` and must render `<html>` and `<body>`. In development, the error overlay is shown instead.

Error and not-found boundaries reset when the pathname changes. Layouts inside them keep their state across navigations; templates are keyed by the segment directly below them, so `app/blog/template.tsx` remounts when going from `/blog/a` to `/blog/b` but a root template does not. As in Next.js, everything below a layout remounts when one of the segments between it and the next layout changes, dynamic param values included: going from `/items/1` to `/items/2` resets the state of `app/items/[id]/page.tsx`, while the layouts above keep theirs.

### Active segments

`useSelectedLayoutSegment()` returns the active segment one level below the layout that calls it, and `useSelectedLayoutSegments()` all of them. In `app/layout.tsx` on `/blog/hello`, they return `"blog"` and `["blog", "hello"]`; in `app/blog/layout.tsx`, `"hello"` and `["hello"]`. Dynamic segments return their value, route groups their folder name (`"(shop)"`), and the page itself `null`. Reading the active segment of a [parallel route](#parallel-routes) slot is not supported: a key other than `"children"` returns `null` / `[]`.

```tsx
"use client";
import Link from "next/link";
import { useSelectedLayoutSegment } from "next/navigation";

export function NavLink({ slug, children }: { slug: string; children: React.ReactNode }) {
  const active = useSelectedLayoutSegment() === slug;
  return <Link href={`/${slug}`} className={active ? "font-bold" : ""}>{children}</Link>;
}
```

### Route segment config

| Export | Effect |
| --- | --- |
| `dynamic = "auto"` | Default: static routes are prerendered, routes with a Rust loader or dynamic segments render per request. |
| `dynamic = "force-dynamic"` | Renders on every request. |
| `dynamic = "force-static"` | One cached render per pathname shared by every visitor, even with a Rust loader (its first output is cached; without a loader the page is prerendered). `searchParams` is always empty, and a Rust loader sees a request without query, cookies or headers (only the route params), so no visitor's data ends up in the shared render. Client navigations get the same data. |
| `dynamic = "error"` | Like `force-static`, and the build fails if the route has a Rust loader, which reads the request. |
| `revalidate`, `dynamicParams` | See [Static generation](./static-generation.md). |
| `fetchCache`, `runtime`, `preferredRegion`, `maxDuration` | Accepted and ignored. |

The page's export wins over its layouts'. `unstable_noStore()` (`next/cache`) and `await connection()` (`next/server`) are accepted no-ops; use `dynamic = "force-dynamic"` to opt out of caching.

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

## Parallel routes

A folder named `@name` inside a segment is a **slot**: its pages render next to the segment's page, and the segment's layout receives each slot as a prop alongside `children`. Slots do not affect the URL.

```
app/dashboard/
├── layout.tsx              receives { children, analytics, team }
├── page.tsx                /dashboard
├── default.tsx             children of /dashboard/views (only a slot has that URL)
├── settings/page.tsx       /dashboard/settings
├── @analytics/
│   ├── page.tsx            analytics at /dashboard
│   ├── views/page.tsx      analytics at /dashboard/views
│   └── default.tsx         analytics anywhere else
└── @team/
    ├── page.tsx            team at /dashboard
    ├── settings/page.tsx   team at /dashboard/settings
    └── default.tsx
```

```tsx
// app/dashboard/layout.tsx
import type { LayoutProps } from "volt-rs";

export default function Dashboard({ children, analytics, team }: LayoutProps<{}, "analytics" | "team">) {
  return (
    <>
      {children}
      <aside>{analytics}{team}</aside>
    </>
  );
}
```

Each slot is matched against the URL on its own and can have its own `layout`, `loading`, `error`, `not-found` and `template` files and nested folders, dynamic segments and catch-alls included. Slot pages receive `{ data, params, searchParams }` like pages; `default.tsx` receives `{ params }` (`DefaultProps`).

When a slot has no page for the URL:

- on a **client navigation** within the layout it keeps what it showed before (soft navigation), so `/dashboard` → `/dashboard/settings` keeps the analytics overview;
- on a **hard load** (first visit, refresh), or a client navigation that mounts the layout, it renders its `default.tsx`. A slot without `default.tsx` makes the whole page a 404, as in Next.js.

A URL that only a slot matches (`/dashboard/views` above) works the same way for `children`: a client navigation from inside the segment keeps the current page, and a hard load renders the segment's own `default.tsx` as `children` (a 404 without one). A page always wins: if `app/dashboard/[id]/page.tsx` matches the URL, it renders as `children` even when a slot page matches too. Catch-all slot pages, the usual way to clear a slot on other URLs, render where the URL exists but do not make every URL exist.

Metadata comes from the page and its layouts only. Slot pages get their own data as the `data` prop; inside a slot, `useData()` returns the slot page's data and `useParams()` adds the slot page's params to the page's. A slot page can have a Rust loader: register it under the page's folder path, groups removed, e.g. `#[loader("/dashboard/@team/settings")]`. When the loader fails with `not_found()`, the nearest `not-found.tsx` inside the slot renders in its place; any other error renders the nearest `error.tsx` inside the slot. The rest of the layout renders as usual. Without such a file in the slot, the whole page fails as if the page's own loader had.

## Intercepting routes

A folder named `(.)x`, `(..)x`, `(..)(..)x` or `(...)x` intercepts the route `x` during client-side navigation: the intercepting page renders instead, while a hard load or refresh of the same URL renders the original route. The prefix is relative to route segments (slots and groups do not count): `(.)` is the same level, `(..)` one level up, `(..)(..)` two levels up and `(...)` the app root.

The classic use is a modal: a photo opens over the gallery when clicked, and has its own full page when shared or refreshed.

```
app/
├── layout.tsx                         receives { children, modal }
├── @modal/
│   ├── default.tsx                    returns null
│   ├── [...catchAll]/page.tsx         returns null: closes the modal on other navigations
│   └── (.)photos/[id]/page.tsx        the modal
└── photos/
    ├── page.tsx                       the gallery
    └── [id]/page.tsx                  the full page
```

```tsx
// app/@modal/(.)photos/[id]/page.tsx
import { useRouter } from "volt-rs/navigation";

export default function PhotoModal({ params }: { params: { id: string } }) {
  const router = useRouter();
  return (
    <dialog open>
      <img src={`/photos/${params.id}.jpg`} alt="" />
      <button onClick={() => router.back()}>Close</button>
    </dialog>
  );
}
```

Clicking `<Link href="/photos/1">` in the gallery keeps the gallery mounted and renders the modal into `@modal`; the URL becomes `/photos/1`. Back closes the modal and forward reopens it, because each history entry remembers how it was resolved, also after the page was reloaded on another entry. Refreshing `/photos/1` renders `app/photos/[id]/page.tsx`.

Interception applies when the client navigates from a page inside the segment that holds the interception folder (any page for a folder in `app/` or `app/@modal`, pages under `/feed` for `app/feed/(..)photo/[id]`). An interception folder outside a slot, like `app/feed/(..)photo/[id]`, replaces the page itself on those navigations instead of rendering into a slot.

The client runtime sends the route it navigates from in an `x-volt-from` header on `/_volt/data` requests; that is how the server tells a client navigation from a hard load. With `volt export` there is no server to resolve interceptions, so client navigations render the original route; slots still keep their state.

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

With [`[i18n]`](i18n.md), hrefs are written without the locale prefix: `<Link>` and `router.push` keep the current locale, `locale="de"` switches it, `locale={false}` uses the href as given, and `useLocale()` returns the active locale.

`next/link`, `next/navigation`, `next/router`, `next/image`, `next/script`, `next/head`, `next/form`, `next/dynamic`, `next/cache`, `next/server`, `next/document`, `next/app`, `next/font/google`, `next/font/local` and `next` resolve to their volt equivalents (`volt-rs/...`), so existing components and shadcn blocks work unchanged.

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
