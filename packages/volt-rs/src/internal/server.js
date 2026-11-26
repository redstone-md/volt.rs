import { createElement as h } from "react";
import { renderToString } from "react-dom/server";
import { isNotFound, readRedirect } from "./errors.js";
import { App, NOT_FOUND_ROUTE, createView } from "./router.js";

async function renderState(routes, state) {
  const initial = await createView(routes, state);
  return renderToString(h(App, { routes, initial }));
}

/** Installs the render protocol used by the Rust SSR engine. */
export function createServer(routes) {
  globalThis.__VOLT__ = {
    async render(payload) {
      const state = JSON.parse(payload);
      try {
        const html = await renderState(routes, state);
        return JSON.stringify({ html, status: state.status ?? 200 });
      } catch (error) {
        const redirect = readRedirect(error);
        if (redirect) return JSON.stringify({ html: "", status: redirect.status, redirect });
        if (!isNotFound(error) || state.route === NOT_FOUND_ROUTE) throw error;
        const html = await renderState(routes, { ...state, route: NOT_FOUND_ROUTE, status: 404 });
        return JSON.stringify({ html, status: 404 });
      }
    },
    routes() {
      return JSON.stringify(
        Object.entries(routes).map(([id, route]) => ({
          id,
          revalidate: typeof route.page?.revalidate === "number" ? route.page.revalidate : null,
          dynamic: route.page?.dynamic ?? "auto",
        })),
      );
    },
  };
}
