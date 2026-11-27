//! `config.matcher` of `middleware.ts`: Next.js path patterns (`/about/:path*`, `/(api|docs)/:id(\d+)`).

/// Which paths middleware runs on.
#[derive(Debug, Clone, Default)]
pub struct PathMatcher {
    native: Vec<regex::Regex>,
    /// Patterns whose regular expressions only JavaScript can evaluate (lookarounds, backreferences).
    script: Vec<String>,
    every_path: bool,
}

/// Whether a path is matched, or the patterns QuickJS must test it against to decide.
#[derive(Debug, PartialEq, Eq)]
pub enum Verdict<'a> {
    Yes,
    No,
    Ask(&'a [String]),
}

impl PathMatcher {
    /// Compiles matcher sources; an empty list matches every path.
    pub fn new(sources: &[String]) -> Result<Self, String> {
        let mut matcher = Self { every_path: sources.is_empty(), ..Self::default() };
        for source in sources {
            let pattern = compile(source)?;
            match regex::Regex::new(&pattern) {
                Ok(regex) => matcher.native.push(regex),
                Err(_) => matcher.script.push(pattern),
            }
        }
        Ok(matcher)
    }

    pub fn check(&self, path: &str) -> Verdict<'_> {
        if self.every_path || self.native.iter().any(|regex| regex.is_match(path)) {
            Verdict::Yes
        } else if self.script.is_empty() {
            Verdict::No
        } else {
            Verdict::Ask(&self.script)
        }
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

    fn matches(source: &str, path: &str) -> bool {
        PathMatcher::new(&[source.to_owned()]).unwrap().check(path) == Verdict::Yes
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
        let matcher = PathMatcher::new(&["/((?!api|_volt).*)".to_owned()]).unwrap();
        let Verdict::Ask(patterns) = matcher.check("/x") else { panic!("expected a script pattern") };
        assert_eq!(patterns, ["^/((?!api|_volt).*)/?$"]);
        assert_eq!(PathMatcher::new(&[]).unwrap().check("/anything"), Verdict::Yes);
        assert!(compile("about").is_err());
        assert!(compile("/a/:(x").is_err());
    }
}
