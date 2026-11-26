// Creates the @volt-rs/cli-<platform>-<arch> packages from release binaries.
import { chmodSync, copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const [dist, out] = process.argv.slice(2);
const { version, repository, license } = JSON.parse(readFileSync("packages/volt-rs/package.json", "utf8"));
const platforms = ["linux-x64", "linux-arm64", "darwin-x64", "darwin-arm64", "win32-x64"];

for (const platform of platforms) {
  const [os, cpu] = platform.split("-");
  const exe = os === "win32" ? "volt.exe" : "volt";
  const source = join(dist, `volt-${platform}${os === "win32" ? ".exe" : ""}`);
  if (!existsSync(source)) {
    console.warn(`missing ${source}, skipping`);
    continue;
  }
  const dir = join(out, `cli-${platform}`);
  mkdirSync(join(dir, "bin"), { recursive: true });
  copyFileSync(source, join(dir, "bin", exe));
  chmodSync(join(dir, "bin", exe), 0o755);
  const manifest = {
    name: `@volt-rs/cli-${platform}`,
    version,
    description: `The volt binary for ${platform}`,
    license,
    repository,
    os: [os],
    cpu: [cpu],
    files: ["bin"],
  };
  writeFileSync(join(dir, "package.json"), `${JSON.stringify(manifest, null, 2)}\n`);
}
