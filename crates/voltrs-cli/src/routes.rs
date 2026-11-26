//! File-system routing over the `app/` directory.

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

#[derive(Debug, Clone)]
pub struct Route {
    pub id: String,
    pub segments: Vec<Segment>,
    pub page: Module,
}

impl Route {
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

/// Scans the app directory into routes, adding the not-found and error pseudo routes.
pub fn scan(app_dir: &Path) -> Result<Vec<Route>> {
    let mut found = BTreeMap::new();
    let mut root = Segment::read(app_dir);
    if root.layout.is_none() {
        root.layout = Some(Module::Builtin("RootLayout"));
    }
    walk(app_dir, "", &mut vec![root.clone()], &mut found)?;
    let shell = Segment { layout: root.layout.clone(), ..Segment::default() };
    let mut routes: Vec<Route> = found.into_values().collect();
    routes.push(Route {
        id: NOT_FOUND.into(),
        segments: vec![shell.clone()],
        page: root.not_found.clone().unwrap_or(Module::Builtin("NotFound")),
    });
    routes.push(Route {
        id: ERROR.into(),
        segments: vec![shell],
        page: root.error.clone().unwrap_or(Module::Builtin("ErrorPage")),
    });
    Ok(routes)
}

fn walk(dir: &Path, url: &str, stack: &mut Vec<Segment>, found: &mut BTreeMap<String, Route>) -> Result<()> {
    if let Some(page) = find(dir, "page") {
        let id = RoutePattern::parse(url).as_str().to_owned();
        let segments = stack.iter().filter(|s| !s.is_empty()).cloned().collect();
        let route = Route { id: id.clone(), segments, page };
        if let Some(existing) = found.insert(id.clone(), route) {
            bail!("two pages resolve to `{id}`: {:?}", existing.page);
        }
    }
    let mut children: Vec<PathBuf> =
        std::fs::read_dir(dir)?.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    children.sort();
    for child in children {
        let name = child.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if skipped(&name) {
            continue;
        }
        stack.push(Segment::read(&child));
        walk(&child, &format!("{url}/{name}"), stack, found)?;
        stack.pop();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_app_dir() {
        let dir = std::env::temp_dir().join(format!("volt-routes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for file in [
            "layout.tsx",
            "page.tsx",
            "(site)/about/page.tsx",
            "blog/[slug]/page.tsx",
            "blog/layout.tsx",
            "_lib/page.tsx",
        ] {
            let path = dir.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "").unwrap();
        }
        let routes = scan(&dir).unwrap();
        let ids: Vec<&str> = routes.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["/", "/about", "/blog/[slug]", NOT_FOUND, ERROR]);
        let blog = routes.iter().find(|r| r.id == "/blog/[slug]").unwrap();
        assert_eq!(blog.segments.len(), 2);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
