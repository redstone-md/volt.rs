# AGENTS.md

This is a [volt.rs](https://github.com/redstone-md/volt.rs) app: React 19 pages, server-rendered by the volt Rust server. Next.js conventions apply unless stated below. It has no Rust code yet; to add loaders and actions, create `Cargo.toml` and `server/main.rs` as in the `default` template.

## Commands

| Task | Command |
| --- | --- |
| Dev server with live reload | `npm run dev` |
| Production build | `npm run build` |
| Run production server | `npm run start` |
| Static site in `out/` (no server) | `npx volt export` |
| Type-check TypeScript | `npm run typecheck` |
| Regenerate the typed client | `npx volt sdk` |
| List routes | `npx volt routes` |
| Add a shadcn/ui component | `npx shadcn@latest add <name>` |

## Layout

- `app/` file-system routes: `page.tsx`, `layout.tsx`, `loading.tsx`, `error.tsx`, `not-found.tsx`, `template.tsx`. Segments: `[id]`, `[...slug]`, `[[...slug]]`, `(group)`. Folders starting with `_` are private.
- Pages stream: wrap slow parts in `<Suspense>` (`use(promise)`, `dynamic()` from `volt-rs/dynamic`) and add `loading.tsx` to a segment for instant loading UI on navigation. Server timers are real; crawlers get the complete page.
- `components/ui/` shadcn/ui components. `lib/utils.ts` exports `cn()`.
- `public/` static files served at `/`.
- `volt.toml` configuration.
- `.volt/` generated output. Never edit it. `.volt/types/api.ts` is the typed client imported as `@volt/api`.

## Rules

- Components render on the server (inside QuickJS) and in the browser. Touch `window`, `document` and `localStorage` only inside effects or event handlers. Do not use Node APIs in components.
- No async server components and no `"use server"`.
- Navigation: `volt-rs/link` and `volt-rs/navigation` (`useRouter`, `usePathname`, `useSearchParams`, `useParams`, `notFound`, `redirect`). `next/link`, `next/navigation`, `next/image`, `next/script` and `next/head` imports also work.
- Pages Router: a `pages/` directory (`index.tsx`, `[slug].tsx`, `_app.tsx`, `_document.tsx`, `404.tsx`) works next to `app/`; a URL may not exist in both. Page props are the Rust loader output (spread when it is an object) merged with `getServerSideProps` / `getStaticProps` props. Use `next/router` (`volt-rs/router`) and `next/document` (`volt-rs/document`) there. `getStaticProps` must not rely on a Rust loader that reads the request (such pages are never cached). `pages/api` is ignored: write Rust handlers.
- Metadata: export `metadata` or `generateMetadata` from a page or layout. The root layout sets `title.template`.
- Images: `volt-rs/image` (`<Image src="/photo.jpg" width={800} height={600} alt="" />`) resizes local images on demand.
- Static pages: dynamic routes export `generateStaticParams()` (from the page or a parent layout) to be prerendered by `npm run build`; add `export const dynamicParams = false` to answer other params with 404. `export const revalidate = 60` re-renders a cached page in the background after 60 seconds; `revalidate = 0` renders on every request. Generators run in QuickJS at build time: no Node APIs, no network.
- Hrefs, redirects and `<Image src>` are written without `[build] base_path`; volt adds it. Plain `<img>` and `<a>` to `public/` files need the prefix.
- Styling: Tailwind CSS v4 in `app/globals.css`. Merge classes with `cn()`. CSS Modules (`*.module.css`, `*.module.scss`) and Sass also work.
- Fonts: `next/font/google` (`Inter({ subsets: ["latin"], variable: "--font-sans" })`) or `next/font/local` called at module top level with literal options; apply `font.className` or `font.variable` in the root layout.

## Before you finish

Run `npm run typecheck` and `npm run build`. Both must pass.
