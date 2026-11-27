//! `middleware.ts` (or Next.js 16 `proxy.ts`) at the project root or in `src/`, with its static `config.matcher`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use oxc_allocator::Allocator;
use oxc_ast::ast::{ArrayExpressionElement, Declaration, Expression, ObjectPropertyKind, Statement};
use oxc_parser::Parser;
use oxc_span::SourceType;
use voltrs::manifest::MiddlewareEntry;
use voltrs::matcher::PathMatcher;

const NAMES: [&str; 2] = ["middleware", "proxy"];
const DIRS: [&str; 2] = ["", "src"];

/// The project's middleware file and the paths it runs on.
#[derive(Debug, Clone)]
pub struct Middleware {
    pub file: PathBuf,
    /// `config.matcher` patterns, empty for every path.
    pub matcher: Vec<String>,
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

/// The `matcher` of `export const config`: a string, or an array of strings and `{ source }` objects.
fn matcher(path: &Path, code: &str) -> Result<Vec<String>> {
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
                    value => Ok(vec![source(value)?]),
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

fn element(element: &ArrayExpressionElement) -> Result<String> {
    match element.as_expression() {
        Some(expression) => source(unwrap(expression)),
        None => bail!("matcher entries must be strings or `{{ source }}` objects"),
    }
}

/// A matcher pattern: a string literal, a template without substitutions, or `{ source: "..." }`.
fn source(expression: &Expression) -> Result<String> {
    let text = match expression {
        Expression::StringLiteral(literal) => literal.value.as_str(),
        Expression::TemplateLiteral(template) => template.single_quasi().and_then(|q| q.as_str()),
        Expression::ObjectExpression(object) => {
            let source = object.properties.iter().find_map(|property| match property {
                ObjectPropertyKind::ObjectProperty(p) if p.key.static_name()?.as_str() == Some("source") => {
                    Some(&p.value)
                }
                _ => None,
            });
            return match source {
                Some(value) => self::source(unwrap(value)),
                None => bail!("matcher objects need a `source`"),
            };
        }
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
        matcher(Path::new("middleware.ts"), code)
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
}
