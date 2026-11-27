//! The build pipeline shared by `volt dev`, `volt build` and `volt export`.

use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result};
use serde_json::Value;
use voltrs::manifest::{ClientAssets, Manifest, RouteEntry, prerender_file};
use voltrs::router::{Params, RoutePattern};
use voltrs::sdk::{SERVER_INFO_FILE, Sdk, ServerInfo};
use voltrs::ssr::{Engine, EngineOptions};

use crate::bundler::{Assets, Bundle};
use crate::css::Css;
use crate::fonts::Fonts;
use crate::media::Media;
use crate::project::Project;
use crate::routes::Route;
use crate::styles::Styles;
use crate::util::hash;

/// Route exports read from the server bundle by `__VOLT__.routes()`.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct RouteConfig {
    id: String,
    revalidate: Option<u64>,
    dynamic: String,
    #[serde(default = "enabled")]
    dynamic_params: bool,
    #[serde(default)]
    static_params: bool,
    /// A Pages Router page with `getStaticProps`.
    #[serde(default, rename = "static")]
    is_static: bool,
}

fn enabled() -> bool {
    true
}

/// Result of `__VOLT__.staticParams(id)`: `generateStaticParams` or `getStaticPaths` output.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct StaticParams {
    #[serde(default)]
    params: Vec<Value>,
    #[serde(default)]
    paths: Vec<String>,
    dynamic_params: Option<bool>,
}

impl StaticParams {
    /// Concrete URL paths for `pattern`, skipping entries that miss a param.
    fn resolve(self, pattern: &RoutePattern) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for value in &self.params {
            match Params::from_json(value).and_then(|params| pattern.href(&params)) {
                Some(path) => out.push(path),
                None => voltrs::log::warn(&format!("{}: ignored static params {value}", pattern.as_str())),
            }
        }
        for path in self.paths {
            if pattern.matches(&path).is_some() {
                out.push(path);
            } else {
                voltrs::log::warn(&format!("{}: static path `{path}` does not match the route", pattern.as_str()));
            }
        }
        let mut seen = std::collections::HashSet::new();
        out.retain(|path| seen.insert(path.clone()));
        out
    }
}

pub struct Pipeline<'a> {
    project: &'a Project,
    dev: bool,
}

impl<'a> Pipeline<'a> {
    pub fn new(project: &'a Project, dev: bool) -> Self {
        Self { project, dev }
    }

    /// Writes `@volt/api` from the Rust server binary, or an empty one for frontend-only apps.
    pub fn sdk(&self, binary: Option<&Path>) -> Result<ServerInfo> {
        let dir = self.project.types_dir();
        match binary {
            Some(binary) => crate::cargo::emit_sdk(binary, &self.project.root, &dir)?,
            None => Sdk::generate().write(&dir)?,
        }
        let text = std::fs::read_to_string(dir.join(SERVER_INFO_FILE))?;
        Ok(serde_json::from_str(&text)?)
    }

    pub async fn frontend(&self, info: &ServerInfo) -> Result<Manifest> {
        let started = Instant::now();
        let routes = self.project.routes()?;
        if !self.dev {
            crate::util::reset_dir(&self.project.static_dir())?;
        }
        let media = Media::new(self.project.static_dir().join("media"), self.project.config.static_prefix());
        let styles = Arc::new(Styles::new(self.project, media.clone())?);
        let fonts = Arc::new(Fonts::new(media.clone(), self.project.cache_dir()));
        let bundle = Bundle::new(self.project, &routes, self.dev, Assets { media, styles: styles.clone(), fonts })?;
        let (server, client) = tokio::join!(bundle.server(), bundle.client());
        let (server, client) = (server?, client?);
        let css = Css::new(self.project, &styles, !self.dev).build(&server.css)?;
        let files = routes.iter().flat_map(|r| r.segments.iter().flat_map(|s| &s.files).chain(&r.files));
        let metadata = crate::metadata::write(files, &self.project.out_dir)?;

        let mut manifest = Manifest {
            build_id: String::new(),
            dev: self.dev,
            server: server.file.clone(),
            client: ClientAssets {
                entry: client.entry,
                css: css.into_iter().collect(),
                preload: client.preload,
                fonts: server.fonts,
            },
            routes: routes
                .iter()
                .map(|r| RouteEntry {
                    id: r.id.clone(),
                    chunks: client.routes.get(&r.id).cloned().unwrap_or_default(),
                    dynamic: r.is_dynamic() || info.loaders.contains(&r.id),
                    pages: r.is_pages(),
                    ..RouteEntry::default()
                })
                .collect(),
            metadata,
            metadata_routes: bundle.metadata_routes(),
            error: None,
        };
        let fingerprint = format!("{}{}{:?}", manifest.server, manifest.client.entry, manifest.client.css);
        manifest.build_id = hash(fingerprint.as_bytes());
        manifest.save(&self.project.out_dir)?;

        if !self.dev {
            self.apply_route_config(&mut manifest, &routes, info).await?;
            manifest.save(&self.project.out_dir)?;
            if self.project.config.build.prerender || voltrs::config::is_export() {
                self.prerender(&mut manifest, info).await?;
            }
            manifest.save(&self.project.out_dir)?;
            let compressed = crate::compress::precompress(&self.project.static_dir())?;
            voltrs::log::ready(&format!("compressed {compressed} assets"));
        }
        voltrs::log::ready(&format!("built {} routes in {}ms", routes.len() - 2, started.elapsed().as_millis()));
        Ok(manifest)
    }

    /// Reads route exports (`revalidate`, `dynamic`, `dynamicParams`) and build-time paths
    /// (`generateStaticParams`, `getStaticPaths`) from the built server bundle.
    async fn apply_route_config(&self, manifest: &mut Manifest, routes: &[Route], info: &ServerInfo) -> Result<()> {
        let engine = Engine::start(EngineOptions {
            bundle: self.project.out_dir.join(&manifest.server),
            workers: 1,
            memory_limit: self.project.config.server.memory_limit_mb * 1024 * 1024,
            dev: false,
        })
        .map_err(|e| anyhow::anyhow!("{}", e.message()))?;
        let output = engine.call("routes", String::new()).await.map_err(|e| anyhow::anyhow!("{}", e.message()))?;
        let configs: Vec<RouteConfig> = serde_json::from_str(&output).context("invalid route config")?;
        for config in configs {
            let Some(route) = routes.iter().find(|r| r.id == config.id).filter(|r| !r.is_special()) else {
                continue;
            };
            let Some(entry) = manifest.routes.iter_mut().find(|r| r.id == config.id) else {
                continue;
            };
            entry.dynamic_params = config.dynamic_params;
            entry.static_props = config.is_static;
            let loader = info.loaders.contains(&route.id);
            if config.is_static && loader {
                voltrs::log::warn(&format!(
                    "{} has getStaticProps and a Rust loader: it renders per request and is never cached",
                    config.id
                ));
            }
            // Like Next.js, `revalidate = 0` opts out of caching instead of expiring on every request.
            if config.dynamic == "force-dynamic" || config.revalidate == Some(0) {
                entry.dynamic = true;
                continue;
            }
            match config.dynamic.as_str() {
                "error" if loader => anyhow::bail!(
                    "{} exports dynamic = \"error\" but has a Rust loader, which reads the request; \
                     use \"force-static\" to cache its output anyway",
                    config.id
                ),
                "force-static" | "error" => {
                    entry.force_static = true;
                    entry.dynamic = false;
                }
                "auto" => {}
                other => {
                    voltrs::log::warn(&format!("{}: unknown `dynamic` value {other:?}, using \"auto\"", config.id))
                }
            }
            entry.revalidate = config.revalidate;
            if !route.is_dynamic() {
                entry.paths = vec![route.id.clone()];
                continue;
            }
            if !config.static_params {
                continue;
            }
            let output = engine
                .call("staticParams", route.id.clone())
                .await
                .map_err(|e| anyhow::anyhow!("generating static params of {}: {}", route.id, e.message()))?;
            let Some(params) = serde_json::from_str::<Option<StaticParams>>(&output)
                .with_context(|| format!("invalid static params of {}", route.id))?
            else {
                continue;
            };
            entry.dynamic_params &= params.dynamic_params.unwrap_or(true);
            entry.paths = params.resolve(&RoutePattern::parse(&route.id));
            entry.dynamic = loader && !entry.force_static;
        }
        Ok(())
    }

    /// Renders every known path of routes without a Rust loader into `prerender/`.
    async fn prerender(&self, manifest: &mut Manifest, info: &ServerInfo) -> Result<()> {
        let targets: Vec<String> = manifest
            .routes
            .iter()
            .filter(|r| !r.id.starts_with("/_") && !r.dynamic && !info.loaders.contains(&r.id))
            .flat_map(|r| r.paths.iter().cloned())
            .collect();
        let dir = self.project.out_dir.join(voltrs::manifest::PRERENDER_DIR);
        crate::util::reset_dir(&dir)?;
        if targets.is_empty() {
            return Ok(());
        }
        let renderer = voltrs::App::new()
            .root(&self.project.root)
            .static_renderer()
            .map_err(|e| anyhow::anyhow!("{}", e.message()))?;
        let mut jobs = tokio::task::JoinSet::new();
        for path in targets {
            let renderer = renderer.clone();
            jobs.spawn(async move { renderer.page(&path).await });
        }
        let mut count = 0;
        while let Some(page) = jobs.join_next().await {
            let page = page?;
            if page.status != voltrs::StatusCode::OK {
                voltrs::log::warn(&format!("skipped prerendering {}: status {}", page.path, page.status));
                continue;
            }
            let (Some(html), Some(data)) = (prerender_file(&page.path, "html"), prerender_file(&page.path, "json"))
            else {
                voltrs::log::warn(&format!("skipped prerendering {}: unsafe file name", page.path));
                continue;
            };
            for (file, body) in [(html, &page.html), (data, &page.data)] {
                let path = self.project.out_dir.join(file);
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(&path, body)?;
            }
            if let Some(entry) = manifest.routes.iter_mut().find(|r| r.paths.contains(&page.path)) {
                entry.revalidate = entry.revalidate.or(page.revalidate);
                entry.prerendered.push(page.path);
            }
            count += 1;
        }
        for entry in &mut manifest.routes {
            entry.prerendered.sort();
        }
        voltrs::log::ready(&format!("prerendered {count} pages"));
        Ok(())
    }

    /// Keeps the previous build but records the error so the dev server shows an overlay.
    pub fn record_error(&self, error: &anyhow::Error) {
        let out = &self.project.out_dir;
        if let Ok(mut manifest) = Manifest::load(out) {
            manifest.error = Some(format!("{error:#}"));
            let _ = manifest.save(out);
        }
    }
}
