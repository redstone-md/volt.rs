//! Assembles the final HTML document around React's output.

use serde::Serialize;
use serde_json::Value;

use crate::manifest::{Manifest, RouteEntry};

pub const NOT_FOUND_ROUTE: &str = "/_not-found";
pub const ERROR_ROUTE: &str = "/_error";

/// Everything the client and the server renderer need to build the same React tree.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageState {
    pub route: String,
    pub pathname: String,
    pub search: String,
    pub params: Value,
    pub search_params: Value,
    pub data: Value,
    pub status: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<Value>,
    pub build_id: String,
    pub site: Site,
    /// Set by `volt export`: the client reads route data from `/_volt/data/<path>.json` files.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub export: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Site {
    pub name: String,
    pub url: String,
}

/// JSON-safe for embedding inside `<script>`.
pub fn script_json<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value)
        .unwrap_or_else(|_| "null".into())
        .replace('<', "\\u003c")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

pub fn escape_html(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for c in input.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

pub struct Document<'a> {
    pub manifest: &'a Manifest,
    pub route: Option<&'a RouteEntry>,
    pub state: &'a PageState,
    pub dev: bool,
    pub base_path: &'a str,
}

impl Document<'_> {
    pub fn render(&self, html: &str) -> String {
        let head = self.head();
        let body = self.body();
        let mut out = String::with_capacity(html.len() + head.len() + body.len() + 64);
        if !html.trim_start().to_ascii_lowercase().starts_with("<!doctype") {
            out.push_str("<!DOCTYPE html>");
        }
        if !html.contains("<html") {
            out.push_str("<html><head>");
            out.push_str(&head);
            out.push_str("</head><body><div id=\"__volt\">");
            out.push_str(html);
            out.push_str("</div>");
            out.push_str(&body);
            out.push_str("</body></html>");
            return out;
        }
        let head_at = html.find("</head>");
        let body_at = html.rfind("</body>");
        match (head_at, body_at) {
            (Some(h), Some(b)) if h < b => {
                out.push_str(&html[..h]);
                out.push_str(&head);
                out.push_str(&html[h..b]);
                out.push_str(&body);
                out.push_str(&html[b..]);
            }
            _ => {
                let insert = html.find("<body").unwrap_or(html.len());
                out.push_str(&html[..insert]);
                out.push_str("<head>");
                out.push_str(&head);
                out.push_str("</head>");
                out.push_str(&html[insert..]);
                out.push_str(&body);
            }
        }
        out
    }

    fn head(&self) -> String {
        let client = &self.manifest.client;
        let mut head = String::new();
        for font in &client.fonts {
            head.push_str(&format!(
                "<link rel=\"preload\" href=\"{font}\" as=\"font\" type=\"{}\" crossorigin=\"\">",
                font_type(font)
            ));
        }
        for css in &client.css {
            head.push_str(&format!("<link rel=\"stylesheet\" href=\"{css}\">"));
        }
        let chunks = self.route.map(|r| r.chunks.as_slice()).unwrap_or_default();
        for chunk in std::iter::once(&client.entry).chain(&client.preload).chain(chunks) {
            head.push_str(&format!("<link rel=\"modulepreload\" href=\"{chunk}\">"));
        }
        head
    }

    fn body(&self) -> String {
        let mut body = format!(
            "<script id=\"__VOLT_DATA__\" type=\"application/json\">{}</script><script type=\"module\" src=\"{}\"></script>",
            script_json(self.state),
            self.manifest.client.entry
        );
        if self.dev {
            body.push_str(&crate::dev::client(self.base_path));
        }
        body
    }
}

fn font_type(url: &str) -> &'static str {
    match url.rsplit('.').next() {
        Some("woff") => "font/woff",
        Some("ttf") => "font/ttf",
        Some("otf") => "font/otf",
        _ => "font/woff2",
    }
}

/// Development error page for render failures.
pub fn error_overlay(title: &str, detail: &str, base_path: &str) -> String {
    format!(
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>{t}</title><style>\
body{{margin:0;font:14px/1.6 ui-monospace,monospace;background:#0b0b0f;color:#e4e4e7}}\
main{{max-width:960px;margin:0 auto;padding:48px 24px}}h1{{color:#facc15;font-size:18px}}\
pre{{white-space:pre-wrap;background:#18181b;border:1px solid #27272a;border-radius:8px;padding:16px;overflow:auto}}\
</style></head><body><main><h1>⚡ {t}</h1><pre>{d}</pre></main>{c}</body></html>",
        t = escape_html(title),
        d = escape_html(detail),
        c = crate::dev::client(base_path)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_script_json() {
        assert_eq!(script_json(&"</script>"), "\"\\u003c/script>\"");
    }

    #[test]
    fn preloads_fonts() {
        let mut manifest = Manifest::default();
        manifest.client.fonts = vec!["/_volt/static/media/a-1.woff2".into()];
        manifest.client.css = vec!["/_volt/static/css/app.css".into()];
        let state = PageState {
            route: "/".into(),
            pathname: "/".into(),
            search: String::new(),
            params: Value::Null,
            search_params: Value::Null,
            data: Value::Null,
            status: 200,
            error: None,
            build_id: String::new(),
            site: Site { name: String::new(), url: String::new() },
            export: false,
        };
        let head = Document { manifest: &manifest, route: None, state: &state, dev: false, base_path: "" }.head();
        assert!(head.starts_with(
            "<link rel=\"preload\" href=\"/_volt/static/media/a-1.woff2\" as=\"font\" type=\"font/woff2\" crossorigin=\"\">"
        ));
    }
}
