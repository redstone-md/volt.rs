//! `config.matcher` of `middleware.ts`: Next.js path patterns (`/about/:path*`, `/(api|docs)/:id(\d+)`), with the
//! `has` and `missing` request conditions of the object form.

use serde::{Deserialize, Serialize};

/// One `config.matcher` entry: a path pattern and the conditions of its `{ source, has, missing }` form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "RuleForm", into = "RuleForm")]
pub struct MatcherRule {
    pub source: String,
    /// Conditions that must all hold.
    pub has: Vec<Condition>,
    /// Conditions none of which may hold.
    pub missing: Vec<Condition>,
}

impl MatcherRule {
    /// A pattern without conditions, the string form.
    pub fn path(source: impl Into<String>) -> Self {
        Self { source: source.into(), has: Vec::new(), missing: Vec::new() }
    }
}

/// The manifest spells a rule without conditions as its source string.
#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum RuleForm {
    Path(String),
    Rule {
        source: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        has: Vec<Condition>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        missing: Vec<Condition>,
    },
}

impl From<RuleForm> for MatcherRule {
    fn from(form: RuleForm) -> Self {
        match form {
            RuleForm::Path(source) => Self::path(source),
            RuleForm::Rule { source, has, missing } => Self { source, has, missing },
        }
    }
}

impl From<MatcherRule> for RuleForm {
    fn from(rule: MatcherRule) -> Self {
        if rule.has.is_empty() && rule.missing.is_empty() {
            Self::Path(rule.source)
        } else {
            Self::Rule { source: rule.source, has: rule.has, missing: rule.missing }
        }
    }
}

/// A `has` / `missing` entry: `{ type: "header", key: "x-flag", value: "on|yes" }`. Without `value` the header,
/// cookie or query parameter only has to be present; `value` is a regular expression matching the whole value.
/// `host` conditions match their `value` against the host name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Condition {
    #[serde(rename = "type")]
    pub kind: ConditionKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
}

/// What a [`Condition`] reads from the request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConditionKind {
    Header,
    Cookie,
    Query,
    Host,
}

impl ConditionKind {
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "header" => Self::Header,
            "cookie" => Self::Cookie,
            "query" => Self::Query,
            "host" => Self::Host,
            _ => return None,
        })
    }
}

/// Reads a request value for a condition: a header, cookie or query parameter named `key`, or the host name.
pub type Lookup<'a> = dyn Fn(ConditionKind, &str) -> Option<String> + 'a;

/// Which paths middleware runs on.
#[derive(Debug, Clone, Default)]
pub struct PathMatcher {
    rules: Vec<Rule>,
    every_path: bool,
}

#[derive(Debug, Clone)]
struct Rule {
    path: Pattern,
    has: Vec<Test>,
    missing: Vec<Test>,
}

#[derive(Debug, Clone)]
enum Pattern {
    Native(regex::Regex),
    /// A regular expression only JavaScript can evaluate (lookarounds, backreferences).
    Script(String),
}

#[derive(Debug, Clone)]
struct Test {
    kind: ConditionKind,
    key: String,
    value: Option<regex::Regex>,
}

impl Test {
    fn new(condition: &Condition) -> Result<Self, String> {
        let key = condition.key.clone().unwrap_or_default();
        if key.is_empty() && condition.kind != ConditionKind::Host {
            return Err(format!("{:?} conditions need a `key`", condition.kind).to_lowercase());
        }
        let value = match &condition.value {
            Some(value) => Some(
                regex::Regex::new(&format!("^(?:{value})$"))
                    .map_err(|e| format!("condition value `{value}` is not a supported regular expression: {e}"))?,
            ),
            None if condition.kind == ConditionKind::Host => return Err("host conditions need a `value`".into()),
            None => None,
        };
        let key = if condition.kind == ConditionKind::Header { key.to_ascii_lowercase() } else { key };
        Ok(Self { kind: condition.kind, key, value })
    }

    fn holds(&self, lookup: &Lookup) -> bool {
        match (lookup(self.kind, &self.key), &self.value) {
            (None, _) => false,
            (Some(_), None) => true,
            (Some(found), Some(value)) => value.is_match(&found),
        }
    }
}

/// Whether a path is matched, or the patterns QuickJS must test it against to decide.
#[derive(Debug, PartialEq, Eq)]
pub enum Verdict<'a> {
    Yes,
    No,
    Ask(Vec<&'a str>),
}

impl PathMatcher {
    /// Compiles matcher rules; an empty list matches every path.
    pub fn new(rules: &[MatcherRule]) -> Result<Self, String> {
        let mut matcher = Self { every_path: rules.is_empty(), ..Self::default() };
        for rule in rules {
            let pattern = compile(&rule.source)?;
            let path = match regex::Regex::new(&pattern) {
                Ok(regex) => Pattern::Native(regex),
                Err(_) => Pattern::Script(pattern),
            };
            let tests = |conditions: &[Condition]| conditions.iter().map(Test::new).collect::<Result<Vec<_>, _>>();
            matcher.rules.push(Rule { path, has: tests(&rule.has)?, missing: tests(&rule.missing)? });
        }
        Ok(matcher)
    }

    /// Checks `path` and, for rules with conditions, the request values `lookup` reads.
    pub fn check(&self, path: &str, lookup: &Lookup) -> Verdict<'_> {
        if self.every_path {
            return Verdict::Yes;
        }
        let mut script = Vec::new();
        for rule in &self.rules {
            if !rule.has.iter().all(|test| test.holds(lookup)) || rule.missing.iter().any(|test| test.holds(lookup)) {
                continue;
            }
            match &rule.path {
                Pattern::Native(regex) if regex.is_match(path) => return Verdict::Yes,
                Pattern::Native(_) => {}
                Pattern::Script(pattern) => script.push(pattern.as_str()),
            }
        }
        if script.is_empty() { Verdict::No } else { Verdict::Ask(script) }
    }
}

/// Translates a path-to-regexp pattern into an anchored regular expression understood by both Rust's `regex`
/// (when it uses no lookaround) and JavaScript. `:name` matches one segment, `:name*` zero or more, `:name+`
/// one or more and `:name?` an optional one; `(...)` after a name, or alone, gives a custom expression.
pub fn compile(source: &str) -> Result<String, String> {
    if !source.starts_with('/') {
        return Err(format!("matcher `{source}` must start with `/`"));
    }
    let chars: Vec<char> = source.chars().collect();
    let mut out = String::from("^");
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        // A `/` right before a parameter becomes part of it, so that `/:path*` also matches nothing.
        let prefixed = c == '/' && matches!(chars.get(i + 1), Some(':' | '('));
        let start = if prefixed { i + 1 } else { i };
        let next = chars[start];
        if next == ':' || next == '(' {
            let mut j = start;
            if next == ':' {
                j += 1;
                let begin = j;
                while j < chars.len() && (chars[j].is_alphanumeric() || chars[j] == '_') {
                    j += 1;
                }
                if j == begin {
                    return Err(format!("matcher `{source}` has a `:` without a parameter name"));
                }
            }
            let pattern = if chars.get(j) == Some(&'(') {
                let (group, end) = group(&chars, j).ok_or_else(|| format!("matcher `{source}` has an unclosed `(`"))?;
                j = end;
                group
            } else {
                "[^/]+?".to_owned()
            };
            let modifier = chars.get(j).copied().filter(|m| matches!(m, '?' | '*' | '+'));
            if modifier.is_some() {
                j += 1;
            }
            let prefix = if prefixed { "/" } else { "" };
            out.push_str(&match modifier {
                None => format!("{prefix}({pattern})"),
                Some('?') => format!("(?:{prefix}({pattern}))?"),
                Some('*') => format!("(?:{prefix}((?:{pattern})(?:{prefix}(?:{pattern}))*))?"),
                _ => format!("(?:{prefix}((?:{pattern})(?:{prefix}(?:{pattern}))*))"),
            });
            i = j;
            continue;
        }
        if c == '\\' && i + 1 < chars.len() {
            out.push_str(&regex::escape(&chars[i + 1].to_string()));
            i += 2;
            continue;
        }
        out.push_str(&regex::escape(&c.to_string()));
        i += 1;
    }
    out.push_str("/?$");
    Ok(out)
}

/// The expression inside the parentheses opening at `open`, and the index after them.
fn group(chars: &[char], open: usize) -> Option<(String, usize)> {
    let mut depth = 0;
    let mut i = open;
    while i < chars.len() {
        match chars[i] {
            '\\' => i += 1,
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some((chars[open + 1..i].iter().collect(), i + 1));
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none(_: ConditionKind, _: &str) -> Option<String> {
        None
    }

    fn matches(source: &str, path: &str) -> bool {
        PathMatcher::new(&[MatcherRule::path(source)]).unwrap().check(path, &none) == Verdict::Yes
    }

    #[test]
    fn translates_parameters() {
        assert!(matches("/about", "/about"));
        assert!(!matches("/about", "/about/team"));
        assert!(matches("/about/:path*", "/about"));
        assert!(matches("/about/:path*", "/about/a/b"));
        assert!(!matches("/about/:path*", "/aboutx"));
        assert!(matches("/blog/:slug", "/blog/x"));
        assert!(!matches("/blog/:slug", "/blog"));
        assert!(matches("/docs/:path+", "/docs/a/b"));
        assert!(!matches("/docs/:path+", "/docs"));
        assert!(matches("/shop/:id?", "/shop"));
        assert!(matches("/post/:id(\\d+)", "/post/12"));
        assert!(!matches("/post/:id(\\d+)", "/post/ab"));
        assert!(matches("/(api|docs)/:rest*", "/api/v1"));
        assert!(matches("/file.json", "/file.json"));
        assert!(!matches("/file.json", "/fileXjson"));
    }

    #[test]
    fn defers_lookarounds_to_javascript() {
        let matcher = PathMatcher::new(&[MatcherRule::path("/((?!api|_volt).*)")]).unwrap();
        let Verdict::Ask(patterns) = matcher.check("/x", &none) else { panic!("expected a script pattern") };
        assert_eq!(patterns, ["^/((?!api|_volt).*)/?$"]);
        assert_eq!(PathMatcher::new(&[]).unwrap().check("/anything", &none), Verdict::Yes);
        assert!(compile("about").is_err());
        assert!(compile("/a/:(x").is_err());
    }

    #[test]
    fn applies_has_and_missing() {
        let condition = |kind, key: Option<&str>, value: Option<&str>| Condition {
            kind,
            key: key.map(str::to_owned),
            value: value.map(str::to_owned),
        };
        let rule = MatcherRule {
            source: "/admin/:path*".into(),
            has: vec![condition(ConditionKind::Header, Some("X-Beta"), Some("on|yes"))],
            missing: vec![condition(ConditionKind::Cookie, Some("session"), None)],
        };
        let matcher = PathMatcher::new(std::slice::from_ref(&rule)).unwrap();
        let request = |header: Option<&'static str>, cookie: Option<&'static str>| {
            move |kind: ConditionKind, key: &str| match kind {
                ConditionKind::Header if key == "x-beta" => header.map(str::to_owned),
                ConditionKind::Cookie if key == "session" => cookie.map(str::to_owned),
                _ => None,
            }
        };
        assert_eq!(matcher.check("/admin/x", &request(Some("yes"), None)), Verdict::Yes);
        assert_eq!(matcher.check("/admin/x", &request(Some("yesno"), None)), Verdict::No);
        assert_eq!(matcher.check("/admin/x", &request(Some("on"), Some("1"))), Verdict::No);
        assert_eq!(matcher.check("/admin/x", &request(None, None)), Verdict::No);
        let host = MatcherRule {
            has: vec![condition(ConditionKind::Host, None, Some(r"(www\.)?example\.com"))],
            ..MatcherRule::path("/")
        };
        let matcher = PathMatcher::new(&[host]).unwrap();
        let on = |name: &'static str| move |kind, _: &str| (kind == ConditionKind::Host).then(|| name.to_owned());
        assert_eq!(matcher.check("/", &on("www.example.com")), Verdict::Yes);
        assert_eq!(matcher.check("/", &on("example.org")), Verdict::No);
        let json = serde_json::to_value([MatcherRule::path("/a"), rule.clone()]).unwrap();
        assert_eq!(json[0], "/a");
        assert_eq!(json[1]["has"][0]["type"], "header");
        assert_eq!(serde_json::from_value::<Vec<MatcherRule>>(json).unwrap()[1], rule);
        assert!(
            PathMatcher::new(&[MatcherRule {
                has: vec![condition(ConditionKind::Query, None, None)],
                ..MatcherRule::path("/")
            }])
            .is_err()
        );
    }
}
