import { useActionState as useReactActionState, useCallback, useContext, useMemo, useState, useTransition } from "react";
import { endpoint, localizeHref, withBase } from "./internal/config.js";
import { ActionError } from "./internal/errors.js";
import { RouterContext, getRouter, getRouterState, revalidatePaths } from "./internal/router.js";

export { ActionError };

const REVALIDATE_HEADER = "x-volt-revalidate";
const isBlob = (value) => typeof Blob !== "undefined" && value instanceof Blob;
// A file input left blank submits an empty, unnamed file; it carries nothing worth a multipart body.
const isUpload = (value) => isBlob(value) && (value.size > 0 || Boolean(value.name));

function toFormData(input) {
  const data = new FormData();
  for (const [key, value] of Object.entries(input)) {
    for (const item of Array.isArray(value) ? value : [value]) {
      if (item === undefined || item === null) continue;
      if (isBlob(item)) data.append(key, item);
      else data.append(key, typeof item === "object" ? JSON.stringify(item) : String(item));
    }
  }
  return data;
}

const hasUploads = (input) =>
  Object.values(input).some((value) => (Array.isArray(value) ? value.some(isUpload) : isUpload(value)));

function encode(input) {
  if (typeof FormData !== "undefined" && input instanceof FormData) {
    if ([...input.values()].some(isUpload)) return { body: input };
    const params = new URLSearchParams();
    for (const [key, value] of input) if (typeof value === "string") params.append(key, value);
    return { body: params, type: "application/x-www-form-urlencoded" };
  }
  if (input && typeof input === "object" && !Array.isArray(input) && hasUploads(input)) {
    return { body: toFormData(input) };
  }
  return { body: JSON.stringify(input ?? null), type: "application/json" };
}

function readRevalidate(response) {
  const header = response.headers.get(REVALIDATE_HEADER);
  if (!header) return [];
  return header.split(",").filter(Boolean).map((path) => {
    try {
      return decodeURIComponent(path);
    } catch {
      return path;
    }
  });
}

async function call(name, input) {
  const { body, type } = encode(input);
  const locale = getRouterState()?.locale;
  const headers = { accept: "application/json", "x-volt-action": "1", ...(locale ? { "x-volt-locale": locale } : {}) };
  if (type) headers["content-type"] = type;
  const response = await fetch(endpoint(`/_volt/action/${name}`), { method: "POST", headers, body, credentials: "same-origin" });
  const payload = await response.json().catch(() => ({ error: { status: response.status, message: response.statusText } }));
  const paths = readRevalidate(response);
  const redirect = payload.error?.redirect;
  const revalidated = paths.length > 0 ? await revalidatePaths(paths, { refresh: !redirect }) : false;
  if (redirect) {
    getRouter()?.push(redirect) ?? (location.href = withBase(localizeHref(redirect, locale)));
    return { data: undefined, revalidated: true };
  }
  if (payload.error) throw new ActionError(payload.error);
  return { data: payload.data, revalidated };
}

/** Hidden form fields in the shape React reads from `$$FORM_ACTION` (`FormData` is missing during SSR). */
class HiddenFields {
  constructor(entries) {
    this.entries = entries;
  }
  append(key, value) {
    this.entries.push([key, value]);
  }
  forEach(callback, self) {
    for (const [key, value] of this.entries) callback.call(self, value, key, this);
  }
}

/**
 * Progressive enhancement for `<form action={fn}>`: React renders this URL and these hidden fields so the
 * form posts without JavaScript. `$ACTION_ID_<name>` lets the server run the action when a `permalink`
 * sends the form to a page URL; `$volt_state` marks forms whose result goes back to `useActionState`.
 */
function decorate(fn, name, stateful = false) {
  return Object.defineProperties(fn, {
    $$volt: { value: name },
    $$FORM_ACTION: {
      value: () => {
        const fields = [[`$ACTION_ID_${name}`, ""]];
        if (stateful) fields.push(["$volt_state", "1"]);
        return { action: endpoint(`/_volt/action/${name}`), method: "POST", encType: "multipart/form-data", data: new HiddenFields(fields) };
      },
    },
    // React binds the previous state onto actions (useActionState); keep the markers on bound copies.
    bind: { value: (self, ...args) => decorate(Function.prototype.bind.call(fn, self, ...args), name, stateful) },
  });
}

/**
 * Creates a typed client for a Rust `#[action]`. Used by the generated `@volt/api`.
 * The last argument is the input, so React's `useActionState` (which passes `prevState, formData`) works.
 */
export function createAction(name) {
  const action = decorate(async (...args) => (await call(name, args[args.length - 1])).data, name);
  // Resolves to `{ data, revalidated }` so callers skip a refresh the server already triggered.
  return Object.defineProperty(action, "$$run", { value: (input) => call(name, input) });
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
            const result = action.$$run ? await action.$$run(input) : { data: await action(input), revalidated: false };
            setState({ data: result.data, error: undefined });
            if (refresh && !result.revalidated) await getRouter()?.refresh();
            resolve(result.data);
          } catch (error) {
            setState({ data: undefined, error });
            reject(error);
          }
        });
      }),
    [action, refresh],
  );
  return { run, pending, ...state, fieldErrors: state.error?.fieldErrors };
}

/**
 * React's `useActionState` for volt actions: `[state, formAction, isPending, error]`.
 * An `ActionError` (validation errors included) keeps the previous state and is returned as `error`
 * instead of reaching the error boundary.
 */
export function useActionState(action, initialState, permalink) {
  // A submission without JavaScript comes back as a redirect whose page carries the action result.
  const submitted = useContext(RouterContext)?.state.action;
  const [initial] = useState(() => {
    if (!action.$$volt || submitted?.action !== action.$$volt) return { value: initialState, error: undefined };
    if (submitted.error) return { value: initialState, error: new ActionError(submitted.error) };
    return { value: submitted.data, error: undefined };
  });
  const run = useMemo(() => {
    const wrapped = async (previous, payload) => {
      try {
        return { value: await action(previous.value, payload), error: undefined };
      } catch (error) {
        if (error instanceof ActionError) return { value: previous.value, error };
        throw error;
      }
    };
    return action.$$volt ? decorate(wrapped, action.$$volt, true) : wrapped;
  }, [action]);
  const [state, dispatch, pending] = useReactActionState(run, initial, permalink);
  return [state.value, dispatch, pending, state.error];
}
