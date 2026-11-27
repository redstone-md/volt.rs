//! Finds font loader calls in a module with oxc and rewrites them.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Result, bail};
use oxc_allocator::Allocator;
use oxc_ast::ast::{
    ArrayExpressionElement, CallExpression, Expression, IdentifierReference, ImportDeclarationSpecifier,
    ObjectPropertyKind, Statement,
};
use oxc_ast_visit::{Visit, walk};
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType, Span};
use rolldown_sourcemap::SourceMap;
use serde_json::{Map, Value};
use string_wizard::{MagicString, SourceMapOptions};

use super::{FONT_MODULE, Fonts, Loader, Request};

/// A module whose font loader calls were rewritten, with a source map back to the original.
#[derive(Debug)]
pub struct Transformed {
    pub code: String,
    pub map: SourceMap,
}

struct Binding {
    loader: Loader,
    family: String,
}

struct Call {
    span: Span,
    binding: String,
    options: Option<Value>,
}

struct Finder<'b> {
    bindings: &'b HashMap<String, Binding>,
    calls: Vec<Call>,
    stray: Option<String>,
}

impl<'a> Visit<'a> for Finder<'_> {
    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        if let Expression::Identifier(callee) = &call.callee
            && self.bindings.contains_key(callee.name.as_str())
        {
            let options = match call.arguments.first() {
                None => Some(Value::Object(Map::new())),
                Some(argument) => argument.as_expression().and_then(literal),
            };
            self.calls.push(Call { span: call.span, binding: callee.name.to_string(), options });
            return;
        }
        walk::walk_call_expression(self, call);
    }

    fn visit_identifier_reference(&mut self, it: &IdentifierReference<'a>) {
        if self.bindings.contains_key(it.name.as_str()) && self.stray.is_none() {
            self.stray = Some(it.name.to_string());
        }
    }
}

/// Evaluates a literal expression (strings, numbers, booleans, arrays, plain objects) to JSON.
fn literal(expression: &Expression) -> Option<Value> {
    match expression {
        Expression::StringLiteral(s) => s.value.as_str().map(|s| Value::String(s.to_owned())),
        Expression::NumericLiteral(n) => serde_json::Number::from_f64(n.value).map(|number| {
            if n.value.fract() == 0.0 && n.value.abs() < 1e15 {
                Value::from(n.value as i64)
            } else {
                Value::Number(number)
            }
        }),
        Expression::BooleanLiteral(b) => Some(Value::Bool(b.value)),
        Expression::TemplateLiteral(t) if t.expressions.is_empty() => {
            t.quasis.first().and_then(|q| q.value.cooked?.as_str()).map(|s| Value::String(s.to_owned()))
        }
        Expression::ParenthesizedExpression(p) => literal(&p.expression),
        Expression::TSAsExpression(e) => literal(&e.expression),
        Expression::TSSatisfiesExpression(e) => literal(&e.expression),
        Expression::ArrayExpression(array) => array
            .elements
            .iter()
            .map(|element| match element {
                ArrayExpressionElement::SpreadElement(_) | ArrayExpressionElement::Elision(_) => None,
                element => element.as_expression().and_then(literal),
            })
            .collect::<Option<Vec<_>>>()
            .map(Value::Array),
        Expression::ObjectExpression(object) => {
            let mut map = Map::new();
            for property in &object.properties {
                let ObjectPropertyKind::ObjectProperty(property) = property else { return None };
                if property.computed || property.method {
                    return None;
                }
                let key = property.key.static_name()?.as_str()?.to_owned();
                map.insert(key, literal(&property.value)?);
            }
            Some(Value::Object(map))
        }
        _ => None,
    }
}

/// Rewrites the font loader calls of one module. `id` is the module path.
pub fn transform(fonts: &Fonts, id: &str, code: &str) -> Result<Option<Transformed>> {
    if !code.contains("font/google") && !code.contains("font/local") {
        return Ok(None);
    }
    let path = Path::new(id);
    let source_type = SourceType::from_path(path).unwrap_or_else(|_| SourceType::tsx());
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, code, source_type).parse();
    if parsed.diagnostics.has_errors() {
        return Ok(None);
    }

    let mut bindings = HashMap::new();
    let mut imports = Vec::new();
    for statement in &parsed.program.body {
        let Statement::ImportDeclaration(import) = statement else { continue };
        let Some(loader) = import.source.value.as_str().and_then(Loader::from_specifier) else { continue };
        if import.import_kind.is_type() {
            continue;
        }
        imports.push(import.span);
        for specifier in import.specifiers.iter().flatten() {
            match (loader, specifier) {
                (Loader::Google, ImportDeclarationSpecifier::ImportSpecifier(named)) => {
                    let family = named.imported.name().replace('_', " ");
                    bindings.insert(named.local.name.to_string(), Binding { loader, family });
                }
                (Loader::Local, ImportDeclarationSpecifier::ImportDefaultSpecifier(default)) => {
                    bindings.insert(default.local.name.to_string(), Binding { loader, family: String::new() });
                }
                (_, specifier) => bail!(
                    "{id}: unsupported font import `{}`; use `import {{ Inter }} from \"next/font/google\"` or `import localFont from \"next/font/local\"`",
                    &code[specifier.span().start as usize..specifier.span().end as usize]
                ),
            }
        }
    }
    if imports.is_empty() {
        return Ok(None);
    }

    let mut finder = Finder { bindings: &bindings, calls: Vec::new(), stray: None };
    finder.visit_program(&parsed.program);
    if let Some(name) = finder.stray {
        bail!("{id}: font loader `{name}` must be called directly, e.g. `const font = {name}({{ ... }})`");
    }

    let mut edits: Vec<(Span, String)> = Vec::new();
    let mut keys = Vec::new();
    for call in &finder.calls {
        let binding = &bindings[&call.binding];
        let Some(options) = call.options.clone() else {
            bail!("{id}: the options of `{}()` must be a literal object with literal values", call.binding);
        };
        let request =
            Request { loader: binding.loader, family: binding.family.clone(), options, importer: path.into() };
        let key = fonts.load(&request).map_err(|e| anyhow::anyhow!("{id}: {e:#}"))?;
        let font = fonts.get(&key).ok_or_else(|| anyhow::anyhow!("font {key} was not loaded"))?;
        edits.push((call.span, format!("({})", font.object)));
        if !keys.contains(&key) {
            keys.push(key);
        }
    }
    let stylesheets: String = keys.iter().map(|key| format!("import \"{FONT_MODULE}{key}\";")).collect();
    for (index, span) in imports.iter().enumerate() {
        edits.push((*span, if index == 0 { stylesheets.clone() } else { String::new() }));
    }
    let mut magic = MagicString::new(code);
    for (span, replacement) in edits {
        let edited = if replacement.is_empty() {
            magic.remove(span.start, span.end)
        } else {
            magic.update(span.start, span.end, replacement)
        };
        edited.map_err(|e| anyhow::anyhow!("{id}: {e}"))?;
    }
    let map = magic.source_map(SourceMapOptions { source: id.into(), include_content: true, ..Default::default() });
    Ok(Some(Transformed { code: magic.to_string(), map }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::Media;

    #[test]
    fn rewrites_local_font_calls() {
        let dir = std::env::temp_dir().join(format!("volt-font-source-{}", std::process::id()));
        crate::util::reset_dir(&dir).unwrap();
        std::fs::write(dir.join("a.woff2"), b"a").unwrap();
        let fonts = Fonts::new(Media::new(dir.join("media"), voltrs::manifest::STATIC_PREFIX), dir.join("cache"));
        let code = "import localFont from \"next/font/local\";\nimport x from \"./x\";\nconst brand = localFont({ src: [{ path: \"./a.woff2\", weight: 400 }], variable: `--font-brand` } as const);\nexport default brand;\n";
        let id = dir.join("layout.tsx");
        let out = transform(&fonts, &id.to_string_lossy(), code).unwrap().unwrap();
        assert_eq!(out.code.matches("import \"volt:font/").count(), 1);
        assert!(out.code.starts_with("import \"volt:font/") && out.code.contains("\";\nimport x"), "{}", out.code);
        assert!(out.code.contains("const brand = ({\"className\":\"__className_"), "{}", out.code);
        assert!(!out.code.contains("localFont"));
    }

    #[test]
    fn rejects_dynamic_options_and_stray_references() {
        let fonts = Fonts::new(Media::new("/tmp/none".into(), voltrs::manifest::STATIC_PREFIX), "/tmp/none".into());
        let dynamic =
            "import { Inter } from \"next/font/google\";\nconst w = \"400\";\nconst f = Inter({ weight: w });";
        assert!(transform(&fonts, "/p/a.ts", dynamic).unwrap_err().to_string().contains("literal object"));
        let stray = "import { Inter } from \"next/font/google\";\nexport const f = Inter;";
        assert!(transform(&fonts, "/p/a.ts", stray).unwrap_err().to_string().contains("called directly"));
        assert!(transform(&fonts, "/p/a.ts", "import x from \"next/link\";").unwrap().is_none());
    }
}
