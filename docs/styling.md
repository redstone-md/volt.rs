# Styling and assets

## Tailwind CSS

Templates ship with Tailwind CSS v4. Import it from a stylesheet and import the stylesheet from the root layout:

```css
/* app/globals.css */
@import "tailwindcss";
```

```tsx
// app/layout.tsx
import "./globals.css";
```

volt detects `@import "tailwindcss"` and compiles CSS with the Tailwind CLI from `node_modules` (`@tailwindcss/cli`). Class names are discovered automatically from the project files. In production the output is minified.

## shadcn/ui

Templates include `components.json`, `lib/utils.ts` with `cn()`, theme variables in `app/globals.css` and a few components in `components/ui/`. Add more with the shadcn CLI:

```bash
npx shadcn@latest add dialog dropdown-menu
```

Components that import `next/link` or `next/navigation` work as-is.

Tailwind can also run through PostCSS: if `@tailwindcss/cli` is not installed but `postcss.config.*` lists `@tailwindcss/postcss` (the usual Next.js setup), volt compiles Tailwind with PostCSS instead.

## Global CSS and Sass

Any `.css`, `.scss` or `.sass` file can be imported from a component. All imported stylesheets (global files, CSS modules and fonts) are combined in module evaluation order, so the root layout's styles come before a page's, into one hashed file linked from every page.

Without Tailwind, every stylesheet goes through [Lightning CSS](https://lightningcss.dev):

- `@import` of relative files and npm packages (`@import "pkg/dist/style.css"`) is inlined.
- Nesting, modern color syntax and other newer features are lowered and vendor prefixes are added for your browser targets: the `browserslist` field of `package.json` (a query string, an array, or `{ "production": [...] }`), or Next.js' defaults (`chrome 64, edge 79, firefox 67, opera 51, safari 12`).
- In production the output is minified.

Sass is compiled in Rust by [grass](https://github.com/connorskees/grass), no `sass` package needed. `@use` and `@import` resolve relative to the file, then from `node_modules` (`@use "bootstrap/scss/bootstrap"`):

```scss
// app/globals.scss
@use "./theme"; // app/_theme.scss

.card {
  padding: theme.$gap;
  &:hover { color: theme.$accent; }
}
```

As with Sass itself, `url()`s in a partial resolve relative to the file that was compiled (the one you import from code), not the partial.

## CSS Modules

Files named `*.module.css`, `*.module.scss` or `*.module.sass` are CSS modules: the import is an object of class names scoped to the file.

```css
/* app/page.module.css */
.base { letter-spacing: 0.02em; }
.title { composes: base; color: crimson; }
```

```tsx
import styles from "./page.module.css";

<h1 className={styles.title}>Hello</h1>; // class="page-module__title__a1B2c3 page-module__base__a1B2c3"
```

Class names have the form `[file]__[class]__[hash]`, where the hash is derived from the file path, so they stay the same while you edit the styles. `composes` works with classes of the same file, `composes: x from global`, and `composes: x from "./other.module.css"`. Animation, grid and container names are scoped too; custom properties are not. The scoped CSS joins the global stylesheet.

## url() assets

Relative `url()` references in CSS, Sass, CSS modules and Tailwind output (`background: url("./hero.png")`, fonts in `@font-face`) are copied to `/_volt/static/media/` with a content hash and served with immutable caching. Absolute paths (`/images/x.png` from `public/`), `data:` URIs and remote URLs are left as written; `~pkg/file.png` resolves from `node_modules`. A reference that does not exist is reported as a warning and kept unchanged.

## PostCSS

If the project has a `postcss.config.mjs`, `postcss.config.js`, `postcss.config.cjs` or `.postcssrc.json` and `postcss` is installed, volt runs its plugins with Node.js on the combined stylesheet, before minification. Both the object form (`plugins: { autoprefixer: {} }`) and the array form are supported. When the stylesheet was already compiled by the Tailwind CLI, Tailwind's own PostCSS plugin is skipped. Without a config or without `postcss` installed, nothing runs.

## Fonts

`volt-rs/font/google` and `volt-rs/font/local` (also importable as `next/font/google` and `next/font/local`) load fonts at build time, like Next.js:

```tsx
// app/layout.tsx
import { Inter, Roboto_Mono } from "next/font/google";
import localFont from "next/font/local";

const inter = Inter({ subsets: ["latin"], variable: "--font-sans", display: "swap" });
const mono = Roboto_Mono({ subsets: ["latin"], weight: ["400", "700"] });
const brand = localFont({
  src: [
    { path: "./fonts/Brand-Regular.woff2", weight: "400", style: "normal" },
    { path: "./fonts/Brand-Bold.woff2", weight: "700", style: "normal" },
  ],
  variable: "--font-brand",
  fallback: ["system-ui", "sans-serif"],
});

export default function RootLayout({ children }: LayoutProps) {
  return (
    <html lang="en" className={`${inter.variable} ${brand.variable}`}>
      <body className={inter.className}>{children}</body>
    </html>
  );
}
```

Each call becomes `{ className, style: { fontFamily, fontWeight?, fontStyle? }, variable? }`. `className` sets `font-family` (and the weight and style when exactly one was requested); `variable` is a class that defines the CSS custom property, for use in Tailwind (`--font-sans` in `@theme`) or your own CSS. The generated `@font-face` rules use a unique family name (`__Inter_a1b2c3`) and join the global stylesheet.

- **Google fonts** are downloaded from `fonts.googleapis.com` during the build and served from your app; nothing is requested from Google at runtime. Responses and font files are cached in `.volt/cache/fonts`, so later builds work offline. Options: `weight` (a value, an array or `"variable"`; omitted means the full variable weight range, or 400 for static families), `style`, `subsets`, `display` (default `swap`), `variable`, `fallback`, `preload`. Any family works: replace spaces with underscores (`Noto_Sans_JP`). If the download fails the build stops with the requested URL.
- **Local fonts**: `src` is a path relative to the calling file or an array of `{ path, weight, style }`. Files are copied to `/_volt/static/media/`. Options: `weight`, `style`, `display`, `variable`, `fallback`, `preload`, `declarations` (extra `@font-face` descriptors).

Options must be literals (strings, numbers, arrays, objects) because they are evaluated at build time, and the loader must be called directly (`const inter = Inter({...})`).

Fonts declared by the root layout or a module it imports are preloaded on every page with `<link rel="preload" as="font" crossorigin>`; for Google fonts only the requested `subsets` are preloaded, and nothing is preloaded when `subsets` is omitted. Set `preload: false` to opt out.

Not supported: the metric-adjusted fallback fonts that Next.js generates (`adjustFontFallback` is accepted and ignored) and Google variable axes other than weight (`axes`).

## Static files

Files in `public/` are served from the site root (`public/robots.txt` at `/robots.txt`) with ETag revalidation.

## Imported assets

Importing an image or font from code copies it into the build with a content hash:

```tsx
import hero from "./hero.jpg";

<Image src={hero} alt="" priority />;
```

Images import as `{ src, width, height }`, other files as a URL string.

## Images

```tsx
import Image from "volt-rs/image";

<Image src="/photos/team.jpg" alt="The team" width={1200} height={800} sizes="(max-width: 768px) 100vw, 50vw" />
<Image src="/cover.jpg" alt="" fill priority />
```

For local images volt generates a `srcset` served by `/_volt/image`, which resizes on demand and caches the result on disk. JPEG keeps JPEG (with the requested quality), PNG and GIF become PNG, WebP stays WebP. `priority` preloads the image with `fetchpriority="high"`; everything else is lazy-loaded. Set `unoptimized` to serve the original, or pass a custom `loader` for an image CDN.

## Scripts

```tsx
import Script from "volt-rs/script";

<Script src="https://example.com/analytics.js" strategy="lazyOnload" />
```

Strategies: `beforeInteractive`, `afterInteractive` (default), `lazyOnload`.
