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
export function useParams<T extends Record<string, string | string[]> = Record<string, string | string[]>>(): T;
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
