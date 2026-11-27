//! Client and server bundles built with rolldown.

use std::borrow::Cow;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, anyhow};
use arcstr::ArcStr;
use rolldown::plugin::{
    HookLoadArgs, HookLoadOutput, HookLoadReturn, HookResolveIdArgs, HookResolveIdOutput, HookResolveIdReturn,
    HookUsage, Plugin, PluginContext, SharedLoadPluginContext,
};
use rolldown::{
    Bundler, BundlerOptions, InputItem, OutputFormat, Platform, RawMinifyOptions, ResolveOptions, SourceMapType,
};
use rolldown_common::side_effects::HookSideEffects;
use rolldown_common::{ModuleType, Output};

use crate::project::Project;
use crate::routes::{Kind, Module, Route};
use crate::util::{hash, to_slash};

const CLIENT_ENTRY: &str = "volt:client";
const SERVER_ENTRY: &str = "volt:server";
const MEDIA: [&str; 13] =
    ["png", "jpg", "jpeg", "gif", "webp", "avif", "svg", "ico", "woff", "woff2", "ttf", "otf", "mp4"];

/// Next.js import paths served by the volt runtime.
const COMPAT: [(&str, &str); 11] = [
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
    ("next", "volt-rs"),
];

#[derive(Debug)]
struct Shared {
    modules: HashMap<String, String>,
    css: Mutex<BTreeSet<String>>,
    media_dir: PathBuf,
    root: PathBuf,
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
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "asset".into());
        let name = format!("{stem}-{}.{ext}", hash(&bytes));
        let target = self.0.media_dir.join(&name);
        if !target.exists() {
            std::fs::create_dir_all(&self.0.media_dir)?;
            std::fs::write(&target, &bytes)?;
        }
        let url = format!("{}media/{name}", voltrs::manifest::STATIC_PREFIX);
        let code = match imagesize::blob_size(&bytes) {
            Ok(size) if !matches!(ext, "woff" | "woff2" | "ttf" | "otf" | "mp4") => {
                format!("export default {{ src: {url:?}, width: {}, height: {} }};", size.width, size.height)
            }
            _ => format!("export default {url:?};"),
        };
        Self::module(code, false)
    }
}

impl Plugin for VoltPlugin {
    fn name(&self) -> Cow<'static, str> {
        Cow::Borrowed("volt")
    }

    fn register_hook_usage(&self) -> HookUsage {
        HookUsage::ResolveId | HookUsage::Load
    }

    async fn resolve_id(&self, ctx: &PluginContext, args: &HookResolveIdArgs<'_>) -> HookResolveIdReturn {
        if self.0.modules.contains_key(args.specifier) {
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
        match extension(args.id).as_deref() {
            Some("css") => {
                if let Ok(mut css) = self.0.css.lock() {
                    css.insert(strip_query(args.id).to_owned());
                }
                Self::module("", true)
            }
            Some(ext) if MEDIA.contains(&ext) => self.media(args.id, ext),
            _ => Ok(None),
        }
    }
}

pub struct ServerBundle {
    pub file: String,
    pub css: Vec<PathBuf>,
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
}

impl<'a> Bundle<'a> {
    pub fn new(project: &'a Project, routes: &'a [Route], dev: bool) -> Self {
        let document = crate::routes::document(project.pages_dir.as_deref());
        Self { project, routes, document, dev }
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
        Arc::new(Shared {
            modules: self.modules(client),
            css: Mutex::default(),
            media_dir: self.project.static_dir().join("media"),
            root: self.project.root.clone(),
        })
    }

    fn options(&self, entry: &str, client: bool) -> BundlerOptions {
        let build = &self.project.config.build;
        let env = if self.dev { "development" } else { "production" };
        let mut define: Vec<(String, String)> = vec![("process.env.NODE_ENV".into(), format!("{env:?}"))];
        define.extend(crate::env::public().into_iter().map(|(k, v)| (format!("process.env.{k}"), format!("{v:?}"))));
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

        let collected = shared.css.lock().map(|c| c.clone()).unwrap_or_default();
        let mut css: Vec<PathBuf> = Vec::new();
        for id in &chunk.module_ids {
            let id = strip_query(id.as_str());
            if collected.contains(id) && !css.iter().any(|p| p == Path::new(id)) {
                css.push(PathBuf::from(id));
            }
        }
        for id in collected {
            if !css.iter().any(|p| p == Path::new(&id)) {
                css.push(PathBuf::from(id));
            }
        }
        Ok(ServerBundle { file: format!("server/{file}"), css })
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
        let url = |file: &str| format!("{}js/{file}", voltrs::manifest::STATIC_PREFIX);
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
            .find(|c| c.is_entry && facade(c).as_deref() == Some(&format!("\0{CLIENT_ENTRY}")))
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
