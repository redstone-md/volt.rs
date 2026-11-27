import { createElement as h } from "react";
import { renderToString } from "react-dom/server";
import DefaultDocument, { DocumentContext } from "../document.js";
import { isNotFound, readRedirect } from "./errors.js";
import { resolveProps, staticPaths } from "./pages.js";
import { App, NOT_FOUND_ROUTE, createView } from "./router.js";

const isPages = (route) => route?.kind === "pages";

async function loadRoute(routes, id) {
  const entry = routes[id] ?? routes[NOT_FOUND_ROUTE];
  const module = typeof entry === "function" ? await entry() : entry;
  return module.default ?? module;
}

/** Renders the Pages Router document on the server, with the app tree inside `<Main />`. */
async function renderDocument(document, main, state) {
  const Doc = document?.default ?? DefaultDocument;
  const query = { ...(state.searchParams ?? {}), ...(state.params ?? {}) };
  const context = {
    pathname: state.route,
    query,
    asPath: `${state.pathname}${state.search ?? ""}`,
    renderPage: () => ({ html: "", head: [] }),
    defaultGetInitialProps: (ctx) => DefaultDocument.getInitialProps(ctx),
  };
  const props = typeof Doc.getInitialProps === "function" ? await Doc.getInitialProps(context) : {};
  return renderToString(h(DocumentContext.Provider, { value: { main } }, h(Doc, props ?? {})));
}

async function renderPage(routes, options, state) {
  const route = await loadRoute(routes, state.route);
  if (!isPages(route)) {
    const initial = await createView(routes, state);
    return { html: renderToString(h(App, { routes, initial })), status: state.status ?? 200 };
  }
  const result = await resolveProps(route, state);
  if (result.notFound) return notFound(routes, options, state);
  if (result.redirect) return { html: "", status: result.redirect.status, redirect: result.redirect };
  const next = { ...state, data: result.props };
  const initial = await createView(routes, next);
  const html = await renderDocument(options.document, h(App, { routes, initial }), next);
  return { html, status: next.status ?? 200, data: next.data, static: result.static, revalidate: result.revalidate };
}

async function notFound(routes, options, state) {
  if (state.route === NOT_FOUND_ROUTE) throw new Error("the not-found page itself reported not found");
  const rendered = await renderPage(routes, options, { ...state, route: NOT_FOUND_ROUTE, status: 404, data: null });
  return { ...rendered, status: 404, route: NOT_FOUND_ROUTE, static: false, revalidate: null };
}

/** Installs the render protocol used by the Rust SSR engine. */
export function createServer(routes, options = {}) {
  globalThis.__VOLT__ = {
    async render(payload) {
      const state = JSON.parse(payload);
      try {
        return JSON.stringify(await renderPage(routes, options, state));
      } catch (error) {
        const redirect = readRedirect(error);
        if (redirect) return JSON.stringify({ html: "", status: redirect.status, redirect });
        if (!isNotFound(error) || state.route === NOT_FOUND_ROUTE) throw error;
        return JSON.stringify(await notFound(routes, options, state));
      }
    },
    /** Pages Router props for a client navigation (`/_volt/data`). */
    async props(payload) {
      const state = JSON.parse(payload);
      const route = await loadRoute(routes, state.route);
      if (!isPages(route)) return JSON.stringify({ data: state.data });
      const result = await resolveProps(route, state);
      if (result.notFound) return JSON.stringify({ notFound: true });
      if (result.redirect) return JSON.stringify({ redirect: result.redirect });
      return JSON.stringify({ data: result.props });
    },
    async routes() {
      const configs = [];
      for (const id of Object.keys(routes)) {
        const route = await loadRoute(routes, id);
        const page = route.page ?? {};
        if (!isPages(route)) {
          configs.push({
            id,
            revalidate: typeof page.revalidate === "number" ? page.revalidate : null,
            dynamic: page.dynamic ?? "auto",
          });
          continue;
        }
        const prerenderable = typeof page.getStaticProps === "function" && typeof page.getStaticPaths === "function";
        configs.push({
          id,
          revalidate: null,
          dynamic: typeof page.getServerSideProps === "function" ? "force-dynamic" : "auto",
          static: typeof page.getStaticProps === "function" && typeof page.getServerSideProps !== "function",
          paths: prerenderable ? (await staticPaths(page, id)).paths : [],
        });
      }
      return JSON.stringify(configs);
    },
  };
}
