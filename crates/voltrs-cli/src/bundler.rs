//! Client and server bundles built with rolldown.

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, anyhow};
use arcstr::ArcStr;
use oxc_allocator::Allocator;
use oxc_ast::ast::Statement;
use oxc_parser::Parser;
use oxc_span::SourceType;
use rolldown::plugin::{
    HookBuildEndArgs, HookLoadArgs, HookLoadOutput, HookLoadReturn, HookNoopReturn, HookResolveIdArgs,
    HookResolveIdOutput, HookResolveIdReturn, HookTransformArgs, HookTransformOutput, HookTransformReturn, HookUsage,
    Plugin, PluginContext, SharedLoadPluginContext, SharedTransformPluginContext,
};
use rolldown::{
    Bundler, BundlerOptions, InputItem, OutputFormat, Platform, RawMinifyOptions, ResolveOptions, SourceMapType,
};
use rolldown_common::side_effects::HookSideEffects;
use rolldown_common::{ModuleType, Output};
use string_wizard::{MagicString, SourceMapOptions};

use crate::css::Stylesheet;
use crate::fonts::{FONT_MODULE, Fonts};
use crate::media::Media;
use crate::project::Project;
use crate::routes::{Kind, Module, Route, Segment, Slot};
use crate::styles::{ClassPart, CssModule, Styles};
use crate::util::{hash, to_slash};

const CLIENT_ENTRY: &str = "volt:client";
const SERVER_ENTRY: &str = "volt:server";
const MEDIA: [&str; 13] =
    ["png", "jpg", "jpeg", "gif", "webp", "avif", "svg", "ico", "woff", "woff2", "ttf", "otf", "mp4"];
const SCRIPTS: [&str; 8] = ["js", "jsx", "ts", "tsx", "mjs", "cjs", "mts", "cts"];

/// Next.js import paths served by the volt runtime.
const COMPAT: [(&str, &str); 15] = [
    ("next/link", "volt-rs/link"),
    ("next/navigation", "volt-rs/navigation"),
    ("next/router", "volt-rs/router"),
    ("next/document", "volt-rs/document"),
    ("next/app", "volt-rs/app"),
    ("next/image", "volt-rs/image"),
    ("next/script", "volt-rs/script"),
    ("next/head", "volt-rs/head"),
    ("next/form", "volt-rs/form"),
    ("next/dynamic", "volt-rs/dynamic"),
    ("next/cache", "volt-rs/cache"),
    ("next/server", "volt-rs/server"),
    ("next/font/google", "volt-rs/font/google"),
    ("next/font/local", "volt-rs/font/local"),
    ("next", "volt-rs"),
];

#[derive(Debug)]
struct Shared {
    modules: HashMap<String, String>,
    /// Stylesheets seen while loading, keyed by module id; ordered later by module evaluation order.
    css: Mutex<BTreeMap<String, Stylesheet>>,
    media: Media,
    styles: Arc<Styles>,
    fonts: Arc<Fonts>,
    root: PathBuf,
    root_layout: Option<String>,
    preload_fonts: Mutex<Vec<String>>,
    /// Module ids in evaluation order, recorded when the build ends.
    order: Mutex<Vec<String>>,
    /// Full messages of load hook errors; rolldown reports only "plugin `volt` threw an error" for them.
    failures: Mutex<Vec<String>>,
}

impl Shared {
    /// Appends the recorded load failures to a rolldown error message.
    fn explain(&self, message: String) -> String {
        let failures = self.failures.lock().map(|f| f.clone()).unwrap_or_default();
        failures.iter().fold(message, |message, failure| format!("{message}\n{failure}"))
    }
}

#[derive(Debug)]
struct VoltPlugin(Arc<Shared>);

fn strip_query(id: &str) -> &str {
    id.split(['?', '#']).next().unwrap_or(id)
}

fn extension(id: &str) -> Option<String> {
    Path::new(strip_query(id)).extension().map(|e| e.to_string_lossy().to_ascii_lowercase())
}

impl VoltPlugin {
    fn module(code: impl Into<ArcStr>, side_effects: bool) -> HookLoadReturn {
        Ok(Some(HookLoadOutput {
            code: code.into(),
            map: None,
            side_effects: side_effects.then_some(HookSideEffects::True),
            module_type: Some(ModuleType::Js),
        }))
    }

    /// Copies an imported media file into the static dir and exports its URL and size.
    fn media(&self, id: &str, ext: &str) -> HookLoadReturn {
        let path = Path::new(strip_query(id));
        let bytes = std::fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
        let url = self.0.media.add(path, &bytes)?;
        let code = match imagesize::blob_size(&bytes) {
            Ok(size) if !matches!(ext, "woff" | "woff2" | "ttf" | "otf" | "mp4") => {
                format!("export default {{ src: {url:?}, width: {}, height: {} }};", size.width, size.height)
            }
            _ => format!("export default {url:?};"),
        };
        Self::module(code, false)
    }

    fn stylesheet(&self, id: &str, sheet: Stylesheet) {
        if let Ok(mut css) = self.0.css.lock() {
            css.insert(id.to_owned(), sheet);
        }
    }

    /// Global stylesheets load as empty modules; CSS modules export their scoped class names.
    fn css(&self, id: &str) -> HookLoadReturn {
        let id = strip_query(id);
        let path = Path::new(id);
        if !crate::styles::is_module(path) {
            self.stylesheet(id, Stylesheet::File(path.to_path_buf()));
            return Self::module("", true);
        }
        let module = self.0.styles.module(path)?;
        self.stylesheet(id, Stylesheet::Compiled(module.css.clone()));
        // Importing a CSS module includes its CSS even when no class is used, so it must not be tree-shaken.
        Self::module(css_module_code(&module), true)
    }

    fn load_module(&self, args: &HookLoadArgs<'_>) -> HookLoadReturn {
        if let Some(name) = args.id.strip_prefix('\0')
            && let Some(code) = self.0.modules.get(name)
        {
            return Self::module(code.clone(), true);
        }
        if let Some(key) = args.id.strip_prefix('\0').and_then(|id| id.strip_prefix(FONT_MODULE)) {
            let font = self.0.fonts.get(key).with_context(|| format!("unknown font {key}"))?;
            self.stylesheet(args.id, Stylesheet::Compiled(font.css.clone()));
            return Self::module("", true);
        }
        match extension(args.id).as_deref() {
            Some(ext) if crate::styles::is_stylesheet(ext) => self.css(args.id),
            Some(ext) if MEDIA.contains(&ext) => self.media(args.id, ext),
            _ => Ok(None),
        }
    }

    /// Font files declared by the root layout and the modules it imports.
    fn root_fonts(&self, ctx: &PluginContext) -> Vec<String> {
        let Some(root) = &self.0.root_layout else { return Vec::new() };
        let prefix = format!("\0{FONT_MODULE}");
        let mut files: Vec<String> = Vec::new();
        for id in execution_order(ctx, root, false) {
            if let Some(font) = id.strip_prefix(&prefix).and_then(|key| self.0.fonts.get(key)) {
                for file in &font.preload {
                    if !files.contains(file) {
                        files.push(file.clone());
                    }
                }
            }
        }
        files
    }
}

/// Modules reachable from `entry` in ES evaluation order: imports before importers, in source order.
fn execution_order(ctx: &PluginContext, entry: &str, dynamic: bool) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut order = Vec::new();
    let mut stack = vec![(entry.to_owned(), false)];
    while let Some((id, expanded)) = stack.pop() {
        if expanded {
            order.push(id);
            continue;
        }
        if !seen.insert(id.clone()) {
            continue;
        }
        stack.push((id.clone(), true));
        if let Some(info) = ctx.get_module_info(&id) {
            let imports =
                info.imported_ids.iter().chain(dynamic.then_some(&info.dynamically_imported_ids).into_iter().flatten());
            let children: Vec<String> = imports.map(|i| i.as_str().to_owned()).collect();
            stack.extend(children.into_iter().rev().filter(|c| !seen.contains(c)).map(|c| (c, false)));
        }
    }
    order
}

/// Bare imports re-adding every stylesheet a module imports with bindings. TypeScript drops imports whose
/// bindings are unused, but importing a stylesheet must include its CSS either way.
fn stylesheet_imports(id: &str, code: &str) -> String {
    if ![".css", ".scss", ".sass"].iter().any(|ext| code.contains(ext)) {
        return String::new();
    }
    let source_type = SourceType::from_path(id).unwrap_or_else(|_| SourceType::tsx());
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, code, source_type).parse();
    let mut kept = String::new();
    for statement in &parsed.program.body {
        let Statement::ImportDeclaration(import) = statement else { continue };
        let Some(specifier) = import.source.value.as_str() else { continue };
        if import.import_kind.is_type()
            || import.specifiers.as_ref().is_none_or(|s| s.is_empty())
            || !extension(specifier).is_some_and(|ext| crate::styles::is_stylesheet(&ext))
        {
            continue;
        }
        kept.push_str(&format!("\nimport {specifier:?};"));
    }
    kept
}

/// JavaScript for a CSS module: `export default { local: "scoped composed..." }`.
fn css_module_code(module: &CssModule) -> String {
    let mut imports: Vec<String> = Vec::new();
    let mut entries = Vec::new();
    for (local, parts) in &module.exports {
        let mut values = Vec::new();
        for part in parts {
            values.push(match part {
                ClassPart::Name(name) => format!("{name:?}"),
                ClassPart::Import { specifier, name } => {
                    let index = match imports.iter().position(|s| s == specifier) {
                        Some(index) => index,
                        None => {
                            imports.push(specifier.clone());
                            imports.len() - 1
                        }
                    };
                    format!("d{index}[{name:?}]")
                }
            });
        }
        entries.push(format!("  {local:?}: {},", values.join(" + \" \" + ")));
    }
    let mut code: String =
        imports.iter().enumerate().map(|(i, specifier)| format!("import d{i} from {specifier:?};\n")).collect();
    code.push_str(&format!("export default {{\n{}\n}};\n", entries.join("\n")));
    code
}

impl Plugin for VoltPlugin {
    fn name(&self) -> Cow<'static, str> {
        Cow::Borrowed("volt")
    }

    fn register_hook_usage(&self) -> HookUsage {
        HookUsage::ResolveId | HookUsage::Load | HookUsage::Transform | HookUsage::BuildEnd
    }

    async fn resolve_id(&self, ctx: &PluginContext, args: &HookResolveIdArgs<'_>) -> HookResolveIdReturn {
        if self.0.modules.contains_key(args.specifier) || args.specifier.starts_with(FONT_MODULE) {
            return Ok(Some(HookResolveIdOutput { id: format!("\0{}", args.specifier).into(), ..Default::default() }));
        }
        let virtual_importer = args.importer.is_some_and(|i| i.starts_with('\0'));
        if virtual_importer && !args.specifier.starts_with('.') {
            let anchor = to_slash(&self.0.root.join("package.json"));
            let resolved = ctx
                .resolve(args.specifier, Some(&anchor), None)
                .await?
                .map_err(|e| anyhow!("cannot resolve `{}`: {e:?}", args.specifier))?;
            return Ok(Some(HookResolveIdOutput { id: resolved.id.as_str().into(), ..Default::default() }));
        }
        Ok(None)
    }

    async fn load(&self, _ctx: SharedLoadPluginContext, args: &HookLoadArgs<'_>) -> HookLoadReturn {
        let result = self.load_module(args);
        if let Err(error) = &result
            && let Ok(mut failures) = self.0.failures.lock()
        {
            failures.push(format!("{error:#}"));
        }
        result
    }

    async fn transform(&self, _ctx: SharedTransformPluginContext, args: &HookTransformArgs<'_>) -> HookTransformReturn {
        if args.id.starts_with('\0') || !extension(args.id).is_some_and(|ext| SCRIPTS.contains(&ext.as_str())) {
            return Ok(None);
        }
        let id = strip_query(args.id);
        let kept = stylesheet_imports(id, args.code);
        let transformed = match crate::fonts::transform(&self.0.fonts, id, args.code)? {
            Some(mut transformed) => {
                transformed.code.push_str(&kept);
                transformed
            }
            None if kept.is_empty() => return Ok(None),
            None => {
                let mut magic = MagicString::new(args.code);
                magic.append(kept);
                let map = magic.source_map(SourceMapOptions {
                    source: id.into(),
                    include_content: true,
                    ..Default::default()
                });
                crate::fonts::Transformed { code: magic.to_string(), map }
            }
        };
        Ok(Some(HookTransformOutput {
            code: Some(transformed.code),
            map: transformed.map.into(),
            ..Default::default()
        }))
    }

    async fn build_end(&self, ctx: &PluginContext, _args: Option<&HookBuildEndArgs<'_>>) -> HookNoopReturn {
        let fonts = self.root_fonts(ctx);
        if let Ok(mut preload) = self.0.preload_fonts.lock() {
            *preload = fonts;
        }
        let order = execution_order(ctx, &format!("\0{SERVER_ENTRY}"), true);
        if let Ok(mut recorded) = self.0.order.lock() {
            *recorded = order;
        }
        Ok(())
    }
}

/// Writes the JavaScript of a route module: imports of every file and the segment tree referencing them.
struct RouteWriter<'b, 'a> {
    bundle: &'b Bundle<'a>,
    imports: String,
    names: usize,
}

impl<'b, 'a> RouteWriter<'b, 'a> {
    fn new(bundle: &'b Bundle<'a>) -> Self {
        Self { bundle, imports: String::new(), names: 0 }
    }

    fn name(&mut self) -> String {
        self.names += 1;
        format!("m{}", self.names - 1)
    }

    fn module(&mut self, module: &Module) -> String {
        let name = self.name();
        self.imports.push_str(&Bundle::import(module, &name));
        name
    }

    /// `[{ layout: m0, slots: { team: { key, default, routes: { id: { segments, page } } } } }, ...]`
    fn segments(&mut self, segments: &[Segment]) -> String {
        let items: Vec<String> = segments
            .iter()
            .map(|segment| {
                let mut fields: Vec<String> = segment
                    .modules()
                    .into_iter()
                    .filter_map(|(key, m)| m.map(|m| format!("{key}: {}", self.module(m))))
                    .collect();
                fields.push(format!("path: {}", serde_json::json!(segment.path)));
                fields.extend(self.bundle.files_field(&segment.files, &segment.path));
                if !segment.slots.is_empty() {
                    let slots: Vec<String> = segment.slots.iter().map(|slot| self.slot(slot)).collect();
                    fields.push(format!("slots: {{ {} }}", slots.join(", ")));
                }
                format!("{{ {} }}", fields.join(", "))
            })
            .collect();
        format!("[{}]", items.join(", "))
    }

    fn slot(&mut self, slot: &Slot) -> String {
        let default = match &slot.default {
            Some(module) => self.module(module),
            None => "null".into(),
        };
        let routes: Vec<String> = slot
            .routes
            .iter()
            .map(|route| {
                let segments = self.segments(&route.segments);
                format!("{:?}: {{ segments: {segments}, page: {} }}", route.id, self.module(&route.page))
            })
            .collect();
        format!("{:?}: {{ key: {:?}, default: {default}, routes: {{ {} }} }}", slot.name, slot.key, routes.join(", "))
    }
}

pub struct ServerBundle {
    pub file: String,
    pub css: Vec<Stylesheet>,
    /// Font files to preload on every page: those declared by the root layout and its imports.
    pub fonts: Vec<String>,
}

pub struct ClientBundle {
    pub entry: String,
    pub preload: Vec<String>,
    pub routes: HashMap<String, Vec<String>>,
}

pub struct Bundle<'a> {
    project: &'a Project,
    routes: &'a [Route],
    /// Pages Router document, imported by the server bundle only.
    document: Option<Module>,
    /// `app/sitemap.ts`, `app/robots.ts` and `app/manifest.ts`, imported by the server bundle only.
    metadata_routes: Vec<(voltrs::seo::MetadataRoute, PathBuf)>,
    dev: bool,
    assets: Assets,
}

/// What the bundles share with the CSS and font steps of the same build.
pub struct Assets {
    pub media: Media,
    pub styles: Arc<Styles>,
    pub fonts: Arc<Fonts>,
}

impl<'a> Bundle<'a> {
    pub fn new(project: &'a Project, routes: &'a [Route], dev: bool, assets: Assets) -> Result<Self> {
        let document = crate::routes::document(project.pages_dir.as_deref());
        let metadata_routes = crate::metadata::routes(project.app_dir.as_deref())?;
        Ok(Self { project, routes, document, metadata_routes, dev, assets })
    }

    /// Names of the code-defined metadata routes, for the manifest.
    pub fn metadata_routes(&self) -> Vec<String> {
        self.metadata_routes.iter().map(|(route, _)| route.name().to_owned()).collect()
    }

    /// Imports a route module as the namespace `name`.
    fn import(module: &Module, name: &str) -> String {
        match module {
            Module::File(path) => format!("import * as {name} from {:?};\n", to_slash(path)),
            Module::Builtin(export) => format!(
                "import {{ {export} as {name}_c, notFoundMetadata as {name}_m }} from \"volt-rs/internal/defaults\";\nconst {name} = {{ default: {name}_c{} }};\n",
                if *export == "NotFound" { format!(", metadata: {name}_m") } else { String::new() }
            ),
        }
    }

    /// Virtual module of a route; its last path segment becomes the chunk name.
    fn route_key(&self, index: usize) -> String {
        let name = |id: &str| -> String {
            let words: Vec<String> =
                id.split(|c: char| !c.is_ascii_alphanumeric()).filter(|s| !s.is_empty()).map(str::to_owned).collect();
            if words.is_empty() { "home".to_owned() } else { words.join("-") }
        };
        let own = name(&self.routes[index].id);
        let taken = self.routes[..index].iter().any(|r| name(&r.id) == own);
        if taken { format!("volt:route/{own}-{index}") } else { format!("volt:route/{own}") }
    }

    /// Imports only the default export, so tree-shaking drops server-only data functions from browser chunks.
    fn import_default(module: &Module, name: &str) -> String {
        match module {
            Module::File(path) => {
                format!("import {name}_d from {:?};\nconst {name} = {{ default: {name}_d }};\n", to_slash(path))
            }
            Module::Builtin(_) => Self::import(module, name),
        }
    }

    /// Virtual module of a route's segments without its page: what client navigations render
    /// while the page's data and chunk load.
    fn shell_key(&self, index: usize) -> String {
        self.route_key(index).replacen("volt:route/", "volt:shell/", 1)
    }

    /// `files: {...}` of a route module: metadata from the files of a segment at `path`.
    fn files_field(&self, files: &[crate::metadata::File], path: &[String]) -> Option<String> {
        let manifest = path
            .is_empty()
            .then(|| self.metadata_routes.iter().find(|(r, _)| *r == voltrs::seo::MetadataRoute::Manifest))
            .flatten()
            .map(|(route, _)| *route);
        crate::metadata::head(files, self.project.config.base_path(), manifest).map(|head| format!("files: {head}"))
    }

    /// Route modules: `{ segments, page }`, with `page: null` for a navigation shell.
    fn route_module(&self, route: &Route, client: bool, with_page: bool) -> String {
        let mut writer = RouteWriter::new(self);
        let segments = writer.segments(&route.segments);
        let shell = match &route.kind {
            Kind::App => {
                let mut fields = vec![format!("path: {}", serde_json::json!(route.path))];
                fields.extend(self.files_field(&route.files, &route.path));
                fields.extend(route.global_error.as_ref().map(|m| format!("globalError: {}", writer.module(m))));
                format!("{}, ", fields.join(", "))
            }
            Kind::Pages { app } => format!("kind: \"pages\", app: {}, ", writer.module(app)),
        };
        let page = if !with_page {
            "null".to_owned()
        } else if client && route.is_pages() {
            let name = writer.name();
            writer.imports.push_str(&Self::import_default(&route.page, &name));
            name
        } else {
            writer.module(&route.page)
        };
        format!("{}export default {{ {shell}segments: {segments}, page: {page} }};\n", writer.imports)
    }

    fn modules(&self, client: bool) -> HashMap<String, String> {
        let mut modules: HashMap<String, String> = self
            .routes
            .iter()
            .enumerate()
            .map(|(i, route)| (self.route_key(i), self.route_module(route, client, true)))
            .collect();
        let lazy: Vec<String> = self
            .routes
            .iter()
            .enumerate()
            .map(|(i, r)| format!("  {:?}: () => import({:?}),", r.id, self.route_key(i)))
            .collect();
        let mut shells = Vec::new();
        for (index, route) in self.routes.iter().enumerate() {
            if client && !route.is_pages() && route.segments.iter().any(|s| s.loading.is_some()) {
                modules.insert(self.shell_key(index), self.route_module(route, client, false));
                shells.push(format!("  {:?}: () => import({:?}),", route.id, self.shell_key(index)));
            }
        }
        modules.insert(
            CLIENT_ENTRY.into(),
            format!(
                "import {{ boot }} from \"volt-rs/internal/client\";\nboot({{\n{}\n}}, {{\n{}\n}});\n",
                lazy.join("\n"),
                shells.join("\n")
            ),
        );
        let imports: Vec<String> =
            (0..self.routes.len()).map(|i| format!("import r{i} from {:?};", self.route_key(i))).collect();
        let table: Vec<String> = self.routes.iter().enumerate().map(|(i, r)| format!("  {:?}: r{i},", r.id)).collect();
        let (document_import, document) = match &self.document {
            Some(module) => (Self::import(module, "doc"), "doc"),
            None => (String::new(), "undefined"),
        };
        let mut metadata_imports = String::new();
        let mut metadata = Vec::new();
        for (index, (route, path)) in self.metadata_routes.iter().enumerate() {
            metadata_imports.push_str(&Self::import(&Module::File(path.clone()), &format!("meta{index}")));
            metadata.push(format!("{}: meta{index}", route.name()));
        }
        modules.insert(
            SERVER_ENTRY.into(),
            format!(
                "import {{ createServer }} from \"volt-rs/internal/server\";\n{}\n{document_import}{metadata_imports}createServer({{\n{}\n}}, {{ document: {document}, metadata: {{ {} }} }});\n",
                imports.join("\n"),
                table.join("\n"),
                metadata.join(", ")
            ),
        );
        modules
    }

    fn shared(&self, client: bool) -> Arc<Shared> {
        // The App Router root layout wraps every app page; `_app` plays that role in the Pages Router.
        let root_layout = self
            .routes
            .iter()
            .find_map(|route| match &route.kind {
                Kind::App => route.segments.first()?.layout.as_ref(),
                Kind::Pages { app } => Some(app),
            })
            .and_then(|module| match module {
                Module::File(path) => Some(to_slash(path)),
                Module::Builtin(_) => None,
            });
        Arc::new(Shared {
            modules: self.modules(client),
            css: Mutex::default(),
            media: self.assets.media.clone(),
            styles: self.assets.styles.clone(),
            fonts: self.assets.fonts.clone(),
            root: self.project.root.clone(),
            root_layout,
            preload_fonts: Mutex::default(),
            order: Mutex::default(),
            failures: Mutex::default(),
        })
    }

    fn options(&self, entry: &str, client: bool) -> BundlerOptions {
        let build = &self.project.config.build;
        let env = if self.dev { "development" } else { "production" };
        let mut define: Vec<(String, String)> = vec![("process.env.NODE_ENV".into(), format!("{env:?}"))];
        define.extend(crate::env::public().into_iter().map(|(k, v)| (format!("process.env.{k}"), format!("{v:?}"))));
        let config = &self.project.config;
        let optimize_images = config.images.optimize && !voltrs::config::is_export();
        define.extend([
            ("process.env.__VOLT_BASE_PATH__".to_owned(), format!("{:?}", config.base_path())),
            ("process.env.__VOLT_TRAILING_SLASH__".to_owned(), build.trailing_slash.to_string()),
            ("process.env.__VOLT_IMAGE_OPTIMIZATION__".to_owned(), optimize_images.to_string()),
        ]);
        define.extend(build.define.iter().map(|(k, v)| (k.clone(), v.clone())));
        let sdk = to_slash(&self.project.sdk_file());
        let mut alias: Vec<(String, Vec<Option<String>>)> =
            COMPAT.iter().map(|(from, to)| ((*from).to_owned(), vec![Some((*to).to_owned())])).collect();
        alias.push(("@volt/api".into(), vec![Some(sdk)]));
        for (from, to) in &build.aliases {
            let target = if to.starts_with('.') { to_slash(&self.project.root.join(to)) } else { to.clone() };
            alias.push((from.clone(), vec![Some(target)]));
        }
        let minify = client && build.minify && !self.dev;
        BundlerOptions {
            input: Some(vec![InputItem {
                name: Some(if client { "main" } else { "server" }.into()),
                import: entry.into(),
            }]),
            cwd: Some(self.project.root.clone()),
            platform: Some(Platform::Browser),
            format: Some(if client { OutputFormat::Esm } else { OutputFormat::Iife }),
            dir: Some(to_slash(&self.project.static_dir().join("js"))),
            entry_filenames: Some("[name]-[hash].js".to_string().into()),
            chunk_filenames: Some("[name]-[hash].js".to_string().into()),
            minify: Some(if minify { RawMinifyOptions::Bool(true) } else { RawMinifyOptions::DeadCodeEliminationOnly }),
            sourcemap: (client && (self.dev || build.sourcemap)).then_some(SourceMapType::File),
            define: Some(define.into_iter().collect()),
            resolve: Some(ResolveOptions {
                alias: Some(alias),
                modules: Some(vec!["node_modules".into(), to_slash(&self.project.node_modules())]),
                ..Default::default()
            }),
            code_splitting: (!client).then_some(rolldown::CodeSplittingMode::Bool(false)),
            tsconfig: Some(rolldown::TsConfig::Auto(true)),
            ..Default::default()
        }
    }

    async fn run(&self, entry: &str, client: bool, shared: Arc<Shared>, write: bool) -> Result<Vec<Output>> {
        let plugin = rolldown::plugin::__inner::Pluginable::new_shared(VoltPlugin(shared.clone()));
        let mut bundler = Bundler::with_plugins(self.options(entry, client), vec![plugin])
            .map_err(|e| anyhow!(crate::util::diagnostics(&e)))?;
        let output = if write { bundler.write().await } else { bundler.generate().await };
        let output = output.map_err(|e| anyhow!(shared.explain(crate::util::diagnostics(&e))))?;
        let unresolved: Vec<String> = output
            .warnings
            .iter()
            .filter(|w| matches!(w.kind(), rolldown_error::EventKind::UnresolvedImport))
            .map(|w| w.to_diagnostic().to_string())
            .collect();
        if !unresolved.is_empty() {
            return Err(anyhow!("{}", unresolved.join("\n")));
        }
        for warning in output.warnings.iter().filter(|w| !crate::util::is_noise(w)) {
            voltrs::log::warn(&warning.to_diagnostic().to_string());
        }
        Ok(output.assets)
    }

    /// Builds the server bundle evaluated by QuickJS. Also reports CSS in execution order.
    pub async fn server(&self) -> Result<ServerBundle> {
        let shared = self.shared(false);
        let assets = self.run(SERVER_ENTRY, false, shared.clone(), false).await?;
        let chunk = assets
            .iter()
            .find_map(|a| match a {
                Output::Chunk(chunk) => Some(chunk),
                Output::Asset(_) => None,
            })
            .context("server bundle produced no chunk")?;
        let dir = self.project.server_dir();
        crate::util::reset_dir(&dir)?;
        let file = format!("server-{}.js", hash(chunk.code.as_bytes()));
        std::fs::write(dir.join(&file), &chunk.code)?;

        let mut collected = shared.css.lock().map(|c| c.clone()).unwrap_or_default();
        let mut css: Vec<Stylesheet> = Vec::new();
        let order = shared.order.lock().map(|o| o.clone()).unwrap_or_default();
        for id in &order {
            if let Some(sheet) = collected.remove(strip_query(id)) {
                css.push(sheet);
            }
        }
        css.extend(collected.into_values());
        let fonts = shared.preload_fonts.lock().map(|f| f.clone()).unwrap_or_default();
        Ok(ServerBundle { file: format!("server/{file}"), css, fonts })
    }

    /// Builds the browser bundle with one lazy chunk per route.
    pub async fn client(&self) -> Result<ClientBundle> {
        crate::util::reset_dir(&self.project.static_dir().join("js"))?;
        let assets = self.run(CLIENT_ENTRY, true, self.shared(true), true).await?;
        let chunks: HashMap<&str, &rolldown_common::OutputChunk> = assets
            .iter()
            .filter_map(|a| match a {
                Output::Chunk(chunk) => Some((chunk.filename.as_str(), chunk.as_ref())),
                Output::Asset(_) => None,
            })
            .collect();
        let prefix = self.project.config.static_prefix();
        let url = |file: &str| format!("{prefix}js/{file}");
        let closure = |start: &str| -> Vec<String> {
            let mut seen = HashSet::new();
            let mut stack = vec![start.to_owned()];
            let mut order = Vec::new();
            while let Some(file) = stack.pop() {
                if !seen.insert(file.clone()) {
                    continue;
                }
                order.push(file.clone());
                if let Some(chunk) = chunks.get(file.as_str()) {
                    stack.extend(chunk.imports.iter().map(ToString::to_string));
                }
            }
            order
        };
        let facade = |chunk: &rolldown_common::OutputChunk| chunk.facade_module_id.as_ref().map(|id| id.to_string());
        let entry_chunk = chunks
            .values()
            .find(|c| c.is_entry && facade(c).as_deref() == Some(format!("\0{CLIENT_ENTRY}").as_str()))
            .context("client bundle has no entry chunk")?;
        let entry_files = closure(&entry_chunk.filename);
        let entry_set: HashSet<&String> = entry_files.iter().collect();
        let mut routes = HashMap::new();
        for (index, route) in self.routes.iter().enumerate() {
            let key = format!("\0{}", self.route_key(index));
            if let Some(chunk) = chunks.values().find(|c| facade(c).as_deref() == Some(key.as_str())) {
                let files: Vec<String> =
                    closure(&chunk.filename).into_iter().filter(|f| !entry_set.contains(f)).map(|f| url(&f)).collect();
                routes.insert(route.id.clone(), files);
            }
        }
        Ok(ClientBundle {
            entry: url(&entry_chunk.filename),
            preload: entry_files.iter().skip(1).map(|f| url(f)).collect(),
            routes,
        })
    }
}
