import { createElement as h } from "react";
import { hydrateRoot } from "react-dom/client";
import { App, createView } from "./router.js";

/** Hydrates the server-rendered document. Called by the generated client entry. */
export async function boot(routes) {
  const script = document.getElementById("__VOLT_DATA__");
  const state = JSON.parse(script.textContent);
  const initial = await createView(routes, state);
  const container = document.getElementById("__next") ?? document.getElementById("__volt") ?? document;
  hydrateRoot(container, h(App, { routes, initial }), {
    onRecoverableError(error) {
      if (process.env.NODE_ENV !== "production") console.warn("[volt] hydration:", error);
    },
  });
}
