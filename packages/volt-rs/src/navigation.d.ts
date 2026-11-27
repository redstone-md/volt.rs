export interface NavigateOptions {
  scroll?: boolean;
  /** `[i18n]` locale to navigate to; `false` uses the href as given. Defaults to the current locale. */
  locale?: string | false;
}

export interface I18n {
  locale: string;
  /** `[i18n] locales`; empty when internationalized routing is off. */
  locales: readonly string[];
  defaultLocale: string;
}

export interface Router {
  push(href: string, options?: NavigateOptions): Promise<void>;
  replace(href: string, options?: NavigateOptions): Promise<void>;
  refresh(): Promise<void>;
  back(): void;
  forward(): void;
  prefetch(href: string, options?: { locale?: string | false }): void;
}

export function useRouter(): Router;
export function usePathname(): string;
export function useSearchParams(): URLSearchParams;
/** Dynamic params of the route; inside a parallel route slot, the slot page's params are added. */
export function useParams<T extends Record<string, string | string[]> = Record<string, string | string[]>>(): T;
/** Data of the route's Rust loader; inside a parallel route slot, the slot page's loader. */
export function useData<T = unknown>(): T;
export function useNavigationPending(): boolean;
/** Active locale: the `[i18n]` locale of the URL, else the default locale, else `[site] language`. */
export function useLocale(): string;
export function useI18n(): I18n;
export function getRouter(): Router | null;
export function notFound(): never;
export function redirect(location: string, type?: "replace" | "push"): never;
export function permanentRedirect(location: string, type?: "replace" | "push"): never;
export function useSelectedLayoutSegment(parallelRoutesKey?: string): string | null;
export function useSelectedLayoutSegments(parallelRoutesKey?: string): string[];
export function useServerInsertedHTML(callback: () => import("react").ReactNode): void;
export function unstable_noStore(): void;
export function connection(): Promise<void>;
