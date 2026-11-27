//! `sitemap.xml`, `robots.txt`, `llms.txt` and code-defined metadata routes.

use serde::{Deserialize, Deserializer, Serialize};

use crate::config::Config;
use crate::types::{Type, TypeScript};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ChangeFrequency {
    Always,
    Hourly,
    Daily,
    Weekly,
    Monthly,
    Yearly,
    Never,
}

impl ChangeFrequency {
    fn as_str(self) -> &'static str {
        match self {
            Self::Always => "always",
            Self::Hourly => "hourly",
            Self::Daily => "daily",
            Self::Weekly => "weekly",
            Self::Monthly => "monthly",
            Self::Yearly => "yearly",
            Self::Never => "never",
        }
    }
}

/// One `<url>` of the sitemap. `url` may be a path; it is resolved against `site.url`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SitemapEntry {
    pub url: String,
    #[serde(default)]
    pub last_modified: Option<String>,
    #[serde(default)]
    pub change_frequency: Option<ChangeFrequency>,
    #[serde(default)]
    pub priority: Option<f32>,
}

impl SitemapEntry {
    pub fn new(url: impl Into<String>) -> Self {
        Self { url: url.into(), last_modified: None, change_frequency: None, priority: None }
    }

    pub fn last_modified(mut self, date: impl Into<String>) -> Self {
        self.last_modified = Some(date.into());
        self
    }

    pub fn change_frequency(mut self, frequency: ChangeFrequency) -> Self {
        self.change_frequency = Some(frequency);
        self
    }

    pub fn priority(mut self, priority: f32) -> Self {
        self.priority = Some(priority.clamp(0.0, 1.0));
        self
    }
}

impl Type for SitemapEntry {
    fn ts(ts: &mut TypeScript) -> String {
        ts.named("SitemapEntry", |_| {
            "export interface SitemapEntry { url: string; lastModified?: string | null; changeFrequency?: string | null; priority?: number | null }".into()
        })
    }
}

/// A metadata route defined in code: the default export of `app/<name>.(ts|js)`, evaluated per request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetadataRoute {
    Sitemap,
    Robots,
    Manifest,
}

impl MetadataRoute {
    pub const ALL: [Self; 3] = [Self::Sitemap, Self::Robots, Self::Manifest];

    /// File stem in `app/`, also the name passed to `__VOLT__.metadataRoute`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Sitemap => "sitemap",
            Self::Robots => "robots",
            Self::Manifest => "manifest",
        }
    }

    /// The URL it is served at, relative to the base path.
    pub fn path(self) -> &'static str {
        match self {
            Self::Sitemap => "/sitemap.xml",
            Self::Robots => "/robots.txt",
            Self::Manifest => "/manifest.webmanifest",
        }
    }

    pub fn from_path(path: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|route| route.path() == path)
    }
}

/// Accepts a single value or an array, as Next.js metadata objects do.
fn one_or_many<'de, D: Deserializer<'de>, T: Deserialize<'de>>(deserializer: D) -> Result<Vec<T>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany<T> {
        One(T),
        Many(Vec<T>),
    }
    Ok(match Option::<OneOrMany<T>>::deserialize(deserializer)? {
        None => Vec::new(),
        Some(OneOrMany::One(value)) => vec![value],
        Some(OneOrMany::Many(values)) => values,
    })
}

/// The object returned by `app/robots.ts`, as in Next.js.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Robots {
    #[serde(default, deserialize_with = "one_or_many")]
    pub rules: Vec<RobotsRule>,
    #[serde(default, deserialize_with = "one_or_many")]
    pub sitemap: Vec<String>,
    #[serde(default)]
    pub host: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RobotsRule {
    #[serde(default, deserialize_with = "one_or_many")]
    pub user_agent: Vec<String>,
    #[serde(default, deserialize_with = "one_or_many")]
    pub allow: Vec<String>,
    #[serde(default, deserialize_with = "one_or_many")]
    pub disallow: Vec<String>,
    #[serde(default)]
    pub crawl_delay: Option<f64>,
}

impl Robots {
    pub fn render(&self) -> String {
        let mut text = String::new();
        for rule in &self.rules {
            if rule.user_agent.is_empty() {
                text.push_str("User-Agent: *\n");
            }
            for agent in &rule.user_agent {
                text.push_str(&format!("User-Agent: {agent}\n"));
            }
            for path in &rule.allow {
                text.push_str(&format!("Allow: {path}\n"));
            }
            for path in &rule.disallow {
                text.push_str(&format!("Disallow: {path}\n"));
            }
            if let Some(delay) = rule.crawl_delay {
                text.push_str(&format!("Crawl-delay: {delay}\n"));
            }
            text.push('\n');
        }
        if let Some(host) = &self.host {
            text.push_str(&format!("Host: {host}\n"));
        }
        for sitemap in &self.sitemap {
            text.push_str(&format!("Sitemap: {sitemap}\n"));
        }
        text
    }
}

pub fn escape_xml(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for c in input.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

fn absolute(config: &Config, url: &str) -> String {
    if url.starts_with("http://") || url.starts_with("https://") {
        url.to_owned()
    } else {
        format!("{}{}", config.site_url(), config.public_path(&format!("/{}", url.trim_start_matches('/'))))
    }
}

/// A sitemap `<url>`: an entry with its absolute location and its `hreflang` alternates.
struct Location<'a> {
    entry: &'a SitemapEntry,
    url: String,
    alternates: Vec<(String, String)>,
}

/// Every entry once per locale when `[i18n]` is configured. Relative URLs that already carry a locale
/// prefix and absolute URLs are kept as they are; duplicates are listed once.
fn locations<'a>(config: &Config, entries: &'a [SitemapEntry]) -> Vec<Location<'a>> {
    let i18n = &config.i18n;
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for entry in entries {
        let relative = !entry.url.starts_with("http://") && !entry.url.starts_with("https://");
        let path = format!("/{}", entry.url.trim_start_matches('/'));
        if !i18n.is_enabled() || !relative || i18n.split(&path).0.is_some() {
            let url = absolute(config, &entry.url);
            if seen.insert(url.clone()) {
                out.push(Location { entry, url, alternates: Vec::new() });
            }
            continue;
        }
        let variants: Vec<(String, String)> = i18n
            .variants(&path)
            .into_iter()
            .map(|(locale, localized)| (locale.to_owned(), absolute(config, &localized)))
            .collect();
        let mut alternates = variants.clone();
        alternates.push(("x-default".to_owned(), variants[0].1.clone()));
        for (_, url) in variants {
            if seen.insert(url.clone()) {
                out.push(Location { entry, url, alternates: alternates.clone() });
            }
        }
    }
    out
}

pub fn sitemap(config: &Config, entries: &[SitemapEntry]) -> String {
    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\"",
    );
    if config.i18n.is_enabled() {
        xml.push_str(" xmlns:xhtml=\"http://www.w3.org/1999/xhtml\"");
    }
    xml.push_str(">\n");
    for Location { entry, url, alternates } in locations(config, entries) {
        xml.push_str("  <url>\n");
        xml.push_str(&format!("    <loc>{}</loc>\n", escape_xml(&url)));
        for (lang, href) in &alternates {
            xml.push_str(&format!(
                "    <xhtml:link rel=\"alternate\" hreflang=\"{}\" href=\"{}\"/>\n",
                escape_xml(lang),
                escape_xml(href)
            ));
        }
        if let Some(date) = &entry.last_modified {
            xml.push_str(&format!("    <lastmod>{}</lastmod>\n", escape_xml(date)));
        }
        if let Some(frequency) = entry.change_frequency {
            xml.push_str(&format!("    <changefreq>{}</changefreq>\n", frequency.as_str()));
        }
        if let Some(priority) = entry.priority {
            xml.push_str(&format!("    <priority>{priority:.1}</priority>\n"));
        }
        xml.push_str("  </url>\n");
    }
    xml.push_str("</urlset>\n");
    xml
}

pub fn robots(config: &Config) -> String {
    let mut text = String::from("User-agent: *\nAllow: /\n");
    for path in ["/_volt/action/", "/_volt/data"].iter().copied().chain(config.seo.disallow.iter().map(String::as_str))
    {
        text.push_str(&format!("Disallow: {}{path}\n", config.base_path()));
    }
    if config.seo.sitemap {
        text.push_str(&format!("\nSitemap: {}{}/sitemap.xml\n", config.site_url(), config.base_path()));
    }
    text
}

pub fn llms(config: &Config, entries: &[SitemapEntry]) -> String {
    let mut text = format!("# {}\n\n> {}\n\n## Pages\n\n", config.site.name, config.site_url());
    for location in locations(config, entries) {
        text.push_str(&format!("- {}\n", location.url));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_sitemap() {
        let mut config = Config::default();
        config.site.url = "https://volt.rs/".into();
        let xml = sitemap(&config, &[SitemapEntry::new("/blog?a=1&b=2").priority(0.8)]);
        assert!(xml.contains("<loc>https://volt.rs/blog?a=1&amp;b=2</loc>"));
        assert!(xml.contains("<priority>0.8</priority>"));
        assert!(robots(&config).contains("Sitemap: https://volt.rs/sitemap.xml"));
        assert!(!xml.contains("xhtml"));
    }

    #[test]
    fn lists_locale_variants_with_alternates() {
        let mut config = Config::default();
        config.site.url = "https://volt.rs".into();
        config.i18n.locales = vec!["en".into(), "de".into()];
        config.i18n.default_locale = "en".into();
        let entries = [SitemapEntry::new("/"), SitemapEntry::new("/about"), SitemapEntry::new("/de/only")];
        let xml = sitemap(&config, &entries);
        for loc in ["https://volt.rs/", "https://volt.rs/de", "https://volt.rs/about", "https://volt.rs/de/about"] {
            assert!(xml.contains(&format!("<loc>{loc}</loc>")), "{loc}");
        }
        assert!(xml.contains("<xhtml:link rel=\"alternate\" hreflang=\"de\" href=\"https://volt.rs/de/about\"/>"));
        assert!(xml.contains("hreflang=\"x-default\" href=\"https://volt.rs/about\""));
        assert_eq!(xml.matches("<loc>https://volt.rs/de/only</loc>").count(), 1);
        assert_eq!(xml.matches("<url>").count(), 5);
    }

    #[test]
    fn renders_code_robots() {
        let robots: Robots = serde_json::from_str(
            r#"{"rules":[{"userAgent":"*","allow":"/","disallow":["/admin","/tmp"]},{"userAgent":["a","b"],"crawlDelay":2}],"sitemap":"https://x.dev/sitemap.xml","host":"https://x.dev"}"#,
        )
        .unwrap();
        assert_eq!(
            robots.render(),
            "User-Agent: *\nAllow: /\nDisallow: /admin\nDisallow: /tmp\n\nUser-Agent: a\nUser-Agent: b\nCrawl-delay: 2\n\nHost: https://x.dev\nSitemap: https://x.dev/sitemap.xml\n"
        );
        assert_eq!(MetadataRoute::from_path("/manifest.webmanifest"), Some(MetadataRoute::Manifest));
    }
}
