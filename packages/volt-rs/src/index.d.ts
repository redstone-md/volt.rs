import type { ReactNode } from "react";

export { default as Link } from "./link";
export { default as Image } from "./image";
export { default as Script } from "./script";
export { default as Head } from "./head";
export { default as Form, useFormPending } from "./form";
export { ActionError, createAction, useAction } from "./actions";
export type { Action } from "./actions";
export { JsonLd } from "./seo";
export {
  getRouter,
  notFound,
  permanentRedirect,
  redirect,
  useData,
  useNavigationPending,
  useParams,
  usePathname,
  useRouter,
  useSearchParams,
} from "./navigation";
export type { Router } from "./navigation";

export type Params = Record<string, string | string[]>;
export type SearchParams = Record<string, string | string[] | undefined>;

/** Plain values that can also be awaited, for Next.js 15 compatibility. */
export type Awaitable<T> = T & Promise<T>;

export interface PageProps<Data = undefined, P extends Params = Params> {
  data: Data;
  params: Awaitable<P>;
  searchParams: Awaitable<SearchParams>;
}

export interface LayoutProps<P extends Params = Params> {
  children: ReactNode;
  params: Awaitable<P>;
}

/**
 * `export function generateStaticParams()` of a dynamic page or layout, evaluated by `volt build`.
 * Child generators receive the params produced by parent layouts.
 */
export type GenerateStaticParams<P extends Params = Params, Parent extends Params = Params> = (props: {
  params: Parent;
}) => P[] | Promise<P[]>;

/** Result of a Pages Router `getStaticPaths`. `fallback: false` answers 404 for other params. */
export interface GetStaticPathsResult<P extends Params = Params> {
  paths: (string | { params: P })[];
  fallback: boolean | "blocking";
}

export type GetStaticPaths<P extends Params = Params> = () => GetStaticPathsResult<P> | Promise<GetStaticPathsResult<P>>;

export interface ErrorProps {
  error: Error & { status?: number; digest?: string };
  reset: () => void;
}

type Url = string | URL;

export interface ImageDescriptor {
  url: Url;
  width?: number;
  height?: number;
  alt?: string;
  type?: string;
}

export interface Metadata {
  metadataBase?: Url;
  title?: string | { default?: string; template?: string; absolute?: string };
  description?: string;
  applicationName?: string;
  generator?: string;
  keywords?: string | string[];
  authors?: { name: string; url?: string } | { name: string; url?: string }[];
  creator?: string;
  publisher?: string;
  referrer?: string;
  category?: string;
  robots?:
    | string
    | {
        index?: boolean;
        follow?: boolean;
        noarchive?: boolean;
        nosnippet?: boolean;
        noimageindex?: boolean;
        nocache?: boolean;
        maxSnippet?: number;
        maxImagePreview?: "none" | "standard" | "large";
        maxVideoPreview?: number;
        googleBot?: Metadata["robots"];
      };
  alternates?: { canonical?: Url; languages?: Record<string, Url> };
  manifest?: string;
  icons?: string | { icon?: string | string[] | { url: string; type?: string; sizes?: string }[]; apple?: string | string[]; shortcut?: string | string[] };
  openGraph?: {
    title?: string;
    description?: string;
    url?: Url;
    siteName?: string;
    locale?: string;
    type?: "website" | "article" | "profile" | "book" | (string & {});
    images?: Url | ImageDescriptor | (Url | ImageDescriptor)[];
    publishedTime?: string;
    modifiedTime?: string;
    authors?: string | string[];
  };
  twitter?: {
    card?: "summary" | "summary_large_image" | "app" | "player";
    site?: string;
    creator?: string;
    title?: string;
    description?: string;
    images?: Url | ImageDescriptor | (Url | ImageDescriptor)[];
  };
  verification?: { google?: string | string[]; yandex?: string | string[]; yahoo?: string | string[]; bing?: string | string[]; other?: Record<string, string | string[]> };
  other?: Record<string, string | string[]>;
}

export interface Viewport {
  width?: string | number;
  height?: string | number;
  initialScale?: number;
  minimumScale?: number;
  maximumScale?: number;
  userScalable?: boolean;
  viewportFit?: "auto" | "cover" | "contain";
  themeColor?: string | { color: string; media?: string } | { color: string; media?: string }[];
  colorScheme?: "normal" | "light" | "dark" | "light dark" | "dark light";
}

export type GenerateMetadata<Data = undefined> = (props: PageProps<Data>) => Metadata | Promise<Metadata>;

/** Pages Router: a page component with optional `getLayout`-style statics. */
export type NextPage<P = {}, IP = P> = import("react").ComponentType<P> & { getInitialProps?: (context: unknown) => IP | Promise<IP> };

export type Redirect = { destination: string; permanent: boolean } | { destination: string; statusCode: 301 | 302 | 303 | 307 | 308 };

export interface GetServerSidePropsContext<P extends Params = Params> {
  params: P;
  query: Record<string, string | string[]>;
  resolvedUrl: string;
}

export type GetServerSidePropsResult<P> = { props: P | Promise<P> } | { notFound: true } | { redirect: Redirect };

export type GetServerSideProps<P extends Record<string, any> = Record<string, any>, Q extends Params = Params> = (
  context: GetServerSidePropsContext<Q>,
) => Promise<GetServerSidePropsResult<P>> | GetServerSidePropsResult<P>;

export interface GetStaticPropsContext<P extends Params = Params> {
  params: P;
}

export type GetStaticPropsResult<P> = { props: P; revalidate?: number | false } | { notFound: true; revalidate?: number | false } | { redirect: Redirect; revalidate?: number | false };

export type GetStaticProps<P extends Record<string, any> = Record<string, any>, Q extends Params = Params> = (
  context: GetStaticPropsContext<Q>,
) => Promise<GetStaticPropsResult<P>> | GetStaticPropsResult<P>;

export interface GetStaticPathsResult<P extends Params = Params> {
  paths: (string | { params: P })[];
  fallback: boolean | "blocking";
}

export type GetStaticPaths<P extends Params = Params> = () => Promise<GetStaticPathsResult<P>> | GetStaticPathsResult<P>;

type Unwrap<T> = T extends Promise<infer U> ? U : T;
export type InferGetServerSidePropsType<T extends (...args: any) => any> = Unwrap<ReturnType<T>> extends { props: infer P } ? Unwrap<P> : never;
export type InferGetStaticPropsType<T extends (...args: any) => any> = Unwrap<ReturnType<T>> extends { props: infer P } ? P : never;

export type { AppProps } from "./app";
