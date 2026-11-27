// React Fast Refresh for `volt dev`. Only development client builds import this module: the generated entry
// calls `inject` before React DOM loads, and every app module registers its components here.
import RefreshRuntime from "react-refresh/runtime";

/** URLs of the app modules evaluated so far, as served by the build. */
const evaluated = new Set();
/** The instance this page holds of each module it evaluated again: where it was imported from, and the revision of its code. */
const held = new Map();
/** The revision of the build the page loaded: the code of the modules not evaluated again since. */
let loadedAt = null;

/** Route exports that only run on the server: the client never reads them, so they never block an update. */
const SERVER_EXPORTS = new Set(["generateStaticParams", "getServerSideProps", "getStaticProps", "getStaticPaths"]);

/** Static and dynamic import specifiers in the ES modules rolldown emits. */
const SPECIFIER = /(\bfrom\s*|\bimport\s*\(\s*|^\s*import\s*)"([^"\n]+)"/gm;

/** Hooks the refresh runtime into React DOM, which must not have loaded yet. */
export function inject() {
  if (typeof window === "undefined") return;
  RefreshRuntime.injectIntoGlobalHook(window);
  window.__VOLT_REFRESH__ = { apply };
}

/** Registers a component of a module under `<module id> <name>`. */
export function register(type, id) {
  RefreshRuntime.register(type, id);
}

/** The hook signature function of one component. */
export function signature() {
  return RefreshRuntime.createSignatureFunctionForTransform();
}

/** Marks the module served at `url` as evaluated. */
export function loaded(url) {
  evaluated.add(url);
}

const isComponent = (value) => RefreshRuntime.isLikelyComponentType(value);
const isNamespace = (value) => value !== null && typeof value === "object" && value[Symbol.toStringTag] === "Module";

/** Whether a value is plain data: primitives, and arrays and plain objects of them. Functions never are. */
function isData(value, path = new Set()) {
  if (typeof value === "function") return false;
  if (value === null || typeof value !== "object") return true;
  const prototype = Object.getPrototypeOf(value);
  if (path.has(value) || (!Array.isArray(value) && prototype !== Object.prototype && prototype !== null)) return false;
  path.add(value);
  const data = Object.values(value).every((member) => isData(member, path));
  path.delete(value);
  return data;
}

function equal(a, b) {
  if (Object.is(a, b)) return true;
  if (a === null || b === null || typeof a !== "object" || typeof b !== "object") return false;
  if (Array.isArray(a) !== Array.isArray(b)) return false;
  const keys = Object.keys(a);
  return keys.length === Object.keys(b).length && keys.every((key) => Object.hasOwn(b, key) && equal(a[key], b[key]));
}

/**
 * Whether a module is a refresh boundary, as in Next.js: it exports components, and otherwise only plain data
 * (route config such as `metadata`). Its importers keep their bindings when it is evaluated again, which is
 * safe for components, which React resolves to their latest registration, and for data that did not change.
 * Any other module (helpers, hooks, constants, contexts) is evaluated again with its importers.
 */
function isBoundary(exports) {
  let components = 0;
  const allowed = (namespace) =>
    Object.keys(namespace).every((key) => {
      const value = namespace[key];
      if (SERVER_EXPORTS.has(key)) return true;
      if (isNamespace(value)) return allowed(value);
      if (isComponent(value)) return ++components > 0;
      return isData(value);
    });
  return allowed(exports) && components > 0;
}

/** Whether the new instance of a boundary keeps its exports: components stay components and data is unchanged. */
function compatible(previous, next) {
  const keys = Object.keys(next);
  if (keys.length !== Object.keys(previous).length) return false;
  return keys.every((key) => {
    if (!(key in previous)) return false;
    if (SERVER_EXPORTS.has(key)) return true;
    const [a, b] = [previous[key], next[key]];
    if (isNamespace(a) && isNamespace(b)) return compatible(a, b);
    if (isComponent(a)) return isComponent(b);
    return isData(b) && equal(a, b);
  });
}

const relative = (specifier) => specifier.startsWith(".") || specifier.startsWith("/");

/** The modules a module imports, as absolute URLs. */
function imports(code, url) {
  return [...code.matchAll(SPECIFIER)].filter((m) => relative(m[2])).map((m) => new URL(m[2], url).href);
}

/**
 * The code of the module served at `url`, importing through `resolve`. It is evaluated from a blob, so its
 * specifiers, `import.meta.url` and source map become absolute, and stack traces name it after `url`.
 */
function rebase(code, url, revision, resolve) {
  const rebased = code
    .replace(SPECIFIER, (match, head, specifier) =>
      relative(specifier) ? head + JSON.stringify(resolve(new URL(specifier, url).href)) : match,
    )
    .replaceAll("import.meta.url", JSON.stringify(url))
    .replace(/^\/\/# sourceMappingURL=(\S+)$/m, (_, map) => `//# sourceMappingURL=${new URL(map, url).href}`);
  return `${rebased}\n//# sourceURL=${url}?t=${revision}\n`;
}

/**
 * Brings the modules this page evaluated up to date with an update, then re-renders their components with
 * their state kept. A changed module that is not a refresh boundary is evaluated again with its importers, up
 * to the nearest boundaries, and the new instances import each other: a helper that closes over a changed
 * constant reaches its callers. Resolves to `false` when the page must reload instead: the update reaches a
 * module outside the app (the entry, a route table), a boundary's exports changed, or the new code does not
 * link. Rejects with the error thrown while evaluating a module, which the next update retries.
 */
async function apply(update, revision) {
  loadedAt ??= revision;
  const absolute = (url) => new URL(url, location.href).href;
  const importers = new Map(Object.entries(update.importers).map(([url, list]) => [absolute(url), list.map(absolute)]));
  const instance = (url) => held.get(url)?.url ?? url;
  const changed = update.modules
    .map((module) => ({ url: absolute(module.url), revision: module.revision }))
    .filter(({ url, revision }) => evaluated.has(url) && revision > (held.get(url)?.revision ?? loadedAt));

  const previous = new Map();
  const boundaries = new Set();
  const queue = changed.map(({ url }) => url);
  while (queue.length > 0) {
    const url = queue.shift();
    if (previous.has(url)) continue;
    const exports = await import(instance(url));
    previous.set(url, exports);
    if (isBoundary(exports)) {
      boundaries.add(url);
      continue;
    }
    for (const importer of importers.get(url) ?? []) {
      if (!importers.has(importer)) return false;
      if (evaluated.has(importer)) queue.push(importer);
    }
  }
  if (previous.size === 0) return true;

  const sources = new Map();
  for (const url of previous.keys()) {
    const response = await fetch(url, { cache: "no-cache" });
    if (!response.ok) return false;
    sources.set(url, await response.text());
  }
  // Dependencies first, so each new instance imports the new instances of the others.
  const fresh = new Map();
  const visiting = new Set();
  const create = (url) => {
    if (fresh.has(url)) return true;
    if (visiting.has(url)) return false;
    visiting.add(url);
    const code = sources.get(url);
    if (!imports(code, url).every((dependency) => !sources.has(dependency) || create(dependency))) return false;
    const rebased = rebase(code, url, update.revision, (dependency) => fresh.get(dependency) ?? instance(dependency));
    fresh.set(url, URL.createObjectURL(new Blob([rebased], { type: "text/javascript" })));
    return true;
  };
  if (![...sources.keys()].every(create)) return false;

  const next = new Map();
  for (const [url, blob] of fresh) {
    try {
      next.set(url, await import(blob));
    } catch (error) {
      // Linking errors (an import the loaded modules do not export) cannot be fixed in place.
      if (error instanceof SyntaxError) return false;
      throw error;
    }
  }
  if (![...boundaries].every((url) => compatible(previous.get(url), next.get(url)))) return false;
  for (const [url, blob] of fresh) held.set(url, { url: blob, revision: update.revision });
  RefreshRuntime.performReactRefresh();
  return true;
}
