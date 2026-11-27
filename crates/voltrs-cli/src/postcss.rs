//! Runs the project's PostCSS plugins through Node when it has a `postcss.config.*` and `postcss` installed.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::project::Project;

const CONFIGS: [&str; 4] = ["postcss.config.mjs", "postcss.config.js", "postcss.config.cjs", ".postcssrc.json"];

/// Loads the config, instantiates its plugins (object or array form) and processes stdin-like files.
/// Tailwind's own plugin is skipped when volt already compiled Tailwind.
const RUNNER: &str = r#"import { readFileSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { pathToFileURL } from "node:url";
const [config, input, output, skipTailwind] = process.argv.slice(2);
const require = createRequire(pathToFileURL(process.cwd() + "/package.json"));
const load = async (name) => { const m = await import(pathToFileURL(require.resolve(name)).href); return m.default ?? m; };
const postcss = await load("postcss");
let options = config.endsWith(".json") ? JSON.parse(readFileSync(config, "utf8")) : (await import(pathToFileURL(config).href)).default;
if (typeof options === "function") options = await options({ env: process.env.NODE_ENV });
const tailwind = (name) => skipTailwind === "1" && /(^|\/)(@tailwindcss\/postcss|tailwindcss)$/.test(name);
let plugins = [];
const entries = options?.plugins ?? [];
if (Array.isArray(entries)) {
  for (const entry of entries) {
    if (typeof entry === "string") { if (!tailwind(entry)) plugins.push((await load(entry))()); }
    else if (Array.isArray(entry)) { if (!tailwind(entry[0])) plugins.push((await load(entry[0]))(entry[1])); }
    else if (entry && !tailwind(entry.postcssPlugin ?? "")) plugins.push(entry);
  }
} else {
  for (const [name, value] of Object.entries(entries)) {
    if (value === false || tailwind(name)) continue;
    plugins.push((await load(name))(value === true ? {} : value));
  }
}
const css = readFileSync(input, "utf8");
if (plugins.length === 0) { writeFileSync(output, css); process.exit(0); }
const result = await postcss(plugins).process(css, { from: input, to: output, map: false });
writeFileSync(output, result.css);
"#;

pub struct PostCss<'a> {
    project: &'a Project,
}

impl<'a> PostCss<'a> {
    pub fn new(project: &'a Project) -> Self {
        Self { project }
    }

    fn config(&self) -> Option<PathBuf> {
        CONFIGS.iter().map(|name| self.project.root.join(name)).find(|path| path.is_file())
    }

    /// Whether PostCSS will run: the project has a config and `postcss` is installed.
    pub fn enabled(&self) -> bool {
        self.config().is_some() && self.project.node_modules().join("postcss/package.json").is_file()
    }

    /// Processes CSS with the configured plugins, or returns it unchanged when PostCSS is not set up.
    pub fn process(&self, css: String, tailwind_done: bool) -> Result<String> {
        if !self.enabled() {
            return Ok(css);
        }
        let input = self.dir()?.join("input.css");
        std::fs::write(&input, css)?;
        self.process_file(&input, tailwind_done)
    }

    fn dir(&self) -> Result<PathBuf> {
        let dir = self.project.cache_dir().join("postcss");
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    /// Whether the config lists Tailwind's plugin, which then compiles Tailwind when its CLI is not installed.
    pub fn has_tailwind(&self) -> bool {
        self.enabled()
            && self
                .config()
                .and_then(|path| std::fs::read_to_string(path).ok())
                .is_some_and(|c| c.contains("tailwindcss"))
    }

    /// Processes a stylesheet on disk, so its `@import`s resolve relative to it.
    pub fn process_file(&self, input: &Path, tailwind_done: bool) -> Result<String> {
        let Some(config) = self.config() else { bail!("no PostCSS config found") };
        let dir = self.dir()?;
        let (runner, output) = (dir.join("run.mjs"), dir.join("output.css"));
        std::fs::write(&runner, RUNNER)?;
        let result = Command::new("node")
            .arg(&runner)
            .arg(&config)
            .arg(input)
            .arg(&output)
            .arg(if tailwind_done { "1" } else { "0" })
            .current_dir(&self.project.root)
            .output()
            .context("cannot run node for PostCSS; is Node.js installed?")?;
        if !result.status.success() {
            bail!("postcss failed:\n{}", String::from_utf8_lossy(&result.stderr));
        }
        std::fs::read_to_string(&output).context("postcss produced no output")
    }
}
