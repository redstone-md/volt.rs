// Build settings inlined by the volt bundler from `volt.toml`.

/** `[build] base_path`, e.g. `/docs`, or an empty string. */
export const BASE_PATH = process.env.__VOLT_BASE_PATH__ ?? "";
/** `[build] trailing_slash`. */
export const TRAILING_SLASH = process.env.__VOLT_TRAILING_SLASH__ === true;
/** Whether `/_volt/image` is available (`[images] optimize`, and not a static export). */
export const IMAGE_OPTIMIZATION = process.env.__VOLT_IMAGE_OPTIMIZATION__ !== false;

const isAppHref = (href) => typeof href === "string" && href.startsWith("/") && !href.startsWith("//");

/** Public URL of an app href: prefixes the base path and applies the trailing slash policy. */
export function withBase(href) {
  if (!isAppHref(href)) return href;
  const cut = href.search(/[?#]/);
  const path = cut < 0 ? href : href.slice(0, cut);
  const rest = cut < 0 ? "" : href.slice(cut);
  const trimmed = path.replace(/\/+$/, "");
  const file = trimmed.slice(trimmed.lastIndexOf("/") + 1).includes(".");
  let out;
  if (!trimmed) out = TRAILING_SLASH || !BASE_PATH ? "/" : "";
  else out = TRAILING_SLASH && !file ? `${trimmed}/` : trimmed;
  return `${BASE_PATH}${out}${rest}`;
}

/** App path of a public pathname, or `null` when it lies outside the base path. */
export function stripBase(pathname) {
  let path = pathname;
  if (BASE_PATH) {
    if (path !== BASE_PATH && !path.startsWith(`${BASE_PATH}/`)) return null;
    path = path.slice(BASE_PATH.length);
  }
  return path.replace(/\/+$/, "") || "/";
}

/** URL of a prefixed internal endpoint such as `/_volt/action/x`. */
export const endpoint = (path) => `${BASE_PATH}${path}`;

/** `[i18n] locales`; empty when internationalized routing is off. */
export const LOCALES = (process.env.__VOLT_LOCALES__ ?? "").split(",").filter(Boolean);
/** Locale of unprefixed URLs: `[i18n] default_locale`, else `[site] language`. */
export const DEFAULT_LOCALE = process.env.__VOLT_DEFAULT_LOCALE__ ?? "en";

const findLocale = (candidate) => LOCALES.find((locale) => locale.toLowerCase() === candidate.toLowerCase()) ?? null;

/** Splits an app pathname into its locale prefix (`null` when unprefixed) and the route path below it. */
export function splitLocale(pathname) {
  const [, first = "", ...rest] = pathname.split("/");
  const locale = first ? findLocale(first) : null;
  if (!locale) return { locale: null, path: pathname };
  return { locale, path: rest.length ? `/${rest.join("/")}` : "/" };
}

/** App path of a route path in `locale`: prefixed unless it is the default locale. */
export function localizePath(path, locale) {
  // Collapsing leading slashes keeps `/en//host` from turning into the protocol-relative `//host`.
  const local = `/${path.replace(/^[/\\]+/, "")}`;
  if (!LOCALES.length || !locale || locale === DEFAULT_LOCALE) return local;
  return local === "/" ? `/${locale}` : `/${locale}${local}`;
}

/**
 * Adds the locale prefix to an app href. With `explicit`, an existing prefix is replaced by `locale`;
 * otherwise hrefs that already name a locale are left alone.
 */
export function localizeHref(href, locale, explicit = false) {
  if (!LOCALES.length || !isAppHref(href) || href.startsWith("/_volt/")) return href;
  const cut = href.search(/[?#]/);
  const path = cut < 0 ? href : href.slice(0, cut);
  const rest = cut < 0 ? "" : href.slice(cut);
  const split = splitLocale(path);
  if (split.locale && !explicit) return href;
  return `${localizePath(split.path, locale)}${rest}`;
}

/** Remembers a visitor's explicit locale choice for `[i18n] locale_detection`. Browser only. */
export function rememberLocale(locale) {
  if (!LOCALES.length || typeof document === "undefined") return;
  document.cookie = `VOLT_LOCALE=${encodeURIComponent(locale)}; path=${BASE_PATH || "/"}; max-age=31536000; samesite=lax`;
}
