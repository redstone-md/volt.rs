//! Per-stylesheet processing with Lightning CSS: Sass, CSS Modules, `@import` inlining, `url()` assets,
//! vendor prefixes and minification.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use lightningcss::bundler::{Bundler, FileProvider, ResolveResult, SourceProvider};
use lightningcss::css_modules::{Config as ModulesConfig, CssModuleReference, Pattern};
use lightningcss::dependencies::{Dependency, DependencyOptions};
use lightningcss::printer::PrinterOptions;
use lightningcss::stylesheet::{MinifyOptions, ParserOptions, StyleSheet};
use lightningcss::targets::{Browsers, Targets};

use crate::media::Media;
use crate::project::Project;

/// Next.js' default browserslist, used when `package.json` has none.
const DEFAULT_BROWSERS: [&str; 5] = ["chrome 64", "edge 79", "firefox 67", "opera 51", "safari 12"];
const MODULE_PATTERN: &str = "[name]__[local]__[hash]";

pub fn is_stylesheet(ext: &str) -> bool {
    matches!(ext, "css" | "scss" | "sass")
}

pub fn is_module(path: &Path) -> bool {
    path.file_name().is_some_and(|name| {
        let name = name.to_string_lossy();
        [".module.css", ".module.scss", ".module.sass"].iter().any(|suffix| name.ends_with(suffix))
    })
}

fn is_sass(path: &Path) -> bool {
    path.extension().is_some_and(|e| e == "scss" || e == "sass")
}

/// One part of a CSS module export: a class of this file or an export of another module.
#[derive(Debug, Clone, PartialEq)]
pub enum ClassPart {
    Name(String),
    Import { specifier: String, name: String },
}

/// A compiled CSS module: its scoped CSS and the classes exported to JavaScript.
#[derive(Debug, Clone)]
pub struct CssModule {
    pub css: String,
    pub exports: BTreeMap<String, Vec<ClassPart>>,
}

/// Resolves `@import` like a browser for relative paths and like Node for bare package specifiers.
struct Imports {
    files: FileProvider,
    node_modules: PathBuf,
}

impl SourceProvider for Imports {
    type Error = std::io::Error;

    fn read<'a>(&'a self, file: &Path) -> Result<&'a str, Self::Error> {
        self.files.read(file)
    }

    fn resolve(&self, specifier: &str, originating_file: &Path) -> Result<ResolveResult, Self::Error> {
        if is_remote(specifier) {
            return Ok(ResolveResult::External(specifier.to_owned()));
        }
        if specifier.starts_with('.') || specifier.starts_with('/') {
            return Ok(originating_file.with_file_name(specifier).into());
        }
        let path = self.node_modules.join(specifier.trim_start_matches('~'));
        if path.extension().is_none() && !path.is_file() {
            return Ok(path.with_extension("css").into());
        }
        Ok(path.into())
    }
}

fn is_remote(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://") || url.starts_with("//")
}

/// Whether a `url()` points at a file next to the stylesheet rather than the site or the network.
fn is_local(url: &str) -> bool {
    !(url.is_empty()
        || is_remote(url)
        || url.starts_with('/')
        || url.starts_with('#')
        || url.starts_with("data:")
        || url.contains("://"))
}

fn css_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\a ")
}

#[derive(Debug)]
pub struct Styles {
    root: PathBuf,
    node_modules: PathBuf,
    media: Media,
    browsers: Option<Browsers>,
}

impl Styles {
    pub fn new(project: &Project, media: Media) -> Result<Self> {
        let queries = browserslist(&project.root);
        let browsers = Browsers::from_browserslist(&queries).map_err(|e| anyhow!("invalid browserslist: {e}"))?;
        Ok(Self { root: project.root.clone(), node_modules: project.node_modules(), media, browsers })
    }

    fn targets(&self) -> Targets {
        Targets { browsers: self.browsers, ..Targets::default() }
    }

    /// Reads a stylesheet, compiling Sass. `@use` and `@import` resolve relative to the file, then `node_modules`.
    pub fn source(&self, path: &Path) -> Result<String> {
        if !is_sass(path) {
            return std::fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()));
        }
        let mut options = grass::Options::default().load_path(&self.node_modules).quiet(true);
        if let Some(dir) = path.parent() {
            options = options.load_path(dir);
        }
        grass::from_path(path, &options).map_err(|e| anyhow!("{}: {e}", path.display()))
    }

    fn relative(&self, path: &Path) -> String {
        crate::util::to_slash(path.strip_prefix(&self.root).unwrap_or(path))
    }

    /// Compiles a CSS module. Class names are derived from the file path, so they stay stable across edits.
    pub fn module(&self, path: &Path) -> Result<CssModule> {
        let source = self.source(path)?;
        let filename = self.relative(path);
        let pattern = Pattern::parse(MODULE_PATTERN).map_err(|e| anyhow!("{e}"))?;
        let options = ParserOptions {
            filename: filename.clone(),
            css_modules: Some(ModulesConfig { pattern, ..ModulesConfig::default() }),
            ..ParserOptions::default()
        };
        let sheet = StyleSheet::parse(&source, options).map_err(|e| anyhow!("{}: {e}", path.display()))?;
        let (css, exports) = self.print(sheet, false, None)?;
        let exports = exports
            .into_iter()
            .map(|(local, export)| {
                let mut parts = vec![ClassPart::Name(export.name)];
                parts.extend(export.composes.into_iter().map(|reference| match reference {
                    CssModuleReference::Local { name } | CssModuleReference::Global { name } => ClassPart::Name(name),
                    CssModuleReference::Dependency { name, specifier } => ClassPart::Import { specifier, name },
                }));
                (local, parts)
            })
            .collect();
        Ok(CssModule { css, exports })
    }

    /// Compiles a global stylesheet with its `@import`s inlined and `url()` assets copied.
    pub fn global(&self, path: &Path) -> Result<String> {
        if is_sass(path) {
            let source = self.source(path)?;
            let options = ParserOptions { filename: self.relative(path), ..ParserOptions::default() };
            let sheet = StyleSheet::parse(&source, options).map_err(|e| anyhow!("{}: {e}", path.display()))?;
            return Ok(self.print(sheet, false, None)?.0);
        }
        let imports = Imports { files: FileProvider::new(), node_modules: self.node_modules.clone() };
        let mut bundler = Bundler::new(&imports, None, ParserOptions::default());
        let sheet = bundler.bundle(path).map_err(|e| anyhow!("{}: {e}", path.display()))?;
        Ok(self.print(sheet, false, None)?.0)
    }

    /// Copies files referenced by relative `url()`s in CSS whose origin is lost, such as Tailwind's output
    /// (it inlines imports without rebasing). Each URL resolves against the first of `dirs` that has the file.
    pub fn rebase(&self, css: &str, dirs: &[PathBuf], minify: bool) -> Result<String> {
        let options = ParserOptions { filename: "app.css".into(), error_recovery: true, ..ParserOptions::default() };
        let sheet = StyleSheet::parse(css, options).map_err(|e| anyhow!("app.css: {e}"))?;
        Ok(self.print(sheet, minify, Some(dirs))?.0)
    }

    /// Final pass over the combined stylesheet: lowers modern syntax, adds vendor prefixes, minifies.
    pub fn finish(&self, css: &str, minify: bool) -> Result<String> {
        let options = ParserOptions { filename: "app.css".into(), error_recovery: true, ..ParserOptions::default() };
        let mut sheet = StyleSheet::parse(css, options).map_err(|e| anyhow!("app.css: {e}"))?;
        let targets = self.targets();
        sheet.minify(MinifyOptions { targets, ..MinifyOptions::default() }).map_err(|e| anyhow!("app.css: {e}"))?;
        let printed = sheet
            .to_css(PrinterOptions { minify, targets, ..PrinterOptions::default() })
            .map_err(|e| anyhow!("app.css: {e}"))?;
        Ok(printed.code)
    }

    /// Prints a stylesheet with its relative `url()`s resolved against `dirs`, or against their own file.
    fn print(
        &self,
        sheet: StyleSheet,
        minify: bool,
        dirs: Option<&[PathBuf]>,
    ) -> Result<(String, lightningcss::css_modules::CssModuleExports)> {
        let printed = sheet
            .to_css(PrinterOptions {
                minify,
                analyze_dependencies: Some(DependencyOptions { remove_imports: false }),
                ..PrinterOptions::default()
            })
            .map_err(|e| anyhow!("{e}"))?;
        let mut code = printed.code;
        for dependency in printed.dependencies.unwrap_or_default() {
            let (placeholder, url) = match dependency {
                Dependency::Url(url) => {
                    let own = self.root.join(&url.loc.file_path).parent().map(Path::to_path_buf).into_iter().collect();
                    let resolved =
                        self.asset(&url.url, dirs.map(<[PathBuf]>::to_vec).unwrap_or(own), &url.loc.file_path)?;
                    (url.placeholder, resolved)
                }
                Dependency::Import(import) => (import.placeholder, import.url),
            };
            code = code.replace(&placeholder, &css_string(&url));
        }
        Ok((code, printed.exports.unwrap_or_default()))
    }

    /// Copies a relative `url()` target into the build and returns its public URL.
    fn asset(&self, url: &str, dirs: Vec<PathBuf>, stylesheet: &str) -> Result<String> {
        if !is_local(url) {
            return Ok(url.to_owned());
        }
        let split = url.find(['?', '#']).unwrap_or(url.len());
        let (file, suffix) = url.split_at(split);
        let decoded = percent_decode(file);
        let candidates = match decoded.strip_prefix('~') {
            Some(package) => vec![self.node_modules.join(package)],
            None => dirs.iter().map(|dir| dir.join(&decoded)).collect(),
        };
        let Some(path) = candidates.into_iter().find(|path| path.is_file()) else {
            voltrs::log::warn(&format!("cannot resolve url({url}) in {stylesheet}"));
            return Ok(url.to_owned());
        };
        Ok(format!("{}{suffix}", self.media.copy(&path)?))
    }
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(byte) = input.get(i + 1..i + 3).and_then(|hex| u8::from_str_radix(hex, 16).ok())
        {
            out.push(byte);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Browserslist queries from `package.json`'s `browserslist` field, or Next.js' defaults.
fn browserslist(root: &Path) -> Vec<String> {
    let field = std::fs::read_to_string(root.join("package.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|package| package.get("browserslist").cloned());
    let queries: Vec<String> = match field {
        Some(serde_json::Value::String(query)) => vec![query],
        Some(serde_json::Value::Array(items)) => items.iter().filter_map(|q| q.as_str().map(str::to_owned)).collect(),
        Some(serde_json::Value::Object(envs)) => envs
            .get("production")
            .and_then(|q| q.as_array())
            .map(|items| items.iter().filter_map(|q| q.as_str().map(str::to_owned)).collect())
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    if queries.is_empty() { DEFAULT_BROWSERS.iter().map(|q| (*q).to_owned()).collect() } else { queries }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn styles(root: &Path) -> Styles {
        Styles {
            root: root.to_path_buf(),
            node_modules: root.join("node_modules"),
            media: Media::new(root.join("media"), voltrs::manifest::STATIC_PREFIX),
            browsers: Browsers::from_browserslist(DEFAULT_BROWSERS).unwrap(),
        }
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("volt-styles-{name}-{}", std::process::id()));
        crate::util::reset_dir(&dir).unwrap();
        dir
    }

    #[test]
    fn detects_modules() {
        assert!(is_module(Path::new("/a/card.module.css")));
        assert!(is_module(Path::new("/a/card.module.scss")));
        assert!(!is_module(Path::new("/a/globals.css")));
        assert!(is_local("./a.png") && is_local("a.png") && !is_local("/a.png") && !is_local("data:x"));
    }

    #[test]
    fn compiles_modules_with_composes() {
        let dir = temp("modules");
        std::fs::write(
            dir.join("card.module.scss"),
            "$c: red;\n.base { color: $c; }\n.title { composes: base; composes: x from './other.module.css'; }\n",
        )
        .unwrap();
        let module = styles(&dir).module(&dir.join("card.module.scss")).unwrap();
        assert!(module.css.contains("color: red"), "{}", module.css);
        let title = &module.exports["title"];
        assert!(matches!(&title[0], ClassPart::Name(n) if n.starts_with("card-module__title__")));
        assert!(title.contains(&ClassPart::Import { specifier: "./other.module.css".into(), name: "x".into() }));
        assert!(title.iter().any(|p| matches!(p, ClassPart::Name(n) if n.starts_with("card-module__base__"))));
    }

    #[test]
    fn inlines_imports_and_copies_urls() {
        let dir = temp("urls");
        std::fs::create_dir_all(dir.join("styles/img")).unwrap();
        std::fs::write(dir.join("styles/img/dot.png"), b"png").unwrap();
        std::fs::write(dir.join("styles/part.css"), ".part { background: url(./img/dot.png?v=1); }").unwrap();
        std::fs::write(dir.join("main.css"), "@import './styles/part.css';\n.main { background: url(\"/x.png\"); }")
            .unwrap();
        let css = styles(&dir).global(&dir.join("main.css")).unwrap();
        assert!(css.contains("/_volt/static/media/dot-"), "{css}");
        assert!(css.contains("?v=1"), "{css}");
        assert!(css.contains("\"/x.png\""), "{css}");
        assert!(dir.join("media").read_dir().unwrap().count() == 1);
    }

    #[test]
    fn finishes_with_prefixes_and_minification() {
        let dir = temp("finish");
        let css = styles(&dir).finish(".a {\n  user-select: none;\n  .b { color: #ff0000; }\n}\n", true).unwrap();
        assert!(css.contains("-webkit-user-select:none"), "{css}");
        assert!(css.contains(".a .b{color:red}"), "{css}");
        assert!(!css.contains('\n'));
    }

    #[test]
    fn reads_browserslist() {
        let dir = temp("browserslist");
        assert_eq!(browserslist(&dir).len(), DEFAULT_BROWSERS.len());
        std::fs::write(dir.join("package.json"), r#"{"browserslist":["last 1 chrome version"]}"#).unwrap();
        assert_eq!(browserslist(&dir), vec!["last 1 chrome version".to_owned()]);
    }
}
