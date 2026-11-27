//! Google Fonts downloads, cached under `.volt/cache/fonts` so builds work offline once fetched.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};

use crate::util::hash;

const CSS_API: &str = "https://fonts.googleapis.com/css2";
const METADATA_API: &str = "https://fonts.google.com/metadata/fonts";
/// A current browser user agent makes the API answer with WOFF2 files split by unicode subset.
const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

/// What to ask the CSS API for.
#[derive(Debug, Clone)]
pub struct Query {
    pub family: String,
    pub weights: Vec<String>,
    pub styles: Vec<String>,
    pub display: String,
}

impl Query {
    /// The `css2` URL; `weights` holds explicit weights or one variable range such as `100..900`.
    pub fn url(&self) -> String {
        let family = self.family.trim().replace(' ', "+");
        let italic = self.styles.iter().any(|s| s == "italic");
        let normal = self.styles.is_empty() || self.styles.iter().any(|s| s == "normal");
        let mut weights: Vec<String> = self.weights.iter().filter(|w| *w != "variable").cloned().collect();
        weights.sort_by_key(|w| w.parse::<u32>().unwrap_or(0));
        weights.dedup();
        let axes = if !italic {
            if weights.is_empty() { String::new() } else { format!(":wght@{}", weights.join(";")) }
        } else {
            if weights.is_empty() {
                weights.push("400".into());
            }
            let mut tuples = Vec::new();
            for ital in [normal.then_some("0"), Some("1")].into_iter().flatten() {
                tuples.extend(weights.iter().map(|w| format!("{ital},{w}")));
            }
            format!(":ital,wght@{}", tuples.join(";"))
        };
        format!("{CSS_API}?family={family}{axes}&display={}", self.display)
    }
}

/// One `@font-face` from the API response.
#[derive(Debug, Clone, PartialEq)]
pub struct RemoteFace {
    pub subset: Option<String>,
    pub descriptors: Vec<(String, String)>,
    pub sources: Vec<(String, String)>,
}

pub struct Google {
    cache: PathBuf,
    agent: ureq::Agent,
}

impl Google {
    pub fn new(cache: &Path) -> Self {
        let tls = ureq::tls::TlsConfig::builder().root_certs(ureq::tls::RootCerts::PlatformVerifier).build();
        let agent =
            ureq::Agent::config_builder().tls_config(tls).timeout_global(Some(Duration::from_secs(60))).build().into();
        Self { cache: cache.join("fonts"), agent }
    }

    fn get(&self, url: &str) -> std::result::Result<Vec<u8>, ureq::Error> {
        let mut response = self.agent.get(url).header("User-Agent", USER_AGENT).call()?;
        response.body_mut().with_config().limit(64 * 1024 * 1024).read_to_vec()
    }

    fn cached(&self, url: &str, ext: &str, fetch: impl FnOnce() -> Result<Vec<u8>>) -> Result<Vec<u8>> {
        let path = self.cache.join(format!("{}.{ext}", hash(url.as_bytes())));
        if let Ok(bytes) = std::fs::read(&path) {
            return Ok(bytes);
        }
        let bytes = fetch()?;
        std::fs::create_dir_all(&self.cache)?;
        std::fs::write(&path, &bytes)?;
        Ok(bytes)
    }

    /// The weight axis of a variable family, e.g. `100..900`, from the Google Fonts catalog.
    fn variable_weights(&self, family: &str) -> Option<String> {
        let bytes =
            self.cached(METADATA_API, "json", || self.get(METADATA_API).map_err(|e| anyhow::anyhow!("{e}"))).ok()?;
        let text = String::from_utf8_lossy(&bytes);
        let catalog: serde_json::Value = serde_json::from_str(text.trim_start_matches(")]}'")).ok()?;
        let entry = catalog["familyMetadataList"].as_array()?.iter().find(|f| f["family"] == family)?;
        let axis = entry["axes"].as_array()?.iter().find(|a| a["tag"] == "wght")?;
        Some(format!("{}..{}", axis["min"].as_f64()?, axis["max"].as_f64()?))
    }

    pub fn faces(&self, query: &Query) -> Result<Vec<RemoteFace>> {
        let mut query = query.clone();
        let variable = query.weights.is_empty() || query.weights.iter().any(|w| w == "variable");
        if variable && let Some(range) = self.variable_weights(&query.family) {
            query.weights = vec![range];
        }
        let url = query.url();
        let result = self.cached(&url, "css", || {
            self.get(&url).map_err(|error| match error {
                ureq::Error::StatusCode(400) => anyhow::anyhow!("unknown family, weight or style"),
                error => anyhow::anyhow!("{error}"),
            })
        });
        match result {
            Ok(bytes) => Ok(parse(&String::from_utf8_lossy(&bytes))),
            Err(error) => bail!(
                "cannot download Google font `{}` from fonts.googleapis.com: {error}\n  requested {url}\n  fonts are downloaded at build time and cached in .volt/cache/fonts; check the family name, the weights and the network connection",
                query.family
            ),
        }
    }

    /// Downloads a font file and returns its bytes and extension.
    pub fn file(&self, url: &str) -> Result<(Vec<u8>, String)> {
        let ext = Path::new(url.split(['?', '#']).next().unwrap_or(url))
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_else(|| "woff2".into());
        let bytes = self
            .cached(url, &ext, || self.get(url).map_err(|e| anyhow::anyhow!("{e}")))
            .with_context(|| format!("cannot download font file {url}"))?;
        Ok((bytes, ext))
    }
}

/// Parses the CSS API response; each rule is preceded by a `/* subset */` comment.
pub fn parse(css: &str) -> Vec<RemoteFace> {
    let mut faces = Vec::new();
    let mut subset = None;
    let mut rest = css;
    loop {
        let comment = rest.find("/*");
        let rule = rest.find("@font-face");
        match (comment, rule) {
            (Some(c), r) if r.is_none_or(|r| c < r) => {
                let Some(end) = rest[c..].find("*/") else { break };
                subset = Some(rest[c + 2..c + end].trim().to_owned());
                rest = &rest[c + end + 2..];
            }
            (_, Some(r)) => {
                let Some(open) = rest[r..].find('{') else { break };
                let Some(close) = rest[r + open..].find('}') else { break };
                let body = &rest[r + open + 1..r + open + close];
                faces.push(face(body, subset.take()));
                rest = &rest[r + open + close + 1..];
            }
            _ => break,
        }
    }
    faces
}

fn face(body: &str, subset: Option<String>) -> RemoteFace {
    let mut descriptors = Vec::new();
    let mut sources = Vec::new();
    for declaration in body.split(';') {
        let Some((name, value)) = declaration.split_once(':') else { continue };
        let (name, value) = (name.trim().to_ascii_lowercase(), value.trim());
        match name.as_str() {
            "font-family" => {}
            "src" => {
                for source in value.split(',') {
                    let url = source.split_once("url(").and_then(|(_, rest)| rest.split_once(')')).map(|(url, _)| url);
                    let format = source
                        .split_once("format(")
                        .and_then(|(_, rest)| rest.split_once(')'))
                        .map(|(format, _)| format.trim_matches(['\'', '"', ' ']).to_owned());
                    if let Some(url) = url {
                        let url = url.trim().trim_matches(['\'', '"']).to_owned();
                        sources.push((url, format.unwrap_or_else(|| "woff2".into())));
                    }
                }
            }
            _ => descriptors.push((name, value.to_owned())),
        }
    }
    RemoteFace { subset, descriptors, sources }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RESPONSE: &str = "/* cyrillic */\n@font-face {\n  font-family: 'Inter';\n  font-style: normal;\n  font-weight: 400;\n  font-display: swap;\n  src: url(https://fonts.gstatic.com/s/inter/v1/a.woff2) format('woff2');\n  unicode-range: U+0301, U+0400-045F;\n}\n/* latin */\n@font-face {\n  font-family: 'Inter';\n  font-style: normal;\n  font-weight: 400;\n  font-display: swap;\n  src: url(https://fonts.gstatic.com/s/inter/v1/b.woff2) format('woff2');\n  unicode-range: U+0000-00FF;\n}\n";

    #[test]
    fn parses_api_response() {
        let faces = parse(RESPONSE);
        assert_eq!(faces.len(), 2);
        assert_eq!(faces[1].subset.as_deref(), Some("latin"));
        assert_eq!(faces[1].sources, vec![("https://fonts.gstatic.com/s/inter/v1/b.woff2".into(), "woff2".into())]);
        assert!(faces[1].descriptors.contains(&("unicode-range".into(), "U+0000-00FF".into())));
        assert!(!faces[0].descriptors.iter().any(|(n, _)| n == "font-family"));
    }

    #[test]
    fn builds_query_urls() {
        let query = |weights: &[&str], styles: &[&str]| Query {
            family: "Roboto Mono".into(),
            weights: weights.iter().map(|w| (*w).to_owned()).collect(),
            styles: styles.iter().map(|s| (*s).to_owned()).collect(),
            display: "swap".into(),
        };
        let api = "https://fonts.googleapis.com/css2?family=Roboto+Mono";
        assert_eq!(query(&["700", "400"], &[]).url(), format!("{api}:wght@400;700&display=swap"));
        assert_eq!(query(&[], &[]).url(), format!("{api}&display=swap"));
        assert_eq!(query(&["100..700"], &[]).url(), format!("{api}:wght@100..700&display=swap"));
        assert_eq!(query(&["400"], &["normal", "italic"]).url(), format!("{api}:ital,wght@0,400;1,400&display=swap"));
    }
}
