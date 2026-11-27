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

- `@import` of relative files and npm packages (`@import "pkg/dist/style.css"`) is inlined, keeping its media, `supports()` and `layer()` conditions. This also applies to `@import` of plain CSS from Sass files and CSS modules.
- Remote imports (`@import url("https://fonts.googleapis.com/css2?family=Inter")`) are kept and moved to the top of the combined stylesheet, where browsers accept them. A rule that is invalid after combining is dropped with a warning instead of silently.
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

Relative `url()`s and plain-CSS `@import "*.css"`s in a partial resolve relative to the partial, as with webpack's `resolve-url-loader` in Next.js, so `app/parts/_bg.scss` can use `url("./img/bg.png")` for `app/parts/img/bg.png`. References built with interpolation or variables (`url(#{$base}/x.png)`) are left as written and resolve relative to the compiled file.

A compile error stops the build (or shows the dev overlay) with the Sass or Lightning CSS message and its location.

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

Class names have the form `[file]__[class]__[hash]`, where the hash is derived from the file path, so they stay the same while you edit the styles. `composes` works with classes of the same file, `composes: x from global`, and `composes: x from "./other.module.css"`. Animation, grid and container names are scoped too; custom properties are not. The scoped CSS joins the global stylesheet. Importing a module always includes its CSS, even when none of its classes is used.

## url() assets

Relative `url()` references in CSS, Sass, CSS modules and Tailwind output (`background: url("./hero.png")`, fonts in `@font-face`) are copied to `/_volt/static/media/` with a content hash and served with immutable caching. Absolute paths (`/images/x.png` from `public/`), `data:` URIs and remote URLs are left as written; `~pkg/file.png` resolves from `node_modules`. A reference that does not exist is reported as a warning and kept unchanged. Tailwind inlines `@import`s itself without recording where each rule came from, so in a Tailwind build a `url()` inside a nested `@import`ed file resolves against the directories of the stylesheets imported from code; keep such references relative to those files or use `public/` paths.

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

- **Google fonts** are downloaded from `fonts.googleapis.com` during the build and served from your app; nothing is requested from Google at runtime. Responses and font files are cached in `.volt/cache/fonts`, so later builds work offline. Options: `weight` (a value, an array or `"variable"`; omitted means the full variable weight range, or the only weight of a static family; static families with several weights need an explicit `weight`), `style`, `subsets`, `display` (default `swap`), `variable`, `fallback`, `preload`. Any family works: replace spaces with underscores (`Noto_Sans_JP`). The weight ranges come from the Google Fonts family catalog, refreshed when the cached copy is older than a week (an offline build reuses the old copy with a warning). If a download fails the build stops with the requested URL.
- **Local fonts**: `src` is a path relative to the calling file or an array of `{ path, weight, style }`. Files are copied to `/_volt/static/media/`. Options: `weight`, `style`, `display`, `variable`, `fallback`, `preload`, `declarations` (extra `@font-face` descriptors).

Options must be literals (strings, numbers, arrays, objects) because they are evaluated at build time, and the loader must be called directly (`const inter = Inter({...})`).

Fonts declared by the root layout (`pages/_app` in the Pages Router) or a module it imports are preloaded on every page with `<link rel="preload" as="font" crossorigin>`; for Google fonts only the requested `subsets` are preloaded, and nothing is preloaded when `subsets` is omitted. Set `preload: false` to opt out.

Not supported: the metric-adjusted fallback fonts that Next.js generates (`adjustFontFallback` is accepted and ignored) and Google variable axes other than weight (`axes`).

## Static files

Files in `public/` are served from the site root (`public/robots.txt` at `/robots.txt`) with ETag revalidation.

## Imported assets

Importing an image or font from code copies it into the build with a content hash:

```tsx
import hero from "./hero.jpg";

<Image src={hero} alt="" priority />;
```

Images import as `{ src, width, height, blurDataURL, blurWidth, blurHeight }`, other files as a URL string. `blurDataURL` is a PNG of at most 8 pixels on its longer side, computed at build time for PNG, JPEG, WebP and GIF imports, so `placeholder="blur"` needs nothing else.

## Images

```tsx
import Image from "volt-rs/image"; // or "next/image"
import hero from "./hero.jpg";

<Image src="/photos/team.jpg" alt="The team" width={1200} height={800} sizes="(max-width: 768px) 100vw, 50vw" />
<Image src={hero} alt="" placeholder="blur" priority />
<div style={{ position: "relative", height: 320 }}>
  <Image src="https://images.example.com/cover.jpg" alt="" fill />
</div>
```

`<Image>` renders an `<img>` with explicit dimensions, lazy loading, `decoding="async"` and a `srcset` served by `/_volt/image`, which resizes on demand and caches every variant on disk (`.volt/cache/images`).

| Prop | |
| --- | --- |
| `src` | A `public/` path, a static import, or a remote URL allowed by `remote_patterns`. |
| `width`, `height` | Intrinsic size in pixels. A static import fills in what is missing from its aspect ratio. |
| `fill` | Covers the parent (which needs `position: relative`, `fixed` or `absolute`) instead; `sizes` defaults to `100vw`. Set `object-fit` with `style`. |
| `sizes` | Turns the `srcset` into width descriptors; with a `vw` value, widths below that share of the smallest device size are left out. |
| `quality` | 1 to 100, default 75, rounded to the closest of `[images] qualities` (only `75` by default). |
| `priority` / `preload` | Preloads the image from `<head>` (`<link rel="preload" as="image" imagesrcset>`) and loads it eagerly with `fetchpriority="high"`. |
| `placeholder` | `"empty"` (default), `"blur"` (uses `blurDataURL`, blurred with an SVG filter) or a `data:image/...` URL shown as is. The placeholder is removed once the image has loaded. |
| `blurDataURL` | Placeholder for images that are not static imports. |
| `onLoad`, `onError` | Called also when the image finished loading before hydration. On error the alt text becomes visible. |
| `overrideSrc` | Replaces the `src` attribute and keeps the generated `srcset`, e.g. to keep an old URL for SEO. |
| `loader` | `({ src, width, quality }) => url` for an image CDN; any `src` goes through it. |
| `unoptimized` | Serves the original file without a `srcset`. |

Without `sizes` an image with a `width` gets `1x` and `2x` candidates; without either it gets the device sizes. `getImageProps(props)` returns the `<img>` attributes without rendering, for `<picture>` art direction. The sizes, remote patterns and SVG policy come from `volt.toml` at build time, so the component only requests URLs the optimizer accepts.

### Formats

`/_volt/image` picks the first format of `[images] formats` that the request's `Accept` header lists (`image/webp` by default; add `image/avif` to prefer AVIF) and answers with `Vary: Accept`; other clients get the source format: JPEG stays JPEG at the requested quality, PNG, WebP and GIF become PNG. AVIF is encoded with rav1e (pure Rust, lossy, honours `quality`). The WebP encoder available without C libraries is lossless, so volt keeps the WebP only when it is smaller than the source format at the same size and serves the JPEG or PNG otherwise: graphics get WebP, photos usually stay JPEG unless AVIF is enabled. Lossy WebP would need `libwebp` through C bindings; the pure-Rust alternatives are licensed under the AGPL. Animated GIF, PNG and WebP, AVIF sources and formats volt cannot decode are served unchanged; an AVIF source is answered with 406 to clients whose `Accept` does not list `image/avif`. AVIF encodes take far longer than the other formats, so they use at most half of `concurrency` and cannot hold up WebP, JPEG and PNG requests. JPEG orientation from EXIF is applied.

Concurrent requests for the same variant or remote original wait for the first one instead of encoding or downloading it again. Responses carry an `ETag` (answered with 304), `Cache-Control: public, max-age=86400, stale-while-revalidate=604800` (remote images: `minimum_cache_ttl`), `Content-Disposition` with a matching file name, and `X-Volt-Cache: HIT` or `MISS`.

### Remote images

Remote URLs are optimized only when they match `[[images.remote_patterns]]`; others render as a plain `<img>` and `/_volt/image` refuses them with 400:

```toml
[[images.remote_patterns]]
protocol = "https"
hostname = "**.example.com"   # `*.` one subdomain level, `**.` any number
pathname = "/media/**"        # `*` within a segment, `**` across segments
# port = 8443                 # any port when omitted
# search = "?v=1"             # exact query; any when omitted
```

Paths with dot segments, also percent-encoded (`%2e%2e`), or encoded slashes (`%2f`, `%5c`) are refused, as origins may normalize them to a path outside the pattern. The optimizer downloads the original (HTTP/1.1 or HTTP/2, rustls with the Mozilla root certificates, 5-second connect timeout and a 10-second budget for the whole download including redirects, 20 MB at most, `Content-Type: image/*` required, at most 8 downloads at once), asks only for formats it can decode (`Accept: image/webp,image/png,image/jpeg,image/gif,image/*;q=0.5`, so CDNs that negotiate formats do not answer AVIF), follows up to three redirects that match a pattern as well, and keeps it on disk for `minimum_cache_ttl` seconds (4 hours by default); a failed refresh keeps serving the previous copy. Host names that resolve to loopback, private or link-local addresses are refused unless `dangerously_allow_local_ip = true`.

### Security

SVG can carry scripts, so `<Image>` serves SVG files unoptimized and `/_volt/image` refuses them unless `dangerously_allow_svg = true`, which serves them unchanged. Every optimizer response has `Content-Security-Policy: script-src 'none'; frame-src 'none'; sandbox;` (`content_security_policy`) and `X-Content-Type-Options: nosniff`. Sources over `max_pixels` (50 million) or with a side over `max_dimension` (16384) are refused from their header before anything is decoded, which stops decompression bombs; decoders run with matching memory limits. Widths must be one of `device_sizes` or `image_sizes` and the quality one of `qualities`, so varying `w` or `q` cannot fill the cache or keep the encoders busy. Cached variants are keyed by these security settings as well, so changing `dangerously_allow_svg`, `max_pixels` or `max_dimension` takes effect for images that were already cached.

## Scripts

```tsx
import Script from "volt-rs/script";

<Script src="https://example.com/analytics.js" strategy="lazyOnload" />
```

Strategies: `beforeInteractive`, `afterInteractive` (default), `lazyOnload`.
