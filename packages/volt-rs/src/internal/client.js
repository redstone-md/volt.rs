import { createElement as h } from "react";
import { hydrateRoot } from "react-dom/client";
import { App, createView, isPagesView } from "./router.js";

/** Hydrates the server-rendered document; called by the generated client entry with the routes and their loading shells. */
export async function boot(routes, shells = {}) {
  const script = document.getElementById("__VOLT_DATA__");
  const state = JSON.parse(script.textContent);
  const initial = await createView(routes, state);
  // Pages Router trees render inside `<Main />`; App Router trees own the whole document, whatever ids they use.
  const container = document.getElementById(isPagesView(initial) ? "__next" : "__volt") ?? document;
  hydrateRoot(container, h(App, { routes, shells, initial }), {
    onRecoverableError(error) {
      if (process.env.NODE_ENV !== "production") console.warn("[volt] hydration:", error);
    },
  });
}
