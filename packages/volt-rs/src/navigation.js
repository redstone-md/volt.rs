import { useContext, useMemo } from "react";
import { RouterContext, getRouter } from "./internal/router.js";

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
