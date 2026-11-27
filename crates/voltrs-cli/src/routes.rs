//! File-system routing over the `app/` (App Router) and `pages/` (Pages Router) directories.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use voltrs::manifest::{Intercept, SlotEntry};
use voltrs::router::RoutePattern;

use crate::metadata::File;

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
    /// Directory key relative to `app/`: `/` for the root, `/dashboard`, `/@modal/(.)photos`.
    pub key: String,
    pub layout: Option<Module>,
    pub loading: Option<Module>,
    pub error: Option<Module>,
    pub not_found: Option<Module>,
    pub template: Option<Module>,
    /// Directory names from the app root to this segment, groups and dynamic segments included.
    pub path: Vec<String>,
    /// Metadata files (`icon.png`, `opengraph-image.jpg`, ...) applying to every route below.
    pub files: Vec<File>,
    /// Parallel route slots (`@name` folders) passed to the layout next to `children`.
    pub slots: Vec<Slot>,
}

impl Segment {
    fn read(dir: &Path, place: &Place) -> Result<Self> {
        let path = place.path.clone();
        let url: String = path.iter().map(|part| format!("/{part}")).collect();
        // Metadata files belong to URL segments, which slot and interception folders are not.
        let files = if place.in_slot || place.intercept.is_some() {
            Vec::new()
        } else {
            File::scan(dir, &url, path.is_empty())?
        };
        Ok(Self {
            key: place.key(),
            slots: Vec::new(),
            layout: find(dir, "layout"),
            loading: find(dir, "loading"),
            error: find(dir, "error"),
            not_found: find(dir, "not-found"),
            template: find(dir, "template"),
            files,
            path,
        })
    }

    fn is_empty(&self) -> bool {
        self.layout.is_none()
            && self.loading.is_none()
            && self.error.is_none()
            && self.not_found.is_none()
            && self.template.is_none()
            && self.files.is_empty()
            && self.slots.is_empty()
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

/// A parallel route slot: an `@name` folder with its own pages, rendered as the `name` prop of the layout.
#[derive(Debug, Clone)]
pub struct Slot {
    pub name: String,
    /// Directory key, e.g. `/dashboard/@team`.
    pub key: String,
    /// Rendered when no page of the slot matches the URL.
    pub default: Option<Module>,
    pub routes: Vec<SlotRoute>,
}

impl Slot {
    /// The manifest entry the server resolves the slot with; `segment` is the key of the layout's directory.
    pub fn entry(&self, segment: &str) -> SlotEntry {
        SlotEntry {
            key: self.key.clone(),
            segment: segment.to_owned(),
            default: self.default.is_some(),
            routes: self
                .routes
                .iter()
                .map(|r| voltrs::manifest::SlotRoute {
                    id: r.id.clone(),
                    pattern: r.pattern.clone(),
                    intercept: r.intercept,
                    segments: r.dirs.clone(),
                })
                .collect(),
        }
    }
}

/// A page inside a slot.
#[derive(Debug, Clone)]
pub struct SlotRoute {
    /// Directory path with groups removed, e.g. `/@modal/(.)photos/[id]`; loaders register under it.
    pub id: String,
    /// URL pattern the page matches.
    pub pattern: String,
    /// Inside an interception folder: matches only on client navigations.
    pub intercept: bool,
    /// Directory keys from the slot folder to the page.
    pub dirs: Vec<String>,
    /// Segments inside the slot, outermost first.
    pub segments: Vec<Segment>,
    pub page: Module,
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
    /// Directory names from the app root to the page, for `useSelectedLayoutSegment`.
    pub path: Vec<String>,
    /// Metadata files next to the page; they take precedence over the page's own metadata.
    pub files: Vec<File>,
    /// `app/global-error.tsx`, rendered instead of the root layout when it fails.
    pub global_error: Option<Module>,
    /// Directory keys from `app/` to the page, including directories without special files.
    pub dirs: Vec<String>,
    /// Set on routes inside an interception folder (`(.)x`, `(..)x`, `(...)x`).
    pub intercept: Option<Intercept>,
    /// Key of the directory whose `default` page this route renders, for URLs that only exist in its slots.
    pub fallback: Option<String>,
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

    /// Every slot the route can render, with the key of the directory whose layout receives it.
    pub fn slots(&self) -> Vec<(&str, &Slot)> {
        let mut out = Vec::new();
        let mut pending: Vec<&Segment> = self.segments.iter().collect();
        while let Some(segment) = pending.pop() {
            for slot in &segment.slots {
                out.push((segment.key.as_str(), slot));
                pending.extend(slot.routes.iter().flat_map(|r| &r.segments));
            }
        }
        out
    }

    fn new(id: &str, kind: Kind, segments: Vec<Segment>, page: Module) -> Self {
        Self {
            id: id.into(),
            kind,
            segments,
            page,
            path: Vec::new(),
            files: Vec::new(),
            global_error: None,
            dirs: Vec::new(),
            intercept: None,
            fallback: None,
        }
    }
}

fn find(dir: &Path, name: &str) -> Option<Module> {
    find_file(dir, name).map(Module::File)
}

/// The script `name.(tsx|ts|jsx|js)` in `dir`.
pub fn find_file(dir: &Path, name: &str) -> Option<PathBuf> {
    EXTENSIONS.iter().map(|ext| dir.join(format!("{name}.{ext}"))).find(|p| p.is_file())
}

fn skipped(name: &str) -> bool {
    name.starts_with('_') || name.starts_with('@') || name.starts_with('.') || name == "node_modules"
}

/// Reads an interception folder name: `(.)x`, `(..)x`, `(..)(..)x` or `(...)x` as the number of route segments
/// to go up (`None` for the root) and the remaining segment name.
fn interception(name: &str) -> Option<(Option<usize>, &str)> {
    if let Some(rest) = name.strip_prefix("(...)") {
        return Some((None, rest));
    }
    if let Some(rest) = name.strip_prefix("(.)") {
        return Some((Some(0), rest));
    }
    let mut rest = name;
    let mut up = 0;
    while let Some(next) = rest.strip_prefix("(..)") {
        rest = next;
        up += 1;
    }
    (up > 0).then_some((Some(up), rest))
}

/// Where a directory sits in `app/`: on the file system, in the URL and in the slot and interception structure.
#[derive(Debug, Clone)]
struct Place {
    /// Path relative to `app/`, empty at the root.
    rel: String,
    /// URL path of the directory: slot folders add nothing, interception folders rewrite it.
    url: String,
    /// Key of the nearest directory outside slots, which scopes interceptions declared below it.
    scope: String,
    /// Directory keys from the root of the tree (`app/` or the slot folder) to this directory.
    dirs: Vec<String>,
    /// Directory names from `app/` to this directory, slot folders excluded.
    path: Vec<String>,
    in_slot: bool,
    /// Scope of the enclosing interception folder.
    intercept: Option<String>,
}

impl Place {
    fn root() -> Self {
        Self {
            rel: String::new(),
            url: String::new(),
            scope: "/".into(),
            dirs: vec!["/".into()],
            path: Vec::new(),
            in_slot: false,
            intercept: None,
        }
    }

    fn key(&self) -> String {
        if self.rel.is_empty() { "/".into() } else { self.rel.clone() }
    }

    fn slot(&self, name: &str) -> Self {
        let rel = format!("{}/@{name}", self.rel);
        Self { dirs: vec![rel.clone()], rel, in_slot: true, ..self.clone() }
    }

    fn child(&self, name: &str) -> Result<Self> {
        let rel = format!("{}/{name}", self.rel);
        let mut dirs = self.dirs.clone();
        dirs.push(rel.clone());
        let mut path = self.path.clone();
        path.push(name.to_owned());
        let mut child = Self { rel: rel.clone(), url: format!("{}/{name}", self.url), dirs, path, ..self.clone() };
        if !self.in_slot {
            child.scope = rel.clone();
        }
        if let Some((up, rest)) = interception(name) {
            let mut base: Vec<&str> =
                self.url.split('/').filter(|s| !s.is_empty() && !(s.starts_with('(') && s.ends_with(')'))).collect();
            match up {
                Some(up) if up > base.len() => bail!("`app{rel}` intercepts above the app root"),
                Some(up) => base.truncate(base.len() - up),
                None => base.clear(),
            }
            base.push(rest);
            child.url = format!("/{}", base.join("/"));
            child.intercept = Some(self.scope.clone());
        }
        Ok(child)
    }

    fn route_id(&self) -> String {
        let path = if self.intercept.is_some() || self.in_slot { &self.rel } else { &self.url };
        RoutePattern::parse(path).as_str().to_owned()
    }

    fn pattern(&self) -> String {
        RoutePattern::parse(&self.url).as_str().to_owned()
    }
}

/// Pages found while walking `app/`: URL routes, and `default` pages of layouts with slots.
#[derive(Default)]
struct Found {
    routes: BTreeMap<String, Route>,
    fallbacks: Vec<Route>,
}

/// Scans the app and pages directories into routes, adding the not-found and error pseudo routes.
pub fn scan(app_dir: Option<&Path>, pages_dir: Option<&Path>) -> Result<Vec<Route>> {
    let app = app_dir.map(AppDir::new).transpose()?;
    let pages = pages_dir.map(PagesDir::new);
    let mut found = match &app {
        Some(app) => app.scan()?,
        None => BTreeMap::new(),
    };
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
    let error = match &app {
        // Without a root `error.tsx`, failures render `global-error.tsx` in place of the whole document.
        Some(app) if app.root.error.is_none() && app.global_error.is_some() => {
            let page = app.global_error.clone().unwrap_or(Module::Builtin("ErrorPage"));
            Route { segments: Vec::new(), ..app.route(ERROR, page) }
        }
        _ => special(ERROR, app.as_ref(), pages.as_ref(), |s| s.error.clone(), &["500", "_error"], "ErrorPage"),
    };
    routes.push(error);
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
        (None, None) => Route::new(id, Kind::App, Vec::new(), Module::Builtin(builtin)),
    }
}

struct AppDir {
    dir: PathBuf,
    root: Segment,
    global_error: Option<Module>,
}

impl AppDir {
    fn new(dir: &Path) -> Result<Self> {
        let mut root = Self::segment(dir, &Place::root())?;
        if root.layout.is_none() {
            root.layout = Some(Module::Builtin("RootLayout"));
            if !root.slots.is_empty() {
                voltrs::log::warn("app/ has @slot folders but no layout.tsx to render them");
            }
        }
        Ok(Self { dir: dir.to_path_buf(), root, global_error: find(dir, "global-error") })
    }

    fn route(&self, id: &str, page: Module) -> Route {
        let shell = Segment { layout: self.root.layout.clone(), files: self.root.files.clone(), ..Segment::default() };
        Route { global_error: self.global_error.clone(), ..Route::new(id, Kind::App, vec![shell], page) }
    }

    /// Every URL route, including those that only exist in slots and render the `default` page of their layout.
    fn scan(&self) -> Result<BTreeMap<String, Route>> {
        let mut found = Found::default();
        let mut stack = vec![self.root.clone()];
        Self::walk(&self.dir, &Place::root(), &mut stack, &mut found, &mut None)?;
        for route in found.fallbacks {
            found.routes.entry(route.id.clone()).or_insert(route);
        }
        for route in found.routes.values_mut() {
            route.global_error = self.global_error.clone();
        }
        Ok(found.routes)
    }

    /// Reads the special files of a directory and scans its `@slot` folders.
    fn segment(dir: &Path, place: &Place) -> Result<Segment> {
        let mut segment = Segment::read(dir, place)?;
        for child in sorted(dir)?.into_iter().filter(|p| p.is_dir()) {
            let name = file_name(&child);
            let Some(slot) = name.strip_prefix('@') else { continue };
            let inner = place.slot(slot);
            let mut routes = Vec::new();
            let mut stack = vec![Self::segment(&child, &inner)?];
            Self::walk(&child, &inner, &mut stack, &mut Found::default(), &mut Some(&mut routes))?;
            segment.slots.push(Slot {
                name: slot.to_owned(),
                key: inner.key(),
                default: find(&child, "default"),
                routes,
            });
        }
        if !segment.slots.is_empty() && segment.layout.is_none() && !place.rel.is_empty() {
            voltrs::log::warn(&format!("app{} has @slot folders but no layout.tsx to render them", place.rel));
        }
        Ok(segment)
    }

    /// Walks a directory tree. Pages go to `slot` inside a slot folder, otherwise to `found`.
    fn walk(
        dir: &Path,
        place: &Place,
        stack: &mut Vec<Segment>,
        found: &mut Found,
        slot: &mut Option<&mut Vec<SlotRoute>>,
    ) -> Result<()> {
        // The page directory's own metadata files move to the route, where they take precedence.
        let segments = |stack: &[Segment]| {
            let mut segments = stack.to_vec();
            let own = segments.last_mut().map(|s| std::mem::take(&mut s.files)).unwrap_or_default();
            segments.retain(|s| !s.is_empty());
            (segments, own)
        };
        if let Some(page) = find(dir, "page") {
            let id = place.route_id();
            match slot {
                Some(routes) => routes.push(SlotRoute {
                    id,
                    pattern: place.pattern(),
                    intercept: place.intercept.is_some(),
                    dirs: place.dirs.clone(),
                    segments: segments(stack).0,
                    page,
                }),
                None => {
                    let intercept = place.intercept.clone().map(|scope| Intercept { target: place.pattern(), scope });
                    let (segments, files) = segments(stack);
                    let route = Route {
                        dirs: place.dirs.clone(),
                        path: place.path.clone(),
                        files,
                        intercept,
                        ..Route::new(&id, Kind::App, segments, page)
                    };
                    if let Some(existing) = found.routes.insert(id.clone(), route) {
                        bail!("two pages resolve to `{id}`: {:?}", existing.page);
                    }
                }
            }
        }
        if slot.is_none()
            && let Some(default) = find(dir, "default")
            && let Some(own) = stack.last()
        {
            let (segments, files) = segments(stack);
            for route in own.slots.iter().flat_map(|s| &s.routes).filter(|r| !r.intercept) {
                found.fallbacks.push(Route {
                    dirs: place.dirs.clone(),
                    path: place.path.clone(),
                    files: files.clone(),
                    fallback: Some(place.key()),
                    ..Route::new(&route.pattern, Kind::App, segments.clone(), default.clone())
                });
            }
        }
        for child in sorted(dir)?.into_iter().filter(|p| p.is_dir()) {
            let name = file_name(&child);
            if skipped(&name) {
                continue;
            }
            let inner = place.child(&name)?;
            stack.push(Self::segment(&child, &inner)?);
            Self::walk(&child, &inner, stack, found, slot)?;
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
        Route::new(id, Kind::Pages { app: self.app.clone() }, Vec::new(), page)
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
    fn collects_segment_paths_metadata_files_and_global_error() {
        let dir = fixture(
            "conventions",
            &[
                "page.tsx",
                "icon.png",
                "global-error.tsx",
                "(shop)/cart/page.tsx",
                "blog/opengraph-image.png",
                "blog/[slug]/page.tsx",
            ],
        );
        let routes = scan(Some(&dir), None).unwrap();
        let route = |id: &str| routes.iter().find(|r| r.id == id).unwrap();
        let home = route("/");
        assert_eq!(home.files.len(), 1);
        assert!(home.segments[0].files.is_empty());
        let post = route("/blog/[slug]");
        assert_eq!(post.path, vec!["blog", "[slug]"]);
        assert_eq!(post.segments.iter().map(|s| s.path.join("/")).collect::<Vec<_>>(), vec!["", "blog"]);
        assert_eq!(post.segments[1].files[0].route, "/blog/opengraph-image.png");
        assert_eq!(route("/cart").path, vec!["(shop)", "cart"]);
        let global = Some(Module::File(dir.join("global-error.tsx")));
        assert!(routes.iter().all(|r| r.global_error == global));
        let error = route(ERROR);
        assert!(error.segments.is_empty());
        assert_eq!(Some(error.page.clone()), global);
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

    #[test]
    fn scans_slots_and_interceptions() {
        let dir = fixture(
            "slots",
            &[
                "layout.tsx",
                "page.tsx",
                "@modal/default.tsx",
                "@modal/(.)photos/[id]/page.tsx",
                "photos/page.tsx",
                "photos/[id]/page.tsx",
                "dashboard/layout.tsx",
                "dashboard/page.tsx",
                "dashboard/default.tsx",
                "dashboard/@team/page.tsx",
                "dashboard/@team/default.tsx",
                "dashboard/@analytics/page.tsx",
                "dashboard/@analytics/views/page.tsx",
                "(main)/feed/page.tsx",
                "(main)/feed/(..)photos/[id]/page.tsx",
            ],
        );
        let routes = scan(Some(&dir), None).unwrap();
        let ids: Vec<&str> = routes.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(
            ids,
            vec![
                "/",
                "/dashboard",
                "/dashboard/views",
                "/feed",
                "/feed/(..)photos/[id]",
                "/photos",
                "/photos/[id]",
                NOT_FOUND,
                ERROR
            ]
        );
        let root = &routes[0].segments[0];
        assert_eq!(root.key, "/");
        let modal = &root.slots[0];
        assert_eq!((modal.name.as_str(), modal.key.as_str()), ("modal", "/@modal"));
        assert!(modal.default.is_some());
        assert_eq!(modal.routes[0].id, "/@modal/(.)photos/[id]");
        assert_eq!(modal.routes[0].pattern, "/photos/[id]");
        assert!(modal.routes[0].intercept);

        let dashboard = routes.iter().find(|r| r.id == "/dashboard").unwrap();
        let slots = dashboard.slots();
        let keys: Vec<(&str, &str)> = slots.iter().map(|(segment, slot)| (*segment, slot.key.as_str())).collect();
        assert!(keys.contains(&("/dashboard", "/dashboard/@analytics")));
        assert!(keys.contains(&("/dashboard", "/dashboard/@team")));
        assert!(keys.contains(&("/", "/@modal")));
        let views = routes.iter().find(|r| r.id == "/dashboard/views").unwrap();
        assert_eq!(views.fallback.as_deref(), Some("/dashboard"));
        assert_eq!(views.page, Module::File(dir.join("dashboard/default.tsx")));

        let feed = routes.iter().find(|r| r.id == "/feed/(..)photos/[id]").unwrap();
        let intercept = feed.intercept.as_ref().unwrap();
        assert_eq!((intercept.target.as_str(), intercept.scope.as_str()), ("/photos/[id]", "/(main)/feed"));
        assert_eq!(
            feed.dirs,
            vec!["/", "/(main)", "/(main)/feed", "/(main)/feed/(..)photos", "/(main)/feed/(..)photos/[id]"]
        );
        assert!(routes.iter().find(|r| r.id == "/photos/[id]").unwrap().intercept.is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn reads_interception_markers() {
        assert_eq!(interception("(.)photo"), Some((Some(0), "photo")));
        assert_eq!(interception("(..)(..)photo"), Some((Some(2), "photo")));
        assert_eq!(interception("(...)photo"), Some((None, "photo")));
        assert_eq!(interception("(group)"), None);
        assert!(Place::root().child("a").unwrap().child("(..)(..)b").is_err());
    }
}
