import { createElement as h } from "react";

/** Renders schema.org structured data. */
export function JsonLd({ data }) {
  const json = JSON.stringify(data).replace(/</g, "\\u003c");
  return h("script", { type: "application/ld+json", dangerouslySetInnerHTML: { __html: json } });
}
