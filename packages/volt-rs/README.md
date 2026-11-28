# volt-rs

The JavaScript runtime and `volt` command of [volt.rs](https://github.com/redstone-md/volt.rs), a React framework with a Rust server. Pages follow the Next.js App Router and Pages Router conventions; the server, loaders and server actions are Rust, and React is rendered on the server inside QuickJS.

## Get started

```bash
npm create volt-rs@latest my-app
cd my-app && npm run dev
```

See [Getting started](https://github.com/redstone-md/volt.rs/blob/main/docs/getting-started.md).

## What this package contains

- The `volt` bin (`volt dev`, `volt build`, `volt start`, `volt export`, `volt sdk`, `volt routes`, `volt new`). It runs a prebuilt binary from the optional dependency `@volt-rs/cli-<platform>` (`linux-x64`, `linux-arm64`, `darwin-x64`, `darwin-arm64`, `win32-x64`), falls back to `volt` in `~/.cargo/bin` (`cargo install voltrs-cli`), and uses `VOLT_BINARY` when it is set.
- The browser and server runtime: router, hydration, client navigation and the modules below. `next/*` imports resolve to them.

| Module | Next.js equivalent |
| --- | --- |
| `volt-rs/link` | `next/link` |
| `volt-rs/navigation` | `next/navigation` (`useRouter`, `usePathname`, `useSearchParams`, `useParams`, `useLocale`, `notFound`, `redirect`, ...) |
| `volt-rs/router` | `next/router` |
| `volt-rs/document`, `volt-rs/app` | `next/document`, `next/app` |
| `volt-rs/image` | `next/image` |
| `volt-rs/script`, `volt-rs/head` | `next/script`, `next/head` |
| `volt-rs/form` | `next/form`, with server actions |
| `volt-rs/actions` | `createAction`, `useAction`, `useActionState`, `ActionError` |
| `volt-rs/dynamic` | `next/dynamic` |
| `volt-rs/cache`, `volt-rs/server` | `next/cache`, `next/server` |
| `volt-rs/font/google`, `volt-rs/font/local` | `next/font/google`, `next/font/local` |
| `volt-rs/seo` | `JsonLd` |
| `volt-rs/env` | types for asset imports and `next/*` modules |

Requires React 19 (`react` and `react-dom` are peer dependencies).

## Documentation

- [Routing](https://github.com/redstone-md/volt.rs/blob/main/docs/routing.md)
- [Data: loaders, actions, API routes, route handlers](https://github.com/redstone-md/volt.rs/blob/main/docs/data.md)
- [Configuration](https://github.com/redstone-md/volt.rs/blob/main/docs/configuration.md)
- [CLI](https://github.com/redstone-md/volt.rs/blob/main/docs/cli.md)
- [Coming from Next.js](https://github.com/redstone-md/volt.rs/blob/main/docs/migrating-from-nextjs.md)

## License

MIT
