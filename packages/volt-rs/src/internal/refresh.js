// React Fast Refresh for `volt dev`. Only development client builds import this module: the generated entry
// calls `inject` before React DOM loads, and every app module registers its components here.
import RefreshRuntime from "react-refresh/runtime";

/** Ids of the app modules evaluated so far. */
const evaluated = new Set();
/** URL of the latest instance of each module that was updated in place. */
const latest = new Map();

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

/** Marks a module as evaluated. */
export function loaded(id) {
  evaluated.add(id);
}

const isComponent = (value) => RefreshRuntime.isLikelyComponentType(value);

function same(a, b) {
  if (Object.is(a, b)) return true;
  if (typeof a === "function" && typeof b === "function") return String(a) === String(b);
  try {
    return JSON.stringify(a) === JSON.stringify(b);
  } catch {
    return false;
  }
}

/**
 * Whether the new instance of a module can replace the old one without its importers: they keep the old
 * bindings, which is only safe for components, which React resolves to their latest registration, and for
 * other exports that did not change (route config such as `metadata`, constants, helpers).
 */
function replaceable(previous, next) {
  const keys = Object.keys(next);
  if (keys.length === 0 || keys.length !== Object.keys(previous).length) return false;
  return keys.every(
    (key) => key in previous && ((isComponent(previous[key]) && isComponent(next[key])) || same(previous[key], next[key])),
  );
}

/**
 * Evaluates the new code of the changed modules that this page has loaded, then re-renders their components
 * with their state kept. Resolves to `false` when the page must reload instead; rejects with the error thrown
 * while evaluating a module, which the next update retries.
 */
async function apply(modules, revision) {
  const pairs = [];
  for (const { id, url } of modules) {
    if (!evaluated.has(id)) continue;
    const previous = await import(latest.get(id) ?? url);
    const next = `${url}?t=${revision}`;
    let instance;
    try {
      instance = await import(next);
    } catch (error) {
      // Linking errors (an import the loaded modules do not export) cannot be fixed in place.
      if (error instanceof SyntaxError) return false;
      throw error;
    }
    latest.set(id, next);
    pairs.push([previous, instance]);
  }
  if (!pairs.every(([previous, next]) => replaceable(previous, next))) return false;
  RefreshRuntime.performReactRefresh();
  return true;
}
