//! `middleware.ts` (or Next.js 16 `proxy.ts`) at the project root or in `src/`, with its static `config.matcher`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use oxc_allocator::Allocator;
use oxc_ast::ast::{ArrayExpressionElement, Declaration, Expression, ObjectExpression, ObjectPropertyKind, Statement};
use oxc_parser::Parser;
use oxc_span::SourceType;
use voltrs::manifest::MiddlewareEntry;
use voltrs::matcher::{Condition, ConditionKind, MatcherRule, PathMatcher};

const NAMES: [&str; 2] = ["middleware", "proxy"];
const DIRS: [&str; 2] = ["", "src"];

/// The project's middleware file and the paths it runs on.
#[derive(Debug, Clone)]
pub struct Middleware {
    pub file: PathBuf,
    /// `config.matcher` rules, empty for every path.
    pub matcher: Vec<MatcherRule>,
}

impl Middleware {
    /// Finds the middleware file of `root` and reads its matcher, which must be written as literals as in Next.js.
    pub fn find(root: &Path) -> Result<Option<Self>> {
        let found = DIRS
            .iter()
            .flat_map(|dir| NAMES.iter().map(move |name| (dir, name)))
            .find_map(|(dir, name)| crate::routes::find_file(&root.join(dir), name));
        let Some(file) = found else { return Ok(None) };
        let code = std::fs::read_to_string(&file).with_context(|| format!("cannot read {}", file.display()))?;
        let matcher = matcher(&file, &code).with_context(|| format!("{}", file.display()))?;
        PathMatcher::new(&matcher).map_err(|e| anyhow::anyhow!("{}: {e}", file.display()))?;
        Ok(Some(Self { file, matcher }))
    }

    pub fn entry(&self) -> MiddlewareEntry {
        MiddlewareEntry { matcher: self.matcher.clone() }
    }
}

/// The `matcher` of `export const config`: a string, or an array of strings and `{ source, has, missing }` objects.
fn matcher(path: &Path, code: &str) -> Result<Vec<MatcherRule>> {
    let allocator = Allocator::default();
    let source_type = SourceType::from_path(path).unwrap_or_else(|_| SourceType::ts());
    let parsed = Parser::new(&allocator, code, source_type).parse();
    // The bundler reports syntax errors with their location.
    if parsed.diagnostics.has_errors() {
        return Ok(Vec::new());
    }
    for statement in &parsed.program.body {
        let Statement::ExportDeclaration(export) = statement else { continue };
        let Declaration::VariableDeclaration(variables) = &export.declaration else { continue };
        for declarator in &variables.declarations {
            if declarator.id.get_identifier_name().is_none_or(|name| name.as_str() != "config") {
                continue;
            }
            let Some(Expression::ObjectExpression(object)) = declarator.init.as_ref().map(unwrap) else {
                bail!("`config` must be an object literal");
            };
            for property in &object.properties {
                let ObjectPropertyKind::ObjectProperty(property) = property else { continue };
                if property.key.static_name().as_ref().and_then(|n| n.as_str()) != Some("matcher") {
                    continue;
                }
                return match unwrap(&property.value) {
                    Expression::ArrayExpression(array) => array.elements.iter().map(element).collect(),
                    value => Ok(vec![rule(value)?]),
                };
            }
        }
    }
    Ok(Vec::new())
}

/// The expression inside `as const`, `satisfies T` and parentheses.
fn unwrap<'a, 'b>(expression: &'b Expression<'a>) -> &'b Expression<'a> {
    match expression {
        Expression::TSAsExpression(e) => unwrap(&e.expression),
        Expression::TSSatisfiesExpression(e) => unwrap(&e.expression),
        Expression::ParenthesizedExpression(e) => unwrap(&e.expression),
        other => other,
    }
}

fn element(element: &ArrayExpressionElement) -> Result<MatcherRule> {
    match element.as_expression() {
        Some(expression) => rule(unwrap(expression)),
        None => bail!("matcher entries must be strings or `{{ source }}` objects"),
    }
}

/// A matcher entry: a pattern string, or `{ source: "...", has: [...], missing: [...] }`.
fn rule(expression: &Expression) -> Result<MatcherRule> {
    let Expression::ObjectExpression(object) = expression else { return Ok(MatcherRule::path(string(expression)?)) };
    let source = property(object, "source").ok_or_else(|| anyhow::anyhow!("matcher objects need a `source`"))?;
    let conditions = |name| match property(object, name) {
        Some(Expression::ArrayExpression(array)) => array
            .elements
            .iter()
            .map(|element| match element.as_expression().map(unwrap) {
                Some(Expression::ObjectExpression(object)) => condition(object),
                _ => bail!("`{name}` entries must be `{{ type, key, value }}` objects"),
            })
            .collect(),
        Some(_) => bail!("`{name}` must be an array"),
        None => Ok(Vec::new()),
    };
    Ok(MatcherRule { source: string(source)?, has: conditions("has")?, missing: conditions("missing")? })
}

/// A `has` / `missing` entry.
fn condition(object: &ObjectExpression) -> Result<Condition> {
    let text = |name| property(object, name).map(string).transpose();
    let kind = text("type")?.unwrap_or_default();
    let kind = ConditionKind::parse(&kind)
        .ok_or_else(|| anyhow::anyhow!("condition type `{kind}` must be header, cookie, query or host"))?;
    Ok(Condition { kind, key: text("key")?, value: text("value")? })
}

/// The unwrapped value of the property `name` of an object literal.
fn property<'a, 'b>(object: &'b ObjectExpression<'a>, name: &str) -> Option<&'b Expression<'a>> {
    object.properties.iter().find_map(|property| match property {
        ObjectPropertyKind::ObjectProperty(p) if p.key.static_name()?.as_str() == Some(name) => Some(unwrap(&p.value)),
        _ => None,
    })
}

/// A string literal or a template without substitutions.
fn string(expression: &Expression) -> Result<String> {
    let text = match expression {
        Expression::StringLiteral(literal) => literal.value.as_str(),
        Expression::TemplateLiteral(template) => template.single_quasi().and_then(|q| q.as_str()),
        _ => None,
    };
    match text {
        Some(text) => Ok(text.to_owned()),
        None => bail!("matcher values must be string literals, as Next.js reads them at build time"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(code: &str) -> Result<Vec<String>> {
        Ok(matcher(Path::new("middleware.ts"), code)?.into_iter().map(|rule| rule.source).collect())
    }

    #[test]
    fn reads_static_matchers() {
        let code = r#"
            import { NextResponse } from "next/server";
            export function middleware() { return NextResponse.next(); }
            export const config = { matcher: ["/a/:path*", { source: "/b" }, `/c`] } satisfies object;
        "#;
        assert_eq!(read(code).unwrap(), vec!["/a/:path*", "/b", "/c"]);
        assert_eq!(read("export const config = { matcher: '/x' };").unwrap(), vec!["/x"]);
        assert!(read("export function middleware() {}").unwrap().is_empty());
        assert!(read("const m = '/x'; export const config = { matcher: m };").is_err());
    }

    #[test]
    fn reads_conditions() {
        let code = r#"export const config = {
            matcher: [{ source: "/a", has: [{ type: "header", key: "x-beta", value: "on" }], missing: [{ type: "cookie", key: "s" }] }],
        };"#;
        let rules = matcher(Path::new("middleware.ts"), code).unwrap();
        let header = Condition { kind: ConditionKind::Header, key: Some("x-beta".into()), value: Some("on".into()) };
        assert_eq!(rules[0].has, [header]);
        assert_eq!(rules[0].missing, [Condition { kind: ConditionKind::Cookie, key: Some("s".into()), value: None }]);
        let bad = r#"export const config = { matcher: [{ source: "/a", has: [{ type: "path", key: "x" }] }] };"#;
        assert!(matcher(Path::new("middleware.ts"), bad).is_err());
    }
}
