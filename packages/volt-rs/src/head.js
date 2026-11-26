import { Fragment, createElement as h } from "react";

/** Compatibility wrapper: React 19 hoists `<title>`, `<meta>` and `<link>` into `<head>` itself. */
export default function Head({ children }) {
  return h(Fragment, null, children);
}
