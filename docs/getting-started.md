# Getting started

## Requirements

- Node.js 18 or newer (for npm packages and Tailwind CSS)
- Rust 1.88 or newer, only if the app has a Rust server (`default` template)

## Create an app

```bash
npm create volt-rs@latest my-app
```

The scaffolder picks the `default` template (React + Rust server) when `cargo` is installed and the `web` template (React only) otherwise. Choose explicitly with `--template default` or `--template web`.

The `volt` binary can also scaffold:

```bash
cargo install voltrs-cli
volt new my-app
```

## Run it

```bash
cd my-app
npm run dev
```

Open http://localhost:3000. Edit `app/page.tsx` and the change appears in place, with the page state kept (React Fast Refresh). Edit `server/main.rs` and the server recompiles, the typed client in `@volt/api` is regenerated and the page reloads.

## Project structure

```
my-app/
├── app/                 routes: page.tsx, layout.tsx, loading.tsx, error.tsx, not-found.tsx
│   ├── globals.css      Tailwind CSS and theme variables
│   ├── layout.tsx       root layout: <html> and <body>
│   └── page.tsx         /
├── components/ui/       shadcn/ui components
├── lib/utils.ts         cn() helper
├── public/              static files served at /
├── server/main.rs       Rust loaders, actions, API routes
├── Cargo.toml           Rust server crate
├── package.json
├── tsconfig.json        `@/*` and `@volt/api` path aliases
├── volt.toml            configuration
└── .volt/               build output (generated)
```

## Build for production

```bash
npm run build
npm run start
```

`volt build` compiles the Rust server in release mode, bundles and minifies the client, compiles Tailwind CSS, prerenders static pages and precompresses assets with Brotli and gzip. `volt start` runs the server binary against that build.

Next: [Routing](./routing.md), [Data and server actions](./data.md).
