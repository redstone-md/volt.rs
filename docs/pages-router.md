# Pages Router

volt supports the Next.js Pages Router next to the App Router. A project can have only `pages/`, only `app/`, or both. `src/pages/` works as well as `pages/`.

## Files

| File | URL |
| --- | --- |
| `pages/index.tsx` | `/` |
| `pages/about.tsx` | `/about` |
| `pages/blog/index.tsx` | `/blog` |
| `pages/blog/[slug].tsx` | `/blog/:slug` |
| `pages/docs/[...path].tsx` | `/docs/*` (one or more segments) |
| `pages/shop/[[...rest]].tsx` | `/shop` and `/shop/*` |

Special files in the root of `pages/`:

| File | Role |
| --- | --- |
| `_app.tsx` | Wraps every page: `export default function App({ Component, pageProps, router })`. Stays mounted across client navigations. |
| `_document.tsx` | The HTML shell, rendered on the server only, with `Html`, `Head`, `Main` and `NextScript` from `next/document` (`volt-rs/document`). |
| `404.tsx` | Page for unknown URLs and `notFound: true`. |
| `500.tsx` / `_error.tsx` | Page for server errors; receives `statusCode` and `error`. |

Files and folders starting with `_` or `.` are not routes, nor are `.d.ts` files. `pages/api/**` is ignored with a warning: write App Router [route handlers](./data.md#route-handlers) (`app/api/**/route.ts`) or Rust handlers (`#[get]`, `#[post]`, ...).

A URL defined in both `app/` and `pages/` is a build error. When both directories exist, an explicit `app/not-found.tsx` / `app/error.tsx` is used for unknown URLs and errors; otherwise `pages/404.tsx` / `pages/500.tsx` are; otherwise the built-in App Router pages. Navigating between a Pages Router page and an App Router page performs a full page load, as in Next.js.

## Document

```tsx
// pages/_document.tsx
import { Html, Head, Main, NextScript } from "next/document";

export default function Document() {
  return (
    <Html lang="fr">
      <Head />
      <body className="antialiased">
        <Main />
        <NextScript />
      </body>
    </Html>
  );
}
```

The document is never hydrated, so event handlers and state do not belong in it. `<Main />` renders `<div id="__next">`, which the page tree renders and hydrates into. `<Head>` always emits `<meta charset="utf-8">` and a `width=device-width, initial-scale=1` viewport. `NextScript` is a placeholder: the server injects the stylesheet, `modulepreload` hints, the page state and the client entry itself. Class documents (`class MyDocument extends Document`) and `static getInitialProps` work; `ctx.renderPage()` returns an empty result because the page is rendered in the same pass. Without `_document`, a default document is used; its `<html lang>` is the active locale (the URL's `[i18n]` locale, else the default locale, else `[site] language`, which defaults to `en`).

Per-page head tags go through `next/head` (`volt-rs/head`). React 19 hoists `<title>`, `<meta>` and `<link>` into `<head>`; the `key` deduplication of Next.js is not implemented, so render each tag once.

## Data

A page's props are built from, in order:

1. The output of a Rust `#[loader("/route")]` for the route. An object is spread into the props; any other value is passed as `data`.
2. `getServerSideProps` or `getStaticProps` exported from the page, evaluated in QuickJS on the server. Their `props` are merged over the loader output.

```tsx
// pages/blog/[slug].tsx
import type { GetServerSideProps } from "next";

export const getServerSideProps: GetServerSideProps = async ({ params, query, resolvedUrl }) => {
  if (params.slug === "old") return { redirect: { destination: "/blog/new", permanent: true } };
  const post = await fetch(`https://api.example.com/posts/${params.slug}`).then((r) => (r.ok ? r.json() : null));
  if (!post) return { notFound: true };
  return { props: { post } };
};
```

- `getServerSideProps(context)` runs on every request and every client navigation. The context has `params`, `query` (search params merged with params), `resolvedUrl` and, with `[i18n]`, `locale`, `locales` and `defaultLocale`; `req`, `res` and preview data are not available. Use a Rust loader for cookies, headers and database access.
- `getStaticProps({ params })` runs at build time when the page is prerendered. A numeric `revalidate` re-renders the page on the first request after that many seconds. The HTML and its props are cached per pathname (the query string is ignored, as `getStaticProps` cannot see it) and client navigations receive the cached props instead of running `getStaticProps` again. In development it runs on every request.
- A page with `getStaticProps` and a Rust loader is rendered per request and sent with private cache headers, because the loader can read cookies and headers; `volt build` warns about it. Use `getServerSideProps` or drop `getStaticProps` on such pages.
- `getStaticPaths()` lists the concrete paths of a dynamic `getStaticProps` page, as `{ params }` objects or strings. They are prerendered at build. With `fallback: false` other paths are 404; with `true` or `"blocking"` they render on the first request and are cached (both behave like `"blocking"`: there is no fallback state). In production `getStaticPaths` runs once per render worker; pages rendered at runtime share the `[server] page_cache_mb` budget of the page cache (prerendered pages are not counted), least recently used first out.
- All three may return `{ notFound: true }` or `{ redirect: { destination, permanent } }` (or `statusCode`).

Browser chunks import only the page's default export, so tree-shaking drops `getServerSideProps`, `getStaticProps` and `getStaticPaths` from them; modules imported by those functions are dropped too unless they have side effects. Keep secrets in Rust loaders all the same.

Pages without any data needs, and `getStaticProps` pages, are prerendered by `volt build` (`[build] prerender = true`, the default).

## Router

```tsx
import { useRouter } from "next/router"; // or "volt-rs/router"

const router = useRouter();
router.pathname; // "/blog/[slug]"
router.asPath; // "/blog/hello?ref=nav"
router.query; // { slug: "hello", ref: "nav" }
router.push("/blog/other");
router.push({ pathname: "/blog/[slug]", query: { slug: "other" } });
router.replace("/login", undefined, { scroll: false });
router.push("/search?q=volt", undefined, { shallow: true });
router.back();
router.reload();
router.prefetch("/blog/other");
```

`useRouter()` returns `pathname` (the route pattern, `/404` on the not-found page), `route`, `asPath` (without the locale prefix), `query`, `locale`, `locales` and `defaultLocale` (with [`[i18n]`](i18n.md), else `undefined`), `isReady` (always `true`, pages render on the server), `isFallback` (always `false`), `basePath`, and `push(url, as?, options?)`, `replace`, `back`, `forward`, `reload`, `prefetch`, `beforePopState` and `events`. `shallow: true` updates the URL and `query` on the same pathname without fetching data.

`router.events` emits `routeChangeStart(url, { shallow })`, `beforeHistoryChange`, `routeChangeComplete` after the new page has rendered, and `routeChangeError(error, url)` (with `error.cancelled` when a newer navigation replaced it):

```tsx
useEffect(() => {
  const onDone = (url: string) => analytics.page(url);
  router.events.on("routeChangeComplete", onDone);
  return () => router.events.off("routeChangeComplete", onDone);
}, [router.events]);
```

The default export is the `Router` singleton with the same fields, usable outside components in the browser after hydration; `withRouter(Component)` injects a `router` prop. `next/link` works unchanged.

`useRouter` from `volt-rs/router` also works in App Router pages. App Router code should prefer `volt-rs/navigation`, whose `useRouter` returns `push`, `replace`, `refresh`, `back`, `forward` and `prefetch`.

## Types

`next` (`volt-rs`) exports `NextPage`, `GetServerSideProps`, `GetStaticProps`, `GetStaticPaths`, their context and result types, `InferGetServerSidePropsType` and `InferGetStaticPropsType`; `next/app` exports `AppProps` and the default `App` class. The `next` and `next/*` module declarations come with `volt-rs/env` (listed in the template `tsconfig.json` under `types`), so these imports type-check without installing Next.js.

## Not supported

- `getInitialProps` on pages and `_app`.
- i18n locale domains, `basePath` in `next.config` (use `[build] base_path`), preview and draft mode.
- `req` / `res` in `getServerSideProps` (use a Rust loader).
- The `fallback: true` loading state (`router.isFallback`); it behaves like `"blocking"`.
- `pages/api` routes with `(req, res)` (write `app/**/route.ts` [route handlers](./data.md#route-handlers) or Rust handlers).
- `next/head` `key` deduplication.
