// Builds a fixture app against the local crates and packages, serves it and runs its browser test.
// Usage: node e2e/run.mjs <fixture> [--keep] [--dev]
// With --dev, or `"e2e": { "dev": true }` in the fixture's package.json, the app runs under `volt dev`
// instead of a production build, and the test may edit the files in APP_DIR.
// Env: VOLT_BIN (default target/debug/volt), PORT (default 3900), E2E_DIR (default /tmp/volt-e2e)
import { spawn, spawnSync } from "node:child_process";
import { cpSync, existsSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, "..");
const [fixture, ...flags] = process.argv.slice(2);
if (!fixture) {
  console.error("usage: node e2e/run.mjs <fixture>");
  process.exit(2);
}
const source = join(here, "fixtures", fixture);
const test = join(here, `${fixture}.test.js`);
const volt = resolve(process.env.VOLT_BIN ?? join(repo, "target", "debug", "volt"));
const port = process.env.PORT ?? "3900";
const dir = join(process.env.E2E_DIR ?? "/tmp/volt-e2e", `${fixture}-${port}`);

const run = (command, args, options = {}) => {
  const result = spawnSync(command, args, { stdio: "inherit", cwd: dir, ...options });
  if (result.status !== 0) {
    console.error(`failed: ${command} ${args.join(" ")}`);
    process.exit(1);
  }
};

rmSync(dir, { recursive: true, force: true });
cpSync(source, dir, { recursive: true });
const pkgPath = join(dir, "package.json");
const pkg = JSON.parse(readFileSync(pkgPath, "utf8"));
pkg.dependencies = { ...pkg.dependencies, "volt-rs": `file:${join(repo, "packages", "volt-rs")}` };
writeFileSync(pkgPath, JSON.stringify(pkg, null, 2));
const cargoPath = join(dir, "Cargo.toml");
if (existsSync(cargoPath)) {
  const cargo = readFileSync(cargoPath, "utf8").replace(/^voltrs = .*$/m, `voltrs = { path = ${JSON.stringify(join(repo, "crates", "voltrs"))} }`);
  writeFileSync(cargoPath, cargo);
}
run("npm", ["install", "--install-links", "--no-audit", "--no-fund", "--loglevel=error"]);
const dev = flags.includes("--dev") || pkg.e2e?.dev === true;
if (!dev) run(volt, ["build"]);

const server = spawn(volt, [dev ? "dev" : "start"], { cwd: dir, env: { ...process.env, PORT: port }, stdio: "inherit" });
const base = `http://localhost:${port}`;
let ready = false;
for (let i = 0; i < (dev ? 1200 : 200) && !ready; i++) {
  ready = await fetch(base).then(() => true, () => false);
  if (!ready) await new Promise((r) => setTimeout(r, 100));
}
if (!ready) {
  server.kill();
  console.error("server did not start");
  process.exit(1);
}
const result = spawnSync("node", [test], { stdio: "inherit", env: { ...process.env, BASE_URL: base, APP_DIR: dir } });
server.kill("SIGTERM");
if (!flags.includes("--keep")) rmSync(dir, { recursive: true, force: true });
process.exit(result.status ?? 1);
