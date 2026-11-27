# CLI

Install with the npm package (`volt-rs` ships a `volt` bin backed by a prebuilt binary) or with Cargo:

```bash
cargo install voltrs-cli
```

| Command | Description |
| --- | --- |
| `volt new <dir> [--template default\|web] [--no-install]` | Scaffold a project. |
| `volt dev [--port 3000] [--host 0.0.0.0]` | Dev server: rebuilds on save, React Fast Refresh, recompiles Rust, error overlay. |
| `volt build` | Production build: release server binary, minified bundles, CSS, prerendered pages, Brotli/gzip. |
| `volt export` | Production build written as a static site to `out/` (`[build] export_dir`); fails with a list of routes that need the server. See [static generation](static-generation.md#static-export). |
| `volt start [--port 3000]` | Run the production build. |
| `volt sdk` | Regenerate `@volt/api` from the Rust server. |
| `volt routes` | Print the route table with route kinds, the router (`app` or `pages`) and API endpoints. |

`volt dev` watches the project. TypeScript, CSS and assets trigger a frontend rebuild; Rust changes trigger `cargo build`, SDK regeneration and a server restart, and the page reloads.

### Fast Refresh

Edits to components apply in place with [React Fast Refresh](https://reactnative.dev/docs/fast-refresh): the browser runs the new code and React re-renders the components with their state (`useState`, inputs, scroll) kept. A component whose hooks changed is remounted, and error boundaries that failed are retried.

- Development client builds run the React Refresh transform on the app's own modules (not `node_modules`) and emit one file per module under a stable name (`/_volt/static/js/app/page.js`, served with `Cache-Control: no-cache`). After a rebuild, `volt dev` compares the output with the previous build and sends the page an `update` event with the changed modules; the page imports each one it has loaded again (`page.js?t=<revision>`), so its imports keep resolving to the instances already loaded, then calls `performReactRefresh()`.
- The page reloads instead when the change cannot be applied in place: a changed module exports something other than components whose value changed (a constant, a hook, a helper; unchanged exports such as `metadata` are fine), a module has no exports, the new code imports something the loaded modules do not export, or something other than an app module changed (new or removed routes and segment files, CSS Modules class names, the generated route tables).
- The server bundle is reloaded on every rebuild, so server rendering and client navigations use the new code. Metadata and `generateMetadata` edits show on the next navigation or reload.
- CSS changes swap the stylesheet without a reload, also together with a component update.
- Build errors show an overlay over the page, which keeps running with its state; it closes on the next successful update. Errors thrown while a new module runs and uncaught runtime errors show in the same overlay. Loading a page while the build is broken shows the error page, which reloads once the build succeeds.
- Production builds contain no refresh code: `volt build` emits hashed, per-route chunks without the transform or the runtime.

The runtime is `react-refresh`, a dependency of `volt-rs`.

Set `VOLT_BINARY` to make the npm shim use a specific `volt` executable.
