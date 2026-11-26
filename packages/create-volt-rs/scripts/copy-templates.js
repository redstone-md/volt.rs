import { cpSync, rmSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const source = join(here, "..", "..", "..", "crates", "voltrs-cli", "templates");
const target = join(here, "..", "templates");
rmSync(target, { recursive: true, force: true });
cpSync(source, target, { recursive: true });
