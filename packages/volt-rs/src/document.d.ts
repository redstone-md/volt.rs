import type { Component, Context, HTMLAttributes, HtmlHTMLAttributes, ReactNode } from "react";

export interface DocumentContext {
  pathname: string;
  query: Record<string, string | string[]>;
  asPath: string;
  /** `[i18n]` locale fields; undefined without `[i18n]`. */
  locale?: string;
  locales?: string[];
  defaultLocale?: string;
  renderPage(): { html: string; head: ReactNode[] };
  defaultGetInitialProps(context: DocumentContext): Promise<DocumentInitialProps>;
}

export interface DocumentInitialProps {
  html: string;
  head?: ReactNode[];
  styles?: ReactNode;
}

export type DocumentProps = DocumentInitialProps & Record<string, unknown>;

export function Html(props: HtmlHTMLAttributes<HTMLHtmlElement> & { children?: ReactNode }): JSX.Element;
export function Head(props: HTMLAttributes<HTMLHeadElement> & { children?: ReactNode }): JSX.Element;
export function Main(): JSX.Element;
export function NextScript(props?: { nonce?: string }): null;

export default class Document<P = {}> extends Component<DocumentProps & P> {
  static getInitialProps(context: DocumentContext): Promise<DocumentInitialProps>;
  render(): JSX.Element;
}
export { Document };
