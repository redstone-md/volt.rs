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
    /// Result of a no-JavaScript action submission, for `useActionState` (`{ action, data | error }`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<Value>,
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
    /// The complete document around React's buffered output.
    pub fn render(&self, html: &[u8]) -> String {
        let mut splice = self.splice();
        let mut out = splice.push(html);
        out.extend(splice.finish());
        String::from_utf8(out).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
    }

    /// Inserts this document's head and body additions into HTML that arrives in chunks.
    pub fn splice(&self) -> Splice {
        Splice::new(self.head(), self.body())
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

/// How far into the output an `<html>` tag is looked for before treating it as a fragment.
const DOCUMENT_PROBE: usize = 1024;
const HEAD_END: &[u8] = b"</head>";
const BODY_START: &[u8] = b"<body";
const BODY_END: &[u8] = b"</body>";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    Undecided,
    /// The root layout renders `<html>`: additions go before `</head>` and the last `</body>`.
    Document,
    /// Output without `<html>` is wrapped in a document with a `#__volt` container.
    Fragment,
}

/// Inserts the head additions (stylesheets, preloads) and the body additions (state, entry script)
/// into HTML that may arrive in arbitrary chunks. Markers split across chunks are found by holding
/// back a few bytes, and everything from the last `</body>` is held until the stream ends, since
/// React writes the closing tags only after every Suspense boundary has completed.
#[derive(Debug)]
pub struct Splice {
    head: Option<String>,
    body: String,
    shape: Shape,
    pending: Vec<u8>,
}

impl Splice {
    pub fn new(head: String, body: String) -> Self {
        Self { head: Some(head), body, shape: Shape::Undecided, pending: Vec::new() }
    }

    /// Accepts the next chunk and returns the bytes that can be sent now.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<u8> {
        self.pending.extend_from_slice(chunk);
        self.process(false)
    }

    /// Returns the remaining bytes once the output is complete.
    pub fn finish(&mut self) -> Vec<u8> {
        self.process(true)
    }

    /// Returns the remaining bytes of output that stopped early, closing the document React did not close.
    pub fn cut(&mut self) -> Vec<u8> {
        let mut out = self.process(true);
        if self.shape == Shape::Document && !out.trim_ascii_end().ends_with(b"</html>") {
            out.extend_from_slice(b"</body></html>");
        }
        out
    }

    fn process(&mut self, end: bool) -> Vec<u8> {
        let mut out = Vec::new();
        if self.shape == Shape::Undecided && !self.decide(&mut out, end) {
            return out;
        }
        match self.shape {
            Shape::Fragment => {
                out.append(&mut self.pending);
                if end {
                    out.extend_from_slice(b"</div>");
                    out.extend_from_slice(self.body.as_bytes());
                    out.extend_from_slice(b"</body></html>");
                }
            }
            _ => self.document(&mut out, end),
        }
        out
    }

    /// Picks the shape once enough output is known; returns whether it is decided.
    fn decide(&mut self, out: &mut Vec<u8>, end: bool) -> bool {
        let start = self.pending.iter().position(|b| !b.is_ascii_whitespace()).unwrap_or(self.pending.len());
        let probe = &self.pending[start..self.pending.len().min(start + DOCUMENT_PROBE)];
        let html = memchr::memmem::find(probe, b"<html").is_some();
        if !html && !end && probe.len() < DOCUMENT_PROBE {
            return false;
        }
        if !probe.get(..9).is_some_and(|p| p.eq_ignore_ascii_case(b"<!doctype")) {
            out.extend_from_slice(b"<!DOCTYPE html>");
        }
        if html {
            self.shape = Shape::Document;
        } else {
            self.shape = Shape::Fragment;
            out.extend_from_slice(b"<html><head>");
            out.extend_from_slice(self.head.take().unwrap_or_default().as_bytes());
            out.extend_from_slice(b"</head><body><div id=\"__volt\">");
        }
        true
    }

    fn document(&mut self, out: &mut Vec<u8>, end: bool) {
        if let Some(head) = self.head.take() {
            let body = memchr::memmem::find(&self.pending, BODY_START);
            let close = memchr::memmem::find(&self.pending, HEAD_END).filter(|at| body.is_none_or(|b| *at < b));
            match (close, body) {
                (Some(at), _) => {
                    out.extend(self.pending.drain(..at));
                    out.extend_from_slice(head.as_bytes());
                }
                (_, Some(at)) => {
                    out.extend(self.pending.drain(..at));
                    out.extend_from_slice(b"<head>");
                    out.extend_from_slice(head.as_bytes());
                    out.extend_from_slice(b"</head>");
                }
                (None, None) if end => {
                    out.append(&mut self.pending);
                    out.extend_from_slice(b"<head>");
                    out.extend_from_slice(head.as_bytes());
                    out.extend_from_slice(b"</head>");
                }
                (None, None) => {
                    self.head = Some(head);
                    let keep = HEAD_END.len().max(BODY_START.len()) - 1;
                    out.extend(self.pending.drain(..self.pending.len().saturating_sub(keep)));
                    return;
                }
            }
        }
        if end {
            if self.pending.starts_with(BODY_END) {
                out.extend_from_slice(self.body.as_bytes());
                out.append(&mut self.pending);
            } else {
                out.append(&mut self.pending);
                out.extend_from_slice(self.body.as_bytes());
            }
            return;
        }
        let keep = match memchr::memmem::rfind(&self.pending, BODY_END) {
            Some(at) => self.pending.len() - at,
            None => BODY_END.len() - 1,
        };
        out.extend(self.pending.drain(..self.pending.len().saturating_sub(keep)));
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
            action: None,
            build_id: String::new(),
            site: Site { name: String::new(), url: String::new() },
            export: false,
        };
        let head = Document { manifest: &manifest, route: None, state: &state, dev: false, base_path: "" }.head();
        assert!(head.starts_with(
            "<link rel=\"preload\" href=\"/_volt/static/media/a-1.woff2\" as=\"font\" type=\"font/woff2\" crossorigin=\"\">"
        ));
    }

    fn spliced(chunks: &[&str]) -> String {
        let mut splice = Splice::new("[H]".into(), "[B]".into());
        let mut out = Vec::new();
        for chunk in chunks {
            out.extend(splice.push(chunk.as_bytes()));
        }
        out.extend(splice.finish());
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn splices_documents_and_fragments() {
        let page = "<!DOCTYPE html><html><head><title>t</title></head><body><p>é</p></body></html>";
        assert_eq!(
            spliced(&[page]),
            "<!DOCTYPE html><html><head><title>t</title>[H]</head><body><p>é</p>[B]</body></html>"
        );
        assert_eq!(
            spliced(&["<html><body>x</body></html>"]),
            "<!DOCTYPE html><html><head>[H]</head><body>x[B]</body></html>"
        );
        assert_eq!(
            spliced(&["<p>x</p>"]),
            "<!DOCTYPE html><html><head>[H]</head><body><div id=\"__volt\"><p>x</p></div>[B]</body></html>"
        );
    }

    #[test]
    fn splices_markers_split_across_chunks() {
        let page = "<!DOCTYPE html><html><head></head><body><div>a</div><script>x</script></body></html>";
        let whole = spliced(&[page]);
        for at in 1..page.len() {
            assert_eq!(spliced(&[&page[..at], &page[at..]]), whole, "split at {at}");
        }
        let bytes: Vec<String> = page.chars().map(String::from).collect();
        assert_eq!(spliced(&bytes.iter().map(String::as_str).collect::<Vec<_>>()), whole);
    }

    #[test]
    fn holds_only_the_final_body_end() {
        let mut splice = Splice::new(String::new(), "[B]".into());
        let first = splice.push(b"<!DOCTYPE html><html><head></head><body><p>shell</p><template></template>");
        assert!(String::from_utf8_lossy(&first).contains("shell"));
        let late = splice.push(b"<div hidden id=\"S:0\">late</div></body></html>");
        assert!(String::from_utf8_lossy(&late).contains("late"));
        assert_eq!(splice.finish(), b"[B]</body></html>");
    }

    #[test]
    fn closes_cut_documents() {
        let mut splice = Splice::new(String::new(), "[B]".into());
        let mut out = splice.push(b"<!DOCTYPE html><html><head></head><body><p>shell</p>");
        out.extend(splice.cut());
        assert!(out.ends_with(b"<p>shell</p>[B]</body></html>"));
    }
}
