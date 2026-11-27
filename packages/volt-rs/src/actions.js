import { useCallback, useState, useTransition } from "react";
import { endpoint, withBase } from "./internal/config.js";
import { ActionError } from "./internal/errors.js";
import { getRouter } from "./internal/router.js";

export { ActionError };

function encode(input) {
  if (typeof FormData !== "undefined" && input instanceof FormData) {
    const params = new URLSearchParams();
    for (const [key, value] of input) if (typeof value === "string") params.append(key, value);
    return { body: params, type: "application/x-www-form-urlencoded" };
  }
  return { body: JSON.stringify(input ?? null), type: "application/json" };
}

/** Creates a typed client for a Rust `#[action]`. Used by the generated `@volt/api`. */
export function createAction(name) {
  const action = async (input) => {
    const { body, type } = encode(input);
    const response = await fetch(endpoint(`/_volt/action/${name}`), {
      method: "POST",
      headers: { "content-type": type, accept: "application/json", "x-volt-action": "1" },
      body,
      credentials: "same-origin",
    });
    const payload = await response.json().catch(() => ({ error: { status: response.status, message: response.statusText } }));
    if (payload.error?.redirect) {
      getRouter()?.push(payload.error.redirect) ?? (location.href = withBase(payload.error.redirect));
      return undefined;
    }
    if (payload.error) throw new ActionError(payload.error);
    return payload.data;
  };
  Object.defineProperty(action, "$$volt", { value: name });
  return action;
}

/** Runs an action with pending, error and result state. */
export function useAction(action, { refresh = false } = {}) {
  const [pending, startTransition] = useTransition();
  const [state, setState] = useState({ data: undefined, error: undefined });
  const run = useCallback(
    (input) =>
      new Promise((resolve, reject) => {
        startTransition(async () => {
          try {
            const data = await action(input);
            setState({ data, error: undefined });
            if (refresh) getRouter()?.refresh();
            resolve(data);
          } catch (error) {
            setState({ data: undefined, error });
            reject(error);
          }
        });
      }),
    [action, refresh],
  );
  return { run, pending, ...state };
}
