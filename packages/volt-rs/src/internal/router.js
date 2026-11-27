import {
  Component,
  Fragment,
  Suspense,
  createContext,
  createElement as h,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  use,
  useState,
  useTransition,
} from "react";
import { endpoint, stripBase, withBase } from "./config.js";
import { isNotFound, readRedirect } from "./errors.js";
import { routerEvents } from "./events.js";
import { Metadata, resolveMetadata } from "./metadata.js";
import { ERROR_ROUTE, NOT_FOUND_ROUTE, pagesRouter, toProps } from "./pages.js";

export { ERROR_ROUTE, NOT_FOUND_ROUTE };
const PREFETCH_TTL = 30_000;

export const RouterContext = createContext(null);

/** Segments below the nearest layout, for `useSelectedLayoutSegment(s)`. */
export const LayoutSegmentContext = createContext(null);

/** Registers `useServerInsertedHTML` callbacks; only provided while rendering on the server. */
export const ServerInsertedHTMLContext = createContext(null);

const controller = { router: null, state: null, revalidate: null };

/** Router of the mounted app, for use outside React components. */
export function getRouter() {
  return controller.router;
}

/** Page state of the mounted app, for the `volt-rs/router` singleton. */
export function getRouterState() {
  return controller.state;
}

const kindOf = (route) => route.kind ?? "app";

/** Whether a view renders a Pages Router page. */
export const isPagesView = (view) => kindOf(view.route) === "pages";

const isDynamic = (segment) => segment.startsWith("[");
const isGroup = (segment) => segment.startsWith("(") && segment.endsWith(")");

function decodeSegment(segment) {
  try {
    return decodeURIComponent(segment);
  } catch {
    return segment;
  }
}

const patternSegments = (pattern) => pattern.split("?")[0].split("/").filter((s) => s && !isGroup(s));

/** Params of `pathname` under a route pattern (`/blog/[slug]`, `/docs/[...rest]`, `/[[...all]]`), or `null`. */
export function matchRoute(pattern, pathname) {
  const want = patternSegments(pattern);
  const have = pathname.split("/").filter(Boolean).map(decodeSegment);
  const params = {};
  for (let i = 0; i < want.length; i++) {
    const segment = want[i];
    if (segment.startsWith("[[...")) {
      if (have.length > i) params[segment.slice(5, -2)] = have.slice(i);
      return params;
    }
    if (segment.startsWith("[...")) {
      if (have.length <= i) return null;
      params[segment.slice(4, -1)] = have.slice(i);
      return params;
    }
    if (i >= have.length) return null;
    if (isDynamic(segment)) params[segment.slice(1, -1)] = have[i];
    else if (decodeSegment(segment) !== have[i]) return null;
  }
  return want.length === have.length ? params : null;
}

/** Matches a pathname against a path or route pattern. */
export function matchPath(pattern, pathname) {
  return matchRoute(pattern, pathname) !== null;
}

const specificity = (segment) => (segment.startsWith("[[...") ? 0 : segment.startsWith("[...") ? 1 : isDynamic(segment) ? 2 : 3);

/** The page route serving `pathname`, ranked like the server router: static, then dynamic, then catch-all segments. */
function findRoute(ids, pathname) {
  let best = null;
  for (const id of ids) {
    if (id.startsWith("/_")) continue;
    const params = matchRoute(id, pathname);
    if (!params) continue;
    const ranks = patternSegments(id).map(specificity);
    const better = () => {
      for (let i = 0; i < Math.max(ranks.length, best.ranks.length); i++) {
        const a = ranks[i] ?? -1;
        const b = best.ranks[i] ?? -1;
        if (a !== b) return a > b;
      }
      return false;
    };
    if (!best || better()) best = { id, params, ranks };
  }
  return best;
}

const NEVER = new Promise(() => {});

/** Stands in for a page whose data and chunk are loading, so its nearest `loading` boundary shows. */
function Pending() {
  return use(NEVER);
}
const PENDING_PAGE = { default: Pending };

/**
 * Applies a server revalidation hint: drops cached route data for matching paths and, unless
 * `refresh` is false, re-runs the current route's loader when it matches. Resolves to whether it refreshed.
 */
export async function revalidatePaths(paths, options) {
  return controller.revalidate ? controller.revalidate(paths, options) : false;
}

const thenable = (value) => Object.assign(Promise.resolve(value), value);
const component = (module) => module?.default ?? module;

export function pageProps(state) {
  return { data: state.data, params: thenable(state.params ?? {}), searchParams: thenable(state.searchParams ?? {}) };
}

async function loadRoute(routes, id) {
  const entry = routes[id] ?? routes[NOT_FOUND_ROUTE];
  const module = typeof entry === "function" ? await entry() : entry;
  return module.default ?? module;
}

/** Loads route modules and resolves metadata for a page state. */
export async function createView(routes, state) {
  const route = await loadRoute(routes, state.route);
  const resolve = kindOf(route) === "app" && !state.globalError;
  const metadata = resolve ? await resolveMetadata(route, pageProps(state), state.site, state.notFound) : null;
  return { route, state, metadata };
}

/**
 * Catches errors until `resetKey` changes. Boundaries are reset this way rather than remounted with a `key`,
 * which would also remount the layouts nested inside them on every navigation.
 */
class Boundary extends Component {
  constructor(props) {
    super(props);
    this.state = { error: null, key: props.resetKey };
  }
  static getDerivedStateFromError(error) {
    return { error };
  }
  static getDerivedStateFromProps(props, state) {
    return props.resetKey === state.key ? null : { error: null, key: props.resetKey };
  }
}

class ErrorBoundary extends Boundary {
  render() {
    const { error } = this.state;
    if (!error) return this.props.children;
    if (isNotFound(error) || readRedirect(error)) throw error;
    return h(this.props.fallback, { error, reset: () => this.setState({ error: null }) });
  }
}

class NotFoundBoundary extends Boundary {
  render() {
    const { error } = this.state;
    // `forced`: the server already resolved a `notFound()` to this boundary.
    if (this.props.forced) return h(this.props.fallback);
    if (!error) return this.props.children;
    if (!isNotFound(error)) throw error;
    return h(this.props.fallback);
  }
}

class RootBoundary extends Component {
  constructor(props) {
    super(props);
    this.state = { error: null };
  }
  static getDerivedStateFromError(error) {
    return { error };
  }
  componentDidCatch(error) {
    const redirect = readRedirect(error);
    if (redirect) this.props.onRedirect(redirect);
    else if (isNotFound(error)) this.props.onNotFound();
    else throw error;
  }
  componentDidUpdate(previous) {
    if (previous.view !== this.props.view && this.state.error) this.setState({ error: null });
  }
  render() {
    return this.state.error ? null : this.props.children;
  }
}

/** `global-error.tsx` around the root layout: it replaces the whole document until the next navigation. */
class GlobalErrorBoundary extends Boundary {
  render() {
    const { error } = this.state;
    if (!error) return this.props.children;
    if (isNotFound(error) || readRedirect(error)) throw error;
    return h(this.props.fallback, { error, reset: () => this.setState({ error: null }) });
  }
}

function dynamicName(name) {
  return (/^\[\[?\.\.\.(.+?)\]?\]$/.exec(name) ?? /^\[(.+)\]$/.exec(name))?.[1];
}

/** The route's directories below `depth`, dynamic ones replaced by their values, as in Next.js. */
function segmentsBelow(route, depth, params) {
  const out = [];
  for (const name of (route.path ?? []).slice(depth)) {
    const param = dynamicName(name);
    const value = param === undefined ? name : [].concat(params?.[param] ?? []).join("/");
    if (value !== "") out.push(value);
  }
  return out;
}

/** Pages Router tree: `_app` wrapping the page component. */
function PagesTree({ view }) {
  const { route, state } = view;
  const { router: base } = useContext(RouterContext);
  const router = useMemo(() => pagesRouter(base, state), [base, state]);
  const props = useMemo(
    () => (state.route === ERROR_ROUTE ? { statusCode: state.status, error: state.error } : toProps(state.data)),
    [state],
  );
  return h(component(route.app), { Component: component(route.page), pageProps: props, router });
}

function Tree({ view }) {
  return kindOf(view.route) === "pages" ? h(PagesTree, { view }) : h(AppTree, { view });
}

function AppTree({ view }) {
  const { route, state, metadata } = view;
  const Page = component(route.page);
  const GlobalError = route.globalError && component(route.globalError);
  const props = useMemo(() => pageProps(state), [state]);
  if (state.globalError && GlobalError) return h(GlobalError, { error: state.error, reset: () => location.reload() });
  const node = state.route === ERROR_ROUTE ? h(Page, { error: state.error, reset: () => location.reload() }) : h(Page, props);
  const tree = wrap(route.segments, node, {
    state,
    params: props.params,
    values: state.params,
    path: route.path,
    resetKey: view.key ?? state.pathname,
    notFound: state.notFound,
    metadata,
  });
  return GlobalError ? h(GlobalErrorBoundary, { resetKey: view, fallback: GlobalError }, tree) : tree;
}

/**
 * Wraps a page in the boundaries and layouts of its segments, innermost first. Layouts receive their parallel
 * route slots as props next to `children`. Boundaries reset when `resetKey` changes; `path` and `values` (the
 * page's directories and raw params) key templates and the subtrees below layouts.
 */
function wrap(segments, node, { state, params, values, path, resetKey, notFound, metadata }) {
  // Depth of the next layout down (or of the page), whose subtree is keyed by the segments in between.
  let inner = path?.length ?? 0;
  for (let i = segments.length - 1; i >= 0; i--) {
    const segment = segments[i];
    const depth = segment.path?.length ?? 0;
    const below = segmentsBelow({ path }, depth, values);
    if (segment.notFound) {
      const fallback = component(segment.notFound);
      node = h(NotFoundBoundary, { resetKey, fallback, forced: notFound === i }, node);
    }
    if (segment.loading) node = h(Suspense, { fallback: h(component(segment.loading)) }, node);
    if (segment.error) node = h(ErrorBoundary, { resetKey, fallback: component(segment.error) }, node);
    // As in Next.js, a template remounts when the segment right below it changes.
    if (segment.template) node = h(component(segment.template), { key: below[0] ?? "" }, node);
    if (i === 0 && metadata !== undefined) node = h(Fragment, null, h(Metadata, { metadata }), node);
    if (segment.layout) {
      // As in Next.js, the subtree below a layout remounts when one of its segments changes (`/items/1` to
      // `/items/2`), while the layout itself stays mounted.
      const key = segmentsBelow({ path: (path ?? []).slice(0, inner) }, depth, values).join("/");
      node = h(Fragment, { key }, node);
      const layout = h(component(segment.layout), { params, ...slotProps(segment, state, params) }, node);
      node = h(LayoutSegmentContext.Provider, { value: below }, layout);
      inner = depth;
    }
  }
  return node;
}

function slotProps(segment, state, params) {
  if (!segment.slots) return null;
  const props = {};
  for (const [name, slot] of Object.entries(segment.slots)) props[name] = slotNode(slot, state, params);
  return props;
}

/** The matched page of a slot, else its `default` page, else nothing. */
function slotNode(slot, state, params) {
  const own = state.slots?.[slot.key];
  const route = own?.route ? slot.routes[own.route] : null;
  if (!route) return slot.default ? h(component(slot.default), { params }) : null;
  const props = { data: own.data, params: thenable(own.params ?? {}), searchParams: thenable(state.searchParams ?? {}) };
  const key = `${own.route}:${JSON.stringify(own.params ?? {})}`;
  const failed = own.error ? failure(route.segments, own.error) : null;
  const options = { state, params: props.params, values: own.params, resetKey: key };
  const node = failed
    ? wrap(route.segments.slice(0, failed.depth + 1), failed.node, options)
    : wrap(route.segments, h(component(route.page), props), options);
  return h(SlotScope, { params: own.params, data: own.data }, node);
}

/**
 * The innermost `not-found` or `error` file of a slot page whose loader failed, rendered in place of the page.
 * The server only reports failures that such a file catches.
 */
function failure(segments, error) {
  const field = error.notFound ? "notFound" : "error";
  for (let depth = segments.length - 1; depth >= 0; depth--) {
    const module = segments[depth][field];
    if (!module) continue;
    const props = error.notFound ? {} : { error, reset: () => location.reload() };
    return { depth, node: h(component(module), props) };
  }
  return null;
}

/** Inside a slot, `useParams` adds the slot page's params to the page's and `useData` reads the slot's data. */
function SlotScope({ params, data, children }) {
  const context = useContext(RouterContext);
  const value = useMemo(
    () => context && { ...context, state: { ...context.state, params: { ...context.state.params, ...params }, data } },
    [context, params, data],
  );
  return h(RouterContext.Provider, { value }, children);
}

/**
 * Applies a client navigation result for the app URL `page` on top of the view it starts from: slots without a
 * match keep what they showed, and a `retain` result (an interception or a URL that only exists in slots) keeps
 * the page as well.
 */
function settle(next, base, page) {
  const slots = { ...next.state.slots };
  for (const [key, own] of Object.entries(slots)) {
    const before = base.state.slots?.[key];
    if (own.route === null && before?.route) slots[key] = before;
  }
  if (!next.state.retain) return { ...next, state: { ...next.state, slots }, page };
  const { pathname, search, searchParams } = next.state;
  const state = { ...base.state, pathname, search, searchParams, slots: { ...base.state.slots, ...slots } };
  return { ...base, state, key: base.key ?? base.state.pathname, page: pageOf(base) };
}

/** The app URL of the page a view shows, which differs from the location while slots are retained. */
const pageOf = (view) => view.page ?? `${view.state.pathname}${view.state.search ?? ""}`;

/**
 * History state of an entry. `from` and `page` restore an intercepted view on back and forward after a reload,
 * when the in-memory view of the entry is gone.
 */
const entryState = (key, from, view) => ({ __volt: key, from, page: pageOf(view) });

const historyKey = () => Math.random().toString(36).slice(2, 10);

/** Header naming the route a client navigation starts from, so the server can resolve intercepting routes. */
const FROM_HEADER = "x-volt-from";

/** Where route data comes from: the server, or JSON files written by `volt export`. */
function dataUrl(path, search, exported) {
  if (exported) return endpoint(`/_volt/data${path === "/" ? "/index" : path}.json`);
  return endpoint(`/_volt/data?path=${encodeURIComponent(path + search)}`);
}

/** Resolves an app href to a URL and its app path (`null` when it is not part of this app). */
function locate(href) {
  const url = new URL(withBase(href), location.href);
  return { url, path: url.origin === location.origin ? stripBase(url.pathname) : null };
}

function saveScroll(key) {
  try {
    sessionStorage.setItem(`volt:scroll:${key}`, String(window.scrollY));
  } catch {}
}

function readScroll(key) {
  try {
    return Number(sessionStorage.getItem(`volt:scroll:${key}`)) || 0;
  } catch {
    return 0;
  }
}

const pathOf = (url) => url.pathname + url.search + url.hash;

function searchParams(search) {
  const out = {};
  for (const [key, value] of new URLSearchParams(search)) {
    out[key] = key in out ? [].concat(out[key], value) : value;
  }
  return out;
}

/**
 * Root component: owns the current view and performs client-side navigation. `shells` holds, for routes
 * with `loading` UI, lazy modules of their segments without the page, shown while a navigation loads.
 */
export function App({ routes, shells = {}, initial }) {
  const [view, setView] = useState(initial);
  const [pending, startTransition] = useTransition();
  const cache = useRef(new Map());
  const scroll = useRef(null);
  const current = useRef(view);
  const completing = useRef(null);
  const ticket = useRef(0);
  const inflight = useRef(null);
  const beforePop = useRef(null);
  // Per history entry: the route its navigation started from and the view it showed, so back and forward
  // resolve intercepting routes and unmatched slots as they were.
  const entries = useRef(new Map());
  const buildId = initial.state.buildId;
  current.current = view;
  const exported = initial.state.export === true;

  const load = useCallback(
    (path, search, fresh, from) => {
      const key = `${path}${search} ${from ?? ""}`;
      const cached = cache.current.get(key);
      if (!fresh && cached && Date.now() - cached.at < PREFETCH_TTL) return cached.promise;
      const headers = { accept: "application/json" };
      if (from) headers[FROM_HEADER] = from;
      const promise = fetch(dataUrl(path, search, exported), { headers })
        .then((response) => {
          if (!response.ok && exported) throw new Error(`no exported data for ${path}`);
          return response.json();
        })
        .then((state) => {
          if (state.redirect) return { redirect: state.redirect };
          return state.retain ? { state } : createView(routes, state);
        });
      cache.current.set(key, { promise, at: Date.now() });
      promise.catch(() => cache.current.delete(key));
      return promise;
    },
    [routes, exported],
  );

  /** The loading view of a route with `loading` UI: its layouts around a page that is still pending. */
  const preview = useCallback(
    async (path, search) => {
      const match = findRoute(Object.keys(routes), path);
      const shell = match && shells[match.id];
      if (!shell) return null;
      const entry = routes[match.id];
      if (typeof entry === "function") entry().catch(() => {});
      const module = await shell();
      const state = {
        ...current.current.state,
        route: match.id,
        pathname: path,
        search,
        params: match.params,
        searchParams: searchParams(search),
        data: null,
        status: 200,
        error: undefined,
        action: undefined,
        notFound: undefined,
        globalError: undefined,
      };
      return { route: { ...(module.default ?? module), page: PENDING_PAGE }, state, metadata: current.current.metadata };
    },
    [routes, shells],
  );

  /**
   * Cancels the navigation still loading, if any, and returns a new ticket. A cancelled navigation must
   * not commit its view or touch history once another one, a Back or Forward included, has begun.
   */
  const cancel = useCallback(() => {
    if (inflight.current) {
      const cancelled = Object.assign(new Error("Route Cancelled"), { cancelled: true });
      routerEvents.emit("routeChangeError", cancelled, inflight.current.as, { shallow: inflight.current.shallow });
      inflight.current = null;
    }
    return ++ticket.current;
  }, []);

  /** Starts a navigation and returns its ticket. */
  const begin = useCallback(
    (event) => {
      const own = cancel();
      inflight.current = event;
      routerEvents.emit("routeChangeStart", event.as, { shallow: event.shallow });
      return own;
    },
    [cancel],
  );

  const commit = useCallback((next, after, event) => {
    startTransition(() => {
      scroll.current = after;
      completing.current = event;
      setView(next);
    });
  }, []);

  const navigate = useCallback(
    async (href, { replace = false, scroll: toTop = true, fresh = false, shallow = false } = {}) => {
      const { url, path } = locate(href);
      if (path === null) {
        location.href = url.href;
        return;
      }
      const as = pathOf(url);
      const event = { as, shallow };
      const own = begin(event);
      if (fresh) cache.current.clear();
      saveScroll(history.state?.__volt);
      const key = replace ? (history.state?.__volt ?? historyKey()) : historyKey();
      const target = toTop ? { top: 0 } : null;
      if (shallow && url.pathname === location.pathname) {
        inflight.current = null;
        routerEvents.emit("beforeHistoryChange", as, { shallow });
        history[replace ? "replaceState" : "pushState"]({ ...history.state, __volt: key }, "", url.href);
        const state = { ...current.current.state, search: url.search, searchParams: searchParams(url.search) };
        commit({ ...current.current, state }, url.hash ? { hash: url.hash.slice(1) } : target, event);
        return;
      }
      const base = current.current;
      const from = base.state.route;
      let entered = false;
      // The loading preview enters the history entry early; the final view then completes its state.
      const enter = (state) => {
        if (entered) return history.replaceState(state, "", url.href);
        entered = true;
        routerEvents.emit("beforeHistoryChange", as, { shallow });
        history[replace ? "replaceState" : "pushState"](state, "", url.href);
      };
      const request = load(path, url.search, fresh, from);
      let settled = false;
      let previewed = false;
      request.then(
        () => (settled = true),
        () => (settled = true),
      );
      // An urgent update, not a transition, so the target's `loading` boundary shows instead of the old page.
      if (!isPagesView(base)) {
        preview(path, url.search).then(
          (view) => {
            if (!view || settled || own !== ticket.current) return;
            previewed = true;
            enter({ __volt: key, from });
            scroll.current = url.hash ? null : target;
            completing.current = null;
            setView(view);
          },
          () => {},
        );
      }
      let next;
      try {
        next = await request;
      } catch (error) {
        if (own !== ticket.current) return;
        inflight.current = null;
        routerEvents.emit("routeChangeError", error, as, { shallow });
        location.href = url.href;
        return;
      }
      if (own !== ticket.current) return;
      inflight.current = null;
      if (next.redirect) return navigate(next.redirect, { replace: true, scroll: toTop });
      const view = settle(next, base, path + url.search);
      if (next.state.buildId !== buildId || kindOf(view.route) !== kindOf(base.route)) {
        location.href = url.href;
        return;
      }
      enter(entryState(key, from, view));
      entries.current.set(key, { from, view });
      commit(view, url.hash ? { hash: url.hash.slice(1) } : previewed ? null : target, event);
    },
    [load, commit, preview, begin, buildId],
  );

  const revalidate = useCallback(
    async (paths, { refresh = true } = {}) => {
      const hit = (pathname) => paths.some((path) => path === "*" || matchPath(path, pathname));
      for (const key of [...cache.current.keys()]) if (hit(key.split(/[? ]/)[0])) cache.current.delete(key);
      const current = stripBase(location.pathname);
      if (!refresh || current === null || !hit(current)) return false;
      await navigate(`${current}${location.search}${location.hash}`, { replace: true, scroll: false, fresh: true });
      return true;
    },
    [navigate],
  );

  const router = useMemo(
    () => ({
      push: (href, options) => navigate(href, options),
      replace: (href, options) => navigate(href, { ...options, replace: true }),
      refresh: () =>
        navigate(`${stripBase(location.pathname) ?? "/"}${location.search}${location.hash}`, {
          replace: true,
          scroll: false,
          fresh: true,
        }),
      back: () => history.back(),
      forward: () => history.forward(),
      prefetch: (href) => {
        const { url, path } = locate(href);
        if (path === null) return;
        load(path, url.search, false, current.current.state.route).catch(() => {});
        const match = findRoute(Object.keys(routes), path);
        if (match && shells[match.id]) shells[match.id]().catch(() => {});
      },
      beforePopState: (callback) => {
        beforePop.current = callback;
      },
    }),
    [navigate, load, routes, shells],
  );

  useEffect(() => {
    controller.router = router;
    controller.state = current.current.state;
    routerEvents.emit("ready");
    controller.revalidate = revalidate;
    history.scrollRestoration = "manual";
    // A document load shows the page itself, whatever the entry showed before a reload.
    const initialKey = history.state?.__volt ?? historyKey();
    history.replaceState({ __volt: initialKey }, "", location.href);
    if (!entries.current.has(initialKey)) entries.current.set(initialKey, { from: null, view: current.current });
    // The page under an intercepted view whose entry was created before a reload.
    const restore = async (page) => {
      if (!page) return null;
      const [path, query] = page.split(/\?(.*)/s);
      const view = await load(path, query ? `?${query}` : "", false, null);
      return view.redirect || view.state.retain ? null : view;
    };
    const onPopState = async (event) => {
      const key = event.state?.__volt;
      if (!key) return;
      const as = pathOf(location);
      if (beforePop.current && beforePop.current({ url: as, as, options: {} }) === false) {
        cancel();
        return;
      }
      const change = { as, shallow: false };
      const own = begin(change);
      try {
        const path = stripBase(location.pathname);
        if (path === null) return location.reload();
        const entry = entries.current.get(key);
        const from = entry ? entry.from : event.state.from;
        const next = await load(path, location.search, false, from);
        if (own !== ticket.current) return;
        inflight.current = null;
        if (next.redirect || next.state.buildId !== buildId) return location.reload();
        const base = entry?.view ?? (next.state.retain ? await restore(event.state.page) : current.current);
        if (own !== ticket.current) return;
        if (!base) return location.reload();
        const view = settle(next, base, path + location.search);
        if (kindOf(view.route) !== kindOf(current.current.route)) return location.reload();
        entries.current.set(key, { from: from ?? null, view });
        commit(view, { top: readScroll(key) }, change);
      } catch (error) {
        if (own !== ticket.current) return;
        inflight.current = null;
        routerEvents.emit("routeChangeError", error, as, { shallow: false });
        location.reload();
      }
    };
    addEventListener("popstate", onPopState);
    return () => removeEventListener("popstate", onPopState);
  }, [router, revalidate, load, commit, cancel, begin, buildId]);

  useEffect(() => {
    controller.state = view.state;
    const target = scroll.current;
    scroll.current = null;
    const event = completing.current;
    completing.current = null;
    if (target?.hash) document.getElementById(target.hash)?.scrollIntoView();
    else if (target) window.scrollTo(0, target.top);
    if (event) routerEvents.emit("routeChangeComplete", event.as, { shallow: event.shallow });
  }, [view]);

  const showNotFound = useCallback(async () => {
    const next = await createView(routes, { ...view.state, route: NOT_FOUND_ROUTE, status: 404 });
    commit(next, null, null);
  }, [routes, view, commit]);

  const value = useMemo(() => ({ router, pending, state: view.state }), [router, pending, view.state]);
  return h(
    RouterContext.Provider,
    { value },
    h(RootBoundary, { view, onRedirect: (r) => navigate(r.location, { replace: r.type === "replace" }), onNotFound: showNotFound }, h(Tree, { view })),
  );
}
