import { Component, createContext, createElement as h, useContext } from "react";

/** The page tree rendered by `<Main />` and the active locale, provided by the server renderer. */
export const DocumentContext = createContext({ main: null, locale: undefined });

/** `<html>` of a Pages Router document; `lang` defaults to the active locale. */
export function Html({ children, ...props }) {
  const { locale } = useContext(DocumentContext);
  return h("html", { ...props, lang: props.lang ?? locale }, children);
}

/** `<head>` of the document, with the default charset and viewport tags. Not the same as `volt-rs/head`. */
export function Head({ children, ...props }) {
  return h(
    "head",
    props,
    h("meta", { charSet: "utf-8" }),
    h("meta", { name: "viewport", content: "width=device-width, initial-scale=1" }),
    children,
  );
}

/** Container the page tree renders and hydrates into. */
export function Main() {
  return h("div", { id: "__next" }, useContext(DocumentContext).main);
}

/** Placeholder for the framework scripts: the server injects state and the client entry before `</body>`. */
export function NextScript() {
  return null;
}

/** Default document; extend it for a custom `pages/_document`. Rendered on the server only. */
export default class Document extends Component {
  static async getInitialProps() {
    return { html: "", head: [], styles: null };
  }

  render() {
    return h(Html, null, h(Head), h("body", null, h(Main), h(NextScript)));
  }
}

export { Document };
