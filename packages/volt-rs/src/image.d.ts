import type { ForwardRefExoticComponent, ImgHTMLAttributes, RefAttributes } from "react";

export interface StaticImageData {
  src: string;
  width: number;
  height: number;
  blurDataURL?: string;
}

export type ImageLoader = (props: { src: string; width: number; quality?: number }) => string;

export interface ImageProps extends Omit<ImgHTMLAttributes<HTMLImageElement>, "src" | "width" | "height" | "loading"> {
  src: string | StaticImageData;
  alt: string;
  width?: number | `${number}`;
  height?: number | `${number}`;
  fill?: boolean;
  sizes?: string;
  quality?: number;
  priority?: boolean;
  loading?: "lazy" | "eager";
  placeholder?: "blur" | "empty";
  blurDataURL?: string;
  unoptimized?: boolean;
  loader?: ImageLoader;
}

export const DEVICE_SIZES: number[];
export const IMAGE_SIZES: number[];
export const defaultLoader: ImageLoader;
declare const Image: ForwardRefExoticComponent<ImageProps & RefAttributes<HTMLImageElement>>;
export default Image;
export { Image };
