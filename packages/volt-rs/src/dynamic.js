import { Suspense, createElement as h, lazy, useEffect, useState } from "react";

/**
 * Lazily loaded component, compatible with `next/dynamic`. Unless `ssr` is false, loading starts when
 * `dynamic()` is called, so the server renders the content in place (or streams it once it resolves)
 * and the browser fetches its chunk with the page. With `ssr: false` the server and the first client
 * render show `loading`, and the component loads after hydration.
 */
export default function dynamic(loader, { loading: Loading, ssr = true } = {}) {
  let loaded = null;
  let pending = null;
  const load = () => {
    pending ??= Promise.resolve(loader()).then((module) => (loaded = module.default ?? module));
    return pending;
  };
  const Lazy = lazy(() => load().then((component) => ({ default: component })));
  const fallback = () => (Loading ? h(Loading, { isLoading: true, pastDelay: true, error: null }) : null);
  // The boundary is always there, so server and client trees match whether or not the chunk has arrived.
  const render = (props) => h(Suspense, { fallback: fallback() }, h(loaded ?? Lazy, props));
  if (ssr !== false) {
    load().catch(() => {
      pending = null;
    });
    return function Dynamic(props) {
      return render(props);
    };
  }
  return function DynamicClient(props) {
    const [mounted, setMounted] = useState(false);
    useEffect(() => setMounted(true), []);
    return mounted ? render(props) : fallback();
  };
}
