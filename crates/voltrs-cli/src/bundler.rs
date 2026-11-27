//! Client and server bundles built with rolldown.

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, anyhow};
use arcstr::ArcStr;
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

use crate::css::Stylesheet;
use crate::fonts::{FONT_MODULE, Fonts};
use crate::media::Media;
use crate::project::Project;
use crate::routes::{Kind, Module, Route};
use crate::styles::{ClassPart, CssModule, Styles};
use crate::util::{hash, to_slash};

const CLIENT_ENTRY: &str = "volt:client";
const SERVER_ENTRY: &str = "volt:server";
const MEDIA: [&str; 13] =
    ["png", "jpg", "jpeg", "gif", "webp", "avif", "svg", "ico", "woff", "woff2", "ttf", "otf", "mp4"];
const SCRIPTS: [&str; 8] = ["js", "jsx", "ts", "tsx", "mjs", "cjs", "mts", "cts"];

/// Next.js import paths served by the volt runtime.
const COMPAT: [(&str, &str); 13] = [
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
        let url = self.0.media.copy(path)?;
        let bytes = std::fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
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
        Self::module(css_module_code(&module), false)
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

    async fn transform(&self, _ctx: SharedTransformPluginContext, args: &HookTransformArgs<'_>) -> HookTransformReturn {
        if args.id.starts_with('\0') || !extension(args.id).is_some_and(|ext| SCRIPTS.contains(&ext.as_str())) {
            return Ok(None);
        }
        let transformed = crate::fonts::transform(&self.0.fonts, strip_query(args.id), args.code)?;
        Ok(transformed.map(|t| HookTransformOutput { code: Some(t.code), map: t.map.into(), ..Default::default() }))
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
    dev: bool,
    styles: Arc<Styles>,
    fonts: Arc<Fonts>,
}

impl<'a> Bundle<'a> {
    pub fn new(project: &'a Project, routes: &'a [Route], dev: bool, styles: Arc<Styles>, fonts: Arc<Fonts>) -> Self {
        let document = crate::routes::document(project.pages_dir.as_deref());
        Self { project, routes, document, dev, styles, fonts }
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

    fn route_module(route: &Route, client: bool) -> String {
        let mut imports = String::new();
        let mut names = 0;
        let mut module = |module: &Module| -> String {
            let name = format!("m{names}");
            names += 1;
            imports.push_str(&Self::import(module, &name));
            name
        };
        let segments: Vec<String> = route
            .segments
            .iter()
            .map(|segment| {
                let fields: Vec<String> = segment
                    .modules()
                    .into_iter()
                    .filter_map(|(key, m)| m.map(|m| format!("{key}: {}", module(m))))
                    .collect();
                format!("{{ {} }}", fields.join(", "))
            })
            .collect();
        let shell = match &route.kind {
            Kind::App => String::new(),
            Kind::Pages { app } => format!("kind: \"pages\", app: {}, ", module(app)),
        };
        let page = if client && route.is_pages() {
            let name = format!("m{names}");
            imports.push_str(&Self::import_default(&route.page, &name));
            name
        } else {
            module(&route.page)
        };
        format!("{imports}export default {{ {shell}segments: [{}], page: {page} }};\n", segments.join(", "))
    }

    fn modules(&self, client: bool) -> HashMap<String, String> {
        let mut modules: HashMap<String, String> = self
            .routes
            .iter()
            .enumerate()
            .map(|(i, route)| (self.route_key(i), Self::route_module(route, client)))
            .collect();
        let lazy: Vec<String> = self
            .routes
            .iter()
            .enumerate()
            .map(|(i, r)| format!("  {:?}: () => import({:?}),", r.id, self.route_key(i)))
            .collect();
        modules.insert(
            CLIENT_ENTRY.into(),
            format!("import {{ boot }} from \"volt-rs/internal/client\";\nboot({{\n{}\n}});\n", lazy.join("\n")),
        );
        let imports: Vec<String> =
            (0..self.routes.len()).map(|i| format!("import r{i} from {:?};", self.route_key(i))).collect();
        let table: Vec<String> = self.routes.iter().enumerate().map(|(i, r)| format!("  {:?}: r{i},", r.id)).collect();
        let (document_import, document) = match &self.document {
            Some(module) => (Self::import(module, "doc"), "doc"),
            None => (String::new(), "undefined"),
        };
        modules.insert(
            SERVER_ENTRY.into(),
            format!(
                "import {{ createServer }} from \"volt-rs/internal/server\";\n{}\n{document_import}createServer({{\n{}\n}}, {{ document: {document} }});\n",
                imports.join("\n"),
                table.join("\n")
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
            media: Media::new(self.project.static_dir().join("media"), self.project.config.static_prefix()),
            styles: self.styles.clone(),
            fonts: self.fonts.clone(),
            root: self.project.root.clone(),
            root_layout,
            preload_fonts: Mutex::default(),
            order: Mutex::default(),
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
        let plugin = rolldown::plugin::__inner::Pluginable::new_shared(VoltPlugin(shared));
        let mut bundler = Bundler::with_plugins(self.options(entry, client), vec![plugin])
            .map_err(|e| anyhow!(crate::util::diagnostics(&e)))?;
        let output = if write { bundler.write().await } else { bundler.generate().await };
        let output = output.map_err(|e| anyhow!(crate::util::diagnostics(&e)))?;
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
