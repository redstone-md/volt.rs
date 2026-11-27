# CLI

Install with the npm package (`volt-rs` ships a `volt` bin backed by a prebuilt binary) or with Cargo:

```bash
cargo install voltrs-cli
```

| Command | Description |
| --- | --- |
| `volt new <dir> [--template default\|web] [--no-install]` | Scaffold a project. |
| `volt dev [--port 3000] [--host 0.0.0.0]` | Dev server: rebuilds on save, recompiles Rust, live reload, error overlay. |
| `volt build` | Production build: release server binary, minified bundles, CSS, prerendered pages, Brotli/gzip. |
| `volt start [--port 3000]` | Run the production build. |
| `volt sdk` | Regenerate `@volt/api` from the Rust server. |
| `volt routes` | Print the route table with route kinds, the router (`app` or `pages`) and API endpoints. |

`volt dev` watches the project. TypeScript, CSS and assets trigger a frontend rebuild and a browser reload (CSS-only changes swap stylesheets without reloading). Rust changes trigger `cargo build`, SDK regeneration and a server restart. Build errors are shown in the browser and cleared on the next successful build.

Set `VOLT_BINARY` to make the npm shim use a specific `volt` executable.
