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
    pub i18n: I18nConfig,
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
    /// Largest accepted request body (actions, API routes, uploads) in megabytes.
    pub body_limit_mb: usize,
    /// Streams dynamic pages: the shell first, then each Suspense boundary as it resolves.
    /// `false` renders every page to a complete string with `renderToString`.
    pub streaming: bool,
    /// Case-insensitive regular expression of user agents that get the complete document instead of a stream.
    pub bots: String,
    /// Hosts that `fetch` in server code may reach: `api.example.com`, `*.example.com`, `localhost:4000`,
    /// or `*` for every host. Empty allows every host that resolves to a public address.
    pub fetch_allow: Vec<String>,
    /// Time limit of one `fetch` in server code, in seconds.
    pub fetch_timeout_secs: u64,
}

/// Crawlers and link-preview fetchers that should see the whole page in one response.
pub const DEFAULT_BOTS: &str = r"(bot|crawler|spider|crawling)\b|slurp|mediapartners|facebookexternalhit|facebookcatalog|embedly|quora link preview|outbrain|pinterest|vkshare|w3c_validator|whatsapp|skypeuripreview|bingpreview|ia_archiver|yandex|baiduspider|lighthouse|google-inspectiontool";

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".into(),
            port: 3000,
            workers: 0,
            memory_limit_mb: 256,
            compress: true,
            page_cache_mb: 50,
            body_limit_mb: 8,
            streaming: true,
            bots: DEFAULT_BOTS.into(),
            fetch_allow: Vec::new(),
            fetch_timeout_secs: 10,
        }
    }
}

impl ServerConfig {
    pub fn body_limit(&self) -> usize {
        self.body_limit_mb.saturating_mul(1024 * 1024)
    }

    /// The compiled `bots` pattern; an empty pattern matches no user agent.
    pub fn bot_pattern(&self) -> Result<Option<regex::Regex>, ConfigError> {
        if self.bots.trim().is_empty() {
            return Ok(None);
        }
        regex::RegexBuilder::new(&self.bots)
            .case_insensitive(true)
            .build()
            .map(Some)
            .map_err(|e| ConfigError(format!("server.bots is not a valid regular expression: {e}")))
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
    /// Modern output formats in order of preference, picked by the request's `Accept` header:
    /// `image/avif` and `image/webp`. Empty keeps the source format.
    pub formats: Vec<String>,
    /// Widths of `srcset` candidates for full-width layouts (`sizes` in `vw`, `fill`, no `width`).
    pub device_sizes: Vec<u32>,
    /// Widths of `srcset` candidates for fixed-size images; joined with `device_sizes`.
    pub image_sizes: Vec<u32>,
    /// Remote URLs the optimizer may fetch; anything else is refused.
    pub remote_patterns: Vec<RemotePattern>,
    /// Seconds a fetched remote original is reused before it is downloaded again, and the `max-age` of
    /// optimized remote images.
    pub minimum_cache_ttl: u64,
    /// Serves SVG sources through the optimizer (unchanged, under `content_security_policy`).
    pub dangerously_allow_svg: bool,
    /// Lets remote hosts resolve to loopback, private and link-local addresses.
    pub dangerously_allow_local_ip: bool,
    /// `Content-Security-Policy` of every optimizer response, so an SVG opened directly cannot run scripts.
    pub content_security_policy: String,
    /// `inline` or `attachment` for the `Content-Disposition` of optimizer responses.
    pub content_disposition_type: String,
    /// Largest accepted source in pixels (width times height); larger images are refused before decoding.
    pub max_pixels: u64,
    /// Largest accepted source width or height in pixels.
    pub max_dimension: u32,
}

/// Formats `[images] formats` accepts.
pub const IMAGE_FORMATS: [&str; 2] = ["image/avif", "image/webp"];

impl Default for ImageConfig {
    fn default() -> Self {
        Self {
            optimize: true,
            concurrency: 0,
            formats: vec!["image/webp".into()],
            device_sizes: vec![640, 750, 828, 1080, 1200, 1920, 2048, 3840],
            image_sizes: vec![16, 32, 48, 64, 96, 128, 256, 384],
            remote_patterns: Vec::new(),
            minimum_cache_ttl: 14_400,
            dangerously_allow_svg: false,
            dangerously_allow_local_ip: false,
            content_security_policy: "script-src 'none'; frame-src 'none'; sandbox;".into(),
            content_disposition_type: "inline".into(),
            max_pixels: 50_000_000,
            max_dimension: 16_384,
        }
    }
}

impl ImageConfig {
    /// Every width `/_volt/image` accepts, ascending: `image_sizes` and `device_sizes` joined.
    pub fn widths(&self) -> Vec<u32> {
        let mut widths: Vec<u32> = self.image_sizes.iter().chain(&self.device_sizes).copied().collect();
        widths.sort_unstable();
        widths.dedup();
        widths
    }

    /// Whether a remote URL matches one of `remote_patterns`.
    pub fn allows_remote(&self, url: &RemoteUrl) -> bool {
        self.remote_patterns.iter().any(|pattern| pattern.matches(url))
    }

    fn validate(&mut self) -> Result<(), String> {
        if let Some(bad) = self.formats.iter().find(|f| !IMAGE_FORMATS.contains(&f.as_str())) {
            return Err(format!("images.formats: `{bad}` is not one of {IMAGE_FORMATS:?}"));
        }
        for (name, sizes) in [("device_sizes", &mut self.device_sizes), ("image_sizes", &mut self.image_sizes)] {
            if let Some(bad) = sizes.iter().find(|w| !(1..=10_000).contains(*w)) {
                return Err(format!("images.{name}: {bad} is not a width between 1 and 10000"));
            }
            sizes.sort_unstable();
            sizes.dedup();
        }
        if self.device_sizes.is_empty() {
            return Err("images.device_sizes must list at least one width".into());
        }
        if !matches!(self.content_disposition_type.as_str(), "inline" | "attachment") {
            return Err("images.content_disposition_type must be `inline` or `attachment`".into());
        }
        for (i, pattern) in self.remote_patterns.iter_mut().enumerate() {
            pattern.validate().map_err(|e| format!("images.remote_patterns[{i}]: {e}"))?;
        }
        Ok(())
    }
}

/// One `[[images.remote_patterns]]` entry, matched like Next.js `remotePatterns`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemotePattern {
    /// `http` or `https`; either when omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol: Option<String>,
    /// Host name; `*.example.com` matches one subdomain level, `**.example.com` any number.
    pub hostname: String,
    /// Exact port; any port when omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// Path glob: `*` matches within a segment, `**` across segments. Every path when omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pathname: Option<String>,
    /// Exact query string including `?`; any query when omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search: Option<String>,
}

impl RemotePattern {
    fn validate(&mut self) -> Result<(), String> {
        if let Some(protocol) = &mut self.protocol {
            *protocol = protocol.trim_end_matches(':').to_ascii_lowercase();
            if !matches!(protocol.as_str(), "http" | "https") {
                return Err(format!("protocol `{protocol}` must be `http` or `https`"));
            }
        }
        self.hostname = self.hostname.to_ascii_lowercase();
        if self.hostname.is_empty() || self.hostname.contains(['/', ':', '@']) {
            return Err(format!("hostname `{}` is not a host name", self.hostname));
        }
        if self.pathname.as_deref().is_some_and(|p| !p.starts_with('/')) {
            return Err("pathname must start with `/`".into());
        }
        Ok(())
    }

    /// Whether `url` is covered by this pattern.
    pub fn matches(&self, url: &RemoteUrl) -> bool {
        self.protocol.as_deref().is_none_or(|p| p == url.protocol)
            && self.port.is_none_or(|p| Some(p) == url.port)
            && glob_match(&self.hostname, &url.hostname, '.')
            && glob_match(self.pathname.as_deref().unwrap_or("**"), &url.pathname, '/')
            && self.search.as_deref().is_none_or(|s| s == url.search)
    }
}

/// The parts of an absolute `http(s)` URL that remote patterns look at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteUrl {
    pub protocol: String,
    pub hostname: String,
    pub port: Option<u16>,
    pub pathname: String,
    /// The query including `?`, or empty.
    pub search: String,
}

impl RemoteUrl {
    /// Parses `http(s)://host[:port]/path?query`. Credentials, IPv6 literals, backslashes and dot segments are
    /// rejected, so the URL that was matched is exactly the one fetched.
    pub fn parse(url: &str) -> Option<Self> {
        let (protocol, rest) = url.split_once("://")?;
        let protocol = protocol.to_ascii_lowercase();
        if !matches!(protocol.as_str(), "http" | "https") {
            return None;
        }
        let rest = rest.split('#').next().unwrap_or(rest);
        if rest.contains(['\\', ' ']) {
            return None;
        }
        let (authority, tail) = rest.split_at(rest.find(['/', '?']).unwrap_or(rest.len()));
        let (hostname, port) = match authority.rsplit_once(':') {
            Some((host, port)) => (host, Some(port.parse::<u16>().ok()?)),
            None => (authority, None),
        };
        if hostname.is_empty() || !hostname.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')) {
            return None;
        }
        let (pathname, search) = tail.split_at(tail.find('?').unwrap_or(tail.len()));
        if pathname.split('/').any(|segment| matches!(segment, "." | "..")) {
            return None;
        }
        Some(Self {
            protocol,
            hostname: hostname.to_ascii_lowercase(),
            port,
            pathname: if pathname.is_empty() { "/".into() } else { pathname.into() },
            search: if search == "?" { String::new() } else { search.into() },
        })
    }

    /// The URL to fetch, rebuilt from the parsed parts.
    pub fn href(&self) -> String {
        let port = self.port.map(|p| format!(":{p}")).unwrap_or_default();
        format!("{}://{}{port}{}{}", self.protocol, self.hostname, self.pathname, self.search)
    }
}

/// Glob match where `**` spans separators and `*` stays within one part.
fn glob_match(pattern: &str, text: &str, separator: char) -> bool {
    let mut expression = String::from("^");
    let mut rest = pattern;
    while let Some(c) = rest.chars().next() {
        if let Some(tail) = rest.strip_prefix("**") {
            expression.push_str(".*");
            rest = tail;
        } else if let Some(tail) = rest.strip_prefix('*') {
            expression.push_str(&format!("[^{}]*", regex::escape(&separator.to_string())));
            rest = tail;
        } else {
            expression.push_str(&regex::escape(&c.to_string()));
            rest = &rest[c.len_utf8()..];
        }
    }
    expression.push('$');
    regex::Regex::new(&expression).is_ok_and(|re| re.is_match(text))
}

/// `[i18n]`: locale-prefixed routing. The default locale is served without a prefix.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct I18nConfig {
    /// Supported locales, e.g. `["en", "de"]`. Empty disables internationalized routing.
    pub locales: Vec<String>,
    /// Locale of unprefixed URLs; the first locale when left empty.
    pub default_locale: String,
    /// Redirects `/` to the visitor's preferred locale (cookie, then `Accept-Language`).
    pub locale_detection: bool,
}

impl Default for I18nConfig {
    fn default() -> Self {
        Self { locales: Vec::new(), default_locale: String::new(), locale_detection: true }
    }
}

/// Cookies holding a visitor's chosen locale, read in this order; the client runtime writes the first.
pub const LOCALE_COOKIES: [&str; 2] = ["VOLT_LOCALE", "NEXT_LOCALE"];

impl I18nConfig {
    pub fn is_enabled(&self) -> bool {
        !self.locales.is_empty()
    }

    fn validate(&mut self) -> Result<(), String> {
        if !self.is_enabled() {
            return Ok(());
        }
        if let Some(bad) =
            self.locales.iter().find(|l| l.is_empty() || !l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
        {
            return Err(format!("i18n.locales: `{bad}` is not a locale such as `en` or `pt-BR`"));
        }
        // Prefixes match case-insensitively, so `en` and `EN` would make the second unreachable.
        if let Some((i, twin)) = self
            .locales
            .iter()
            .enumerate()
            .find_map(|(i, l)| self.locales[..i].iter().any(|seen| seen.eq_ignore_ascii_case(l)).then_some((i, l)))
        {
            return Err(format!("i18n.locales: `{twin}` is listed twice (entry {})", i + 1));
        }
        if self.default_locale.is_empty() {
            self.default_locale = self.locales[0].clone();
        }
        if !self.locales.contains(&self.default_locale) {
            return Err(format!("i18n.default_locale `{}` is not listed in i18n.locales", self.default_locale));
        }
        Ok(())
    }

    /// The configured spelling of `candidate`, compared case-insensitively.
    pub fn find(&self, candidate: &str) -> Option<&str> {
        self.locales.iter().find(|l| l.eq_ignore_ascii_case(candidate)).map(String::as_str)
    }

    /// Splits an app path into its locale prefix (if any) and the path below it:
    /// `/de/about` is `(Some("de"), "/about")`, `/de` is `(Some("de"), "/")`.
    pub fn split<'a>(&self, path: &'a str) -> (Option<&str>, &'a str) {
        let rest = path.strip_prefix('/').unwrap_or(path);
        let (first, tail) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
        match self.find(first).filter(|_| !first.is_empty()) {
            Some(locale) => (Some(locale), if tail.is_empty() { "/" } else { tail }),
            None => (None, path),
        }
    }

    /// App path of `path` in `locale`: prefixed unless `locale` is the default.
    /// Leading slashes and backslashes collapse to one `/`, so a tail such as `//host` taken from
    /// `/en//host` can never become a protocol-relative URL to another host.
    pub fn localize(&self, locale: &str, path: &str) -> String {
        let path = format!("/{}", path.trim_start_matches(['/', '\\']));
        if !self.is_enabled() || locale == self.default_locale {
            return path;
        }
        match path.as_str() {
            "/" => format!("/{locale}"),
            _ => format!("/{locale}{path}"),
        }
    }

    /// Prefixes a local redirect target with `locale`, leaving external URLs, internal endpoints
    /// and paths that already name a locale alone.
    pub fn localize_href(&self, locale: &str, href: &str) -> String {
        let local = href.starts_with('/') && !href.starts_with("//") && !href.starts_with("/\\");
        if !self.is_enabled() || !local || href.starts_with("/_volt/") || self.split(href).0.is_some() {
            return href.to_owned();
        }
        let (path, rest) = href.split_at(href.find(['?', '#']).unwrap_or(href.len()));
        format!("{}{rest}", self.localize(locale, path))
    }

    /// `(locale, app path)` of every locale variant of an unprefixed path, the default locale first.
    pub fn variants(&self, path: &str) -> Vec<(&str, String)> {
        let default = std::iter::once(self.default_locale.as_str());
        let others = self.locales.iter().map(String::as_str).filter(|l| *l != self.default_locale);
        default.chain(others).map(|locale| (locale, self.localize(locale, path))).collect()
    }

    /// Best supported locale for an `Accept-Language` header, in the header's quality order: each range
    /// takes its exact tag, else its primary language (`de-AT` and `en-US` pick `de` and `en` over a
    /// sibling region), else a locale sharing that language (`pt` picks `pt-BR`).
    pub fn negotiate(&self, accept_language: &str) -> Option<&str> {
        let mut ranges: Vec<(&str, f32)> = accept_language
            .split(',')
            .filter_map(|part| {
                let mut pieces = part.split(';');
                let tag = pieces.next()?.trim();
                let quality = pieces
                    .find_map(|p| p.trim().strip_prefix("q="))
                    .map_or(Some(1.0), |q| q.trim().parse::<f32>().ok())?;
                (!tag.is_empty() && tag != "*" && quality > 0.0).then_some((tag, quality))
            })
            .collect();
        ranges.sort_by(|a, b| b.1.total_cmp(&a.1));
        let primary = |tag: &str| tag.split('-').next().unwrap_or(tag).to_ascii_lowercase();
        ranges.iter().find_map(|(tag, _)| {
            let language = primary(tag);
            self.find(tag)
                .or_else(|| self.find(&language))
                .or_else(|| self.locales.iter().find(|l| primary(l) == language).map(String::as_str))
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Redirect {
    pub source: String,
    pub destination: String,
    #[serde(default)]
    pub permanent: bool,
    /// With `[i18n]`, the source also matches below every locale prefix and the destination keeps
    /// that locale. `false` matches the full path as written.
    #[serde(default = "matches_locales")]
    pub locale: bool,
}

fn matches_locales() -> bool {
    true
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
        config.server.bot_pattern().map_err(|e| ConfigError(format!("{}: {e}", path.display())))?;
        config.i18n.validate().map_err(|e| ConfigError(format!("{}: {e}", path.display())))?;
        config.images.validate().map_err(|e| ConfigError(format!("{}: {e}", path.display())))?;
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

    /// Locale of pages served without a locale prefix: `[i18n] default_locale`, else `[site] language`.
    pub fn default_locale(&self) -> &str {
        if self.i18n.is_enabled() { &self.i18n.default_locale } else { &self.site.language }
    }

    /// Splits a localized app path into its locale (the default one when unprefixed) and the route path.
    pub fn split_locale<'a>(&self, path: &'a str) -> (&str, &'a str) {
        match self.i18n.split(path) {
            (Some(locale), rest) => (locale, rest),
            (None, rest) => (self.default_locale(), rest),
        }
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

    fn i18n() -> I18nConfig {
        let mut i18n = I18nConfig { locales: vec!["en".into(), "de".into(), "pt-BR".into()], ..I18nConfig::default() };
        i18n.validate().unwrap();
        i18n
    }

    #[test]
    fn splits_and_localizes_paths() {
        let i18n = i18n();
        assert_eq!(i18n.default_locale, "en");
        assert_eq!(i18n.split("/de/about"), (Some("de"), "/about"));
        assert_eq!(i18n.split("/de"), (Some("de"), "/"));
        assert_eq!(i18n.split("/pt-br/x"), (Some("pt-BR"), "/x"));
        assert_eq!(i18n.split("/design"), (None, "/design"));
        assert_eq!(i18n.split("/"), (None, "/"));
        assert_eq!(i18n.localize("de", "/"), "/de");
        assert_eq!(i18n.localize("en", "/a"), "/a");
        assert_eq!(i18n.localize_href("de", "/login?next=1"), "/de/login?next=1");
        assert_eq!(i18n.localize_href("de", "/en/a"), "/en/a");
        assert_eq!(i18n.localize_href("de", "https://x.dev/"), "https://x.dev/");
        let variants: Vec<String> = i18n.variants("/a").into_iter().map(|(_, p)| p).collect();
        assert_eq!(variants, ["/a", "/de/a", "/pt-BR/a"]);
        assert_eq!(I18nConfig::default().localize("de", "/a"), "/a");
    }

    #[test]
    fn localized_paths_stay_on_the_same_host() {
        let i18n = i18n();
        let (locale, tail) = i18n.split("/en//evil.com");
        assert_eq!((locale, tail), (Some("en"), "//evil.com"));
        assert_eq!(i18n.localize("en", tail), "/evil.com");
        assert_eq!(i18n.localize("en", "/\\evil.com"), "/evil.com");
        assert_eq!(i18n.localize("en", "/\\/evil.com/x"), "/evil.com/x");
        assert_eq!(i18n.localize("de", "//evil.com"), "/de/evil.com");
    }

    #[test]
    fn rejects_duplicate_locales() {
        for locales in [["en", "EN"], ["de", "de"]] {
            let mut i18n = I18nConfig { locales: locales.map(String::from).to_vec(), ..I18nConfig::default() };
            assert!(i18n.validate().is_err(), "{locales:?}");
        }
    }

    #[test]
    fn negotiates_accept_language() {
        let i18n = i18n();
        assert_eq!(i18n.negotiate("fr;q=0.9, de-AT;q=0.8, en;q=0.5"), Some("de"));
        assert_eq!(i18n.negotiate("de;q=0.2, en"), Some("en"));
        assert_eq!(i18n.negotiate("pt"), Some("pt-BR"));
        assert_eq!(i18n.negotiate("fr, *"), None);
        assert_eq!(i18n.negotiate("de;q=0"), None);
        let regions = I18nConfig { locales: vec!["en-GB".into(), "en".into()], ..I18nConfig::default() };
        assert_eq!(regions.negotiate("en-US, en;q=0.9"), Some("en"));
        assert_eq!(regions.negotiate("en-GB, en;q=0.9"), Some("en-GB"));
        let mut bad = I18nConfig { locales: vec!["en".into()], default_locale: "de".into(), ..I18nConfig::default() };
        assert!(bad.validate().is_err());
    }

    fn pattern(hostname: &str, pathname: Option<&str>) -> RemotePattern {
        let mut pattern = RemotePattern {
            protocol: Some("https:".into()),
            hostname: hostname.into(),
            port: None,
            pathname: pathname.map(String::from),
            search: None,
        };
        pattern.validate().unwrap();
        pattern
    }

    fn url(raw: &str) -> RemoteUrl {
        RemoteUrl::parse(raw).unwrap_or_else(|| panic!("{raw} should parse"))
    }

    #[test]
    fn matches_remote_patterns() {
        let exact = pattern("cdn.example.com", Some("/img/**"));
        assert!(exact.matches(&url("https://cdn.example.com/img/a/b.png")));
        assert!(exact.matches(&url("https://CDN.example.com/img/a.png?v=1")));
        assert!(!exact.matches(&url("http://cdn.example.com/img/a.png")));
        assert!(!exact.matches(&url("https://cdn.example.com/other/a.png")));
        assert!(!exact.matches(&url("https://cdn.example.com.evil.dev/img/a.png")));
        let one = pattern("*.example.com", Some("/*.png"));
        assert!(one.matches(&url("https://a.example.com/x.png")));
        assert!(!one.matches(&url("https://a.b.example.com/x.png")));
        assert!(!one.matches(&url("https://a.example.com/dir/x.png")));
        assert!(!one.matches(&url("https://example.com/x.png")));
        let any = pattern("**.example.com", None);
        assert!(any.matches(&url("https://a.b.example.com/deep/x.png")));
        let mut port = pattern("localhost", None);
        port.port = Some(8080);
        assert!(port.matches(&url("https://localhost:8080/a")));
        assert!(!port.matches(&url("https://localhost:9090/a")));
        assert_eq!(url("https://Localhost:8080/a?b#c").href(), "https://localhost:8080/a?b");
    }

    #[test]
    fn rejects_ambiguous_remote_urls() {
        for bad in [
            "https://user@cdn.example.com/a.png",
            "https://cdn.example.com@evil.dev/a.png",
            "ftp://cdn.example.com/a.png",
            "https:///a.png",
            "https://cdn.example.com:99999/a.png",
            "https://cdn.example.com/a/../b.png",
            "https://cdn.example.com\\@evil.dev/",
            "//cdn.example.com/a.png",
        ] {
            assert!(RemoteUrl::parse(bad).is_none(), "{bad}");
        }
        assert_eq!(url("https://cdn.example.com").pathname, "/");
    }

    #[test]
    fn validates_image_config() {
        let mut images = ImageConfig { formats: vec!["image/jxl".into()], ..ImageConfig::default() };
        assert!(images.validate().is_err());
        let mut images = ImageConfig { device_sizes: vec![1200, 640, 640], ..ImageConfig::default() };
        images.validate().unwrap();
        assert_eq!(images.device_sizes, [640, 1200]);
        assert_eq!(images.widths().first(), Some(&16));
        let mut images = ImageConfig::default();
        images.remote_patterns.push(RemotePattern {
            protocol: Some("ftp".into()),
            hostname: "x".into(),
            port: None,
            pathname: None,
            search: None,
        });
        assert!(images.validate().is_err());
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
