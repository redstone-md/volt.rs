import { createElement as h, forwardRef } from "react";
import { preload } from "react-dom";

export const DEVICE_SIZES = [640, 750, 828, 1080, 1200, 1920, 2048, 3840];
export const IMAGE_SIZES = [16, 32, 48, 64, 96, 128, 256, 384];
const ALL_SIZES = [...IMAGE_SIZES, ...DEVICE_SIZES];

export function defaultLoader({ src, width, quality }) {
  return `/_volt/image?url=${encodeURIComponent(src)}&w=${width}&q=${quality ?? 75}`;
}

const optimizable = (src) => src.startsWith("/") && !src.startsWith("//") && !/\.svg($|\?)/i.test(src);

function candidates(width, sizes) {
  if (sizes) return { widths: ALL_SIZES, kind: "w" };
  if (!width) return { widths: DEVICE_SIZES, kind: "w" };
  const pick = (target) => ALL_SIZES.find((s) => s >= target) ?? ALL_SIZES[ALL_SIZES.length - 1];
  return { widths: [...new Set([pick(width), pick(width * 2)])], kind: "x" };
}

/** Optimized image: lazy by default, responsive `srcset`, explicit dimensions to avoid layout shift. */
const Image = forwardRef(function Image(
  {
    src: source,
    alt,
    width,
    height,
    fill = false,
    sizes,
    quality,
    priority = false,
    loading,
    placeholder = "empty",
    blurDataURL,
    unoptimized = false,
    loader = defaultLoader,
    style,
    ...rest
  },
  ref,
) {
  const meta = typeof source === "object" ? source : null;
  const src = meta ? meta.src : source;
  const w = fill ? undefined : Number(width ?? meta?.width) || undefined;
  const ht = fill ? undefined : Number(height ?? meta?.height) || undefined;
  const blur = blurDataURL ?? meta?.blurDataURL;
  const optimize = !unoptimized && typeof src === "string" && (loader !== defaultLoader || optimizable(src));

  let srcSet;
  let url = src;
  if (optimize) {
    const { widths, kind } = candidates(w, sizes ?? (fill ? "100vw" : undefined));
    srcSet = widths.map((size, i) => `${loader({ src, width: size, quality })} ${kind === "w" ? `${size}w` : `${i + 1}x`}`).join(", ");
    url = loader({ src, width: widths[widths.length - 1], quality });
  }
  const resolvedSizes = sizes ?? (fill ? "100vw" : undefined);

  if (priority) preload(url, { as: "image", imageSrcSet: srcSet, imageSizes: resolvedSizes, fetchPriority: "high" });

  const layout = fill ? { position: "absolute", inset: 0, width: "100%", height: "100%", objectFit: "cover" } : null;
  const background =
    placeholder === "blur" && blur
      ? { backgroundImage: `url("${blur}")`, backgroundSize: "cover", backgroundPosition: "center", backgroundRepeat: "no-repeat" }
      : null;

  return h("img", {
    ...rest,
    ref,
    alt: alt ?? "",
    src: url,
    srcSet,
    sizes: resolvedSizes,
    width: w,
    height: ht,
    loading: priority ? undefined : (loading ?? "lazy"),
    fetchPriority: priority ? "high" : undefined,
    decoding: "async",
    style: { color: "transparent", ...layout, ...background, ...style },
  });
});

export default Image;
export { Image };
