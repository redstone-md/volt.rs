# AGENTS.md

Instructions for coding agents working on the volt.rs repository. For apps built with volt, see the `AGENTS.md` generated into each project from `crates/voltrs-cli/templates/`.

## Repository

| Path | What |
| --- | --- |
| `crates/voltrs` | Runtime library linked into every app server: HTTP (hyper), router, QuickJS SSR pool, actions, loaders, API routes, SEO, image resizing, TypeScript type bridge. |
| `crates/voltrs-macros` | Proc macros: `action`, `loader`, `get`/`post`/`put`/`patch`/`delete`, `sitemap`, `model`, `derive(Type)`. |
| `crates/voltrs-cli` | `volt` binary: project discovery, route scanner, rolldown bundler plugin, CSS pipeline, dev watcher, build pipeline, scaffolding. Templates live in `crates/voltrs-cli/templates/`. |
| `packages/volt-rs` | npm runtime (plain ESM + `.d.ts`): router, navigation hooks, `Link`, `Form`, `Image`, `Script`, metadata, actions client, SSR and hydration entries, `volt` bin shim. |
| `packages/create-volt-rs` | `npm create volt-rs` scaffolder; copies the CLI templates at pack time. |
| `docs/` | User documentation. Keep it in sync with behavior. |

## Commands

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build -p voltrs-cli            # target/debug/volt
```

End-to-end check against a scaffolded app (uses local crates and packages):

```bash
target/debug/volt new /tmp/app --no-install
cd /tmp/app
sed -i 's|"volt-rs": "^0.0.2"|"volt-rs": "file:../path/to/volt.rs/packages/volt-rs"|' package.json
sed -i 's|^voltrs = "0.0.2"|voltrs = { path = "../path/to/volt.rs/crates/voltrs" }|' Cargo.toml
npm install && ../path/to/volt.rs/target/debug/volt build && ./target/release/app
```

## Conventions

- Rust 2024 edition, `rustfmt.toml` at the root (120 columns). Clippy must be clean.
- Object-oriented structure: one responsibility per type (`Server`, `Engine`, `Pipeline`, `Bundle`, `Css`, `Project`). Shared logic goes in one place: the manifest, config, router and SDK types in `voltrs` are reused by the CLI instead of being redefined.
- Comments explain why, not what. Public items get a one-line doc comment. No commented-out code.
- JavaScript in `packages/volt-rs` is plain ESM without a build step, written with `createElement` (no JSX). Every public module has a matching `.d.ts`.
- The JS runtime must run in both QuickJS (server) and browsers: no Node APIs, and browser globals only inside effects or handlers.
- Keep `docs/`, the template `AGENTS.md` files and `README.md` accurate when behavior changes.

## Commits and versions

- Conventional prefixes only: `feat:`, `fix:`, `chore:`. One short imperative line, lowercase, no trailing period.
- No AI attribution of any kind in commits, code, docs or PRs.
- Versions start at 0.0.1. `0.0.x` for minor changes, `0.x.0` for versions, `x.0.0` for transitional releases. The workspace version, `packages/*/package.json` and the template `{{version}}` must match.
