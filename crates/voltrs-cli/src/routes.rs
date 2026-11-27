//! File-system routing over the `app/` (App Router) and `pages/` (Pages Router) directories.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use voltrs::router::RoutePattern;

const EXTENSIONS: [&str; 4] = ["tsx", "ts", "jsx", "js"];
pub const NOT_FOUND: &str = "/_not-found";
pub const ERROR: &str = "/_error";

/// A route module: a user file or a built-in default from `volt-rs/internal/defaults`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Module {
    File(PathBuf),
    Builtin(&'static str),
}

#[derive(Debug, Clone, Default)]
pub struct Segment {
    pub layout: Option<Module>,
    pub loading: Option<Module>,
    pub error: Option<Module>,
    pub not_found: Option<Module>,
    pub template: Option<Module>,
}

impl Segment {
    fn read(dir: &Path) -> Self {
        Self {
            layout: find(dir, "layout"),
            loading: find(dir, "loading"),
            error: find(dir, "error"),
            not_found: find(dir, "not-found"),
            template: find(dir, "template"),
        }
    }

    fn is_empty(&self) -> bool {
        self.layout.is_none()
            && self.loading.is_none()
            && self.error.is_none()
            && self.not_found.is_none()
            && self.template.is_none()
    }

    pub fn modules(&self) -> [(&'static str, Option<&Module>); 5] {
        [
            ("layout", self.layout.as_ref()),
            ("loading", self.loading.as_ref()),
            ("error", self.error.as_ref()),
            ("notFound", self.not_found.as_ref()),
            ("template", self.template.as_ref()),
        ]
    }
}

/// Which router renders a route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    App,
    /// Rendered inside the custom document, wrapped by `_app`.
    Pages {
        app: Module,
    },
}

#[derive(Debug, Clone)]
pub struct Route {
    pub id: String,
    pub kind: Kind,
    pub segments: Vec<Segment>,
    pub page: Module,
}

impl Route {
    pub fn is_pages(&self) -> bool {
        matches!(self.kind, Kind::Pages { .. })
    }

    pub fn is_special(&self) -> bool {
        self.id.starts_with("/_")
    }

    pub fn is_dynamic(&self) -> bool {
        RoutePattern::parse(&self.id).is_dynamic()
    }
}

fn find(dir: &Path, name: &str) -> Option<Module> {
    EXTENSIONS.iter().map(|ext| dir.join(format!("{name}.{ext}"))).find(|p| p.is_file()).map(Module::File)
}

fn skipped(name: &str) -> bool {
    name.starts_with('_')
        || name.starts_with('@')
        || name.starts_with("(.")
        || name.starts_with('.')
        || name == "node_modules"
}

/// Scans the app and pages directories into routes, adding the not-found and error pseudo routes.
pub fn scan(app_dir: Option<&Path>, pages_dir: Option<&Path>) -> Result<Vec<Route>> {
    let mut found = BTreeMap::new();
    let app = app_dir.map(AppDir::new);
    let pages = pages_dir.map(PagesDir::new);
    if let Some(app) = &app {
        app.walk(&app.dir, "", &mut vec![app.root.clone()], &mut found)?;
    }
    if let Some(pages) = &pages {
        let mut own = BTreeMap::new();
        pages.walk(&pages.dir, "", &mut own)?;
        for (id, route) in own {
            if found.contains_key(&id) {
                bail!("`{id}` is defined in both app/ and pages/: {:?}", route.page);
            }
            found.insert(id, route);
        }
    }
    let mut routes: Vec<Route> = found.into_values().collect();
    routes.push(special(NOT_FOUND, app.as_ref(), pages.as_ref(), |s| s.not_found.clone(), &["404"], "NotFound"));
    routes.push(special(ERROR, app.as_ref(), pages.as_ref(), |s| s.error.clone(), &["500", "_error"], "ErrorPage"));
    Ok(routes)
}

/// A not-found or error route: an explicit App Router file wins, then the Pages Router file, then a built-in.
fn special(
    id: &str,
    app: Option<&AppDir>,
    pages: Option<&PagesDir>,
    own: impl Fn(&Segment) -> Option<Module>,
    files: &[&str],
    builtin: &'static str,
) -> Route {
    if let Some(app) = app
        && let Some(page) = own(&app.root)
    {
        return app.route(id, page);
    }
    if let Some(pages) = pages
        && let Some(page) = files.iter().find_map(|f| find(&pages.dir, f))
    {
        return pages.route(id, page);
    }
    match (app, pages) {
        (Some(app), _) => app.route(id, Module::Builtin(builtin)),
        (None, Some(pages)) => {
            pages.route(id, Module::Builtin(if builtin == "NotFound" { "PagesNotFound" } else { "PagesError" }))
        }
        (None, None) => Route { id: id.into(), kind: Kind::App, segments: Vec::new(), page: Module::Builtin(builtin) },
    }
}

struct AppDir {
    dir: PathBuf,
    root: Segment,
}

impl AppDir {
    fn new(dir: &Path) -> Self {
        let mut root = Segment::read(dir);
        if root.layout.is_none() {
            root.layout = Some(Module::Builtin("RootLayout"));
        }
        Self { dir: dir.to_path_buf(), root }
    }

    fn route(&self, id: &str, page: Module) -> Route {
        let shell = Segment { layout: self.root.layout.clone(), ..Segment::default() };
        Route { id: id.into(), kind: Kind::App, segments: vec![shell], page }
    }

    fn walk(&self, dir: &Path, url: &str, stack: &mut Vec<Segment>, found: &mut BTreeMap<String, Route>) -> Result<()> {
        if let Some(page) = find(dir, "page") {
            let id = RoutePattern::parse(url).as_str().to_owned();
            let segments = stack.iter().filter(|s| !s.is_empty()).cloned().collect();
            let route = Route { id: id.clone(), kind: Kind::App, segments, page };
            if let Some(existing) = found.insert(id.clone(), route) {
                bail!("two pages resolve to `{id}`: {:?}", existing.page);
            }
        }
        for child in sorted(dir)?.into_iter().filter(|p| p.is_dir()) {
            let name = file_name(&child);
            if skipped(&name) {
                continue;
            }
            stack.push(Segment::read(&child));
            self.walk(&child, &format!("{url}/{name}"), stack, found)?;
            stack.pop();
        }
        Ok(())
    }
}

/// Files in the pages root that are not routes.
const PAGES_SPECIAL: [&str; 5] = ["_app", "_document", "_error", "404", "500"];

struct PagesDir {
    dir: PathBuf,
    app: Module,
}

impl PagesDir {
    fn new(dir: &Path) -> Self {
        Self { dir: dir.to_path_buf(), app: find(dir, "_app").unwrap_or(Module::Builtin("PagesApp")) }
    }

    fn route(&self, id: &str, page: Module) -> Route {
        Route { id: id.into(), kind: Kind::Pages { app: self.app.clone() }, segments: Vec::new(), page }
    }

    fn walk(&self, dir: &Path, url: &str, found: &mut BTreeMap<String, Route>) -> Result<()> {
        for path in sorted(dir)? {
            let name = file_name(&path);
            if name.starts_with('.') || name == "node_modules" {
                continue;
            }
            if path.is_dir() {
                if url.is_empty() && name == "api" {
                    voltrs::log::warn(
                        "pages/api is ignored: API routes are Rust handlers (#[get], #[post], ...) in server/",
                    );
                    continue;
                }
                self.walk(&path, &format!("{url}/{name}"), found)?;
                continue;
            }
            let Some((stem, ext)) = name.rsplit_once('.') else {
                continue;
            };
            if !EXTENSIONS.contains(&ext) || stem.ends_with(".d") || stem.starts_with('_') {
                continue;
            }
            if url.is_empty() && PAGES_SPECIAL.contains(&stem) {
                continue;
            }
            let route_url = if stem == "index" { url.to_owned() } else { format!("{url}/{stem}") };
            let id = RoutePattern::parse(&route_url).as_str().to_owned();
            let route = self.route(&id, Module::File(path.clone()));
            if let Some(existing) = found.insert(id.clone(), route) {
                bail!("two pages resolve to `{id}`: {:?} and {:?}", existing.page, path);
            }
        }
        Ok(())
    }
}

/// The document of a Pages Router project: `pages/_document` or the built-in one.
pub fn document(pages_dir: Option<&Path>) -> Option<Module> {
    pages_dir.map(|dir| find(dir, "_document").unwrap_or(Module::Builtin("Document")))
}

fn sorted(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)?.flatten().map(|e| e.path()).collect();
    entries.sort();
    Ok(entries)
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str, files: &[&str]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("volt-routes-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for file in files {
            let path = dir.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "").unwrap();
        }
        dir
    }

    #[test]
    fn scans_app_dir() {
        let dir = fixture(
            "app",
            &[
                "layout.tsx",
                "page.tsx",
                "(site)/about/page.tsx",
                "blog/[slug]/page.tsx",
                "blog/layout.tsx",
                "_lib/page.tsx",
            ],
        );
        let routes = scan(Some(&dir), None).unwrap();
        let ids: Vec<&str> = routes.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["/", "/about", "/blog/[slug]", NOT_FOUND, ERROR]);
        let blog = routes.iter().find(|r| r.id == "/blog/[slug]").unwrap();
        assert_eq!(blog.segments.len(), 2);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn scans_pages_dir() {
        let dir = fixture(
            "pages",
            &[
                "_app.tsx",
                "_document.tsx",
                "404.tsx",
                "index.tsx",
                "about.tsx",
                "blog/index.tsx",
                "blog/[slug].tsx",
                "docs/[...path].tsx",
                "shop/[[...rest]].tsx",
                "api/hello.ts",
                "types.d.ts",
            ],
        );
        let routes = scan(None, Some(&dir)).unwrap();
        let ids: Vec<&str> = routes.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["/", "/about", "/blog", "/blog/[slug]", "/docs/[...path]", "/shop/[[...rest]]", NOT_FOUND, ERROR]
        );
        let app = Module::File(dir.join("_app.tsx"));
        assert!(routes.iter().all(|r| r.kind == Kind::Pages { app: app.clone() }));
        assert_eq!(routes[6].page, Module::File(dir.join("404.tsx")));
        assert_eq!(routes[7].page, Module::Builtin("PagesError"));
        assert_eq!(document(Some(&dir)), Some(Module::File(dir.join("_document.tsx"))));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn rejects_urls_in_both_routers() {
        let dir = fixture("both", &["app/about/page.tsx", "app/page.tsx", "pages/about.tsx", "pages/legacy.tsx"]);
        assert!(scan(Some(&dir.join("app")), Some(&dir.join("pages"))).is_err());
        std::fs::remove_file(dir.join("pages/about.tsx")).unwrap();
        let routes = scan(Some(&dir.join("app")), Some(&dir.join("pages"))).unwrap();
        assert!(routes.iter().find(|r| r.id == "/legacy").unwrap().is_pages());
        assert!(!routes.iter().find(|r| r.id == NOT_FOUND).unwrap().is_pages());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
