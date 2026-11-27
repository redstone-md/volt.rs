import type { CommonFontOptions, FontResult } from "./types";

export type { Font, FontWithVariable, FontDisplay } from "./types";

export interface LocalFontSource {
  /** Path relative to the file that calls `localFont`. */
  path: string;
  weight?: string;
  style?: string;
}

/** Options of `localFont`. Calls are compiled at build time, so every value must be a literal. */
export interface LocalFontOptions extends CommonFontOptions {
  src: string | readonly LocalFontSource[];
  /** Applies to every file when `src` is a string; the class also sets it when it is a single value. */
  weight?: string;
  style?: string;
  /** Extra `@font-face` descriptors. */
  declarations?: readonly { prop: string; value: string }[];
}

/** Copies local font files into the build and generates `@font-face` rules for them. */
export default function localFont<const T extends LocalFontOptions>(options: T): FontResult<T>;
