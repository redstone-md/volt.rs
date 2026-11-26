//! `volt.toml` configuration shared by the runtime and the CLI.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

pub const CONFIG_FILE: &str = "volt.toml";
pub const OUT_DIR: &str = ".volt";

static DEV: OnceLock<bool> = OnceLock::new();

/// True when running under `volt dev`.
pub fn is_dev() -> bool {
    *DEV.get_or_init(|| std::env::var("VOLT_DEV").is_ok_and(|v| v == "1"))
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
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self { host: "0.0.0.0".into(), port: 3000, workers: 0, memory_limit_mb: 256, compress: true }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BuildConfig {
    pub minify: bool,
    pub sourcemap: bool,
    /// Static prerendering of routes without loaders or dynamic segments.
    pub prerender: bool,
    pub aliases: BTreeMap<String, String>,
    pub define: BTreeMap<String, String>,
}

impl Default for BuildConfig {
    fn default() -> Self {
        Self { minify: true, sourcemap: false, prerender: true, aliases: BTreeMap::new(), define: BTreeMap::new() }
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
    pub sizes: Vec<u32>,
    pub quality: u8,
}

impl Default for ImageConfig {
    fn default() -> Self {
        Self { sizes: vec![384, 640, 828, 1080, 1200, 1920, 2048], quality: 75 }
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
}
