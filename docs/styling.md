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

## Plain CSS

Any `.css` file can be imported from a component. All imported stylesheets are combined, in import order, into one hashed file linked from every page. Without Tailwind, files are concatenated as written.

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
