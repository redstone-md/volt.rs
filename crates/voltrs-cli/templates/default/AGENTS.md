# AGENTS.md

This is a [volt.rs](https://github.com/redstone-md/volt.rs) app: React 19 pages, server-rendered by a Rust server. Next.js conventions apply unless stated below.

## Commands

| Task | Command |
| --- | --- |
| Dev server with live reload | `npm run dev` |
| Production build | `npm run build` |
| Run production server | `npm run start` |
| Type-check TypeScript | `npm run typecheck` |
| Lint Rust | `cargo clippy` |
| Regenerate the typed client | `npx volt sdk` |
| List routes | `npx volt routes` |
| Add a shadcn/ui component | `npx shadcn@latest add <name>` |

## Layout

- `app/` file-system routes: `page.tsx`, `layout.tsx`, `loading.tsx`, `error.tsx`, `not-found.tsx`, `template.tsx`. Segments: `[id]`, `[...slug]`, `[[...slug]]`, `(group)`. Folders starting with `_` are private. Parallel routes: `@name` folders are slots passed to the segment's layout as the `name` prop (`LayoutProps<{}, "name">`); add `default.tsx` to each slot, or unmatched hard loads 404. A failing slot loader renders the slot's own `not-found.tsx` or `error.tsx`. Intercepting routes: `@modal/(.)photos/[id]/page.tsx` renders `/photos/[id]` in the modal slot on client navigation, while a refresh renders `photos/[id]/page.tsx`; add `@modal/[...catchAll]/page.tsx` returning `null` to close the modal when navigating elsewhere.
- Pages stream: wrap slow parts in `<Suspense>` (`use(promise)`, `dynamic()` from `volt-rs/dynamic`) and add `loading.tsx` to a segment for instant loading UI on navigation. Server timers are real; crawlers get the complete page.
- `server/main.rs` Rust loaders, actions, API routes and shared state.
- `components/ui/` shadcn/ui components. `lib/utils.ts` exports `cn()`.
- `public/` static files served at `/`.
- `volt.toml` configuration.
- `.volt/` generated output. Never edit it. `.volt/types/api.ts` is the typed client imported as `@volt/api`.

## How data flows

1. A page gets its data from a Rust loader for its route:
   ```rust
   #[loader("/posts/[slug]")]
   async fn post(ctx: Ctx) -> Result<Post> { /* ctx.param("slug") */ }
   ```
   ```tsx
   import type { PageProps } from "@volt/api";
   export default function Post({ data, params }: PageProps<"/posts/[slug]">) {}
   ```
2. Mutations are Rust actions, called from React through `@volt/api` (snake_case in Rust, camelCase in TypeScript):
   ```rust
   #[action]
   async fn add_todo(ctx: Ctx, input: NewTodo) -> Result<Todo> {}
   ```
   ```tsx
   import { addTodo } from "@volt/api";
   import Form from "volt-rs/form";
   <Form action={addTodo}><input name="title" /></Form>
   ```
   `<Form>` works without JavaScript and refreshes the route data after success. Outside forms call `await addTodo({ title })` and then `useRouter().refresh()`, or call `ctx.revalidate_path("/todos")` in the action so the client refreshes matching routes itself. `useFormStatus` (react-dom) and `useActionState` (`volt-rs/actions`, returns `[state, formAction, pending, error]`, and gets the state back after a no-JavaScript post too) work with actions.
   File uploads: add a `file: File` (or `Option<File>`, `Vec<File>`) field to the input model and an `<input type="file" name="file" />` to the form. API routes read uploads with `ctx.multipart()`. Bodies are limited by `[server] body_limit_mb` (default 8).
3. Types shared with TypeScript are `#[model]` structs or enums. Fields become camelCase. `Option<T>` becomes an optional field.
4. JSON APIs: `#[get("/api/x")]`, `#[post(...)]`, `#[put(...)]`, `#[patch(...)]`, `#[delete(...)]` returning `Json<T>`, `String` or `Response`.

`npm run dev` regenerates `@volt/api` whenever Rust code changes.

## Rules

- Components render on the server (inside QuickJS) and in the browser. Touch `window`, `document` and `localStorage` only inside effects or event handlers. Do not use Node APIs in components.
- No async server components, no `"use server"`, no `fetch` to your own backend from components: load data with a loader, mutate with an action.
- Navigation: `volt-rs/link` and `volt-rs/navigation` (`useRouter`, `usePathname`, `useSearchParams`, `useParams`, `useSelectedLayoutSegment`, `notFound`, `redirect`). `next/link`, `next/navigation`, `next/image`, `next/script` and `next/head` imports also work.
- Pages Router: a `pages/` directory (`index.tsx`, `[slug].tsx`, `_app.tsx`, `_document.tsx`, `404.tsx`) works next to `app/`; a URL may not exist in both. Page props are the Rust loader output (spread when it is an object) merged with `getServerSideProps` / `getStaticProps` props. Use `next/router` (`volt-rs/router`) and `next/document` (`volt-rs/document`) there. `getStaticProps` must not rely on a Rust loader that reads the request (such pages are never cached). `pages/api` is ignored: write `app/**/route.ts` handlers or Rust handlers.
- Route handlers and middleware (Next.js style, run in QuickJS with Web APIs only: `Request`, `Response`, `Headers`, `URL`, `FormData`, `fetch`, no Node APIs): `app/**/route.ts` exports `GET`, `POST`, `PUT`, `PATCH`, `DELETE`, `HEAD` or `OPTIONS` taking `(request, { params })` and returning a `Response` (`NextRequest` / `NextResponse` from `next/server`). A folder holds either `page.tsx` or `route.ts`, never both. `export const dynamic = "force-static"` builds a `GET` handler once. `middleware.ts` at the root exports `middleware(request)` returning `NextResponse.next()`, `redirect()`, `rewrite()` or a `Response`, with `export const config = { matcher: ["/dashboard/:path*"] }` written as literals (`{ source, has, missing }` objects work); it also applies to client navigations but not to server actions, so check auth inside every action. Return a `ReadableStream` body to stream (server-sent events). Under `[build] base_path`, `request.nextUrl.pathname` excludes the base path; clone `nextUrl` and set `pathname` to redirect below it. Server-side `fetch` reaches public hosts only unless `[server] fetch_allow` lists them (needed for `localhost`). Rust API routes (`#[get]`, ...) answer first; prefer them for anything that needs state, the database or Node-only libraries.
- Internationalized routing: `[i18n] locales = ["en", "de"]` in `volt.toml` serves the default locale unprefixed and the others under `/de/...`. Write hrefs without the prefix: `<Link href="/about">` keeps the current locale, `<Link href="/about" locale="de">` or `router.push("/about", { locale: "de" })` switches it. Read it with `useLocale()` from `volt-rs/navigation` (use it for `<html lang>` in the root layout), and in Rust with `ctx.locale()`. hreflang tags and sitemap alternates are automatic; `[[redirects]]` and `redirect()` keep the locale, except for targets that are API routes or `public/` files.
- Metadata: export `metadata` or `generateMetadata` from a page or layout. The root layout sets `title.template`. Icons and share images are files in the segment they apply to: `app/favicon.ico`, `icon.png`, `apple-icon.png`, `opengraph-image.png` (alt text in `opengraph-image.alt.txt`), `twitter-image.png`; volt links them with their sizes. `app/sitemap.ts`, `app/robots.ts` and `app/manifest.ts` default-export the Next.js `MetadataRoute` objects (they run in QuickJS: no Node APIs); their output is cached until `export const revalidate = <seconds>` passes or a revalidation, and `dynamic = "force-dynamic"` renders them per request. Two metadata files at the same URL (for example in two route groups) fail the build.
- Errors: `error.tsx` catches errors of its segment, `not-found.tsx` renders for `notFound()` thrown below it, and `app/global-error.tsx` (with its own `<html>` and `<body>`) replaces everything when the root layout fails.
- Images: `volt-rs/image` (`<Image src="/photo.jpg" width={800} height={600} alt="" />`) resizes local images on demand.
- Rust errors: `Error::bad_request("...")`, `Error::validation([("title", "Required")])` (422, read `error.fieldErrors` in React), `not_found()`, `redirect("/login")`, or `?` on any error (becomes a 500).
- Shared state: register with `App::new().state(value)` and read with `ctx.state::<T>()`.
- Static pages: dynamic routes export `generateStaticParams()` (from the page or a parent layout) to be prerendered by `npm run build`; add `export const dynamicParams = false` to answer other params with 404. `export const revalidate = 60` re-renders a cached page in the background after 60 seconds; `revalidate = 0` renders on every request. `export const dynamic = "force-static"` caches one render per pathname and ignores the query (a Rust loader then gets no query, cookies or headers, only params); `"error"` also fails the build when the route has a Rust loader. Generators run in QuickJS at build time: no Node APIs; `fetch` reaches public hosts and those listed in `[server] fetch_allow`. Pages with a Rust loader are never prerendered.
- After a mutation, purge cached pages with `ctx.revalidate_path("/posts/[slug]")` or `ctx.revalidate_prefix("/posts")`.
- Styling: Tailwind CSS v4 in `app/globals.css`. Merge classes with `cn()`. CSS Modules (`*.module.css`, `*.module.scss`) and Sass also work.
- Fonts: `next/font/google` (`Inter({ subsets: ["latin"], variable: "--font-sans" })`) or `next/font/local` called at module top level with literal options; apply `font.className` or `font.variable` in the root layout.

## Before you finish

Run `npm run typecheck`, `cargo clippy` and `npm run build`. All three must pass.
