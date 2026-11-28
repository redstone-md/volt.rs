# voltrs-macros

Procedural macros for [volt.rs](https://github.com/redstone-md/volt.rs): `#[action]`, `#[loader]`, `#[get]`/`#[post]`/`#[put]`/`#[patch]`/`#[delete]`, `#[sitemap]`, `#[model]` and `#[derive(Type)]`.

Don't depend on this crate directly. Use the re-exports from [`voltrs`](https://crates.io/crates/voltrs):

```rust
use voltrs::prelude::*;
```

See the [documentation](https://github.com/redstone-md/volt.rs/tree/main/docs) for how each macro is used.
