#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import { cpSync, existsSync, readdirSync, readFileSync, renameSync, statSync, writeFileSync } from "node:fs";
import { basename, dirname, join, resolve } from "node:path";
import { createInterface } from "node:readline/promises";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const version = JSON.parse(readFileSync(join(here, "package.json"), "utf8")).version;
const templates = join(here, "templates");
const color = (code, text) => (process.stdout.isTTY ? `\x1b[${code}m${text}\x1b[0m` : text);

function parseArgs(argv) {
  const options = { template: null, install: true, name: null };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === "--template" || arg === "-t") options.template = argv[++i];
    else if (arg === "--no-install") options.install = false;
    else if (arg === "--web") options.template = "web";
    else if (arg === "--help" || arg === "-h") options.help = true;
    else if (!arg.startsWith("-")) options.name = arg;
  }
  return options;
}

function packageManager() {
  const agent = process.env.npm_config_user_agent ?? "";
  return ["pnpm", "yarn", "bun"].find((pm) => agent.startsWith(pm)) ?? "npm";
}

function hasCargo() {
  return spawnSync("cargo", ["--version"], { stdio: "ignore" }).status === 0;
}

function render(dir, name) {
  for (const entry of readdirSync(dir)) {
    const path = join(dir, entry);
    if (statSync(path).isDirectory()) {
      render(path, name);
      continue;
    }
    if (/\.(png|jpg|jpeg|gif|webp|ico|woff2?)$/.test(entry)) continue;
    const text = readFileSync(path, "utf8");
    writeFileSync(path, text.replaceAll("{{name}}", name).replaceAll("{{version}}", version));
    if (entry.startsWith("_") && !entry.startsWith("__")) renameSync(path, join(dir, `.${entry.slice(1)}`));
  }
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  if (options.help) {
    console.log("Usage: npm create volt-rs@latest [dir] [--template default|web] [--no-install]");
    return;
  }
  console.log(`\n  ${color("1;33", "⚡ volt.rs")} ${version}\n`);
  let name = options.name;
  if (!name) {
    const rl = createInterface({ input: process.stdin, output: process.stdout });
    name = (await rl.question("  Project name: ")).trim() || "volt-app";
    rl.close();
  }
  const target = resolve(name);
  if (existsSync(target) && readdirSync(target).length > 0) {
    console.error(`  ${target} already exists and is not empty`);
    process.exit(1);
  }
  const template = options.template ?? (hasCargo() ? "default" : "web");
  if (!existsSync(join(templates, template))) {
    console.error(`  unknown template "${template}" (use default or web)`);
    process.exit(1);
  }
  if (template === "default" && !hasCargo()) {
    console.log(`  ${color("33", "!")} Rust is not installed; the server needs it. Get it at https://rustup.rs\n`);
  }
  const slug = basename(target).toLowerCase().replace(/[^a-z0-9-]/g, "-");
  cpSync(join(templates, template), target, { recursive: true });
  render(target, slug);
  console.log(`  ${color("32", "✓")} created ${name} (${template} template)`);

  const pm = packageManager();
  if (options.install) {
    console.log(`  installing dependencies with ${pm}…\n`);
    const result = spawnSync(pm, ["install"], { cwd: target, stdio: "inherit", shell: process.platform === "win32" });
    if (result.status !== 0) console.log(`\n  ${color("33", "!")} ${pm} install failed; run it manually`);
  }
  console.log(`\n  Next steps:\n\n    cd ${name}\n    ${pm} run dev\n`);
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
