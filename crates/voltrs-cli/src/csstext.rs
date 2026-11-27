//! Text-level rewrites of CSS and Sass sources, for steps that run before or between parsers.

pub fn is_remote(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://") || url.starts_with("//")
}

/// Whether a `url()` points at a file next to the stylesheet rather than the site or the network.
pub fn is_local(url: &str) -> bool {
    !(url.is_empty()
        || is_remote(url)
        || url.starts_with('/')
        || url.starts_with('#')
        || url.starts_with("data:")
        || url.contains("://"))
}

fn is_ident(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'
}

/// Whether `keyword` (lowercase) starts at `at`, as a whole word.
fn keyword(source: &str, at: usize, keyword: &str) -> bool {
    let bytes = source.as_bytes();
    let end = at + keyword.len();
    end <= bytes.len()
        && bytes[at..end].eq_ignore_ascii_case(keyword.as_bytes())
        && (at == 0 || !is_ident(bytes[at - 1]) || keyword.starts_with('@'))
        && bytes.get(end).is_none_or(|b| !is_ident(*b))
}

/// The index after a `/* */` comment starting at `at`.
fn comment_end(source: &str, at: usize) -> usize {
    source[at + 2..].find("*/").map_or(source.len(), |end| at + 2 + end + 2)
}

/// The index after a quoted string starting at `at`.
fn string_end(bytes: &[u8], at: usize) -> usize {
    let quote = bytes[at];
    let mut i = at + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b if b == quote || b == b'\n' => return i + 1,
            _ => i += 1,
        }
    }
    bytes.len()
}

/// The index after the `;` ending a statement that starts at `at`, or before the `{`/`}` that cuts it short.
fn statement_end(source: &str, at: usize) -> usize {
    let bytes = source.as_bytes();
    let mut parens = 0usize;
    let mut i = at;
    while i < bytes.len() {
        match bytes[i] {
            b'/' if bytes.get(i + 1) == Some(&b'*') => i = comment_end(source, i),
            b'"' | b'\'' => i = string_end(bytes, i),
            b'\\' => i += 2,
            b'(' => {
                parens += 1;
                i += 1;
            }
            b')' => {
                parens = parens.saturating_sub(1);
                i += 1;
            }
            b';' if parens == 0 => return i + 1,
            b'{' | b'}' if parens == 0 => return i,
            _ => i += 1,
        }
    }
    bytes.len()
}

/// Moves every top-level `@import` to the start and drops `@charset`. Joining stylesheets leaves imports
/// after other rules, where CSS ignores them.
pub fn hoist_imports(css: &str) -> String {
    let bytes = css.as_bytes();
    let mut imports: Vec<&str> = Vec::new();
    let mut rest = String::with_capacity(css.len());
    let mut depth = 0usize;
    let (mut last, mut i) = (0, 0);
    let mut moved = false;
    while i < bytes.len() {
        match bytes[i] {
            b'/' if bytes.get(i + 1) == Some(&b'*') => i = comment_end(css, i),
            b'"' | b'\'' => i = string_end(bytes, i),
            b'\\' => i += 2,
            b'{' => {
                depth += 1;
                i += 1;
            }
            b'}' => {
                depth = depth.saturating_sub(1);
                i += 1;
            }
            b'@' if depth == 0 && (keyword(css, i, "@import") || keyword(css, i, "@charset")) => {
                let end = statement_end(css, i);
                rest.push_str(&css[last..i]);
                let statement = css[i..end].trim();
                if keyword(css, i, "@import") && !imports.contains(&statement) {
                    imports.push(statement);
                }
                moved = true;
                last = end;
                i = end;
            }
            _ => i += 1,
        }
    }
    if !moved {
        return css.to_owned();
    }
    rest.push_str(&css[last.min(css.len())..]);
    let mut out: String = imports.iter().map(|import| format!("{import}\n")).collect();
    out.push_str(rest.trim_start());
    out
}

/// Whether a reference in a Sass partial is a plain relative path that can be rebased.
fn rebasable(value: &str) -> bool {
    is_local(value) && !value.starts_with('~') && !value.contains("#{") && !value.contains(['$', '(', ')'])
}

/// Prefixes relative `url()`s and plain-CSS `@import "*.css"`s with `prefix`, the path from the compiled
/// entry's directory to the partial's. Sass inlines partials without rebasing them, like `resolve-url-loader` fixes.
pub fn rebase_urls(source: &str, prefix: &str) -> String {
    if prefix.is_empty() {
        return source.to_owned();
    }
    let bytes = source.as_bytes();
    let mut out = String::with_capacity(source.len());
    let (mut last, mut i) = (0, 0);
    let rewrite = |out: &mut String, last: &mut usize, start: usize, end: usize| {
        if rebasable(&source[start..end]) {
            out.push_str(&source[*last..start]);
            out.push_str(prefix);
            out.push_str(&source[start..end]);
            *last = end;
        }
    };
    while i < bytes.len() {
        match bytes[i] {
            b'/' if bytes.get(i + 1) == Some(&b'*') => i = comment_end(source, i),
            b'"' | b'\'' => i = string_end(bytes, i),
            b'\\' => i += 2,
            b'u' | b'U' if keyword(source, i, "url") && bytes.get(i + 3) == Some(&b'(') => {
                let mut start = i + 4;
                while bytes.get(start).is_some_and(u8::is_ascii_whitespace) {
                    start += 1;
                }
                let quoted = matches!(bytes.get(start), Some(b'"' | b'\''));
                let (value_end, next) = if quoted {
                    let next = string_end(bytes, start);
                    start += 1;
                    (next.saturating_sub(1).max(start), next)
                } else {
                    let close = source[start..].find(')').map_or(source.len(), |e| start + e);
                    (start + source[start..close].trim_end().len(), close)
                };
                rewrite(&mut out, &mut last, start, value_end);
                i = next.max(i + 4);
            }
            b'@' if keyword(source, i, "@import") => {
                let mut start = i + "@import".len();
                while bytes.get(start).is_some_and(u8::is_ascii_whitespace) {
                    start += 1;
                }
                if !matches!(bytes.get(start), Some(b'"' | b'\'')) {
                    i = start;
                    continue;
                }
                let end = string_end(bytes, start);
                let value_end = end.saturating_sub(1).max(start + 1);
                if source[start + 1..value_end].ends_with(".css") {
                    rewrite(&mut out, &mut last, start + 1, value_end);
                }
                i = end;
            }
            _ => i += 1,
        }
    }
    out.push_str(&source[last..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_urls() {
        assert!(is_local("./a.png") && is_local("a.png") && !is_local("/a.png") && !is_local("data:x"));
        assert!(is_remote("https://x") && !is_local("//cdn/x.png"));
    }

    #[test]
    fn hoists_imports() {
        let css = "@charset \"utf-8\";\n.a{color:red}\n@import url(\"https://f.test/css?a=1;b\");\n@media x{@import \"no\";}\n.b{background:url(\"@import\")}\n@import 'x.css' screen;";
        let out = hoist_imports(css);
        assert!(
            out.starts_with("@import url(\"https://f.test/css?a=1;b\");\n@import 'x.css' screen;\n.a{color:red}"),
            "{out}"
        );
        assert!(out.contains("@media x{@import \"no\";}"), "{out}");
        assert!(out.contains("url(\"@import\")"), "{out}");
        assert!(!out.contains("@charset"), "{out}");
        assert_eq!(hoist_imports(".a{}"), ".a{}");
    }

    #[test]
    fn rebases_partial_urls() {
        let scss = ".a { background: url(\"./img/p.png\"); mask: url( b.svg ); }\n.c { background: url(/abs.png), url(data:x), url(#{$v}), url($v); }\n@import \"./plain.css\";\n@import \"theme\";\n/* url(skip.png) */";
        let out = rebase_urls(scss, "parts/");
        assert!(out.contains("url(\"parts/./img/p.png\")"), "{out}");
        assert!(out.contains("url( parts/b.svg )"), "{out}");
        assert!(out.contains("url(/abs.png), url(data:x), url(#{$v}), url($v)"), "{out}");
        assert!(out.contains("@import \"parts/./plain.css\""), "{out}");
        assert!(out.contains("@import \"theme\""), "{out}");
        assert!(out.contains("url(skip.png)"), "{out}");
        assert_eq!(rebase_urls(scss, ""), scss);
    }
}
