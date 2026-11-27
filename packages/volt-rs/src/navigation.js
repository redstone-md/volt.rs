import { useContext, useMemo } from "react";
import { LayoutSegmentContext, RouterContext, ServerInsertedHTMLContext, getRouter } from "./internal/router.js";

export { notFound, permanentRedirect, redirect } from "./internal/errors.js";
export { getRouter };

function useRouterContext() {
  const context = useContext(RouterContext);
  if (!context) throw new Error("volt-rs: navigation hooks must be used inside the app tree");
  return context;
}

export function useRouter() {
  return useRouterContext().router;
}

export function usePathname() {
  return useRouterContext().state.pathname;
}

export function useSearchParams() {
  const { search } = useRouterContext().state;
  return useMemo(() => new URLSearchParams(search), [search]);
}

export function useParams() {
  return useRouterContext().state.params;
}

/** Data returned by the route's Rust loader. */
export function useData() {
  return useRouterContext().state.data;
}

/** True while a client-side navigation is in flight. */
export function useNavigationPending() {
  return useRouterContext().pending;
}

/**
 * Active segments below the layout this is called from (`["blog", "hello"]` in the root layout on
 * `/blog/hello`). Parallel route slots are not supported, so any key other than `children` returns `[]`.
 */
export function useSelectedLayoutSegments(parallelRoutesKey = "children") {
  const segments = useContext(LayoutSegmentContext);
  return parallelRoutesKey === "children" ? (segments ?? []) : [];
}

/** The active segment one level below the layout this is called from, or `null` at the page itself. */
export function useSelectedLayoutSegment(parallelRoutesKey = "children") {
  return useSelectedLayoutSegments(parallelRoutesKey)[0] ?? null;
}

/**
 * Adds the returned elements to the head of the server-rendered document once the whole tree has rendered,
 * which CSS-in-JS libraries use to flush their styles. Does nothing in the browser.
 */
export function useServerInsertedHTML(callback) {
  const register = useContext(ServerInsertedHTMLContext);
  if (register) register(callback);
}

/** Next.js compatibility no-op: caching follows route exports (`dynamic`, `revalidate`) and Rust loaders. */
export function unstable_noStore() {}

/** Next.js compatibility no-op: resolves immediately. */
export async function connection() {}
