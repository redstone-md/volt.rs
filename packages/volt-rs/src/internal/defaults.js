import { createElement as h } from "react";

const center = {
  minHeight: "100vh",
  display: "flex",
  alignItems: "center",
  justifyContent: "center",
  gap: "16px",
  fontFamily: "system-ui, sans-serif",
};
const code = { fontSize: "24px", fontWeight: 600, paddingRight: "16px", borderRight: "1px solid currentColor" };

function Status({ status, message }) {
  return h("main", { style: center }, h("h1", { style: code }, status), h("p", null, message));
}

export function RootLayout({ children }) {
  return h("html", { lang: "en" }, h("head"), h("body", null, children));
}

export function NotFound() {
  return h(Status, { status: 404, message: "This page could not be found." });
}

export function ErrorPage({ error }) {
  return h(Status, { status: error?.status ?? 500, message: error?.message ?? "Something went wrong." });
}

export const notFoundMetadata = { title: "404: This page could not be found.", robots: { index: false } };
