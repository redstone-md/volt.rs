//! React Fast Refresh instrumentation of app modules in development client builds.

use std::path::{Path, PathBuf};

use oxc_allocator::Allocator;
use oxc_codegen::{Codegen, CodegenOptions};
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::SourceType;
use oxc_transformer::{JsxOptions, ReactRefreshOptions, TransformOptions, Transformer};
use rolldown_sourcemap::SourceMap;

use crate::fonts::Transformed;

/// The runtime module the instrumented code registers with.
pub const RUNTIME: &str = "volt-rs/internal/refresh";

/// Whether a module belongs to the app and is instrumented: dependencies and generated files never are.
pub fn is_app_module(root: &Path, id: &str) -> bool {
    let id = crate::util::to_slash(Path::new(id));
    let root = crate::util::to_slash(root);
    id.strip_prefix(&root)
        .and_then(|rest| rest.strip_prefix('/'))
        .is_some_and(|rest| !is_dependency(rest) && !rest.split('/').any(|part| part == ".volt"))
}

/// Whether a module is a file of an installed package.
pub fn is_dependency(id: &str) -> bool {
    !id.starts_with('\0') && crate::util::to_slash(Path::new(id)).split('/').any(|part| part == "node_modules")
}

/// The id a module registers its components under: its path relative to the project root.
pub fn module_id(root: &Path, id: &str) -> String {
    let id = crate::util::to_slash(Path::new(id));
    let root = format!("{}/", crate::util::to_slash(root));
    id.strip_prefix(&root).unwrap_or(&id).to_owned()
}

/// Adds the `$RefreshReg$` / `$RefreshSig$` calls of the React Refresh transform and binds them to the volt
/// runtime under `id`. Returns `None` for code that does not parse or is not an ES module: rolldown reports
/// the syntax error itself, and CommonJS cannot take the runtime import.
pub fn instrument(path: &str, code: &str, id: &str) -> Option<Transformed> {
    let source_type = SourceType::from_path(path).unwrap_or_else(|_| SourceType::tsx());
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, code, source_type).parse();
    if parsed.fatal_error || parsed.diagnostics.has_errors() || !parsed.module_record.has_module_syntax {
        return None;
    }
    let mut program = parsed.program;
    let scoping = SemanticBuilder::new().build(&program).semantic.into_scoping();
    // Only the refresh pass runs: TypeScript and JSX stay for rolldown, which compiles them with the
    // project's tsconfig.
    program.source_type = source_type.with_javascript(true);
    let options = TransformOptions {
        jsx: JsxOptions { refresh: Some(ReactRefreshOptions::default()), ..JsxOptions::disable() },
        ..TransformOptions::default()
    };
    let transformed = Transformer::new(&allocator, Path::new(path), &options).build_with_scoping(scoping, &mut program);
    if transformed.diagnostics.has_errors() {
        return None;
    }
    program.source_type = source_type;
    let generated = Codegen::new()
        .with_options(CodegenOptions { source_map_path: Some(PathBuf::from(path)), ..CodegenOptions::default() })
        .with_scoping(Some(transformed.scoping))
        .build(&program);
    let map: SourceMap = generated.map?.into_owned();
    let mut code = generated.code;
    code.push_str(&prelude(id));
    Some(Transformed { code, map })
}

/// Declarations the transform's calls resolve to. Appended so the source map of the module stays exact:
/// function declarations are hoisted and imports are bound before the module body runs.
fn prelude(id: &str) -> String {
    let id = serde_json::to_string(id).unwrap_or_default();
    format!(
        "\nimport * as __volt_refresh from {RUNTIME:?};\n\
         function $RefreshReg$(type, name) {{ __volt_refresh.register(type, {id} + \" \" + name); }}\n\
         function $RefreshSig$() {{ return __volt_refresh.signature(); }}\n\
         __volt_refresh.loaded(import.meta.url);\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instruments_components_and_keeps_typescript() {
        let code = "import type { Props } from \"./types\";\nimport { useState } from \"react\";\nexport default function Counter({ n }: { n: number }) {\n  const [count] = useState(n);\n  return <p>{count}</p>;\n}\n";
        let out = instrument("/p/app/page.tsx", code, "app/page.tsx").unwrap();
        assert!(out.code.contains("n: number;"), "{}", out.code);
        assert!(out.code.contains("<p>"), "{}", out.code);
        assert!(out.code.contains("import type { Props }"), "{}", out.code);
        assert!(out.code.contains("$RefreshReg$(_c, \"Counter\")"), "{}", out.code);
        assert!(out.code.contains("$RefreshSig$()"), "{}", out.code);
        assert!(out.code.contains("__volt_refresh.register(type, \"app/page.tsx\" + \" \" + name)"));
        assert!(out.code.contains("__volt_refresh.loaded(import.meta.url)"));
    }

    #[test]
    fn skips_scripts_and_syntax_errors() {
        assert!(instrument("/p/app/a.js", "module.exports = 1;", "app/a.js").is_none());
        assert!(instrument("/p/app/a.tsx", "export const = ;", "app/a.tsx").is_none());
    }

    #[test]
    fn selects_app_modules() {
        let root = Path::new("/p");
        assert!(is_app_module(root, "/p/app/page.tsx"));
        assert!(is_app_module(root, "/p/components/ui/button.tsx"));
        assert!(!is_app_module(root, "/p/node_modules/volt-rs/src/link.js"));
        assert!(!is_app_module(root, "/p/.volt/types/api.ts"));
        assert!(!is_app_module(root, "/other/app/page.tsx"));
        assert!(is_dependency("/p/node_modules/react/index.js"));
        assert!(!is_dependency("/p/app/node_modules.tsx"));
        assert!(!is_dependency("\0rolldown/runtime.js"));
        assert_eq!(module_id(root, "/p/app/page.tsx"), "app/page.tsx");
    }
}
