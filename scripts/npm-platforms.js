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
    homepage: "https://github.com/redstone-md/volt.rs",
    files: ["bin", "README.md"],
  };
  writeFileSync(join(dir, "package.json"), `${JSON.stringify(manifest, null, 2)}\n`);
  // npm shows an empty page for packages without a README.
  writeFileSync(
    join(dir, "README.md"),
    `# @volt-rs/cli-${platform}\n\nThe prebuilt \`volt\` binary for ${platform}. It is installed as an optional dependency of [volt-rs](https://www.npmjs.com/package/volt-rs); don't depend on it directly.\n\nSee https://github.com/redstone-md/volt.rs.\n`,
  );
}
