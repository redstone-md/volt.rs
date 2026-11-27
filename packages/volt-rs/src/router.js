import { createElement as h, useContext, useMemo } from "react";
import { routerEvents } from "./internal/events.js";
import { localeFields, pagesRouter } from "./internal/pages.js";
import { RouterContext, getRouter, getRouterState } from "./internal/router.js";

/** Pages Router style router (`next/router`): pathname, query, asPath, events and navigation methods. */
export function useRouter() {
  const context = useContext(RouterContext);
  if (!context) throw new Error("volt-rs: useRouter must be used inside the app tree");
  const { router, state } = context;
  return useMemo(() => pagesRouter(router, state), [router, state]);
}

/** Injects `router` into a component's props. */
export function withRouter(Component) {
  function WithRouter(props) {
    return h(Component, { ...props, router: useRouter() });
  }
  WithRouter.displayName = `withRouter(${Component.displayName ?? Component.name ?? "Component"})`;
  return WithRouter;
}

function current() {
  const router = getRouter();
  const state = getRouterState();
  if (!router || !state) throw new Error('volt-rs: the Router singleton is only available in the browser after hydration');
  return pagesRouter(router, state);
}

const waiting = [];

/** The router singleton, the default export of `next/router`. */
const Router = {
  events: routerEvents,
  get router() {
    return getRouter() ? current() : null;
  },
  get pathname() {
    return current().pathname;
  },
  get route() {
    return current().route;
  },
  get query() {
    return current().query;
  },
  get asPath() {
    return current().asPath;
  },
  get basePath() {
    return "";
  },
  get isReady() {
    return Boolean(getRouter());
  },
  get isFallback() {
    return false;
  },
  get locale() {
    return localeFields(getRouterState() ?? {}).locale;
  },
  get locales() {
    return localeFields({}).locales;
  },
  get defaultLocale() {
    return localeFields({}).defaultLocale;
  },
  push: (...args) => current().push(...args),
  replace: (...args) => current().replace(...args),
  reload: () => current().reload(),
  back: () => current().back(),
  forward: () => current().forward(),
  prefetch: (...args) => current().prefetch(...args),
  beforePopState: (callback) => current().beforePopState(callback),
  ready(callback) {
    if (getRouter()) callback();
    else waiting.push(callback);
  },
};

routerEvents.on("ready", () => {
  for (const callback of waiting.splice(0)) callback();
});

export default Router;
export { Router };
