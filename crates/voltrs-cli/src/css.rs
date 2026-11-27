//! Global CSS: Tailwind CSS when the project uses it, plain concatenation otherwise.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::project::Project;
use crate::util::{hash, to_slash};

const TAILWIND_CLIS: [&str; 2] = ["@tailwindcss/cli/dist/index.mjs", "tailwindcss/lib/cli.js"];

fn uses_tailwind(source: &str) -> bool {
    source.contains("@import \"tailwindcss") || source.contains("@import 'tailwindcss") || source.contains("@tailwind ")
}

pub struct Css<'a> {
    project: &'a Project,
    minify: bool,
}

impl<'a> Css<'a> {
    pub fn new(project: &'a Project, minify: bool) -> Self {
        Self { project, minify }
    }

    /// Compiles the imported stylesheets into one hashed file and returns its URL.
    pub fn build(&self, files: &[PathBuf]) -> Result<Option<String>> {
        if files.is_empty() {
            return Ok(None);
        }
        let sources: Vec<String> = files
            .iter()
            .map(|f| std::fs::read_to_string(f).with_context(|| format!("cannot read {}", f.display())))
            .collect::<Result<_>>()?;
        let css = if sources.iter().any(|s| uses_tailwind(s)) { self.tailwind(files)? } else { sources.join("\n") };
        let dir = self.project.static_dir().join("css");
        crate::util::reset_dir(&dir)?;
        let name = format!("app-{}.css", hash(css.as_bytes()));
        std::fs::write(dir.join(&name), css)?;
        Ok(Some(format!("{}css/{name}", self.project.config.static_prefix())))
    }

    fn tailwind(&self, files: &[PathBuf]) -> Result<String> {
        let modules = self.project.node_modules();
        let Some(cli) = TAILWIND_CLIS.iter().map(|p| modules.join(p)).find(|p| p.is_file()) else {
            bail!("this project uses Tailwind CSS; install it with `npm install -D tailwindcss @tailwindcss/cli`");
        };
        let cache = self.project.cache_dir();
        std::fs::create_dir_all(&cache)?;
        let entry = cache.join("entry.css");
        let output = cache.join("tailwind.css");
        let imports: String = files.iter().map(|f| format!("@import {:?};\n", relative(&cache, f))).collect();
        std::fs::write(&entry, imports)?;
        let mut command = Command::new("node");
        command.arg(&cli).arg("-i").arg(&entry).arg("-o").arg(&output).current_dir(&self.project.root);
        if self.minify {
            command.arg("--minify");
        }
        let result = command.output().context("cannot run node; is Node.js installed?")?;
        if !result.status.success() {
            bail!("tailwind failed:\n{}", String::from_utf8_lossy(&result.stderr));
        }
        Ok(std::fs::read_to_string(&output)?)
    }
}

fn relative(from: &Path, to: &Path) -> String {
    let from: Vec<_> = from.components().collect();
    let to_parts: Vec<_> = to.components().collect();
    let common = from.iter().zip(&to_parts).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<String> = vec!["..".into(); from.len() - common];
    parts.extend(to_parts[common..].iter().map(|c| c.as_os_str().to_string_lossy().into_owned()));
    to_slash(Path::new(&parts.join("/")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_paths() {
        assert_eq!(relative(Path::new("/p/.volt/cache"), Path::new("/p/app/globals.css")), "../../app/globals.css");
        assert!(uses_tailwind("@import \"tailwindcss\";"));
    }
}
