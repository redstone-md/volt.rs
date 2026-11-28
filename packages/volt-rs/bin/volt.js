#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { createRequire } from "node:module";
import { homedir } from "node:os";
import { join } from "node:path";

const require = createRequire(import.meta.url);
const exe = process.platform === "win32" ? "volt.exe" : "volt";

function resolveBinary() {
  if (process.env.VOLT_BINARY) return process.env.VOLT_BINARY;
  const pkg = `@volt-rs/cli-${process.platform}-${process.arch}`;
  try {
    return join(require.resolve(`${pkg}/package.json`), "..", "bin", exe);
  } catch {}
  const cargo = join(process.env.CARGO_HOME ?? join(homedir(), ".cargo"), "bin", exe);
  if (existsSync(cargo)) return cargo;
  return null;
}

const binary = resolveBinary();
if (!binary) {
  console.error(
    `volt: no prebuilt binary for ${process.platform}-${process.arch}.\n` +
      "Install it with `cargo install voltrs-cli` or set VOLT_BINARY.",
  );
  process.exit(1);
}

const result = spawnSync(binary, process.argv.slice(2), { stdio: "inherit" });
if (result.error) {
  console.error(`volt: ${result.error.message}`);
  process.exit(1);
}
process.exit(result.status ?? 1);
