import { createElement as h, forwardRef, useCallback, useContext, useEffect, useRef } from "react";
import { withBase } from "./internal/config.js";
import { RouterContext, localize } from "./internal/router.js";

const toHref = (href) => (typeof href === "string" ? href : formatUrl(href));

function formatUrl({ pathname = "", query, hash = "" }) {
  const search = query ? new URLSearchParams(query).toString() : "";
  return `${pathname}${search ? `?${search}` : ""}${hash && !hash.startsWith("#") ? `#${hash}` : hash}`;
}

function isLocal(href) {
  if (href.startsWith("/") && !href.startsWith("//")) return true;
  try {
    return new URL(href, location.href).origin === location.origin;
  } catch {
    return false;
  }
}

/**
 * Client-side navigation with prefetching. Renders a plain `<a>`. With `[i18n]`, local hrefs get the
 * current locale prefix; `locale="de"` links to another locale and `locale={false}` keeps `href` as given.
 */
const Link = forwardRef(function Link(
  { href, locale, replace = false, scroll = true, prefetch = true, onClick, onMouseEnter, onTouchStart, children, ...rest },
  forwarded,
) {
  const context = useContext(RouterContext);
  const target = localize(toHref(href), locale, context?.state.locale);
  // The href is already localized; only an explicit locale is passed on, so the router remembers the choice.
  const choice = typeof locale === "string" ? locale : false;
  const own = useRef(null);
  const ref = useCallback(
    (node) => {
      own.current = node;
      if (typeof forwarded === "function") forwarded(node);
      else if (forwarded) forwarded.current = node;
    },
    [forwarded],
  );

  useEffect(() => {
    const node = own.current;
    if (!prefetch || !node || !context || typeof IntersectionObserver === "undefined" || !isLocal(target)) return;
    const observer = new IntersectionObserver((entries) => {
      if (entries.some((e) => e.isIntersecting)) {
        observer.disconnect();
        const idle = window.requestIdleCallback ?? ((fn) => setTimeout(fn, 1));
        idle(() => context.router.prefetch(target, { locale: false }));
      }
    });
    observer.observe(node);
    return () => observer.disconnect();
  }, [prefetch, target, context]);

  const warm = () => prefetch !== false && context && isLocal(target) && context.router.prefetch(target, { locale: false });

  return h(
    "a",
    {
      ...rest,
      ref,
      href: withBase(target),
      onMouseEnter(event) {
        onMouseEnter?.(event);
        warm();
      },
      onTouchStart(event) {
        onTouchStart?.(event);
        warm();
      },
      onClick(event) {
        onClick?.(event);
        if (
          event.defaultPrevented ||
          !context ||
          event.button !== 0 ||
          event.metaKey ||
          event.ctrlKey ||
          event.shiftKey ||
          event.altKey ||
          (rest.target && rest.target !== "_self") ||
          rest.download !== undefined ||
          !isLocal(target)
        )
          return;
        event.preventDefault();
        context.router[replace ? "replace" : "push"](target, { scroll, locale: choice });
      },
    },
    children,
  );
});

export default Link;
export { Link };
