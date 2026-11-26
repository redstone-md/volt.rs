import { createElement as h, useEffect } from "react";

const loaded = new Set();

function inject({ src, id, children, dangerouslySetInnerHTML, onLoad, onReady, onError, strategy, ...attrs }) {
  const key = id ?? src;
  if (key && loaded.has(key)) {
    onReady?.();
    return;
  }
  const element = document.createElement("script");
  for (const [name, value] of Object.entries(attrs)) element.setAttribute(name === "className" ? "class" : name, value);
  if (id) element.id = id;
  const inline = dangerouslySetInnerHTML?.__html ?? (typeof children === "string" ? children : "");
  element.addEventListener("load", (event) => {
    onLoad?.(event);
    onReady?.();
  });
  if (onError) element.addEventListener("error", onError);
  if (src) element.src = src;
  else element.textContent = inline;
  if (key) loaded.add(key);
  document.body.appendChild(element);
  if (!src) onReady?.();
}

/** Third-party scripts with Next.js loading strategies. */
export default function Script(props) {
  const { strategy = "afterInteractive", src, id } = props;
  useEffect(() => {
    if (strategy === "beforeInteractive") return;
    if (strategy === "lazyOnload") {
      const run = () => (window.requestIdleCallback ?? setTimeout)(() => inject(props));
      if (document.readyState === "complete") run();
      else window.addEventListener("load", run, { once: true });
      return;
    }
    inject(props);
  }, [src, id, strategy]);
  if (strategy !== "beforeInteractive") return null;
  const { onLoad, onReady, onError, strategy: _, children, ...attrs } = props;
  if (src) return h("script", { ...attrs, src, async: true });
  return h("script", { ...attrs, dangerouslySetInnerHTML: props.dangerouslySetInnerHTML ?? { __html: children ?? "" } });
}

export { Script };
