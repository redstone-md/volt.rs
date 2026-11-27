//! Build manifest written by `volt build` and read by the server.

use std::path::Path;

use serde::{Deserialize, Serialize};

pub const MANIFEST_FILE: &str = "manifest.json";
pub const STATIC_PREFIX: &str = "/_volt/static/";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub build_id: String,
    pub dev: bool,
    /// Server bundle path relative to the out dir.
    pub server: String,
    pub client: ClientAssets,
    pub routes: Vec<RouteEntry>,
    /// Set by `volt dev` when the last rebuild failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientAssets {
    pub entry: String,
    pub css: Vec<String>,
    pub preload: Vec<String>,
    /// Font files preloaded on every page (fonts declared by the root layout).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fonts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteEntry {
    /// Route pattern, e.g. `/blog/[slug]`.
    pub id: String,
    pub chunks: Vec<String>,
    /// Concrete paths known at build time: the id of a static route, or the paths returned by
    /// `generateStaticParams` / `getStaticPaths`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,
    /// The subset of `paths` written to `prerender/` by `volt build`, see [`prerender_file`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prerendered: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revalidate: Option<u64>,
    /// Rendered on every request instead of being served from the page cache.
    #[serde(default)]
    pub dynamic: bool,
    /// `false` when the route exports `dynamicParams = false`: paths outside `paths` answer 404.
    #[serde(default = "enabled", skip_serializing_if = "Clone::clone")]
    pub dynamic_params: bool,
    /// Rendered by the Pages Router: data requests also run `getServerSideProps` / `getStaticProps`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pages: bool,
    /// Pages Router `getStaticProps` page: HTML and props are cached per pathname and shared by every visitor.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub static_props: bool,
}

impl Default for RouteEntry {
    fn default() -> Self {
        Self {
            id: String::new(),
            chunks: Vec::new(),
            paths: Vec::new(),
            prerendered: Vec::new(),
            revalidate: None,
            dynamic: false,
            dynamic_params: true,
            pages: false,
            static_props: false,
        }
    }
}

fn enabled() -> bool {
    true
}

impl RouteEntry {
    /// Whether responses of this route are kept in the page cache.
    pub fn is_cached(&self) -> bool {
        !self.dynamic || self.revalidate.is_some()
    }

    /// Whether `path` is allowed by `dynamicParams`.
    pub fn accepts(&self, path: &str) -> bool {
        self.dynamic_params || self.paths.iter().any(|known| same_path(known, path))
    }
}

pub const PRERENDER_DIR: &str = "prerender";

/// Decoded file stem of a concrete path: `/` is `index`, `/blog/a%20b` is `blog/a b`.
/// Returns `None` for paths that would escape the output directory.
pub fn file_stem(path: &str) -> Option<String> {
    let decoded = percent_encoding::percent_decode_str(path).decode_utf8().ok()?;
    let parts: Vec<&str> = decoded.split('/').filter(|s| !s.is_empty()).collect();
    if parts.iter().any(|p| *p == "." || *p == ".." || p.contains('\\') || p.contains('\0')) {
        return None;
    }
    Some(if parts.is_empty() { "index".to_owned() } else { parts.join("/") })
}

/// Prerendered file of `path` relative to the out dir, `extension` being `html` or `json`.
pub fn prerender_file(path: &str, extension: &str) -> Option<String> {
    file_stem(path).map(|stem| format!("{PRERENDER_DIR}/{stem}.{extension}"))
}

/// Compares two URL paths after percent-decoding and trimming trailing slashes.
pub fn same_path(a: &str, b: &str) -> bool {
    let normalize = |p: &str| {
        let decoded = percent_encoding::percent_decode_str(p).decode_utf8_lossy().into_owned();
        let trimmed = decoded.trim_end_matches('/').to_owned();
        if trimmed.is_empty() { "/".to_owned() } else { trimmed }
    };
    normalize(a) == normalize(b)
}

impl Manifest {
    pub fn load(out_dir: &Path) -> std::io::Result<Self> {
        let text = std::fs::read_to_string(out_dir.join(MANIFEST_FILE))?;
        serde_json::from_str(&text).map_err(std::io::Error::other)
    }

    pub fn save(&self, out_dir: &Path) -> std::io::Result<()> {
        let text = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(out_dir.join(MANIFEST_FILE), text)
    }

    pub fn route(&self, id: &str) -> Option<&RouteEntry> {
        self.routes.iter().find(|r| r.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_prerendered_files() {
        assert_eq!(prerender_file("/", "html").as_deref(), Some("prerender/index.html"));
        assert_eq!(prerender_file("/blog/a%20b/", "json").as_deref(), Some("prerender/blog/a b.json"));
        assert!(file_stem("/a/%2e%2e/b").is_none());
        assert!(same_path("/blog/a%20b/", "/blog/a b"));
    }

    #[test]
    fn enforces_dynamic_params() {
        let entry = RouteEntry { paths: vec!["/blog/a".into()], dynamic_params: false, ..RouteEntry::default() };
        assert!(entry.accepts("/blog/a/"));
        assert!(!entry.accepts("/blog/b"));
        let json = serde_json::to_string(&RouteEntry::default()).unwrap();
        assert!(!json.contains("dynamicParams"));
        let back: RouteEntry = serde_json::from_str(&json).unwrap();
        assert!(back.dynamic_params);
    }
}
