import { Suspense, createElement as h, lazy } from "react";

/** Lazily loaded component, compatible with `next/dynamic`. */
export default function dynamic(loader, { loading: Loading } = {}) {
  const Lazy = lazy(async () => {
    const module = await loader();
    return { default: module.default ?? module };
  });
  return function Dynamic(props) {
    return h(Suspense, { fallback: Loading ? h(Loading) : null }, h(Lazy, props));
  };
}
