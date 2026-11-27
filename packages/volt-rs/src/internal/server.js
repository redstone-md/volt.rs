import { createElement as h } from "react";
import { renderToReadableStream, renderToString } from "react-dom/server";
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

async function readAll(stream) {
  const chunks = [];
  let length = 0;
  for await (const chunk of stream) {
    chunks.push(chunk);
    length += chunk.byteLength;
  }
  const bytes = new Uint8Array(length);
  let offset = 0;
  for (const chunk of chunks) {
    bytes.set(chunk, offset);
    offset += chunk.byteLength;
  }
  return new TextDecoder().decode(bytes);
}

/**
 * Renders an App Router tree. `"string"` uses `renderToString`; `"complete"` waits for every Suspense
 * boundary; `"stream"` reports the shell to `sink` once it is ready and then writes each chunk as React
 * flushes it, returning `null`. A `notFound()` or `redirect()` raised before the shell is sent is thrown,
 * so it still decides the status code, even when a Suspense boundary caught it.
 */
async function renderApp(element, state, mode, sink) {
  const status = state.status ?? 200;
  if (mode === "string") return { html: renderToString(element), status };
  let raised = null;
  const controller = new AbortController();
  const stream = await renderToReadableStream(element, {
    signal: controller.signal,
    onError(error) {
      if (isNotFound(error) || readRedirect(error)) raised ??= error;
      else console.error(error);
      return typeof error?.digest === "string" ? error.digest : undefined;
    },
  });
  if (mode === "complete") await stream.allReady;
  if (raised) {
    controller.abort();
    throw raised;
  }
  if (mode === "complete") return { html: await readAll(stream), status };
  if (!sink.shell(JSON.stringify({ status, route: state.route }))) {
    controller.abort();
    return null;
  }
  const reader = stream.getReader();
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    if (!sink.write(value)) {
      // The client went away: stop rendering the remaining boundaries.
      controller.abort();
      await reader.cancel().catch(() => {});
      break;
    }
  }
  return null;
}

async function renderPage(routes, options, state, mode, sink) {
  const route = await loadRoute(routes, state.route);
  if (!isPages(route)) {
    const initial = await createView(routes, state);
    return renderApp(h(App, { routes, initial }), state, mode, sink);
  }
  const result = await resolveProps(route, state);
  if (result.notFound) return notFound(routes, options, state, mode, sink);
  if (result.redirect) return { html: "", status: result.redirect.status, redirect: result.redirect };
  const next = { ...state, data: result.props };
  const initial = await createView(routes, next);
  const html = await renderDocument(options.document, h(App, { routes, initial }), next);
  return { html, status: next.status ?? 200, data: next.data, static: result.static, revalidate: result.revalidate };
}

async function notFound(routes, options, state, mode, sink) {
  if (state.route === NOT_FOUND_ROUTE) throw new Error("the not-found page itself reported not found");
  const next = { ...state, route: NOT_FOUND_ROUTE, status: 404, data: null };
  const rendered = await renderPage(routes, options, next, mode, sink);
  if (rendered === null) return null;
  return { ...rendered, status: 404, route: NOT_FOUND_ROUTE, static: false, revalidate: null };
}

/** Renders a page state, turning `notFound()` and `redirect()` into the not-found page and a redirect. */
async function respond(routes, options, payload, mode, sink) {
  const state = JSON.parse(payload);
  try {
    const rendered = await renderPage(routes, options, state, mode, sink);
    return rendered === null ? "" : JSON.stringify(rendered);
  } catch (error) {
    const redirect = readRedirect(error);
    if (redirect) return JSON.stringify({ html: "", status: redirect.status, redirect });
    if (!isNotFound(error) || state.route === NOT_FOUND_ROUTE) throw error;
    const rendered = await notFound(routes, options, state, mode, sink);
    return rendered === null ? "" : JSON.stringify(rendered);
  }
}

/** The page module followed by its layouts, innermost first. */
const modules = (route) => [route.page, ...route.segments.map((segment) => segment.layout).reverse()].filter(Boolean);

/** First `dynamicParams` export, the page taking precedence over its layouts. */
function dynamicParams(route) {
  const found = modules(route).find((module) => typeof module.dynamicParams === "boolean");
  return found ? found.dynamicParams : true;
}

/**
 * Params of every path to prerender. Layout generators run first (outermost first); each later
 * generator is called once per parent result with `{ params }`, as in the Next.js App Router.
 */
async function generateParams(route) {
  const generators = [...route.segments.map((segment) => segment.layout), route.page]
    .map((module) => module?.generateStaticParams)
    .filter((fn) => typeof fn === "function");
  if (!generators.length) return null;
  let sets = [{}];
  for (const generate of generators) {
    const next = [];
    for (const params of sets) {
      for (const child of (await generate({ params })) ?? []) next.push({ ...params, ...child });
    }
    sets = next;
  }
  return sets;
}

/** Installs the render protocol used by the Rust SSR engine. */
export function createServer(routes, options = {}) {
  globalThis.__VOLT__ = {
    /** The page rendered with `renderToString` (`[server] streaming = false`). */
    render: (payload) => respond(routes, options, payload, "string"),
    /** The complete page with every Suspense boundary resolved, for caches, prerendering and crawlers. */
    document: (payload) => respond(routes, options, payload, "complete"),
    /**
     * Streams the page to `sink` (`shell(json)`, then `write(bytes)` per chunk) and resolves to `""`;
     * Pages Router pages and redirects resolve to a complete result instead.
     */
    stream: (payload, sink) => respond(routes, options, payload, "stream", sink),
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
            dynamicParams: dynamicParams(route),
            staticParams:
              typeof page.getStaticPaths === "function" ||
              modules(route).some((module) => typeof module.generateStaticParams === "function"),
          });
          continue;
        }
        configs.push({
          id,
          revalidate: null,
          dynamic: typeof page.getServerSideProps === "function" ? "force-dynamic" : "auto",
          static: typeof page.getStaticProps === "function" && typeof page.getServerSideProps !== "function",
          staticParams: typeof page.getStaticProps === "function" && typeof page.getStaticPaths === "function",
        });
      }
      return JSON.stringify(configs);
    },
    /** Build-time paths of a dynamic route (`getStaticPaths` or `generateStaticParams`), evaluated by `volt build`. */
    async staticParams(id) {
      const route = await loadRoute(routes, id);
      const page = route.page;
      if (!page) return "null";
      if (typeof page.getStaticPaths === "function") {
        const { paths, fallback } = await staticPaths(page, id);
        return JSON.stringify({ params: [], paths, dynamicParams: fallback !== false });
      }
      const params = await generateParams(route);
      return JSON.stringify(params ? { params, paths: [], dynamicParams: dynamicParams(route) } : null);
    },
  };
}
