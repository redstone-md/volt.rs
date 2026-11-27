//! File-system style route patterns: `/blog/[slug]`, `/docs/[...path]`, `/shop/[[...rest]]`.

use percent_encoding::{AsciiSet, CONTROLS, percent_decode_str, utf8_percent_encode};
use serde_json::{Map, Value};

/// Characters escaped inside one path segment.
const SEGMENT: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'/')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'`')
    .add(b'{')
    .add(b'}');

#[derive(Debug, Clone, PartialEq, Eq)]
enum Segment {
    Static(String),
    Dynamic(String),
    CatchAll(String),
    OptionalCatchAll(String),
}

impl Segment {
    fn parse(raw: &str) -> Option<Self> {
        if raw.starts_with('(') && raw.ends_with(')') {
            return None;
        }
        let segment = if let Some(name) = raw.strip_prefix("[[...").and_then(|s| s.strip_suffix("]]")) {
            Self::OptionalCatchAll(name.to_owned())
        } else if let Some(name) = raw.strip_prefix("[...").and_then(|s| s.strip_suffix(']')) {
            Self::CatchAll(name.to_owned())
        } else if let Some(name) = raw.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            Self::Dynamic(name.to_owned())
        } else {
            Self::Static(raw.to_owned())
        };
        Some(segment)
    }

    fn weight(&self) -> u8 {
        match self {
            Self::Static(_) => 0,
            Self::Dynamic(_) => 1,
            Self::CatchAll(_) => 2,
            Self::OptionalCatchAll(_) => 3,
        }
    }
}

/// Captured route parameters in declaration order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Params(Vec<(String, ParamValue)>);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParamValue {
    One(String),
    Many(Vec<String>),
}

impl Params {
    pub fn get(&self, name: &str) -> Option<&str> {
        self.0.iter().find(|(key, _)| key == name).and_then(|(_, value)| match value {
            ParamValue::One(v) => Some(v.as_str()),
            ParamValue::Many(v) => v.first().map(String::as_str),
        })
    }

    pub fn get_all(&self, name: &str) -> Vec<&str> {
        self.0
            .iter()
            .filter(|(key, _)| key == name)
            .flat_map(|(_, value)| match value {
                ParamValue::One(v) => vec![v.as_str()],
                ParamValue::Many(v) => v.iter().map(String::as_str).collect(),
            })
            .collect()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &ParamValue)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// Reads `{ "slug": "a", "path": ["x", "y"] }` as returned by `generateStaticParams`.
    pub fn from_json(value: &Value) -> Option<Self> {
        let scalar = |value: &Value| match value {
            Value::String(s) => Some(s.clone()),
            Value::Number(n) => Some(n.to_string()),
            Value::Bool(b) => Some(b.to_string()),
            _ => None,
        };
        let params = value
            .as_object()?
            .iter()
            .filter(|(_, value)| !value.is_null())
            .map(|(key, value)| {
                let value = match value {
                    Value::Array(items) => ParamValue::Many(items.iter().map(scalar).collect::<Option<_>>()?),
                    other => ParamValue::One(scalar(other)?),
                };
                Some((key.clone(), value))
            })
            .collect::<Option<_>>()?;
        Some(Self(params))
    }

    pub fn to_json(&self) -> Value {
        let map: Map<String, Value> = self
            .0
            .iter()
            .map(|(key, value)| {
                let value = match value {
                    ParamValue::One(v) => Value::String(v.clone()),
                    ParamValue::Many(v) => Value::Array(v.iter().cloned().map(Value::String).collect()),
                };
                (key.clone(), value)
            })
            .collect();
        Value::Object(map)
    }
}

/// A parsed route pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutePattern {
    raw: String,
    segments: Vec<Segment>,
}

impl RoutePattern {
    pub fn parse(raw: &str) -> Self {
        let segments: Vec<Segment> = raw.split('/').filter(|s| !s.is_empty()).filter_map(Segment::parse).collect();
        let raw = Self::render(&segments);
        Self { raw, segments }
    }

    fn render(segments: &[Segment]) -> String {
        let body: Vec<String> = segments
            .iter()
            .map(|segment| match segment {
                Segment::Static(s) => s.clone(),
                Segment::Dynamic(n) => format!("[{n}]"),
                Segment::CatchAll(n) => format!("[...{n}]"),
                Segment::OptionalCatchAll(n) => format!("[[...{n}]]"),
            })
            .collect();
        format!("/{}", body.join("/"))
    }

    pub fn as_str(&self) -> &str {
        &self.raw
    }

    pub fn is_dynamic(&self) -> bool {
        self.segments.iter().any(|s| !matches!(s, Segment::Static(_)))
    }

    /// Whether the pattern ends in a catch-all (`[...x]` or `[[...x]]`).
    pub fn is_catch_all(&self) -> bool {
        self.segments.iter().any(|s| matches!(s, Segment::CatchAll(_) | Segment::OptionalCatchAll(_)))
    }

    /// Whether both patterns match the same paths: equal up to parameter names.
    pub fn same_paths(&self, other: &Self) -> bool {
        self.segments.len() == other.segments.len()
            && self.segments.iter().zip(&other.segments).all(|(a, b)| match (a, b) {
                (Segment::Static(a), Segment::Static(b)) => a == b,
                (a, b) => a.weight() == b.weight(),
            })
    }

    /// Sort key: more specific patterns first.
    fn specificity(&self) -> (Vec<u8>, std::cmp::Reverse<usize>) {
        (self.segments.iter().map(Segment::weight).collect(), std::cmp::Reverse(self.segments.len()))
    }

    /// Whether a [`Router`] holding both patterns tries `self` first, or as early as `other`.
    pub fn ranks_before(&self, other: &Self) -> bool {
        self.specificity() <= other.specificity()
    }

    pub fn matches(&self, path: &str) -> Option<Params> {
        let parts: Vec<String> = path
            .split('/')
            .filter(|s| !s.is_empty())
            .map(|s| percent_decode_str(s).decode_utf8_lossy().into_owned())
            .collect();
        let mut params = Vec::new();
        let mut index = 0;
        for (position, segment) in self.segments.iter().enumerate() {
            match segment {
                Segment::Static(expected) => {
                    if parts.get(index) != Some(expected) {
                        return None;
                    }
                    index += 1;
                }
                Segment::Dynamic(name) => {
                    params.push((name.clone(), ParamValue::One(parts.get(index)?.clone())));
                    index += 1;
                }
                Segment::CatchAll(name) | Segment::OptionalCatchAll(name) => {
                    // Segments after a catch-all (metadata files such as `[...slug]/icon.png`) take the last parts.
                    let after = self.segments.len() - position - 1;
                    let end = parts.len().checked_sub(after)?.max(index);
                    let rest = parts[index..end].to_vec();
                    if rest.is_empty() && matches!(segment, Segment::CatchAll(_)) {
                        return None;
                    }
                    if !rest.is_empty() {
                        params.push((name.clone(), ParamValue::Many(rest)));
                    }
                    index = end;
                }
            }
        }
        (index == parts.len()).then_some(Params(params))
    }

    /// Fills the pattern with params, used by redirects and sitemaps.
    pub fn fill(&self, params: &Params) -> String {
        let parts: Vec<String> = self
            .segments
            .iter()
            .filter_map(|segment| match segment {
                Segment::Static(s) => Some(s.clone()),
                Segment::Dynamic(n) => params.get(n).map(str::to_owned),
                Segment::CatchAll(n) | Segment::OptionalCatchAll(n) => {
                    let all = params.get_all(n);
                    (!all.is_empty()).then(|| all.join("/"))
                }
            })
            .collect();
        format!("/{}", parts.join("/"))
    }
}

impl RoutePattern {
    /// Builds a concrete, percent-encoded URL path; `None` when a required param is missing.
    pub fn href(&self, params: &Params) -> Option<String> {
        let mut parts = Vec::new();
        for segment in &self.segments {
            match segment {
                Segment::Static(s) => parts.push(s.clone()),
                Segment::Dynamic(n) => parts.push(utf8_percent_encode(params.get(n)?, SEGMENT).to_string()),
                Segment::CatchAll(n) | Segment::OptionalCatchAll(n) => {
                    let all = params.get_all(n);
                    if all.is_empty() && matches!(segment, Segment::CatchAll(_)) {
                        return None;
                    }
                    parts.extend(all.into_iter().map(|v| utf8_percent_encode(v, SEGMENT).to_string()));
                }
            }
        }
        Some(format!("/{}", parts.join("/")))
    }
}

/// Ordered collection of patterns resolving a path to the most specific match.
#[derive(Debug, Clone)]
pub struct Router<T> {
    routes: Vec<(RoutePattern, T)>,
}

impl<T> Default for Router<T> {
    fn default() -> Self {
        Self { routes: Vec::new() }
    }
}

impl<T> Router<T> {
    pub fn insert(&mut self, pattern: &str, value: T) {
        self.routes.push((RoutePattern::parse(pattern), value));
        self.routes.sort_by_key(|(pattern, _)| pattern.specificity());
    }

    pub fn find(&self, path: &str) -> Option<(&RoutePattern, &T, Params)> {
        self.routes.iter().find_map(|(pattern, value)| pattern.matches(path).map(|params| (pattern, value, params)))
    }

    /// The most specific match among the values that `accept` allows.
    pub fn find_by(&self, path: &str, mut accept: impl FnMut(&T) -> bool) -> Option<(&RoutePattern, &T, Params)> {
        self.routes
            .iter()
            .filter(|(_, value)| accept(value))
            .find_map(|(pattern, value)| pattern.matches(path).map(|params| (pattern, value, params)))
    }

    pub fn iter(&self) -> impl Iterator<Item = (&RoutePattern, &T)> {
        self.routes.iter().map(|(p, v)| (p, v))
    }

    pub fn len(&self) -> usize {
        self.routes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.routes.is_empty()
    }
}

impl<T> FromIterator<(String, T)> for Router<T> {
    fn from_iter<I: IntoIterator<Item = (String, T)>>(iter: I) -> Self {
        let mut router = Self::default();
        for (pattern, value) in iter {
            router.insert(&pattern, value);
        }
        router
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_groups_and_params() {
        assert_eq!(RoutePattern::parse("/(marketing)/blog/[slug]").as_str(), "/blog/[slug]");
        assert_eq!(RoutePattern::parse("/").as_str(), "/");
    }

    #[test]
    fn matches_segments() {
        let p = RoutePattern::parse("/blog/[slug]");
        assert_eq!(p.matches("/blog/hello").unwrap().get("slug"), Some("hello"));
        assert!(p.matches("/blog").is_none());
        assert!(p.matches("/blog/a/b").is_none());
        let all = RoutePattern::parse("/docs/[...path]");
        assert_eq!(all.matches("/docs/a/b").unwrap().get_all("path"), vec!["a", "b"]);
        assert!(all.matches("/docs").is_none());
        let opt = RoutePattern::parse("/shop/[[...rest]]");
        assert!(opt.matches("/shop").is_some());
        assert_eq!(opt.matches("/shop/x%20y").unwrap().get("rest"), Some("x y"));
    }

    #[test]
    fn matches_segments_after_catch_alls() {
        let icon = RoutePattern::parse("/docs/[...slug]/icon.png");
        assert_eq!(icon.matches("/docs/a/b/icon.png").unwrap().get_all("slug"), vec!["a", "b"]);
        assert!(icon.matches("/docs/icon.png").is_none());
        assert!(icon.matches("/docs/a/b").is_none());
        let optional = RoutePattern::parse("/shop/[[...rest]]/opengraph-image.png");
        assert!(optional.matches("/shop/opengraph-image.png").unwrap().get("rest").is_none());
        assert_eq!(optional.matches("/shop/x/opengraph-image.png").unwrap().get("rest"), Some("x"));
        assert!(optional.matches("/opengraph-image.png").is_none());
    }

    #[test]
    fn prefers_specific_routes() {
        let router: Router<u8> =
            [("/[...all]".to_owned(), 0), ("/blog/[slug]".to_owned(), 1), ("/blog/new".to_owned(), 2)]
                .into_iter()
                .collect();
        assert_eq!(*router.find("/blog/new").unwrap().1, 2);
        assert_eq!(*router.find("/blog/x").unwrap().1, 1);
        assert_eq!(*router.find("/a/b").unwrap().1, 0);
        assert_eq!(*router.find_by("/blog/new", |v| *v != 2).unwrap().1, 1);
        assert!(RoutePattern::parse("/shop/[id]").same_paths(&RoutePattern::parse("/shop/[slug]")));
        assert!(!RoutePattern::parse("/shop/[id]").same_paths(&RoutePattern::parse("/shop/[...id]")));
        assert!(!RoutePattern::parse("/shop/new").same_paths(&RoutePattern::parse("/shop/[id]")));
    }

    #[test]
    fn builds_hrefs_from_static_params() {
        let params = Params::from_json(&serde_json::json!({ "slug": "a b", "rest": ["x", 1] })).unwrap();
        assert_eq!(RoutePattern::parse("/blog/[slug]").href(&params).as_deref(), Some("/blog/a%20b"));
        assert_eq!(RoutePattern::parse("/[...rest]").href(&params).as_deref(), Some("/x/1"));
        assert!(RoutePattern::parse("/[id]").href(&params).is_none());
        assert_eq!(RoutePattern::parse("/shop/[[...all]]").href(&params).as_deref(), Some("/shop"));
    }

    #[test]
    fn fills_patterns() {
        let p = RoutePattern::parse("/blog/[slug]");
        let params = p.matches("/blog/x").unwrap();
        assert_eq!(RoutePattern::parse("/posts/[slug]").fill(&params), "/posts/x");
    }
}
