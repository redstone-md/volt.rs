//! Per-stylesheet processing with Lightning CSS: Sass, CSS Modules, `@import` inlining, `url()` assets,
//! vendor prefixes and minification.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

use anyhow::{Context, Result, anyhow};
use lightningcss::bundler::{Bundler, FileProvider, ResolveResult, SourceProvider};
use lightningcss::css_modules::{Config as ModulesConfig, CssModuleReference, Pattern};
use lightningcss::dependencies::{Dependency, DependencyOptions};
use lightningcss::printer::PrinterOptions;
use lightningcss::rules::CssRule;
use lightningcss::rules::import::ImportRule;
use lightningcss::stylesheet::{MinifyOptions, ParserOptions, StyleSheet};
use lightningcss::targets::{Browsers, Targets};
use lightningcss::traits::ToCss;

use crate::csstext::{hoist_imports, is_local, is_remote, rebase_urls};
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
fn resolve(specifier: &str, originating_file: &Path, node_modules: &Path) -> ResolveResult {
    if is_remote(specifier) {
        return ResolveResult::External(specifier.to_owned());
    }
    if specifier.starts_with('.') || specifier.starts_with('/') {
        return originating_file.with_file_name(specifier).into();
    }
    let path = node_modules.join(specifier.trim_start_matches('~'));
    if path.extension().is_none() && !path.is_file() {
        return path.with_extension("css").into();
    }
    path.into()
}

/// The Lightning CSS bundler's view of `@import`s in plain CSS files.
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
        Ok(resolve(specifier, originating_file, &self.node_modules))
    }
}

/// Files read by the Sass compiler. Partials outside the entry's directory get their relative references
/// rebased onto it, since Sass inlines them as if they were written in the entry.
#[derive(Debug)]
struct SassFiles {
    entry_dir: PathBuf,
}

impl grass::Fs for SassFiles {
    fn is_dir(&self, path: &Path) -> bool {
        path.is_dir()
    }

    fn is_file(&self, path: &Path) -> bool {
        path.is_file()
    }

    fn read(&self, path: &Path) -> std::io::Result<Vec<u8>> {
        let bytes = std::fs::read(path)?;
        let prefix = path.parent().map(|dir| crate::util::relative(&self.entry_dir, dir)).unwrap_or_default();
        if prefix.is_empty() {
            return Ok(bytes);
        }
        Ok(rebase_urls(&String::from_utf8_lossy(&bytes), &format!("{prefix}/")).into_bytes())
    }
}

/// Wraps inlined CSS in the layer, `@supports` and media conditions of the `@import` that pulled it in.
fn conditioned(import: &ImportRule, css: String) -> Result<String> {
    let options = PrinterOptions::default;
    let mut css = css;
    if !import.media.media_queries.is_empty() {
        css = format!("@media {} {{\n{css}\n}}", import.media.to_css_string(options())?);
    }
    if let Some(supports) = &import.supports {
        css = format!("@supports {} {{\n{css}\n}}", supports.to_css_string(options())?);
    }
    match &import.layer {
        Some(Some(name)) => css = format!("@layer {} {{\n{css}\n}}", name.to_css_string(options())?),
        Some(None) => css = format!("@layer {{\n{css}\n}}"),
        None => {}
    }
    Ok(css)
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
    /// Compiled CSS modules, shared by the client and server bundles of one build.
    modules: Mutex<HashMap<PathBuf, CssModule>>,
}

impl Styles {
    pub fn new(project: &Project, media: Media) -> Result<Self> {
        let queries = browserslist(&project.root);
        let browsers = Browsers::from_browserslist(&queries).map_err(|e| anyhow!("invalid browserslist: {e}"))?;
        Ok(Self {
            root: project.root.clone(),
            node_modules: project.node_modules(),
            media,
            browsers,
            modules: Mutex::default(),
        })
    }

    fn targets(&self) -> Targets {
        Targets { browsers: self.browsers, ..Targets::default() }
    }

    /// Reads a stylesheet, compiling Sass. `@use` and `@import` resolve relative to the file, then `node_modules`.
    pub fn source(&self, path: &Path) -> Result<String> {
        if !is_sass(path) {
            return std::fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()));
        }
        let dir = path.parent().unwrap_or(Path::new("."));
        let files = SassFiles { entry_dir: dir.to_path_buf() };
        let options = grass::Options::default().fs(&files).load_path(&self.node_modules).load_path(dir).quiet(true);
        grass::from_path(path, &options).map_err(|e| anyhow!("{}: {e}", path.display()))
    }

    fn relative(&self, path: &Path) -> String {
        crate::util::to_slash(path.strip_prefix(&self.root).unwrap_or(path))
    }

    /// Compiles a CSS module. Class names are derived from the file path, so they stay stable across edits.
    pub fn module(&self, path: &Path) -> Result<CssModule> {
        if let Some(module) = self.modules.lock().ok().and_then(|m| m.get(path).cloned()) {
            return Ok(module);
        }
        let module = self.compile_module(path)?;
        if let Ok(mut modules) = self.modules.lock() {
            modules.insert(path.to_path_buf(), module.clone());
        }
        Ok(module)
    }

    fn compile_module(&self, path: &Path) -> Result<CssModule> {
        let source = self.source(path)?;
        let filename = self.relative(path);
        let pattern = Pattern::parse(MODULE_PATTERN).map_err(|e| anyhow!("{e}"))?;
        let options = ParserOptions {
            filename: filename.clone(),
            css_modules: Some(ModulesConfig { pattern, ..ModulesConfig::default() }),
            ..ParserOptions::default()
        };
        let mut sheet = StyleSheet::parse(&source, options).map_err(|e| anyhow!("{}: {e}", path.display()))?;
        let imported = self.inline_imports(&mut sheet, path)?;
        let (css, exports) = self.print(sheet, false, None)?;
        let css = imported + &css;
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
            let mut sheet = StyleSheet::parse(&source, options).map_err(|e| anyhow!("{}: {e}", path.display()))?;
            let imported = self.inline_imports(&mut sheet, path)?;
            return Ok(imported + &self.print(sheet, false, None)?.0);
        }
        let imports = Imports { files: FileProvider::new(), node_modules: self.node_modules.clone() };
        let mut bundler = Bundler::new(&imports, None, ParserOptions::default());
        let sheet = bundler.bundle(path).map_err(|e| anyhow!("{}: {e}", path.display()))?;
        Ok(self.print(sheet, false, None)?.0)
    }

    /// Takes the local `@import`s out of a sheet parsed without the bundler (Sass output, CSS modules) and
    /// returns their compiled CSS. Remote imports stay; the final pass moves them to the top.
    fn inline_imports(&self, sheet: &mut StyleSheet, origin: &Path) -> Result<String> {
        let mut inlined = String::new();
        let mut kept = Vec::new();
        for rule in std::mem::take(&mut sheet.rules.0) {
            let CssRule::Import(import) = rule else {
                kept.push(rule);
                continue;
            };
            let ResolveResult::File(file) = resolve(&import.url, origin, &self.node_modules) else {
                kept.push(CssRule::Import(import));
                continue;
            };
            let css = if is_module(&file) { self.module(&file)?.css } else { self.global(&file)? };
            inlined.push_str(&conditioned(&import, css)?);
            inlined.push('\n');
        }
        sheet.rules.0 = kept;
        Ok(inlined)
    }

    /// Copies files referenced by relative `url()`s in CSS whose origin is lost, such as Tailwind's output
    /// (it inlines imports without rebasing). Each URL resolves against the first of `dirs` that has the file.
    pub fn rebase(&self, css: &str, dirs: &[PathBuf], minify: bool) -> Result<String> {
        let css = hoist_imports(css);
        let sheet = parse_combined(&css)?;
        Ok(self.print(sheet, minify, Some(dirs))?.0)
    }

    /// Final pass over the combined stylesheet: lowers modern syntax, adds vendor prefixes, minifies.
    pub fn finish(&self, css: &str, minify: bool) -> Result<String> {
        let css = hoist_imports(css);
        let mut sheet = parse_combined(&css)?;
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

/// Parses the combined stylesheet, skipping invalid rules like a browser but reporting each one.
fn parse_combined(css: &str) -> Result<StyleSheet<'_>> {
    let warnings = Arc::new(RwLock::new(Vec::new()));
    let options = ParserOptions {
        filename: "app.css".into(),
        error_recovery: true,
        warnings: Some(warnings.clone()),
        ..ParserOptions::default()
    };
    let sheet = StyleSheet::parse(css, options).map_err(|e| anyhow!("app.css: {e}"))?;
    if let Ok(warnings) = warnings.read() {
        for warning in warnings.iter() {
            voltrs::log::warn(&format!("dropped invalid css: {}", warning.kind));
        }
    }
    Ok(sheet)
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
            modules: Mutex::default(),
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
    fn inlines_css_imported_from_sass_and_modules() {
        let dir = temp("sass-imports");
        std::fs::create_dir_all(dir.join("parts/img")).unwrap();
        std::fs::write(dir.join("parts/img/p.png"), b"png").unwrap();
        std::fs::write(dir.join("parts/_bg.scss"), ".partbg { background: url(\"./img/p.png\"); }").unwrap();
        std::fs::write(dir.join("parts/plain.css"), ".plain { color: green; }").unwrap();
        std::fs::write(dir.join("shared.css"), ".shared { color: olive; }").unwrap();
        std::fs::write(
            dir.join("globals.scss"),
            "@use \"./parts/bg\";\n@import \"./parts/plain.css\";\n@import url(\"https://f.test/a.css\");\n.g { color: red; }",
        )
        .unwrap();
        std::fs::write(dir.join("card.module.css"), "@import \"./shared.css\" screen;\n.card { color: blue; }")
            .unwrap();
        let styles = styles(&dir);
        let global = styles.global(&dir.join("globals.scss")).unwrap();
        assert!(global.contains(".plain"), "{global}");
        assert!(global.contains("/_volt/static/media/p-"), "{global}");
        assert!(global.contains("@import \"https://f.test/a.css\""), "{global}");
        let module = styles.module(&dir.join("card.module.css")).unwrap();
        assert!(module.css.contains("@media screen"), "{}", module.css);
        assert!(module.css.contains(".shared"), "{}", module.css);
        let combined = styles.finish(&format!("{module}\n{global}", module = module.css), true).unwrap();
        assert!(combined.starts_with("@import \"https://f.test/a.css\""), "{combined}");
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
