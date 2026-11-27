# Metadata and SEO

## Metadata

Export `metadata` (or `generateMetadata`) from any layout or page. Values merge from the root layout down to the page, the same way as in Next.js.

```tsx
import type { Metadata } from "volt-rs";

export const metadata: Metadata = {
  title: { default: "Acme", template: "%s · Acme" },
  description: "Tools for builders.",
  keywords: ["rust", "react"],
  alternates: { canonical: "/" },
  openGraph: { type: "website", siteName: "Acme", images: ["/og.jpg"] },
  twitter: { card: "summary_large_image", site: "@acme" },
  robots: { index: true, follow: true },
  icons: { icon: "/favicon.svg", apple: "/apple-touch-icon.png" },
  verification: { google: "token" },
};

export const viewport = { themeColor: "#0a0a0a", colorScheme: "dark light" };
```

```tsx
import type { PageProps } from "@volt/api";

export async function generateMetadata({ data }: PageProps<"/blog/[slug]">) {
  return { title: data.title, description: data.summary, openGraph: { type: "article", images: [data.cover] } };
}
```

Supported fields: `title` (string, `default`, `template`, `absolute`), `description`, `applicationName`, `generator`, `keywords`, `authors`, `creator`, `publisher`, `referrer`, `category`, `robots` (including `googleBot`), `alternates.canonical`, `alternates.languages`, `manifest`, `icons`, `openGraph`, `twitter`, `verification`, `other`, `metadataBase`; and `viewport` with `width`, `initialScale`, `maximumScale`, `userScalable`, `viewportFit`, `themeColor`, `colorScheme`.

Relative URLs in `openGraph.images`, `twitter.images`, `alternates` and `openGraph.url` are resolved against `metadataBase`, which defaults to `site.url` from `volt.toml`. The page title falls back to `site.name`.

Tags are rendered as React 19 head elements: they appear in the server HTML for crawlers and update on client-side navigation.

## Metadata files

Image and manifest files placed in `app/` are served at their Next.js URLs and added to the metadata of every route below the segment they are in:

| File | Where | Head |
| --- | --- | --- |
| `favicon.ico` | `app/` only | `<link rel="icon" href="/favicon.ico" sizes="16x16">` |
| `icon.(ico\|png\|svg\|jpg\|jpeg)`, `icon1.png`, ... | any segment | `<link rel="icon" href="/icon.png?<hash>" type="image/png" sizes="32x32">` |
| `apple-icon.(png\|jpg\|jpeg)` | any segment | `<link rel="apple-touch-icon" ...>` |
| `opengraph-image.(png\|jpg\|jpeg\|gif\|webp)` | any segment | `og:image` with `og:image:width`, `og:image:height`, `og:image:type` |
| `twitter-image.(png\|jpg\|jpeg\|gif\|webp)` | any segment | `twitter:image` with width, height and alt |
| `manifest.(json\|webmanifest)` | `app/` only | `<link rel="manifest">` |
| `robots.txt`, `sitemap.xml` | `app/` only | served as is, replacing the generated files |

The alt text of `opengraph-image.png` is read from `opengraph-image.alt.txt` next to it (same for `twitter-image`). Sizes come from the image headers; SVG icons get `sizes="any"`. A file in a nested segment, such as `app/blog/opengraph-image.png`, replaces the parent's for the routes below it, and files in dynamic segments (`app/blog/[slug]/opengraph-image.png`, `app/docs/[...slug]/icon.png`) are served for every value of the param. Two files with the same URL, such as `app/icon.png` and `app/(shop)/icon.png`, fail the build. `volt export` copies files of dynamic segments under every exported path below them.

Head tags link images as `<url>?<content hash>`, served with `Cache-Control: immutable`; requests without the hash (`/favicon.ico`, crawlers) revalidate with an ETag. Metadata files win over `metadata` exports of the same segment, and a deeper segment's `icons`, `openGraph.images` or `twitter.images` win over a parent's files. Files in `app/` take precedence over `public/` files at the same URL.

Images generated with code (`opengraph-image.tsx` returning `ImageResponse`) are not supported: they need an image renderer that QuickJS does not have. Export the image as a file instead.

## Metadata routes in code

`app/sitemap.ts`, `app/robots.ts` and `app/manifest.ts` export a default function (sync or async) returning the same objects as in Next.js. They run in QuickJS, so they can read anything bundled with the app but have no Node APIs. As in Next.js, their output is rendered once and cached (`Cache-Control: public, s-maxage`); `export const revalidate = 3600` renders it again on the first request after an hour (other requests keep getting the cached copy meanwhile), `ctx.revalidate_path("/sitemap.xml")` drops it, and `export const dynamic = "force-dynamic"` (or `revalidate = 0`) renders it on every request. When the output is cached, `#[sitemap]` providers receive a request without cookies or headers.

```ts
// app/sitemap.ts → /sitemap.xml
import type { MetadataRoute } from "volt-rs";

export default function sitemap(): MetadataRoute.Sitemap {
  return [
    { url: "https://acme.com", lastModified: new Date(), changeFrequency: "weekly", priority: 1 },
    { url: "/about" },
  ];
}
```

```ts
// app/robots.ts → /robots.txt
export default function robots(): MetadataRoute.Robots {
  return { rules: { userAgent: "*", allow: "/", disallow: "/private/" }, sitemap: "https://acme.com/sitemap.xml" };
}
```

```ts
// app/manifest.ts → /manifest.webmanifest, linked from every page
export default function manifest(): MetadataRoute.Manifest {
  return { name: "Acme", short_name: "Acme", start_url: "/", display: "standalone", icons: [{ src: "/icon.png", sizes: "192x192" }] };
}
```

`app/sitemap.ts` replaces the automatic list of routes; entries from Rust `#[sitemap]` providers are still appended, and `/llms.txt` lists the same URLs. Relative `url`s resolve against `site.url`. A static file and a script for the same route (`robots.txt` and `robots.ts`) fail the build. `generateSitemaps` (several sitemaps) is not supported.

## Inserting HTML into the head

`useServerInsertedHTML` from `volt-rs/navigation` (or `next/navigation`) registers a callback that runs once the page has rendered on the server; the elements it returns are added to `<head>`. CSS-in-JS libraries use it to flush the styles they collected:

```tsx
"use client";
import { useServerInsertedHTML } from "next/navigation";

export function StyleRegistry({ children }) {
  useServerInsertedHTML(() => <style id="ssr-styles">{collectStyles()}</style>);
  return children;
}
```

It does nothing in the browser.

## Structured data

```tsx
import { JsonLd } from "volt-rs/seo";

<JsonLd data={{ "@context": "https://schema.org", "@type": "Article", headline: post.title }} />
```

## sitemap.xml, robots.txt and llms.txt

volt serves these automatically unless a file with the same name exists in `app/` or `public/`, or `app/sitemap.ts` / `app/robots.ts` defines them.

- `/sitemap.xml` lists every static route and every path from `generateStaticParams`. Add dynamic URLs from Rust:

  ```rust
  #[sitemap]
  async fn posts(ctx: Ctx) -> Result<Vec<SitemapEntry>> {
      Ok(ctx.state::<Db>().slugs().await?.into_iter()
          .map(|slug| SitemapEntry::new(format!("/blog/{slug}")).change_frequency(ChangeFrequency::Weekly).priority(0.7))
          .collect())
  }
  ```

- `/robots.txt` allows everything, disallows the internal `/_volt/` endpoints and links the sitemap. Add paths with `seo.disallow`.
- `/llms.txt` gives language models an index of the site's pages.

Turn any of them off in `volt.toml`:

```toml
[seo]
sitemap = true
robots = true
llms = false
disallow = ["/admin"]
```

## Performance that search engines measure

- Pages are server-rendered HTML, so content is indexable without JavaScript.
- Static pages are prerendered at build time and served from memory.
- CSS is one hashed, minified file; JavaScript is split per route and loaded with `modulepreload`.
- Hashed assets are served with `Cache-Control: immutable` and precompressed with Brotli.
- `<Image>` sets `width`/`height`, lazy loading, `srcset` and `fetchpriority` to keep Largest Contentful Paint and Cumulative Layout Shift low.
