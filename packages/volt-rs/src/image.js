import { createElement as h, forwardRef, useCallback, useRef, useState } from "react";
import { preload as preloadResource } from "react-dom";
import { BASE_PATH, IMAGE_OPTIMIZATION, IMAGES, endpoint } from "./internal/config.js";

/** `[images] device_sizes`. */
export const DEVICE_SIZES = IMAGES.deviceSizes;
/** `[images] image_sizes`. */
export const IMAGE_SIZES = IMAGES.imageSizes;
const ALL_SIZES = [...new Set([...IMAGE_SIZES, ...DEVICE_SIZES])].sort((a, b) => a - b);

export function defaultLoader({ src, width, quality }) {
  return endpoint(`/_volt/image?url=${encodeURIComponent(src)}&w=${width}&q=${quality ?? 75}`);
}

// Static imports already carry the base path; plain `public/` paths get it here.
const withBasePath = (src) =>
  BASE_PATH && typeof src === "string" && src.startsWith("/") && !src.startsWith("//") && !src.startsWith(`${BASE_PATH}/`)
    ? `${BASE_PATH}${src}`
    : src;

/** Splits an absolute URL like the optimizer does; `null` for anything it would refuse. */
function parseRemote(src) {
  const match = /^(https?):\/\/([^/?#]*)([^?#]*)(\?[^#]*)?/i.exec(src);
  if (!match || /[\\ ]/.test(src)) return null;
  const authority = match[2];
  const colon = authority.lastIndexOf(":");
  const hostname = colon < 0 ? authority : authority.slice(0, colon);
  const port = colon < 0 ? null : authority.slice(colon + 1);
  if (!hostname || !/^[a-z0-9._-]+$/i.test(hostname)) return null;
  if (port !== null && !/^\d{1,5}$/.test(port)) return null;
  const pathname = match[3] || "/";
  if (pathname.split("/").some((segment) => segment === "." || segment === "..")) return null;
  const search = match[4] && match[4] !== "?" ? match[4] : "";
  return { protocol: match[1].toLowerCase(), hostname: hostname.toLowerCase(), port: port === null ? null : Number(port), pathname, search };
}

const escapeRegExp = (text) => text.replace(/[.*+?^${}()|[\]\\/]/g, "\\$&");

/** `**` spans separators, `*` stays within one part (a label of a host name, a segment of a path). */
function globMatch(pattern, text, separator) {
  let source = "^";
  for (let i = 0; i < pattern.length; i++) {
    if (pattern[i] === "*" && pattern[i + 1] === "*") {
      source += ".*";
      i++;
    } else if (pattern[i] === "*") source += `[^${escapeRegExp(separator)}]*`;
    else source += escapeRegExp(pattern[i]);
  }
  return new RegExp(`${source}$`).test(text);
}

function matchesPattern(pattern, url) {
  return (
    (pattern.protocol == null || pattern.protocol === url.protocol) &&
    (pattern.port == null || pattern.port === url.port) &&
    globMatch(pattern.hostname, url.hostname, ".") &&
    globMatch(pattern.pathname ?? "**", url.pathname, "/") &&
    (pattern.search == null || pattern.search === url.search)
  );
}

/** Whether `src` is a remote URL allowed by `[images] remote_patterns`. */
export function isAllowedRemote(src) {
  const url = typeof src === "string" ? parseRemote(src) : null;
  return !!url && IMAGES.remotePatterns.some((pattern) => matchesPattern(pattern, url));
}

const isSvg = (src) => /\.svg($|[?#])/i.test(src);
const isLocal = (src) => src.startsWith("/") && !src.startsWith("//");

/** Whether the default loader can serve `src`: local paths, allowed remote URLs, SVG only when allowed. */
function optimizable(src) {
  if (!IMAGE_OPTIMIZATION) return false;
  if (isSvg(src) && !IMAGES.dangerouslyAllowSVG) return false;
  return isLocal(src) || isAllowedRemote(src);
}

/** `srcset` candidates, chosen like Next.js: `w` descriptors for responsive images, `1x`/`2x` for fixed widths. */
function candidates(width, sizes) {
  if (sizes) {
    const percents = [...sizes.matchAll(/(^|\s)(1?\d?\d)vw/g)].map((match) => Number.parseInt(match[2], 10));
    if (percents.length) {
      const smallest = Math.min(...percents) * 0.01;
      return { widths: ALL_SIZES.filter((size) => size >= DEVICE_SIZES[0] * smallest), kind: "w" };
    }
    return { widths: ALL_SIZES, kind: "w" };
  }
  if (!width) return { widths: DEVICE_SIZES, kind: "w" };
  const pick = (target) => ALL_SIZES.find((size) => size >= target) ?? ALL_SIZES[ALL_SIZES.length - 1];
  return { widths: [...new Set([pick(width), pick(width * 2)])], kind: "x" };
}

/** Next.js-style blurred SVG around a tiny preview, so the placeholder is smooth rather than pixelated. */
function blurSvg({ width, height, blurWidth, blurHeight, blurDataURL, objectFit }) {
  const std = 20;
  const svgWidth = blurWidth ? blurWidth * 40 : width;
  const svgHeight = blurHeight ? blurHeight * 40 : height;
  const viewBox = svgWidth && svgHeight ? `viewBox='0 0 ${svgWidth} ${svgHeight}'` : "";
  const aspect = viewBox ? "none" : objectFit === "contain" ? "xMidYMid" : objectFit === "cover" ? "xMidYMid slice" : "none";
  return `%3Csvg xmlns='http://www.w3.org/2000/svg' ${viewBox}%3E%3Cfilter id='b' color-interpolation-filters='sRGB'%3E%3CfeGaussianBlur stdDeviation='${std}'/%3E%3CfeColorMatrix values='1 0 0 0 0 0 1 0 0 0 0 0 1 0 0 0 0 0 100 -1' result='s'/%3E%3CfeFlood x='0' y='0' width='100%25' height='100%25'/%3E%3CfeComposite operator='out' in='s'/%3E%3CfeComposite in2='SourceGraphic'/%3E%3CfeGaussianBlur stdDeviation='${std}'/%3E%3C/filter%3E%3Cimage width='100%25' height='100%25' x='0' y='0' preserveAspectRatio='${aspect}' style='filter: url(%23b);' href='${blurDataURL}'/%3E%3C/svg%3E`;
}

const toNumber = (value) => {
  const number = typeof value === "string" ? Number.parseFloat(value) : value;
  return Number.isFinite(number) ? number : undefined;
};

/** Resolves `<Image>` props into plain `<img>` attributes plus what the component needs for loading state. */
function resolve(props) {
  const {
    src: source,
    alt,
    width,
    height,
    fill = false,
    sizes,
    quality,
    priority = false,
    preload = false,
    loading,
    placeholder = "empty",
    blurDataURL,
    unoptimized = false,
    overrideSrc,
    loader,
    style,
    onLoad,
    onError,
    onLoadingComplete,
    ...rest
  } = props;
  const meta = source && typeof source === "object" ? (source.default ?? source) : null;
  const src = meta ? meta.src : source;
  let w = fill ? undefined : toNumber(width);
  let ht = fill ? undefined : toNumber(height);
  if (meta && !fill) {
    if (w === undefined && ht === undefined) [w, ht] = [meta.width, meta.height];
    else if (w === undefined) w = Math.round((ht * meta.width) / meta.height);
    else if (ht === undefined) ht = Math.round((w * meta.height) / meta.width);
  }
  const blur = blurDataURL ?? meta?.blurDataURL;
  const resolvedSizes = sizes ?? (fill ? "100vw" : undefined);
  const plain = typeof src !== "string" || src.startsWith("data:") || src.startsWith("blob:");
  const optimize = !unoptimized && !plain && (loader ? true : optimizable(src));
  const load = loader ?? defaultLoader;

  let srcSet;
  let url = withBasePath(src);
  if (optimize) {
    const { widths, kind } = candidates(w, resolvedSizes);
    srcSet = widths.map((size, i) => `${load({ src, width: size, quality })} ${kind === "w" ? `${size}w` : `${i + 1}x`}`).join(", ");
    url = load({ src, width: widths[widths.length - 1], quality });
  }

  const eager = priority || preload;
  const objectFit = style?.objectFit;
  let backgroundImage;
  if (typeof placeholder === "string" && placeholder.startsWith("data:image/")) backgroundImage = `url("${placeholder}")`;
  else if (placeholder === "blur" && blur) {
    const svg = blurSvg({ width: w, height: ht, blurWidth: meta?.blurWidth, blurHeight: meta?.blurHeight, blurDataURL: blur, objectFit });
    backgroundImage = `url("data:image/svg+xml;charset=utf-8,${svg}")`;
  }
  const placeholderStyle = backgroundImage
    ? {
        backgroundImage,
        backgroundSize: objectFit === "contain" ? "contain" : objectFit && objectFit !== "cover" ? "100% 100%" : "cover",
        backgroundPosition: style?.objectPosition ?? "50% 50%",
        backgroundRepeat: "no-repeat",
      }
    : null;
  const layout = fill ? { position: "absolute", height: "100%", width: "100%", left: 0, top: 0, right: 0, bottom: 0 } : null;

  const img = {
    ...rest,
    alt: alt ?? "",
    loading: eager ? undefined : (loading ?? "lazy"),
    fetchPriority: eager ? "high" : rest.fetchPriority,
    width: w,
    height: ht,
    decoding: "async",
    "data-nimg": fill ? "fill" : "1",
    style: { color: "transparent", ...layout, ...style },
    sizes: resolvedSizes,
    srcSet,
    src: overrideSrc ?? url,
  };
  return { img, placeholderStyle, eager, preloadHref: url, handlers: { onLoad, onError, onLoadingComplete } };
}

/** `<img>` props for an image, e.g. to build a `<picture>` with art direction. */
export function getImageProps(props) {
  const { img, placeholderStyle } = resolve(props);
  return { props: placeholderStyle ? { ...img, style: { ...placeholderStyle, ...img.style } } : img };
}

// Images whose load was already reported, keyed by element, so a load seen by the ref and by `onLoad` counts once.
const settled = new WeakMap();

/** Optimized image: lazy by default, responsive `srcset`, explicit dimensions to avoid layout shift. */
const Image = forwardRef(function Image(props, ref) {
  const { img, placeholderStyle, eager, preloadHref, handlers } = resolve(props);
  const [complete, setComplete] = useState(false);
  const [failed, setFailed] = useState(false);
  const latest = useRef(handlers);
  latest.current = handlers;

  if (eager) {
    preloadResource(preloadHref, { as: "image", imageSrcSet: img.srcSet, imageSizes: img.sizes, fetchPriority: "high" });
  }

  const finish = useCallback((element, event) => {
    const key = element.currentSrc || element.src;
    if (settled.get(element) === key) return;
    settled.set(element, key);
    setComplete(true);
    latest.current.onLoad?.(event);
    latest.current.onLoadingComplete?.(element);
  }, []);

  // An image can finish loading before hydration attaches `onLoad`; the ref catches that case.
  const attach = useCallback(
    (element) => {
      if (typeof ref === "function") ref(element);
      else if (ref) ref.current = element;
      if (element?.complete && element.naturalWidth > 0) {
        finish(element, { type: "load", target: element, currentTarget: element, nativeEvent: new Event("load") });
      }
    },
    [ref, finish],
  );

  const style = placeholderStyle && !complete && !failed ? { ...placeholderStyle, ...img.style } : img.style;
  return h("img", {
    ...img,
    ref: attach,
    style: failed ? { ...style, color: undefined } : style,
    onLoad: (event) => finish(event.currentTarget, event),
    onError: (event) => {
      setFailed(true);
      latest.current.onError?.(event);
    },
  });
});

export default Image;
export { Image };
