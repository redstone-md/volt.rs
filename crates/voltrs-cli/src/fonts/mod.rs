//! Build-time fonts: `volt-rs/font/google` and `volt-rs/font/local` (also `next/font/*`).
//!
//! Calls such as `Inter({ subsets: ["latin"] })` are replaced with `{ className, style, variable }` and an
//! import of a virtual stylesheet holding the generated `@font-face` rules and classes.

mod google;
mod source;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

use crate::media::Media;
use crate::util::hash;

pub use source::{Transformed, transform};

/// Prefix of the virtual stylesheet module generated for each font.
pub const FONT_MODULE: &str = "volt:font/";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Loader {
    Google,
    Local,
}

impl Loader {
    pub fn from_specifier(specifier: &str) -> Option<Self> {
        let path = specifier
            .strip_prefix("@next/")
            .or_else(|| specifier.strip_prefix("next/"))
            .or_else(|| specifier.strip_prefix("volt-rs/"))?;
        match path {
            "font/google" => Some(Self::Google),
            "font/local" => Some(Self::Local),
            _ => None,
        }
    }
}

/// A font call site: the loader, the Google family (for Google fonts), the options and the calling module.
#[derive(Debug, Clone)]
pub struct Request {
    pub loader: Loader,
    pub family: String,
    pub options: Value,
    pub importer: PathBuf,
}

/// A loaded font: its stylesheet, the object that replaces the call, and the files to preload.
#[derive(Debug)]
pub struct Font {
    pub css: String,
    pub object: String,
    pub preload: Vec<String>,
}

/// One `@font-face` rule before the family name is applied.
#[derive(Debug, Clone, PartialEq)]
pub struct Face {
    pub descriptors: Vec<(String, String)>,
    pub files: Vec<(String, String)>,
    pub preload: bool,
}

/// A `localFont` file with its per-file descriptors.
struct LocalSource {
    path: PathBuf,
    weight: Option<String>,
    style: Option<String>,
}

/// Loads fonts once per build session; the client and server bundles share one instance.
#[derive(Debug)]
pub struct Fonts {
    media: Media,
    cache: PathBuf,
    loaded: Mutex<HashMap<String, Arc<Font>>>,
}

fn strings(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(items)) => items.iter().filter_map(|v| text(Some(v))).collect(),
        Some(value) => text(Some(value)).into_iter().collect(),
        None => Vec::new(),
    }
}

fn text(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.as_i64().map(|i| i.to_string()).unwrap_or_else(|| n.to_string())),
        _ => None,
    }
}

/// Quotes family names that are not CSS generic keywords.
fn family_list(primary: &str, fallback: &[String]) -> String {
    let mut list = vec![format!("'{primary}'")];
    for name in fallback {
        let generic = name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
        list.push(if generic { name.clone() } else { format!("'{}'", name.trim_matches(['\'', '"'])) });
    }
    list.join(", ")
}

fn identifier(name: &str) -> String {
    name.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect()
}

impl Fonts {
    pub fn new(media: Media, cache: PathBuf) -> Self {
        Self { media, cache, loaded: Mutex::default() }
    }

    /// Loads (or reuses) a font and returns the key of its virtual stylesheet.
    pub fn load(&self, request: &Request) -> Result<String> {
        let key = self.key(request)?;
        let mut loaded = self.loaded.lock().map_err(|_| anyhow::anyhow!("font cache poisoned"))?;
        if !loaded.contains_key(&key) {
            let font = self.build(request, &key)?;
            loaded.insert(key.clone(), Arc::new(font));
        }
        Ok(key)
    }

    pub fn get(&self, key: &str) -> Option<Arc<Font>> {
        self.loaded.lock().ok()?.get(key).cloned()
    }

    fn local_sources(request: &Request) -> Result<Vec<LocalSource>> {
        let dir = request.importer.parent().unwrap_or(Path::new("."));
        match request.options.get("src") {
            Some(Value::String(path)) => Ok(vec![LocalSource { path: dir.join(path), weight: None, style: None }]),
            Some(Value::Array(items)) => items
                .iter()
                .map(|item| {
                    let path = text(item.get("path")).context("each `src` entry needs a `path`")?;
                    Ok(LocalSource {
                        path: dir.join(path),
                        weight: text(item.get("weight")),
                        style: text(item.get("style")),
                    })
                })
                .collect(),
            _ => bail!("localFont() needs `src`: a path or an array of {{ path, weight, style }}"),
        }
    }

    /// Stable identity of a font: the loader, family, options and the content of local files.
    fn key(&self, request: &Request) -> Result<String> {
        let mut identity = format!("{:?}|{}|{}", request.loader, request.family, request.options);
        if request.loader == Loader::Local {
            for LocalSource { path, .. } in Self::local_sources(request)? {
                let bytes = std::fs::read(&path).with_context(|| {
                    format!("font file {} (used in {}) not found", path.display(), request.importer.display())
                })?;
                identity.push_str(&hash(&bytes));
            }
        }
        Ok(hash(identity.as_bytes()))
    }

    fn faces(&self, request: &Request) -> Result<Vec<Face>> {
        let options = &request.options;
        let display = text(options.get("display")).unwrap_or_else(|| "swap".into());
        let preload = options.get("preload").and_then(Value::as_bool).unwrap_or(true);
        let mut faces = match request.loader {
            Loader::Google => {
                let fetcher = google::Google::new(&self.cache);
                let query = google::Query {
                    family: request.family.clone(),
                    weights: strings(options.get("weight")),
                    styles: strings(options.get("style")),
                    display: display.clone(),
                };
                let subsets = strings(options.get("subsets"));
                let mut faces = Vec::new();
                for face in fetcher.faces(&query)? {
                    let wanted = subsets.is_empty() || face.subset.as_ref().is_some_and(|s| subsets.contains(s));
                    if !wanted {
                        continue;
                    }
                    let mut files = Vec::new();
                    for (url, format) in &face.sources {
                        let (bytes, ext) = fetcher.file(url)?;
                        files.push((self.media.write(&identifier(&request.family), &ext, &bytes)?, format.clone()));
                    }
                    faces.push(Face { descriptors: face.descriptors, files, preload: preload && !subsets.is_empty() });
                }
                if faces.is_empty() {
                    bail!("Google font `{}` has no files for subsets {:?}", request.family, subsets);
                }
                faces
            }
            Loader::Local => {
                let weight = text(options.get("weight"));
                let style = text(options.get("style"));
                let mut faces = Vec::new();
                for LocalSource { path, weight: file_weight, style: file_style } in Self::local_sources(request)? {
                    let format = match path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).as_deref() {
                        Some("woff2") => "woff2",
                        Some("woff") => "woff",
                        Some("ttf") => "truetype",
                        Some("otf") => "opentype",
                        Some("eot") => "embedded-opentype",
                        _ => bail!("unsupported font file {}", path.display()),
                    };
                    let mut descriptors = vec![("font-display".to_owned(), display.clone())];
                    if let Some(weight) = file_weight.or_else(|| weight.clone()) {
                        descriptors.push(("font-weight".into(), weight));
                    }
                    if let Some(style) = file_style.or_else(|| style.clone()) {
                        descriptors.push(("font-style".into(), style));
                    }
                    let url = self.media.copy(&path)?;
                    faces.push(Face { descriptors, files: vec![(url, format.into())], preload });
                }
                faces
            }
        };
        if let Some(Value::Array(declarations)) = options.get("declarations") {
            for face in &mut faces {
                for declaration in declarations {
                    if let (Some(prop), Some(value)) = (text(declaration.get("prop")), text(declaration.get("value"))) {
                        face.descriptors.push((prop, value));
                    }
                }
            }
        }
        Ok(faces)
    }

    fn build(&self, request: &Request, key: &str) -> Result<Font> {
        let faces = self.faces(request)?;
        let options = &request.options;
        let base = match request.loader {
            Loader::Google => request.family.clone(),
            Loader::Local => {
                let first = Self::local_sources(request)?.into_iter().next().map(|source| source.path);
                first.and_then(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned())).unwrap_or("font".into())
            }
        };
        let family = format!("__{}_{}", identifier(&base), &key[..6]);
        let fallback = strings(options.get("fallback"));
        let families = family_list(&family, &fallback);

        let mut css = String::new();
        let mut preload = Vec::new();
        for face in &faces {
            css.push_str(&format!("@font-face {{\n  font-family: '{family}';\n"));
            for (name, value) in &face.descriptors {
                css.push_str(&format!("  {name}: {value};\n"));
            }
            let src: Vec<String> =
                face.files.iter().map(|(url, format)| format!("url({url}) format('{format}')")).collect();
            css.push_str(&format!("  src: {};\n}}\n", src.join(", ")));
            if face.preload {
                preload.extend(face.files.iter().map(|(url, _)| url.clone()));
            }
        }
        let weights = strings(options.get("weight"));
        let styles = strings(options.get("style"));
        let weight = (weights.len() == 1 && weights[0] != "variable").then(|| weights[0].clone());
        let style = (styles.len() == 1).then(|| styles[0].clone());

        let class_name = format!("__className_{key}");
        css.push_str(&format!(".{class_name} {{\n  font-family: {families};\n"));
        let mut style_object = json!({ "fontFamily": families });
        if let Some(weight) = &weight {
            css.push_str(&format!("  font-weight: {weight};\n"));
            style_object["fontWeight"] =
                weight.parse::<u32>().map(Value::from).unwrap_or_else(|_| weight.clone().into());
        }
        if let Some(style) = &style {
            css.push_str(&format!("  font-style: {style};\n"));
            style_object["fontStyle"] = style.clone().into();
        }
        css.push_str("}\n");
        let mut object = json!({ "className": class_name, "style": style_object });
        if let Some(variable) = text(options.get("variable")) {
            if !variable.starts_with("--") {
                bail!("font `variable` must be a CSS custom property such as \"--font-sans\", got {variable:?}");
            }
            let variable_class = format!("__variable_{key}");
            css.push_str(&format!(".{variable_class} {{\n  {variable}: {families};\n}}\n"));
            object["variable"] = variable_class.into();
        }
        preload.dedup();
        Ok(Font { css, object: object.to_string(), preload })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_loaders() {
        assert_eq!(Loader::from_specifier("next/font/google"), Some(Loader::Google));
        assert_eq!(Loader::from_specifier("volt-rs/font/local"), Some(Loader::Local));
        assert_eq!(Loader::from_specifier("@next/font/local"), Some(Loader::Local));
        assert_eq!(Loader::from_specifier("volt-rs/link"), None);
    }

    #[test]
    fn builds_local_fonts() {
        let dir = std::env::temp_dir().join(format!("volt-fonts-{}", std::process::id()));
        crate::util::reset_dir(&dir).unwrap();
        std::fs::write(dir.join("Brand.woff2"), b"font").unwrap();
        let fonts = Fonts::new(Media::new(dir.join("media"), voltrs::manifest::STATIC_PREFIX), dir.join("cache"));
        let request = Request {
            loader: Loader::Local,
            family: String::new(),
            options: json!({ "src": "./Brand.woff2", "variable": "--font-brand", "weight": "700", "fallback": ["Helvetica Neue", "sans-serif"] }),
            importer: dir.join("layout.tsx"),
        };
        let key = fonts.load(&request).unwrap();
        assert_eq!(fonts.load(&request).unwrap(), key);
        let font = fonts.get(&key).unwrap();
        assert!(font.css.contains("src: url(/_volt/static/media/Brand-"), "{}", font.css);
        assert!(font.css.contains("format('woff2')"));
        assert!(font.css.contains("--font-brand: '__Brand_"));
        assert!(font.css.contains("'Helvetica Neue', sans-serif"));
        let object: Value = serde_json::from_str(&font.object).unwrap();
        assert_eq!(object["style"]["fontWeight"], 700);
        assert!(object["variable"].as_str().unwrap().starts_with("__variable_"));
        assert_eq!(font.preload.len(), 1);
    }
}
