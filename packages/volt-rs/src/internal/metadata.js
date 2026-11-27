import { createElement as h, Fragment } from "react";
import { fillPattern } from "./pages.js";

const list = (value) => (value == null ? [] : Array.isArray(value) ? value : [value]);

function absolute(url, base) {
  if (!url || !base || /^[a-z][a-z0-9+.-]*:/i.test(url) || url.startsWith("//")) return url;
  return `${String(base).replace(/\/$/, "")}/${String(url).replace(/^\//, "")}`;
}

async function exported(module, props, name, generator) {
  if (!module) return undefined;
  if (typeof module[generator] === "function") return await module[generator](props);
  return module[name];
}

/** A metadata file URL (`/blog/[slug]/opengraph-image.png?hash`) for the current params. */
function fileUrl(url, params) {
  const [path, query] = url.split("?");
  return query ? `${fillPattern(path, params)}?${query}` : fillPattern(path, params);
}

/** Metadata contributed by `app/` metadata files, which take precedence over the same segment's exports. */
function applyFiles(merged, files, params) {
  const fill = (items) => items.map((item) => ({ ...item, url: fileUrl(item.url, params) }));
  if (files.icon || files.apple) {
    const icons = typeof merged.icons === "string" ? { icon: merged.icons } : { ...merged.icons };
    if (files.icon) icons.icon = fill(files.icon);
    if (files.apple) icons.apple = fill(files.apple);
    merged.icons = icons;
  }
  if (files.openGraph) merged.openGraph = { ...merged.openGraph, images: fill(files.openGraph) };
  if (files.twitter) merged.twitter = { ...merged.twitter, images: fill(files.twitter) };
  if (files.manifest) merged.manifest = files.manifest;
}

/** Forgets inherited file metadata that a deeper segment's exports replace. */
function overridden(files, meta) {
  if (meta.icons !== undefined) {
    delete files.icon;
    delete files.apple;
  }
  if (meta.openGraph?.images !== undefined) delete files.openGraph;
  if (meta.twitter?.images !== undefined) delete files.twitter;
  if (meta.manifest !== undefined) delete files.manifest;
}

/**
 * Merges `metadata` / `generateMetadata` and metadata files from every segment, Next.js style. With
 * `notFound` (the index of the segment showing its `not-found`), only that segment and its parents count.
 */
export async function resolveMetadata(route, props, site, notFound) {
  const levels = [...route.segments.map((s) => [s.layout, s.files]), [route.page, route.files]];
  const shown = notFound == null ? levels : levels.slice(0, notFound + 1);
  const merged = { metadataBase: site?.url };
  const files = {};
  const viewport = {};
  let template = null;
  let title;
  for (const [module, found] of shown) {
    const meta = await exported(module, props, "metadata", "generateMetadata");
    const view = await exported(module, props, "viewport", "generateViewport");
    if (view) Object.assign(viewport, view);
    if (meta) overridden(files, meta);
    if (found) Object.assign(files, found);
    if (!meta) continue;
    const { title: own, ...rest } = meta;
    Object.assign(merged, rest);
    if (own != null) {
      if (typeof own === "string") title = template ? template.replace("%s", own) : own;
      else if (own.absolute) title = own.absolute;
      else if (own.default) title = own.default;
      if (typeof own === "object" && own.template) template = own.template;
    }
  }
  applyFiles(merged, files, props.params);
  if (notFound != null) merged.robots = { index: false };
  if (title == null && site?.name) title = site.name;
  return { ...merged, title, viewport };
}

function robots(value) {
  if (!value || typeof value === "string") return value;
  const parts = [];
  if (value.index !== undefined) parts.push(value.index ? "index" : "noindex");
  if (value.follow !== undefined) parts.push(value.follow ? "follow" : "nofollow");
  for (const key of ["noarchive", "nosnippet", "noimageindex", "nocache"]) if (value[key]) parts.push(key);
  for (const key of ["max-snippet", "max-image-preview", "max-video-preview"]) {
    const camel = key.replace(/-(\w)/g, (_, c) => c.toUpperCase());
    if (value[camel] !== undefined) parts.push(`${key}:${value[camel]}`);
  }
  return parts.join(", ");
}

function images(value, base) {
  return list(value).map((image) => (typeof image === "string" ? { url: absolute(image, base) } : { ...image, url: absolute(image.url, base) }));
}

function viewportContent(viewport) {
  const keys = { width: "width", height: "height", initialScale: "initial-scale", minimumScale: "minimum-scale", maximumScale: "maximum-scale", userScalable: "user-scalable", viewportFit: "viewport-fit" };
  const parts = Object.entries(keys)
    .filter(([key]) => viewport[key] !== undefined)
    .map(([key, name]) => `${name}=${typeof viewport[key] === "boolean" ? (viewport[key] ? "yes" : "no") : viewport[key]}`);
  return parts.length ? parts.join(", ") : "width=device-width, initial-scale=1";
}

/** Renders resolved metadata as React 19 hoistable head elements. */
export function Metadata({ metadata: m }) {
  const base = m.metadataBase;
  const tags = [];
  const meta = (key, props) => props.content != null && props.content !== "" && tags.push(h("meta", { key, ...props, content: String(props.content) }));
  const link = (key, props) => props.href && tags.push(h("link", { key, ...props }));

  tags.push(h("meta", { key: "charset", charSet: "utf-8" }));
  tags.push(h("meta", { key: "viewport", name: "viewport", content: viewportContent(m.viewport ?? {}) }));
  if (m.title != null) tags.push(h("title", { key: "title" }, String(m.title)));
  meta("description", { name: "description", content: m.description });
  meta("application-name", { name: "application-name", content: m.applicationName });
  meta("generator", { name: "generator", content: m.generator });
  meta("keywords", { name: "keywords", content: list(m.keywords).join(", ") });
  meta("creator", { name: "creator", content: m.creator });
  meta("publisher", { name: "publisher", content: m.publisher });
  meta("referrer", { name: "referrer", content: m.referrer });
  meta("category", { name: "category", content: m.category });
  meta("robots", { name: "robots", content: robots(m.robots) });
  if (m.robots?.googleBot) meta("googlebot", { name: "googlebot", content: robots(m.robots.googleBot) });
  list(m.authors).forEach((author, i) => {
    meta(`author-${i}`, { name: "author", content: author.name ?? author });
    if (author.url) link(`author-url-${i}`, { rel: "author", href: author.url });
  });
  for (const color of list(m.viewport?.themeColor)) {
    const value = typeof color === "string" ? { color } : color;
    meta(`theme-${value.media ?? "all"}`, { name: "theme-color", content: value.color, media: value.media });
  }
  meta("color-scheme", { name: "color-scheme", content: m.viewport?.colorScheme });

  const alternates = m.alternates ?? {};
  link("canonical", { rel: "canonical", href: absolute(alternates.canonical, base) });
  for (const [lang, href] of Object.entries(alternates.languages ?? {})) link(`alt-${lang}`, { rel: "alternate", hrefLang: lang, href: absolute(href, base) });
  link("manifest", { rel: "manifest", href: m.manifest });

  const icons = typeof m.icons === "string" ? { icon: m.icons } : (m.icons ?? {});
  list(icons.icon).forEach((icon, i) => link(`icon-${i}`, { rel: "icon", ...(typeof icon === "string" ? { href: icon } : { href: icon.url, type: icon.type, sizes: icon.sizes }) }));
  list(icons.apple).forEach((icon, i) => link(`apple-${i}`, { rel: "apple-touch-icon", ...(typeof icon === "string" ? { href: icon } : { href: icon.url, type: icon.type, sizes: icon.sizes }) }));
  list(icons.shortcut).forEach((icon, i) => link(`shortcut-${i}`, { rel: "shortcut icon", href: typeof icon === "string" ? icon : icon.url }));

  const og = m.openGraph;
  if (og) {
    const p = (key, content) => meta(`og:${key}`, { property: `og:${key}`, content });
    p("title", og.title ?? m.title);
    p("description", og.description ?? m.description);
    p("url", absolute(og.url ?? alternates.canonical, base));
    p("site_name", og.siteName);
    p("locale", og.locale);
    p("type", og.type ?? "website");
    images(og.images, base).forEach((image, i) => {
      meta(`og:image-${i}`, { property: "og:image", content: image.url });
      meta(`og:image:type-${i}`, { property: "og:image:type", content: image.type });
      meta(`og:image:width-${i}`, { property: "og:image:width", content: image.width });
      meta(`og:image:height-${i}`, { property: "og:image:height", content: image.height });
      meta(`og:image:alt-${i}`, { property: "og:image:alt", content: image.alt });
    });
    if (og.publishedTime) meta("article:published_time", { property: "article:published_time", content: og.publishedTime });
    if (og.modifiedTime) meta("article:modified_time", { property: "article:modified_time", content: og.modifiedTime });
    list(og.authors).forEach((author, i) => meta(`article:author-${i}`, { property: "article:author", content: author }));
  }

  const tw = m.twitter;
  if (tw || og) {
    const t = (key, content) => meta(`twitter:${key}`, { name: `twitter:${key}`, content });
    const twImages = images(tw?.images ?? og?.images, base);
    t("card", tw?.card ?? (twImages.length ? "summary_large_image" : "summary"));
    t("site", tw?.site);
    t("creator", tw?.creator);
    t("title", tw?.title ?? og?.title ?? m.title);
    t("description", tw?.description ?? og?.description ?? m.description);
    twImages.forEach((image, i) => {
      meta(`twitter:image-${i}`, { name: "twitter:image", content: image.url });
      meta(`twitter:image:width-${i}`, { name: "twitter:image:width", content: image.width });
      meta(`twitter:image:height-${i}`, { name: "twitter:image:height", content: image.height });
      meta(`twitter:image:alt-${i}`, { name: "twitter:image:alt", content: image.alt });
    });
  }

  const verification = m.verification ?? {};
  const verifiers = { google: "google-site-verification", yandex: "yandex-verification", yahoo: "y_key", bing: "msvalidate.01" };
  for (const [key, name] of Object.entries(verifiers)) list(verification[key]).forEach((content, i) => meta(`verify-${key}-${i}`, { name, content }));
  for (const [name, content] of Object.entries(verification.other ?? {})) list(content).forEach((c, i) => meta(`verify-${name}-${i}`, { name, content: c }));
  for (const [name, content] of Object.entries(m.other ?? {})) list(content).forEach((c, i) => meta(`other-${name}-${i}`, { name, content: c }));

  return h(Fragment, null, ...tags);
}
