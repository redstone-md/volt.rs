//! `volt.toml` configuration shared by the runtime and the CLI.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

pub const CONFIG_FILE: &str = "volt.toml";
pub const OUT_DIR: &str = ".volt";

static DEV: OnceLock<bool> = OnceLock::new();
static EXPORT: OnceLock<bool> = OnceLock::new();

/// True when running under `volt dev`.
pub fn is_dev() -> bool {
    *DEV.get_or_init(|| std::env::var("VOLT_DEV").is_ok_and(|v| v == "1"))
}

/// True while `volt export` builds a static site.
pub fn is_export() -> bool {
    *EXPORT.get_or_init(|| std::env::var("VOLT_EXPORT").is_ok_and(|v| v == "1"))
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub site: SiteConfig,
    pub server: ServerConfig,
    pub build: BuildConfig,
    pub seo: SeoConfig,
    pub images: ImageConfig,
    pub headers: BTreeMap<String, String>,
    pub redirects: Vec<Redirect>,
    pub rewrites: Vec<Rewrite>,
    #[serde(skip)]
    pub root: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SiteConfig {
    pub name: String,
    /// Canonical origin, e.g. `https://example.com`. Used for sitemaps, canonical and Open Graph URLs.
    pub url: String,
    pub language: String,
}

impl Default for SiteConfig {
    fn default() -> Self {
        Self { name: "volt app".into(), url: "http://localhost:3000".into(), language: "en".into() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    /// QuickJS workers rendering React. `0` picks the CPU count.
    pub workers: usize,
    /// Per-worker heap limit in megabytes.
    pub memory_limit_mb: usize,
    pub compress: bool,
    /// Memory budget in megabytes for pages cached on demand; build-time pages are not counted.
    pub page_cache_mb: usize,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self { host: "0.0.0.0".into(), port: 3000, workers: 0, memory_limit_mb: 256, compress: true, page_cache_mb: 50 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BuildConfig {
    pub minify: bool,
    pub sourcemap: bool,
    /// Prerendering of routes without loaders, including `generateStaticParams` paths.
    pub prerender: bool,
    pub aliases: BTreeMap<String, String>,
    pub define: BTreeMap<String, String>,
    /// URL prefix the app is mounted under, e.g. `/docs`.
    pub base_path: String,
    /// `true` serves `/about/`, `false` serves `/about`; the other form redirects with 308.
    pub trailing_slash: bool,
    /// Output directory of `volt export`, relative to the project root.
    pub export_dir: String,
}

impl Default for BuildConfig {
    fn default() -> Self {
        Self {
            minify: true,
            sourcemap: false,
            prerender: true,
            aliases: BTreeMap::new(),
            define: BTreeMap::new(),
            base_path: String::new(),
            trailing_slash: false,
            export_dir: "out".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SeoConfig {
    pub sitemap: bool,
    pub robots: bool,
    pub llms: bool,
    pub disallow: Vec<String>,
}

impl Default for SeoConfig {
    fn default() -> Self {
        Self { sitemap: true, robots: true, llms: true, disallow: Vec::new() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ImageConfig {
    /// Serve `/_volt/image` resizing for `<Image>`.
    pub optimize: bool,
    /// Concurrent resize jobs. `0` picks the CPU count.
    pub concurrency: usize,
}

impl Default for ImageConfig {
    fn default() -> Self {
        Self { optimize: true, concurrency: 0 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Redirect {
    pub source: String,
    pub destination: String,
    #[serde(default)]
    pub permanent: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rewrite {
    pub source: String,
    pub destination: String,
}

#[derive(Debug)]
pub struct ConfigError(pub String);

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ConfigError {}

impl Config {
    /// Loads `volt.toml` from `root`, falling back to defaults, then applies env overrides.
    pub fn load(root: &Path) -> Result<Self, ConfigError> {
        let path = root.join(CONFIG_FILE);
        let mut config: Self = match std::fs::read_to_string(&path) {
            Ok(text) => toml::from_str(&text).map_err(|e| ConfigError(format!("{}: {e}", path.display())))?,
            Err(_) => Self::default(),
        };
        config.root = root.to_path_buf();
        config.build.base_path = normalize_base_path(&config.build.base_path)
            .ok_or_else(|| ConfigError(format!("{}: build.base_path must start with `/`", path.display())))?;
        if !is_subdirectory(&config.build.export_dir) {
            return Err(ConfigError(format!(
                "{}: build.export_dir must be a relative path inside the project, without `..`",
                path.display()
            )));
        }
        config.apply_env();
        Ok(config)
    }

    fn apply_env(&mut self) {
        if let Some(port) = std::env::var("PORT").ok().and_then(|p| p.parse().ok()) {
            self.server.port = port;
        }
        if let Ok(host) = std::env::var("HOST") {
            self.server.host = host;
        }
        if let Ok(url) = std::env::var("VOLT_SITE_URL") {
            self.site.url = url;
        }
    }

    pub fn out_dir(&self) -> PathBuf {
        std::env::var_os("VOLT_OUT_DIR").map(PathBuf::from).unwrap_or_else(|| self.root.join(OUT_DIR))
    }

    pub fn public_dir(&self) -> PathBuf {
        self.root.join("public")
    }

    pub fn workers(&self) -> usize {
        match self.server.workers {
            0 => std::thread::available_parallelism().map(usize::from).unwrap_or(2),
            n => n,
        }
    }

    pub fn site_url(&self) -> &str {
        self.site.url.trim_end_matches('/')
    }

    pub fn export_dir(&self) -> PathBuf {
        self.root.join(&self.build.export_dir)
    }

    /// `[build] base_path` without a trailing slash; empty when the app is mounted at `/`.
    pub fn base_path(&self) -> &str {
        &self.build.base_path
    }

    /// URL prefix of hashed build assets, including the base path.
    pub fn static_prefix(&self) -> String {
        format!("{}{}", self.base_path(), crate::manifest::STATIC_PREFIX)
    }

    /// Turns a request path into an app path: strips the base path and any trailing slash.
    /// Returns `None` for paths outside the base path.
    pub fn app_path<'a>(&self, path: &'a str) -> Option<&'a str> {
        let base = self.base_path();
        let path = match path.strip_prefix(base) {
            Some(rest) if base.is_empty() || rest.is_empty() || rest.starts_with('/') => rest,
            _ => return None,
        };
        let trimmed = path.trim_end_matches('/');
        Some(if trimmed.is_empty() { "/" } else { trimmed })
    }

    /// App path that `request_path` should redirect to under `trailing_slash`, or `None` when it
    /// is already canonical. Leading slashes and backslashes collapse to one `/`, so the result
    /// can never be read as a protocol-relative URL to another host.
    pub fn canonical_path(&self, request_path: &str) -> Option<String> {
        let app = self.app_path(request_path)?;
        let local = format!("/{}", app.trim_start_matches(['/', '\\']));
        (self.public_path(&local) != request_path).then_some(local)
    }

    /// Public URL of a local href: prefixes the base path and applies the trailing slash policy.
    /// External URLs and relative hrefs are returned unchanged.
    pub fn public_path(&self, href: &str) -> String {
        if !href.starts_with('/') || href.starts_with("//") {
            return href.to_owned();
        }
        let (path, rest) = href.split_at(href.find(['?', '#']).unwrap_or(href.len()));
        let trimmed = path.trim_end_matches('/');
        let file = trimmed.rsplit('/').next().is_some_and(|last| last.contains('.'));
        let path = if trimmed.is_empty() {
            if self.build.trailing_slash || self.base_path().is_empty() { "/" } else { "" }.to_owned()
        } else if self.build.trailing_slash && !file {
            format!("{trimmed}/")
        } else {
            trimmed.to_owned()
        };
        format!("{}{path}{rest}", self.base_path())
    }
}

/// `volt export` empties this directory, so it may only name a directory below the project root.
fn is_subdirectory(raw: &str) -> bool {
    let path = Path::new(raw);
    path.components().any(|c| matches!(c, Component::Normal(_)))
        && path.components().all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

fn normalize_base_path(raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Some(String::new());
    }
    trimmed.starts_with('/').then(|| trimmed.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(base: &str, trailing_slash: bool) -> Config {
        let mut config = Config::default();
        config.build.base_path = base.into();
        config.build.trailing_slash = trailing_slash;
        config
    }

    #[test]
    fn maps_app_paths() {
        let plain = config("", false);
        assert_eq!(plain.app_path("/"), Some("/"));
        assert_eq!(plain.app_path("/about/"), Some("/about"));
        let docs = config("/docs", false);
        assert_eq!(docs.app_path("/docs"), Some("/"));
        assert_eq!(docs.app_path("/docs/"), Some("/"));
        assert_eq!(docs.app_path("/docs/a/b"), Some("/a/b"));
        assert_eq!(docs.app_path("/docsx"), None);
        assert_eq!(docs.app_path("/"), None);
    }

    #[test]
    fn canonical_paths_stay_on_the_same_host() {
        let plain = config("", false);
        assert_eq!(plain.canonical_path("/about"), None);
        assert_eq!(plain.canonical_path("/about/").as_deref(), Some("/about"));
        assert_eq!(plain.canonical_path("//evil.com/").as_deref(), Some("/evil.com"));
        assert_eq!(plain.canonical_path("//evil.com").as_deref(), Some("/evil.com"));
        assert_eq!(plain.canonical_path("/\\evil.com/").as_deref(), Some("/evil.com"));
        assert_eq!(plain.canonical_path("/\\/evil.com").as_deref(), Some("/evil.com"));
        let docs = config("/docs", true);
        assert_eq!(docs.canonical_path("/docs/a/"), None);
        assert_eq!(docs.canonical_path("/docs//evil.com/").as_deref(), Some("/evil.com"));
        assert_eq!(docs.public_path("/evil.com"), "/docs/evil.com");
        assert_eq!(docs.canonical_path("/other/"), None);
    }

    #[test]
    fn builds_public_paths() {
        let plain = config("", false);
        assert_eq!(plain.public_path("/"), "/");
        assert_eq!(plain.public_path("/about/?x=1"), "/about?x=1");
        assert_eq!(plain.public_path("https://x.dev/a/"), "https://x.dev/a/");
        let docs = config("/docs", true);
        assert_eq!(docs.public_path("/"), "/docs/");
        assert_eq!(docs.public_path("/a#top"), "/docs/a/#top");
        assert_eq!(docs.public_path("/feed.xml"), "/docs/feed.xml");
        assert_eq!(config("/docs", false).public_path("/"), "/docs");
        assert_eq!(normalize_base_path("/docs/").as_deref(), Some("/docs"));
        assert!(normalize_base_path("docs").is_none());
    }

    #[test]
    fn export_dir_stays_inside_the_project() {
        for ok in ["out", "./dist/site", "build/out"] {
            assert!(is_subdirectory(ok), "{ok}");
        }
        for bad in ["", ".", "..", "../..", "out/../..", "/home/user/other", "./"] {
            assert!(!is_subdirectory(bad), "{bad}");
        }
    }
}
