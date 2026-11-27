import type { ForwardRefExoticComponent, ImgHTMLAttributes, RefAttributes, SyntheticEvent } from "react";

export interface StaticImageData {
  src: string;
  width: number;
  height: number;
  /** Tiny PNG preview computed at build time for raster imports. */
  blurDataURL?: string;
  blurWidth?: number;
  blurHeight?: number;
}

export interface ImageLoaderProps {
  src: string;
  width: number;
  quality?: number;
}

export type ImageLoader = (props: ImageLoaderProps) => string;

export type PlaceholderValue = "blur" | "empty" | `data:image/${string}`;

export interface ImageProps
  extends Omit<ImgHTMLAttributes<HTMLImageElement>, "src" | "srcSet" | "width" | "height" | "loading" | "placeholder"> {
  src: string | StaticImageData;
  alt: string;
  width?: number | `${number}`;
  height?: number | `${number}`;
  /** Fills the parent (which needs `position: relative`, `fixed` or `absolute`) instead of fixed dimensions. */
  fill?: boolean;
  sizes?: string;
  /** 1 to 100, default 75. */
  quality?: number | `${number}`;
  /** Preloads the image and loads it eagerly with `fetchpriority="high"`. */
  priority?: boolean;
  /** Same as `priority`, the Next.js 16 name. */
  preload?: boolean;
  loading?: "lazy" | "eager";
  /** `blur` shows `blurDataURL` (automatic for static imports) while loading; a `data:image/...` URL is shown as is. */
  placeholder?: PlaceholderValue;
  blurDataURL?: string;
  unoptimized?: boolean;
  /** Sets the `src` attribute while keeping the generated `srcset`. */
  overrideSrc?: string;
  loader?: ImageLoader;
  onLoad?: (event: SyntheticEvent<HTMLImageElement, Event>) => void;
  onError?: (event: SyntheticEvent<HTMLImageElement, Event>) => void;
  /** @deprecated Use `onLoad`. */
  onLoadingComplete?: (image: HTMLImageElement) => void;
}

export type ImgProps = Omit<ImgHTMLAttributes<HTMLImageElement>, "width" | "height" | "loading"> & {
  src: string;
  srcSet?: string;
  width?: number;
  height?: number;
  loading?: "lazy" | "eager";
};

/** `[images] device_sizes`. */
export const DEVICE_SIZES: number[];
/** `[images] image_sizes`. */
export const IMAGE_SIZES: number[];
export const defaultLoader: ImageLoader;
/** Whether a remote URL is allowed by `[images] remote_patterns`. */
export function isAllowedRemote(src: string): boolean;
/** The `<img>` attributes `<Image>` would render, e.g. for the sources of a `<picture>`. */
export function getImageProps(props: ImageProps): { props: ImgProps };
declare const Image: ForwardRefExoticComponent<ImageProps & RefAttributes<HTMLImageElement>>;
export default Image;
export { Image };
