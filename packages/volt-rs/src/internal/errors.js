const NOT_FOUND = "VOLT_NOT_FOUND";
const REDIRECT = "VOLT_REDIRECT";

export function notFound() {
  const error = new Error(NOT_FOUND);
  error.digest = NOT_FOUND;
  throw error;
}

export function redirect(location, type = "replace") {
  const error = new Error(REDIRECT);
  error.digest = `${REDIRECT};${type};${location};307`;
  throw error;
}

export function permanentRedirect(location, type = "replace") {
  const error = new Error(REDIRECT);
  error.digest = `${REDIRECT};${type};${location};308`;
  throw error;
}

export const isNotFound = (error) => error?.digest === NOT_FOUND;

export function readRedirect(error) {
  if (typeof error?.digest !== "string" || !error.digest.startsWith(REDIRECT)) return null;
  const [, type, ...rest] = error.digest.split(";");
  const status = Number(rest.pop());
  return { location: rest.join(";"), type, status };
}

export class ActionError extends Error {
  constructor(error) {
    super(error?.message ?? "Action failed");
    this.name = "ActionError";
    this.status = error?.status ?? 500;
    this.fieldErrors = error?.fieldErrors;
  }
}
