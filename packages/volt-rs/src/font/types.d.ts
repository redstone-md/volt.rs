/** The object a font loader call is compiled into. */
export interface Font {
  /** Applies the font family (and weight/style when a single one was requested). */
  className: string;
  style: { fontFamily: string; fontWeight?: number; fontStyle?: string };
}

/** Returned when `variable` is set: `variable` is a class that defines the CSS custom property. */
export interface FontWithVariable extends Font {
  variable: string;
}

export type FontDisplay = "auto" | "block" | "swap" | "fallback" | "optional";

export interface CommonFontOptions {
  /** `font-display` of the generated `@font-face` rules. Default `"swap"`. */
  display?: FontDisplay;
  /** CSS custom property defined by the `variable` class, e.g. `"--font-sans"`. */
  variable?: `--${string}`;
  /** Font families appended after the loaded font in `font-family`. */
  fallback?: readonly string[];
  /** Preload the font files from pages that use the root layout. Default `true`. */
  preload?: boolean;
  /** Accepted for Next.js compatibility; volt does not generate metric-adjusted fallbacks. */
  adjustFontFallback?: boolean | string;
}

export type FontResult<T> = T extends { variable: string } ? FontWithVariable : Font;
