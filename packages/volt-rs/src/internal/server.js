import { Fragment, createElement as h } from "react";
import { renderToReadableStream, renderToString } from "react-dom/server";
import DefaultDocument, { DocumentContext } from "../document.js";
import { isNotFound, readRedirect } from "./errors.js";
import { resolveProps, staticPaths } from "./pages.js";
import { App, ERROR_ROUTE, NOT_FOUND_ROUTE, ServerInsertedHTMLContext, createView } from "./router.js";

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

/**
 * Collects `useServerInsertedHTML` callbacks of one render. They run once the tree (or, when streaming, its
 * shell) is complete and what they return goes into the head.
 */
function insertedHTML() {
  const callbacks = [];
  return {
    wrap: (element) => h(ServerInsertedHTMLContext.Provider, { value: (callback) => callbacks.push(callback) }, element),
    head() {
      const nodes = callbacks.map((callback) => callback()).filter((node) => node != null && node !== false);
      return nodes.length ? renderToString(h(Fragment, null, ...nodes)) : "";
    },
  };
}

/**
 * Renders an App Router tree, with `fields` added to its result. `"string"` uses `renderToString`. The other
 * modes report the shell to `sink.shell(json)` and each chunk to `sink.write(bytes)`, then resolve to `null`:
 * `"complete"` waits for every Suspense boundary first, `"stream"` writes chunks as React flushes them. A
 * `notFound()` or `redirect()` raised before the shell is sent is thrown, so it still decides the status code,
 * even when a Suspense boundary caught it. `sink.abort(reason)`, called by the engine when the render runs out
 * of time, makes React finish the document with the boundaries still pending rendered in the browser.
 */
async function renderView(routes, state, mode, sink, fields = {}) {
  const initial = await createView(routes, state);
  const inserted = insertedHTML();
  const element = inserted.wrap(h(App, { routes, initial }));
  const status = state.status ?? 200;
  if (mode === "string") {
    const html = renderToString(element);
    return { html, head: inserted.head(), status, ...fields };
  }
  let raised = null;
  const controller = new AbortController();
  sink.abort = (message) => {
    console.warn(`${state.pathname}: ${message}; pending Suspense boundaries are left to the browser`);
    controller.abort(Object.assign(new Error(message), { volt: "abort" }));
  };
  const stream = await renderToReadableStream(element, {
    signal: controller.signal,
    onError(error) {
      if (isNotFound(error) || readRedirect(error)) raised ??= error;
      else if (error?.volt !== "abort") console.error(error);
      return typeof error?.digest === "string" ? error.digest : undefined;
    },
  });
  if (mode === "complete") await stream.allReady;
  if (raised) {
    controller.abort();
    throw raised;
  }
  const incomplete = controller.signal.aborted;
  const shell = { status, route: state.route, incomplete, head: inserted.head(), ...fields };
  if (!sink.shell(JSON.stringify(shell))) {
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

/** Segments with a `not-found` below the root, innermost first: where a `notFound()` may be caught. */
const notFoundLevels = (route) =>
  route.segments
    .map((segment, index) => (index > 0 && segment.notFound ? index : -1))
    .filter((index) => index > 0)
    .reverse();

/**
 * Renders an App Router page. A `notFound()` that reaches the shell is retried with the innermost `not-found`
 * shown in place of its segment's children, then the next one out (the layout of that segment may be the one
 * that threw), before the root not-found page takes over.
 */
async function renderApp(routes, route, state, mode, sink) {
  try {
    return await renderView(routes, state, mode, sink);
  } catch (error) {
    if (!isNotFound(error)) throw error;
    for (const index of notFoundLevels(route)) {
      try {
        return await renderView(routes, { ...state, notFound: index, status: 404 }, mode, sink, { notFound: index });
      } catch (retry) {
        if (!isNotFound(retry)) throw retry;
      }
    }
    throw error;
  }
}

/** Whether `global-error` can replace the document, when even the error page failed to render. */
async function hasGlobalError(routes, state) {
  if (state.route !== ERROR_ROUTE || state.globalError) return false;
  return Boolean((await loadRoute(routes, ERROR_ROUTE)).globalError);
}

/** `global-error` in place of the document. */
function renderGlobalError(routes, state, mode, sink) {
  const next = { ...state, globalError: true, status: state.status ?? 500 };
  return renderView(routes, next, mode, sink, { globalError: true });
}

async function renderPage(routes, options, state, mode, sink) {
  const route = await loadRoute(routes, state.route);
  if (!isPages(route)) return renderApp(routes, route, state, mode, sink);
  const result = await resolveProps(route, state);
  if (result.notFound) return notFound(routes, options, state, mode, sink);
  if (result.redirect) return { html: "", status: result.redirect.status, redirect: result.redirect };
  const next = { ...state, data: result.props };
  const initial = await createView(routes, next);
  const inserted = insertedHTML();
  const html = await renderDocument(options.document, inserted.wrap(h(App, { routes, initial })), next);
  return {
    html,
    head: inserted.head(),
    status: next.status ?? 200,
    data: next.data,
    static: result.static,
    revalidate: result.revalidate,
  };
}

async function notFound(routes, options, state, mode, sink) {
  if (state.route === NOT_FOUND_ROUTE) throw new Error("the not-found page itself reported not found");
  const next = { ...state, route: NOT_FOUND_ROUTE, status: 404, data: null };
  const rendered = await renderPage(routes, options, next, mode, sink);
  if (rendered === null) return null;
  return { ...rendered, status: 404, route: NOT_FOUND_ROUTE, static: false, revalidate: null };
}

/** A render result as the engine expects it: `""` once it was streamed to the sink. */
const reply = (rendered) => (rendered === null ? "" : JSON.stringify(rendered));

/**
 * Renders a page state, turning `notFound()` and `redirect()` into the not-found page and a redirect, and a
 * failed error page into `global-error`.
 */
async function respond(routes, options, payload, mode, sink) {
  const state = JSON.parse(payload);
  try {
    return reply(await renderPage(routes, options, state, mode, sink));
  } catch (error) {
    const redirect = readRedirect(error);
    if (redirect) return JSON.stringify({ html: "", status: redirect.status, redirect });
    if (isNotFound(error) && state.route !== NOT_FOUND_ROUTE) {
      return reply(await notFound(routes, options, state, mode, sink));
    }
    if (await hasGlobalError(routes, state)) return reply(await renderGlobalError(routes, state, mode, sink));
    throw error;
  }
}

/** The page module followed by its layouts, innermost first. */
const modules = (route) => [route.page, ...route.segments.map((segment) => segment.layout).reverse()].filter(Boolean);

/** First `dynamicParams` export, the page taking precedence over its layouts. */
function dynamicParams(route) {
  const found = modules(route).find((module) => typeof module.dynamicParams === "boolean");
  return found ? found.dynamicParams : true;
}

/** Route segment config `dynamic`, the page taking precedence over its layouts. */
function dynamicMode(route) {
  return modules(route).find((module) => typeof module.dynamic === "string")?.dynamic ?? "auto";
}

const isoDate = (value) => (value instanceof Date ? value.toISOString() : value == null ? null : String(value));

/** Sitemap entries as `voltrs::SitemapEntry` reads them. */
const sitemapEntries = (entries) =>
  (entries ?? []).map((entry) => ({
    url: entry.url,
    lastModified: isoDate(entry.lastModified),
    changeFrequency: entry.changeFrequency ?? null,
    priority: typeof entry.priority === "number" ? entry.priority : null,
  }));

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
    /**
     * The complete page with every Suspense boundary resolved, for caches, prerendering and crawlers,
     * written to `sink` like `stream` once it is ready.
     */
    document: (payload, sink) => respond(routes, options, payload, "complete", sink),
    /**
     * Streams the page to `sink` (`shell(json)`, then `write(bytes)` per chunk) and resolves to `""`;
     * Pages Router pages and redirects resolve to a complete result instead.
     */
    stream: (payload, sink) => respond(routes, options, payload, "stream", sink),
    /** Default export of `app/sitemap.ts`, `app/robots.ts` or `app/manifest.ts`, as JSON. */
    async metadataRoute(name) {
      const module = options.metadata?.[name];
      if (!module) throw new Error(`no metadata route \`${name}\``);
      const value = typeof module.default === "function" ? await module.default() : module.default;
      return JSON.stringify(name === "sitemap" ? sitemapEntries(value) : (value ?? {}));
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
            dynamic: dynamicMode(route),
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
