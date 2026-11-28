# create-volt-rs

Creates a [volt.rs](https://github.com/redstone-md/volt.rs) app: React pages server-rendered by a Rust server, with Tailwind CSS and shadcn/ui.

```bash
npm create volt-rs@latest my-app
cd my-app && npm run dev
```

Without a directory name, it asks for one. Options (npm needs `--` before them):

```bash
npm create volt-rs@latest my-app -- --template web --no-install
```

| Option | Effect |
| --- | --- |
| `--template default`, `-t default` | React + Rust server (`server/main.rs`). Needs Rust 1.88 or newer. |
| `--template web`, `-t web`, `--web` | React only, no Rust code. |
| `--no-install` | Skip installing dependencies. |

Without `--template`, the `default` template is used when `cargo` is installed and `web` otherwise. Dependencies are installed with the package manager that ran the command (npm, pnpm, yarn or bun). Requires Node.js 18 or newer.

See [Getting started](https://github.com/redstone-md/volt.rs/blob/main/docs/getting-started.md).

## License

MIT
