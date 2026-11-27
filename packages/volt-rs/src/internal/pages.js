// Pages Router: data functions (`getServerSideProps`, `getStaticProps`, `getStaticPaths`) and the `next/router` object.
import { BASE_PATH, DEFAULT_LOCALE, LOCALES } from "./config.js";
import { routerEvents } from "./events.js";

export const NOT_FOUND_ROUTE = "/_not-found";
export const ERROR_ROUTE = "/_error";

const isObject = (value) => value !== null && typeof value === "object" && !Array.isArray(value);

/** Rust loader output as page props: objects are spread, other values are passed as `data`. */
export function toProps(data) {
  if (data == null) return {};
  return isObject(data) ? data : { data };
}

const encode = (value) => encodeURIComponent(String(value));

/** Fills a route pattern such as `/blog/[slug]` with params. */
export function fillPattern(pattern, params = {}) {
  const parts = [];
  for (const segment of pattern.split("/").filter(Boolean)) {
    const catchAll = /^\[\[?\.\.\.(.+?)\]?\]$/.exec(segment);
    const dynamic = /^\[(.+)\]$/.exec(segment);
    if (catchAll) {
      const value = params[catchAll[1]];
      const list = value == null ? [] : Array.isArray(value) ? value : [value];
      parts.push(...list.map(encode));
    } else if (dynamic) {
      parts.push(encode(params[dynamic[1]]));
    } else {
      parts.push(segment);
    }
  }
  return `/${parts.join("/")}`;
}

const normalize = (path) => {
  try {
    return decodeURIComponent(path).replace(/\/+$/, "") || "/";
  } catch {
    return path;
  }
};

// In production `getStaticPaths` runs once per worker, like once per build in Next.js; development reruns it.
const pathsCache = new WeakMap();

async function runStaticPaths(page, pattern) {
  const { locales, defaultLocale } = localeFields({});
  const result = (await page.getStaticPaths({ locales, defaultLocale })) ?? {};
  const paths = (result.paths ?? []).map((entry) => (typeof entry === "string" ? entry : fillPattern(pattern, entry.params)));
  return { paths, fallback: result.fallback ?? false };
}

/** Runs `getStaticPaths` and returns concrete URLs. */
export function staticPaths(page, pattern) {
  if (process.env.NODE_ENV !== "production") return runStaticPaths(page, pattern);
  let pending = pathsCache.get(page);
  if (!pending) {
    pending = runStaticPaths(page, pattern);
    pending.catch(() => pathsCache.delete(page));
    pathsCache.set(page, pending);
  }
  return pending;
}

function settle(result, base, isStatic, name) {
  if (!isObject(result)) throw new Error(`${name} must return an object with props, notFound or redirect`);
  if (result.notFound) return { notFound: true };
  if (result.redirect) {
    const { destination, permanent, statusCode } = result.redirect;
    return { redirect: { location: destination, status: statusCode ?? (permanent ? 308 : 307), type: "replace" } };
  }
  return Promise.resolve(result.props).then((props) => ({
    props: { ...base, ...(props ?? {}) },
    static: isStatic,
    revalidate: typeof result.revalidate === "number" ? Math.max(1, Math.round(result.revalidate)) : null,
  }));
}

/** `locale`, `locales` and `defaultLocale` as Next.js passes them, all undefined without `[i18n]`. */
export function localeFields(state) {
  if (!LOCALES.length) return { locale: undefined, locales: undefined, defaultLocale: undefined };
  return { locale: state.locale ?? DEFAULT_LOCALE, locales: LOCALES, defaultLocale: DEFAULT_LOCALE };
}

/** Resolves the props of a Pages Router page for a request. */
export async function resolveProps(route, state) {
  const page = route.page ?? {};
  const base = toProps(state.data);
  const params = state.params ?? {};
  const locales = localeFields(state);
  if (typeof page.getServerSideProps === "function") {
    const context = {
      params,
      query: { ...(state.searchParams ?? {}), ...params },
      resolvedUrl: `${state.pathname}${state.search ?? ""}`,
      ...locales,
    };
    return settle(await page.getServerSideProps(context), base, false, "getServerSideProps");
  }
  if (typeof page.getStaticProps === "function") {
    if (typeof page.getStaticPaths === "function") {
      const { paths, fallback } = await staticPaths(page, state.route);
      if (fallback === false && !paths.some((path) => normalize(path) === normalize(state.pathname))) {
        return { notFound: true };
      }
    }
    return settle(await page.getStaticProps({ params, ...locales }), base, true, "getStaticProps");
  }
  return { props: base, static: false, revalidate: null };
}

function formatUrl(url) {
  if (typeof url === "string") return url;
  if (url instanceof URL) return url.href;
  const { pathname = "", query = {}, hash = "" } = url ?? {};
  const used = new Set();
  const path = pathname.replace(/\[\[?(?:\.\.\.)?([^\]]+)\]?\]/g, (_, name) => {
    used.add(name);
    const value = query[name];
    return Array.isArray(value) ? value.map(encode).join("/") : value == null ? "" : encode(value);
  });
  const search = new URLSearchParams();
  for (const [key, value] of Object.entries(query)) {
    if (used.has(key) || value == null) continue;
    for (const item of Array.isArray(value) ? value : [value]) search.append(key, String(item));
  }
  const text = search.toString();
  return `${path.replace(/\/+$/, "") || "/"}${text ? `?${text}` : ""}${hash && !hash.startsWith("#") ? `#${hash}` : hash}`;
}

/** The URL a `push(url, as)` call navigates to. */
export const resolveHref = (url, as) => formatUrl(as ?? url);

const publicPath = (route) => (route === NOT_FOUND_ROUTE ? "/404" : route);

/** Builds the `next/router` object from the app router and the current page state. */
export function pagesRouter(router, state) {
  const navigate = (method) => (url, as, options) =>
    router[method](resolveHref(url, as), options).then(
      () => true,
      () => false,
    );
  return {
    pathname: publicPath(state.route),
    route: publicPath(state.route),
    query: { ...(state.searchParams ?? {}), ...(state.params ?? {}) },
    asPath: `${state.pathname}${state.search ?? ""}`,
    basePath: BASE_PATH,
    isReady: true,
    isFallback: false,
    isPreview: false,
    isLocaleDomain: false,
    ...localeFields(state),
    events: routerEvents,
    push: navigate("push"),
    replace: navigate("replace"),
    reload: () => location.reload(),
    back: () => router.back(),
    forward: () => router.forward(),
    prefetch: (url, as) => Promise.resolve(router.prefetch(resolveHref(url, as))),
    beforePopState: (callback) => router.beforePopState(callback),
  };
}
