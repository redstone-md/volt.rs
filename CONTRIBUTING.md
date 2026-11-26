# Contributing

Thanks for helping. Bug reports with a minimal reproduction are the most valuable contribution.

## Setup

```bash
git clone https://github.com/redstone-md/volt.rs
cd volt.rs
cargo build -p voltrs-cli
cargo test --workspace
```

The repository layout, commands and code conventions are described in [AGENTS.md](./AGENTS.md); they apply to humans too.

## Pull requests

- Keep changes focused. One feature or fix per pull request.
- `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` must pass.
- Update `docs/` and the template `AGENTS.md` files when behavior changes.
- Commit messages: `feat: ...`, `fix: ...` or `chore: ...`, one short line.

## Releases

Maintainers tag `vX.Y.Z` on `main`. CI builds the `volt` binaries for Linux, macOS and Windows, publishes the npm packages (`volt-rs`, `@volt-rs/cli-*`, `create-volt-rs`) and the crates (`voltrs-macros`, `voltrs`, `voltrs-cli`).
