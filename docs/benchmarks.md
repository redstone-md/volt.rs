# Benchmarks

Numbers in the README come from the setup below. They compare frameworks, not hardware, so run them on your own machine before drawing conclusions.

## Setup

- Machine: 4 vCPU, 16 GB RAM, Linux 6.18. Load generator and server on the same machine.
- Load: [oha](https://github.com/hatoo/oha) 1.17, `oha -z 10s -c 64 --no-tui <url>`, after one warm-up second.
- App: the `default` volt template (shadcn/ui card, input, buttons, lucide icons, Tailwind CSS v4, todo list from a loader, two server actions). The Next.js app is the same UI ported to Next.js 16.4.6 with server components, `export const dynamic = "force-dynamic"` on `/` and server actions with `revalidatePath`.
- volt: `volt build`, then the release server binary with default `volt.toml` (one render worker per CPU).
- Next.js: `next build`, then `next start` (a single Node.js 22 process, default settings).
- Memory: resident set size of all server processes, idle after startup and right after the load runs.

## Results (volt.rs 0.0.1)

| Metric | volt.rs | Next.js 16.4.6 | Ratio |
| --- | ---: | ---: | ---: |
| `/` dynamic SSR, req/s | 1,977 | 329 | 6.0× |
| `/` p50 / p99 latency | 32 ms / 42 ms | 186 ms / 450 ms | |
| `/about` static, req/s | 36,352 | 1,711 | 21× |
| `/about` p50 / p99 latency | 1.7 ms / 3.6 ms | 34 ms / 71 ms | |
| RSS idle | 19 MB | 85 MB | 4.5× less |
| RSS after load | 49 MB | 268 MB | 5.5× less |
| Start to first response | 73 ms | 375 ms | |
| Production build | 1.1 s | 5.9 s | |
| Deploy size | 4.1 MB binary + 4.2 MB `.volt` | 33 MB `.next` + 571 MB `node_modules` | |

## Reproduce

```bash
cargo install oha
npm create volt-rs@latest bench-app -- --template default
cd bench-app && npx volt build
PORT=3100 ./target/release/bench-app &
oha -z 10s -c 64 --no-tui http://localhost:3100/
oha -z 10s -c 64 --no-tui http://localhost:3100/about
```

## Notes

- A single `next start` process uses one core for rendering; Node.js clustering would raise its throughput, at a proportional memory cost.
- The volt render pool size is `server.workers`. Each worker is an isolated QuickJS heap; memory grows roughly linearly with workers.
- Static pages in volt are served from memory with precomputed headers, which is why they approach raw HTTP server speed.
