//! The build pipeline shared by `volt dev` and `volt build`.

use std::path::Path;
use std::time::Instant;

use anyhow::{Context, Result};
use voltrs::manifest::{ClientAssets, Manifest, RouteEntry};
use voltrs::sdk::{SERVER_INFO_FILE, Sdk, ServerInfo};
use voltrs::ssr::{Engine, EngineOptions};

use crate::bundler::Bundle;
use crate::css::Css;
use crate::project::Project;
use crate::routes::{self, Route};
use crate::util::hash;

#[derive(serde::Deserialize)]
struct RouteConfig {
    id: String,
    revalidate: Option<u64>,
    dynamic: String,
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
        let routes = routes::scan(&self.project.app_dir)?;
        if !self.dev {
            crate::util::reset_dir(&self.project.static_dir())?;
        }
        let bundle = Bundle::new(self.project, &routes, self.dev);
        let (server, client) = tokio::join!(bundle.server(), bundle.client());
        let (server, client) = (server?, client?);
        let css = Css::new(self.project, !self.dev).build(&server.css)?;

        let mut manifest = Manifest {
            build_id: String::new(),
            dev: self.dev,
            server: server.file.clone(),
            client: ClientAssets { entry: client.entry, css: css.into_iter().collect(), preload: client.preload },
            routes: routes
                .iter()
                .map(|r| RouteEntry {
                    id: r.id.clone(),
                    chunks: client.routes.get(&r.id).cloned().unwrap_or_default(),
                    dynamic: r.is_dynamic() || info.loaders.contains(&r.id),
                    ..RouteEntry::default()
                })
                .collect(),
            error: None,
        };
        let fingerprint = format!("{}{}{:?}", manifest.server, manifest.client.entry, manifest.client.css);
        manifest.build_id = hash(fingerprint.as_bytes());
        manifest.save(&self.project.out_dir)?;

        if !self.dev {
            self.apply_route_config(&mut manifest).await?;
            if self.project.config.build.prerender {
                self.prerender(&mut manifest, &routes, info).await?;
            }
            manifest.save(&self.project.out_dir)?;
            let compressed = crate::compress::precompress(&self.project.static_dir())?;
            voltrs::log::ready(&format!("compressed {compressed} assets"));
        }
        voltrs::log::ready(&format!("built {} routes in {}ms", routes.len() - 2, started.elapsed().as_millis()));
        Ok(manifest)
    }

    /// Reads `revalidate` and `dynamic` exports from the built server bundle.
    async fn apply_route_config(&self, manifest: &mut Manifest) -> Result<()> {
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
            if let Some(route) = manifest.routes.iter_mut().find(|r| r.id == config.id) {
                route.revalidate = config.revalidate;
                route.dynamic |= config.dynamic == "force-dynamic";
            }
        }
        Ok(())
    }

    async fn prerender(&self, manifest: &mut Manifest, routes: &[Route], info: &ServerInfo) -> Result<()> {
        let targets: Vec<String> = routes
            .iter()
            .filter(|r| !r.is_special() && !info.loaders.contains(&r.id))
            .filter(|r| manifest.route(&r.id).is_some_and(|m| !m.dynamic))
            .map(|r| r.id.clone())
            .collect();
        if targets.is_empty() {
            return Ok(());
        }
        let dir = self.project.out_dir.join("prerender");
        crate::util::reset_dir(&dir)?;
        let rendered = voltrs::App::new()
            .root(&self.project.root)
            .prerender(&targets)
            .await
            .map_err(|e| anyhow::anyhow!("{}", e.message()))?;
        let mut count = 0;
        for (route, html) in rendered {
            let html = match html {
                Ok(html) => html,
                Err(error) => {
                    voltrs::log::warn(&format!("skipped prerendering {route}: {}", error.message()));
                    continue;
                }
            };
            let name =
                if route == "/" { "index.html".to_owned() } else { format!("{}.html", route.trim_start_matches('/')) };
            let path = dir.join(&name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, html)?;
            if let Some(entry) = manifest.routes.iter_mut().find(|r| r.id == route) {
                entry.prerendered = Some(format!("prerender/{name}"));
            }
            count += 1;
        }
        voltrs::log::ready(&format!("prerendered {count} static pages"));
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
