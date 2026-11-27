// `localFont(...)` calls are replaced at build time by the volt bundler; this only runs if one was not.
export default function localFont() {
  throw new Error("localFont() must be called at the top level of a module with literal options");
}
