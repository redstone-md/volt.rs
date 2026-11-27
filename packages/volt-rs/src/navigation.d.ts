export interface NavigateOptions {
  scroll?: boolean;
}

export interface Router {
  push(href: string, options?: NavigateOptions): Promise<void>;
  replace(href: string, options?: NavigateOptions): Promise<void>;
  refresh(): Promise<void>;
  back(): void;
  forward(): void;
  prefetch(href: string): void;
}

export function useRouter(): Router;
export function usePathname(): string;
export function useSearchParams(): URLSearchParams;
/** Dynamic params of the route; inside a parallel route slot, the slot page's params are added. */
export function useParams<T extends Record<string, string | string[]> = Record<string, string | string[]>>(): T;
/** Data of the route's Rust loader; inside a parallel route slot, the slot page's loader. */
export function useData<T = unknown>(): T;
export function useNavigationPending(): boolean;
export function getRouter(): Router | null;
export function notFound(): never;
export function redirect(location: string, type?: "replace" | "push"): never;
export function permanentRedirect(location: string, type?: "replace" | "push"): never;
export function useSelectedLayoutSegment(parallelRoutesKey?: string): string | null;
export function useSelectedLayoutSegments(parallelRoutesKey?: string): string[];
export function useServerInsertedHTML(callback: () => import("react").ReactNode): void;
export function unstable_noStore(): void;
export function connection(): Promise<void>;
