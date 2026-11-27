//! The global stylesheet: Tailwind CSS when the project uses it, Lightning CSS otherwise, then PostCSS.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::postcss::PostCss;
use crate::project::Project;
use crate::styles::Styles;
use crate::util::{hash, to_slash};

const TAILWIND_CLIS: [&str; 2] = ["@tailwindcss/cli/dist/index.mjs", "tailwindcss/lib/cli.js"];

fn uses_tailwind(source: &str) -> bool {
    source.contains("@import \"tailwindcss") || source.contains("@import 'tailwindcss") || source.contains("@tailwind ")
}

/// One part of the global stylesheet, in import order.
#[derive(Debug, Clone, PartialEq)]
pub enum Stylesheet {
    /// A global `.css`, `.scss` or `.sass` file imported from code.
    File(PathBuf),
    /// CSS generated during bundling: CSS modules and fonts.
    Compiled(String),
}

pub struct Css<'a> {
    project: &'a Project,
    styles: &'a Styles,
    minify: bool,
}

impl<'a> Css<'a> {
    pub fn new(project: &'a Project, styles: &'a Styles, minify: bool) -> Self {
        Self { project, styles, minify }
    }

    /// Compiles the stylesheets into one hashed file and returns its URL.
    pub fn build(&self, sheets: &[Stylesheet]) -> Result<Option<String>> {
        if sheets.is_empty() {
            return Ok(None);
        }
        let tailwind = sheets.iter().any(|sheet| match sheet {
            Stylesheet::File(path) => std::fs::read_to_string(path).is_ok_and(|source| uses_tailwind(&source)),
            Stylesheet::Compiled(_) => false,
        });
        let css = if tailwind { self.tailwind(sheets)? } else { self.plain(sheets)? };
        let dir = self.project.static_dir().join("css");
        crate::util::reset_dir(&dir)?;
        let name = format!("app-{}.css", hash(css.as_bytes()));
        std::fs::write(dir.join(&name), css)?;
        Ok(Some(format!("{}css/{name}", self.project.config.static_prefix())))
    }

    fn plain(&self, sheets: &[Stylesheet]) -> Result<String> {
        let parts: Vec<String> = sheets
            .iter()
            .map(|sheet| match sheet {
                Stylesheet::File(path) => self.styles.global(path),
                Stylesheet::Compiled(css) => Ok(css.clone()),
            })
            .collect::<Result<_>>()?;
        let css = PostCss::new(self.project).process(parts.join("\n"), false)?;
        self.styles.finish(&css, self.minify)
    }

    /// Feeds every part to Tailwind through one entry file so `@import "tailwindcss"` sees the whole app.
    fn tailwind(&self, sheets: &[Stylesheet]) -> Result<String> {
        let cache = self.project.cache_dir();
        let generated = cache.join("styles");
        crate::util::reset_dir(&generated)?;
        let mut imports = String::new();
        for (index, sheet) in sheets.iter().enumerate() {
            let path = match sheet {
                Stylesheet::File(path) if path.extension().is_some_and(|e| e == "css") => path.clone(),
                Stylesheet::File(path) => self.write(&generated, index, &self.styles.global(path)?)?,
                Stylesheet::Compiled(css) => self.write(&generated, index, css)?,
            };
            let import = relative(&cache, &path);
            let import = if import.starts_with('.') { import } else { format!("./{import}") };
            imports.push_str(&format!("@import {import:?};\n"));
        }
        let entry = cache.join("entry.css");
        std::fs::write(&entry, imports)?;
        let postcss = PostCss::new(self.project);
        let modules = self.project.node_modules();
        let css = match TAILWIND_CLIS.iter().map(|p| modules.join(p)).find(|p| p.is_file()) {
            Some(cli) => postcss.process(self.cli(&cli, &entry)?, true)?,
            None if postcss.has_tailwind() => postcss.process_file(&entry, false)?,
            None => {
                bail!("this project uses Tailwind CSS; install it with `npm install -D tailwindcss @tailwindcss/cli`")
            }
        };
        let mut dirs = vec![cache];
        for sheet in sheets {
            if let Stylesheet::File(path) = sheet
                && let Some(dir) = path.parent().filter(|dir| !dirs.iter().any(|d| d == dir))
            {
                dirs.push(dir.to_path_buf());
            }
        }
        self.styles.rebase(&css, &dirs, self.minify)
    }

    fn write(&self, dir: &Path, index: usize, css: &str) -> Result<PathBuf> {
        let path = dir.join(format!("{index}-{}.css", hash(css.as_bytes())));
        std::fs::write(&path, css)?;
        Ok(path)
    }

    fn cli(&self, cli: &Path, entry: &Path) -> Result<String> {
        let output = self.project.cache_dir().join("tailwind.css");
        let mut command = Command::new("node");
        command.arg(cli).arg("-i").arg(entry).arg("-o").arg(&output).current_dir(&self.project.root);
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
