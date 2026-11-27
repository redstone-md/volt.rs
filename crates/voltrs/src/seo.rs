//! `sitemap.xml`, `robots.txt` and `llms.txt` generation.

use serde::{Deserialize, Serialize};

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

pub fn sitemap(config: &Config, entries: &[SitemapEntry]) -> String {
    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n",
    );
    for entry in entries {
        xml.push_str("  <url>\n");
        xml.push_str(&format!("    <loc>{}</loc>\n", escape_xml(&absolute(config, &entry.url))));
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
    for entry in entries {
        text.push_str(&format!("- {}\n", absolute(config, &entry.url)));
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
    }
}
