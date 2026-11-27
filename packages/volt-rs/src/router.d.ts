import type { ComponentType } from "react";

export type ParsedUrlQuery = Record<string, string | string[] | undefined>;

export interface UrlObject {
  pathname?: string;
  query?: Record<string, string | number | boolean | (string | number | boolean)[] | null | undefined>;
  hash?: string;
}

export type Url = string | URL | UrlObject;

export interface TransitionOptions {
  shallow?: boolean;
  scroll?: boolean;
  locale?: string | false;
}

export type RouterEvent = "routeChangeStart" | "beforeHistoryChange" | "routeChangeComplete" | "routeChangeError";

export interface RouterEvents {
  on(type: RouterEvent, handler: (...args: any[]) => void): void;
  off(type: RouterEvent, handler: (...args: any[]) => void): void;
  emit(type: RouterEvent, ...args: any[]): void;
}

export interface NextRouter {
  /** The route pattern, such as `/blog/[slug]`. */
  pathname: string;
  route: string;
  /** Dynamic params merged with the search params. */
  query: ParsedUrlQuery;
  /** The path shown in the browser, including the search string. */
  asPath: string;
  basePath: string;
  isReady: boolean;
  isFallback: boolean;
  isPreview: boolean;
  isLocaleDomain: boolean;
  /** Active `[i18n]` locale; undefined without `[i18n]`. */
  locale?: string;
  locales?: string[];
  defaultLocale?: string;
  events: RouterEvents;
  push(url: Url, as?: Url, options?: TransitionOptions): Promise<boolean>;
  replace(url: Url, as?: Url, options?: TransitionOptions): Promise<boolean>;
  reload(): void;
  back(): void;
  forward(): void;
  prefetch(url: Url, as?: Url): Promise<void>;
  beforePopState(callback: (state: { url: string; as: string; options: TransitionOptions }) => boolean): void;
}

export interface SingletonRouter extends Omit<NextRouter, "isPreview" | "isLocaleDomain"> {
  router: NextRouter | null;
  ready(callback: () => void): void;
}

export interface WithRouterProps {
  router: NextRouter;
}

export function useRouter(): NextRouter;
export function withRouter<P extends WithRouterProps>(Component: ComponentType<P>): ComponentType<Omit<P, "router">>;

declare const Router: SingletonRouter;
export default Router;
export { Router };
