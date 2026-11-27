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

## Structured data

```tsx
import { JsonLd } from "volt-rs/seo";

<JsonLd data={{ "@context": "https://schema.org", "@type": "Article", headline: post.title }} />
```

## sitemap.xml, robots.txt and llms.txt

volt serves these automatically unless a file with the same name exists in `public/`.

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
