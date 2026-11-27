//! `volt export`: writes the prerendered build as a static site.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use voltrs::manifest::{Manifest, file_stem, prerender_file};
use voltrs::router::RoutePattern;
use voltrs::sdk::ServerInfo;

use crate::project::Project;

/// Turns a build made with `VOLT_EXPORT=1` into files for any static host.
pub struct Export<'a> {
    project: &'a Project,
    manifest: &'a Manifest,
    info: &'a ServerInfo,
}

impl<'a> Export<'a> {
    pub fn new(project: &'a Project, manifest: &'a Manifest, info: &'a ServerInfo) -> Self {
        Self { project, manifest, info }
    }

    /// Everything that cannot work without the server, one line each.
    pub fn blockers(&self) -> Vec<String> {
        let mut out = Vec::new();
        for route in self.manifest.routes.iter().filter(|r| !r.id.starts_with("/_")) {
            let id = &route.id;
            if self.info.loaders.contains(id) {
                out.push(format!("{id}: has a Rust #[loader]"));
            } else if route.dynamic && RoutePattern::parse(id).is_dynamic() {
                out.push(format!("{id}: dynamic route without generateStaticParams or getStaticPaths"));
            } else if route.dynamic {
                out.push(format!("{id}: renders on every request (dynamic = \"force-dynamic\" or revalidate = 0)"));
            } else {
                out.extend(
                    route
                        .paths
                        .iter()
                        .filter(|p| !route.prerendered.contains(p))
                        .map(|p| format!("{p}: could not be prerendered")),
                );
            }
        }
        out.extend(self.info.actions.iter().map(|a| format!("action `{a}`: server actions need the server")));
        out.extend(self.info.api.iter().map(|a| format!("{a}: API routes need the server")));
        out
    }

    /// Settings a static host cannot apply, reported as warnings.
    pub fn ignored(&self) -> Vec<String> {
        let config = &self.project.config;
        let mut out = Vec::new();
        if !config.redirects.is_empty() || !config.rewrites.is_empty() {
            out.push("[[redirects]] and [[rewrites]] are not applied; configure them on your host".to_owned());
        }
        if !config.headers.is_empty() {
            out.push("[headers] are not applied; configure them on your host".to_owned());
        }
        if self.manifest.routes.iter().any(|r| r.revalidate.is_some()) {
            out.push("`revalidate` has no effect: exported pages are rendered once".to_owned());
        }
        out
    }

    /// Writes the site into `[build] export_dir` and returns the number of pages.
    pub async fn write(&self) -> Result<usize> {
        let out = self.project.config.export_dir();
        Self::check_output(self.project, &out)?;
        crate::util::reset_dir(&out)?;
        let public = self.project.config.public_dir();
        if public.is_dir() {
            copy_dir(&public, &out)?;
        }
        copy_dir(&self.project.static_dir(), &out.join("_volt").join("static"))?;

        let mut pages = 0;
        for route in &self.manifest.routes {
            for path in &route.prerendered {
                let (Some(stem), Some(html), Some(data)) =
                    (file_stem(path), prerender_file(path, "html"), prerender_file(path, "json"))
                else {
                    continue;
                };
                write(&out.join(self.html_name(&stem)), &std::fs::read(self.project.out_dir.join(html))?)?;
                write(
                    &out.join("_volt").join("data").join(format!("{stem}.json")),
                    &std::fs::read(self.project.out_dir.join(data))?,
                )?;
                pages += 1;
            }
        }

        let renderer = voltrs::App::new()
            .root(&self.project.root)
            .static_renderer()
            .map_err(|e| anyhow::anyhow!("{}", e.message()))?;
        write(&out.join("404.html"), renderer.not_found().await.html.as_bytes())?;
        for (name, body) in renderer.files().await {
            write(&out.join(name), &body)?;
        }
        for file in &self.manifest.metadata {
            let body = std::fs::read(self.project.out_dir.join(&file.file))?;
            for url in self.metadata_urls(&file.route) {
                write(&out.join(url.trim_start_matches('/')), &body)?;
            }
        }
        Ok(pages)
    }

    /// Concrete URLs of a metadata file: its route when static, else one per exported page below its segment.
    fn metadata_urls(&self, route: &str) -> BTreeSet<String> {
        let pattern = RoutePattern::parse(route);
        if !pattern.is_dynamic() {
            return BTreeSet::from([pattern.as_str().to_owned()]);
        }
        let dir = pattern.as_str().rsplit_once('/').map_or("", |(dir, _)| dir);
        let mut urls = BTreeSet::new();
        for entry in &self.manifest.routes {
            let id = RoutePattern::parse(&entry.id);
            if id.as_str() != dir && !id.as_str().starts_with(&format!("{dir}/")) {
                continue;
            }
            for path in &entry.prerendered {
                let Some(params) = id.matches(path) else { continue };
                let url = pattern.fill(&params);
                if !url.split('/').any(|part| part == ".." || part == ".") {
                    urls.insert(url);
                }
            }
        }
        urls
    }

    /// `export_dir` is emptied before writing, so refuse anything that holds project files.
    pub fn check_output(project: &Project, out: &Path) -> Result<()> {
        let mut reserved: Vec<PathBuf> = project.app_dir.iter().chain(&project.pages_dir).cloned().collect();
        reserved.extend([
            project.config.public_dir(),
            project.out_dir.clone(),
            project.node_modules(),
            project.root.join("server"),
            project.root.join("src"),
            project.root.join("target"),
        ]);
        if let Some(dir) = reserved.iter().find(|dir| dir.starts_with(out) || out.starts_with(dir)) {
            bail!("[build] export_dir cannot overlap {}", project.relative(dir));
        }
        if !out.exists() {
            return Ok(());
        }
        let resolved = out.canonicalize()?;
        if resolved == project.root || !resolved.starts_with(&project.root) {
            bail!("[build] export_dir resolves outside the project: {}", resolved.display());
        }
        let previous = out.join("_volt").is_dir() || std::fs::read_dir(out)?.next().is_none();
        if !previous {
            bail!("{} is not empty and was not written by `volt export`; remove it first", project.relative(out));
        }
        Ok(())
    }

    /// `about.html`, or `about/index.html` with `trailing_slash`.
    fn html_name(&self, stem: &str) -> String {
        if stem == "index" {
            "index.html".to_owned()
        } else if self.project.config.build.trailing_slash {
            format!("{stem}/index.html")
        } else {
            format!("{stem}.html")
        }
    }
}

fn write(path: &Path, body: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, body)?;
    Ok(())
}

fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)?.flatten() {
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use voltrs::manifest::RouteEntry;

    fn project(root: &Path) -> Project {
        Project {
            config: voltrs::config::Config { root: root.to_path_buf(), ..Default::default() },
            app_dir: Some(root.join("app")),
            pages_dir: Some(root.join("pages")),
            out_dir: root.join(".volt"),
            rust: true,
            root: root.to_path_buf(),
        }
    }

    #[test]
    fn refuses_to_empty_project_directories() {
        let root = std::env::temp_dir().join(format!("volt-export-guard-{}", std::process::id()));
        std::fs::create_dir_all(root.join("app")).unwrap();
        let root = root.canonicalize().unwrap();
        std::fs::create_dir_all(root.join("docs")).unwrap();
        std::fs::write(root.join("docs").join("notes.md"), "keep").unwrap();
        std::fs::create_dir_all(root.join("out").join("_volt")).unwrap();
        let project = project(&root);
        for dir in ["app", "app/blog", "pages", "public", ".volt", "node_modules", "server", "target"] {
            assert!(Export::check_output(&project, &root.join(dir)).is_err(), "{dir}");
        }
        assert!(Export::check_output(&project, &root.join("docs")).is_err());
        assert!(Export::check_output(&project, &root.join("out")).is_ok());
        assert!(Export::check_output(&project, &root.join("dist")).is_ok());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn lists_routes_that_need_the_server() {
        let project = project(&std::env::temp_dir().join("volt-export-test"));
        let route = |id: &str, dynamic: bool, paths: &[&str], prerendered: &[&str]| RouteEntry {
            id: id.into(),
            dynamic,
            paths: paths.iter().map(|p| p.to_string()).collect(),
            prerendered: prerendered.iter().map(|p| p.to_string()).collect(),
            ..RouteEntry::default()
        };
        let manifest = Manifest {
            routes: vec![
                route("/", false, &["/"], &["/"]),
                route("/account", true, &["/account"], &[]),
                route("/blog/[slug]", false, &["/blog/a", "/blog/b"], &["/blog/a"]),
                route("/live/[id]", true, &[], &[]),
                route("/_not-found", false, &[], &[]),
            ],
            ..Manifest::default()
        };
        let info = ServerInfo {
            actions: vec!["save".into()],
            loaders: vec!["/account".into()],
            api: vec!["GET /api/items".into()],
        };
        let blockers = Export::new(&project, &manifest, &info).blockers();
        assert_eq!(
            blockers,
            vec![
                "/account: has a Rust #[loader]",
                "/blog/b: could not be prerendered",
                "/live/[id]: dynamic route without generateStaticParams or getStaticPaths",
                "action `save`: server actions need the server",
                "GET /api/items: API routes need the server",
            ]
        );
    }
}
