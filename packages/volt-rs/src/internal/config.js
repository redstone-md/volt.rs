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
