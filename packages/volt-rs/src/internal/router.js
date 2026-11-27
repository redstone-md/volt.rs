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
  useState,
  useTransition,
} from "react";
import { isNotFound, readRedirect } from "./errors.js";
import { routerEvents } from "./events.js";
import { Metadata, resolveMetadata } from "./metadata.js";
import { ERROR_ROUTE, NOT_FOUND_ROUTE, pagesRouter, toProps } from "./pages.js";

export { ERROR_ROUTE, NOT_FOUND_ROUTE };
const PREFETCH_TTL = 30_000;

export const RouterContext = createContext(null);

const controller = { router: null, state: null };

/** Router of the mounted app, for use outside React components. */
export function getRouter() {
  return controller.router;
}

/** Page state of the mounted app, for the `volt-rs/router` singleton. */
export function getRouterState() {
  return controller.state;
}

const kindOf = (route) => route.kind ?? "app";

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
  const metadata = kindOf(route) === "pages" ? null : await resolveMetadata(route, pageProps(state), state.site);
  return { route, state, metadata };
}

class ErrorBoundary extends Component {
  constructor(props) {
    super(props);
    this.state = { error: null };
  }
  static getDerivedStateFromError(error) {
    return { error };
  }
  render() {
    const { error } = this.state;
    if (!error) return this.props.children;
    if (isNotFound(error) || readRedirect(error)) throw error;
    return h(this.props.fallback, { error, reset: () => this.setState({ error: null }) });
  }
}

class NotFoundBoundary extends Component {
  constructor(props) {
    super(props);
    this.state = { error: null };
  }
  static getDerivedStateFromError(error) {
    return { error };
  }
  render() {
    const { error } = this.state;
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
  const props = useMemo(() => pageProps(state), [state]);
  let node = state.route === ERROR_ROUTE ? h(Page, { error: state.error, reset: () => location.reload() }) : h(Page, props);
  for (let i = route.segments.length - 1; i >= 0; i--) {
    const segment = route.segments[i];
    if (segment.notFound) node = h(NotFoundBoundary, { key: state.pathname, fallback: component(segment.notFound) }, node);
    if (segment.loading) node = h(Suspense, { fallback: h(component(segment.loading)) }, node);
    if (segment.error) node = h(ErrorBoundary, { key: state.pathname, fallback: component(segment.error) }, node);
    if (segment.template) node = h(component(segment.template), { key: state.pathname }, node);
    if (i === 0) node = h(Fragment, null, h(Metadata, { metadata }), node);
    if (segment.layout) node = h(component(segment.layout), { params: props.params }, node);
  }
  return node;
}

const historyKey = () => Math.random().toString(36).slice(2, 10);

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

/** Root component: owns the current view and performs client-side navigation. */
export function App({ routes, initial }) {
  const [view, setView] = useState(initial);
  const [pending, startTransition] = useTransition();
  const cache = useRef(new Map());
  const scroll = useRef(null);
  const current = useRef(view);
  const completing = useRef(null);
  const ticket = useRef(0);
  const inflight = useRef(null);
  const beforePop = useRef(null);
  const buildId = initial.state.buildId;
  current.current = view;

  const load = useCallback(
    (url, fresh) => {
      const key = url.pathname + url.search;
      const cached = cache.current.get(key);
      if (!fresh && cached && Date.now() - cached.at < PREFETCH_TTL) return cached.promise;
      const promise = fetch(`/_volt/data?path=${encodeURIComponent(key)}`, { headers: { accept: "application/json" } })
        .then((response) => response.json())
        .then((state) => (state.redirect ? { redirect: state.redirect } : createView(routes, state)));
      cache.current.set(key, { promise, at: Date.now() });
      promise.catch(() => cache.current.delete(key));
      return promise;
    },
    [routes],
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
      const url = new URL(href, location.href);
      if (url.origin !== location.origin) {
        location.href = url.href;
        return;
      }
      const as = pathOf(url);
      const event = { as, shallow };
      const own = ++ticket.current;
      if (inflight.current) {
        const cancelled = Object.assign(new Error("Route Cancelled"), { cancelled: true });
        routerEvents.emit("routeChangeError", cancelled, inflight.current.as, { shallow: inflight.current.shallow });
      }
      inflight.current = event;
      routerEvents.emit("routeChangeStart", as, { shallow });
      if (fresh) cache.current.clear();
      saveScroll(history.state?.__volt);
      const key = replace ? (history.state?.__volt ?? historyKey()) : historyKey();
      const target = toTop ? { top: 0 } : null;
      if (shallow && url.pathname === location.pathname) {
        inflight.current = null;
        routerEvents.emit("beforeHistoryChange", as, { shallow });
        history[replace ? "replaceState" : "pushState"]({ __volt: key }, "", url.href);
        const state = { ...current.current.state, search: url.search, searchParams: searchParams(url.search) };
        commit({ ...current.current, state }, url.hash ? { hash: url.hash.slice(1) } : target, event);
        return;
      }
      let next;
      try {
        next = await load(url, fresh);
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
      if (next.state.buildId !== buildId || kindOf(next.route) !== kindOf(current.current.route)) {
        location.href = url.href;
        return;
      }
      routerEvents.emit("beforeHistoryChange", as, { shallow });
      history[replace ? "replaceState" : "pushState"]({ __volt: key }, "", url.href);
      commit(next, url.hash ? { hash: url.hash.slice(1) } : target, event);
    },
    [load, commit, buildId],
  );

  const router = useMemo(
    () => ({
      push: (href, options) => navigate(href, options),
      replace: (href, options) => navigate(href, { ...options, replace: true }),
      refresh: () => navigate(location.href, { replace: true, scroll: false, fresh: true }),
      back: () => history.back(),
      forward: () => history.forward(),
      prefetch: (href) => {
        const url = new URL(href, location.href);
        if (url.origin === location.origin) load(url, false).catch(() => {});
      },
      beforePopState: (callback) => {
        beforePop.current = callback;
      },
    }),
    [navigate, load],
  );

  useEffect(() => {
    controller.router = router;
    controller.state = current.current.state;
    routerEvents.emit("ready");
    history.scrollRestoration = "manual";
    if (!history.state?.__volt) history.replaceState({ __volt: historyKey() }, "", location.href);
    const onPopState = async (event) => {
      const key = event.state?.__volt;
      if (!key) return;
      const as = pathOf(location);
      if (beforePop.current && beforePop.current({ url: as, as, options: {} }) === false) return;
      routerEvents.emit("routeChangeStart", as, { shallow: false });
      try {
        const next = await load(new URL(location.href), false);
        if (next.redirect || next.state.buildId !== buildId || kindOf(next.route) !== kindOf(current.current.route)) {
          return location.reload();
        }
        commit(next, { top: readScroll(key) }, { as, shallow: false });
      } catch (error) {
        routerEvents.emit("routeChangeError", error, as, { shallow: false });
        location.reload();
      }
    };
    addEventListener("popstate", onPopState);
    return () => removeEventListener("popstate", onPopState);
  }, [router, load, commit, buildId]);

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
