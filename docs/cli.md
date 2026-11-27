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
| `volt start [--port 3000] [--host 0.0.0.0]` | Run the production build. |
| `volt sdk` | Regenerate `@volt/api` from the Rust server. |
| `volt routes` | Print the route table: pages with their kind and router (`app` or `pages`), `route.ts` handlers, Rust API endpoints and `middleware.ts` with its matcher. |

`volt dev` watches the project. TypeScript, CSS and assets trigger a frontend rebuild; Rust changes trigger `cargo build`, SDK regeneration and a server restart, and the page reloads.

### Fast Refresh

Edits to components apply in place with [React Fast Refresh](https://reactnative.dev/docs/fast-refresh): the browser runs the new code and React re-renders the components with their state (`useState`, inputs, scroll) kept. A component whose hooks changed is remounted, and error boundaries that failed are retried.

- Development client builds run the React Refresh transform on the app's own modules (not `node_modules`) and emit one file per module under a stable name (`/_volt/static/js/app/page.js`, served with `Cache-Control: no-cache` and an `ETag`, so reloads revalidate instead of downloading every module again). After a rebuild, `volt dev` compares the output with the previous build and sends the page an `update` event with the changed modules, the revision each last changed in and the importers of every app module. The page evaluates again only the modules it has loaded whose code is newer than its copy, so module side effects and module state elsewhere are kept, then calls `performReactRefresh()`.
- As in Next.js, a module that exports components (and otherwise only plain data, such as `metadata`) is a refresh boundary: its new instance replaces the old one alone. A changed module that is not a boundary (a constant, a helper, a hook, a context) is evaluated again together with the modules that import it, up to the nearest boundaries, and the new instances import each other: editing a constant or a private value that a helper closes over updates the components that use it, with their state kept.
- The page reloads instead when the change cannot be applied in place: the update reaches a module outside the app (a page or layout that is not a boundary, the generated route tables), a boundary's data exports changed (route config such as `metadata`), the new code imports something the loaded modules do not export, modules evaluated together import each other in a cycle, or something other than an app module changed (new or removed routes and segment files, CSS Modules class names, an installed package after `npm install`). `generateStaticParams` and the Pages Router data functions only run on the server and never cause a reload.
- The server bundle is reloaded on every rebuild, so server rendering and client navigations use the new code. Metadata and `generateMetadata` edits show on the next navigation or reload.
- CSS changes swap the stylesheet without a reload, also together with a component update.
- Build errors show an overlay over the page, which keeps running with its state; it closes on the next successful update. Errors thrown while a new module runs, render errors (also the ones an `error.tsx` boundary catches) and uncaught runtime errors show in the same overlay. A render error with no `error.tsx` above it leaves the page empty under the overlay instead of unmounting the app, and the fix renders it again. Loading a page while the build is broken shows the error page, which reloads once the build succeeds.
- Production builds contain no refresh code: `volt build` emits hashed, per-route chunks without the transform or the runtime.

The runtime is `react-refresh`, a dependency of `volt-rs`.

Set `VOLT_BINARY` to make the npm shim use a specific `volt` executable.
