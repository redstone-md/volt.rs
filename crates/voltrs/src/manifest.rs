//! Build manifest written by `volt build` and read by the server.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

pub const MANIFEST_FILE: &str = "manifest.json";
pub const STATIC_PREFIX: &str = "/_volt/static/";
/// Copies of the `app/` metadata files, relative to the out dir.
pub const METADATA_DIR: &str = "metadata";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub build_id: String,
    pub dev: bool,
    /// Server bundle path relative to the out dir.
    pub server: String,
    pub client: ClientAssets,
    pub routes: Vec<RouteEntry>,
    /// Metadata files of `app/` (`icon.png`, `opengraph-image.jpg`, `robots.txt`, ...).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub metadata: Vec<MetadataFile>,
    /// Metadata routes defined in code (`app/sitemap.ts`, ...), by name: see [`crate::seo::MetadataRoute`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub metadata_routes: Vec<String>,
    /// Parallel route slots (`@name` folders) of every layout.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub slots: Vec<SlotEntry>,
    /// Route handlers (`app/**/route.ts`), run in QuickJS.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub handlers: Vec<HandlerEntry>,
    /// `middleware.ts` (or `proxy.ts`), when the project has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub middleware: Option<MiddlewareEntry>,
    /// Set by `volt dev` when the last rebuild failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Counts the successful `volt dev` builds; 0 for production builds.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub revision: u64,
    /// Set by `volt dev` when the browser can apply this build in place with React Fast Refresh.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hot: Option<HotUpdate>,
}

fn is_zero(value: &u64) -> bool {
    *value == 0
}

/// The client modules that `volt dev` rebuilds changed since the build of revision `base`: a page showing any
/// revision from `base` on evaluates the ones newer than its own copy again, then re-renders in place.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HotUpdate {
    pub base: u64,
    pub modules: Vec<HotModule>,
    /// The URLs of the modules importing each app module, keyed by its URL. An update that reaches an importer
    /// outside the app (the entry, a route table) through modules that cannot be swapped alone reloads.
    pub importers: BTreeMap<String, Vec<String>>,
}

/// A changed app module: the URL of its new code and the revision it last changed in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HotModule {
    pub url: String,
    pub revision: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientAssets {
    pub entry: String,
    pub css: Vec<String>,
    pub preload: Vec<String>,
    /// Font files preloaded on every page (fonts declared by the root layout).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fonts: Vec<String>,
}

/// A metadata file of `app/`, served at its Next.js URL.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetadataFile {
    /// URL pattern relative to the base path, e.g. `/blog/[slug]/opengraph-image.png`.
    pub route: String,
    /// The copy served, relative to the out dir.
    pub file: String,
    /// Content hash; head tags reference the file as `<route>?<hash>`, which is cached forever.
    pub hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteEntry {
    /// Route pattern, e.g. `/blog/[slug]`.
    pub id: String,
    pub chunks: Vec<String>,
    /// Concrete paths known at build time: the id of a static route, or the paths returned by
    /// `generateStaticParams` / `getStaticPaths`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,
    /// The subset of `paths` written to `prerender/` by `volt build`, see [`prerender_file`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prerendered: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revalidate: Option<u64>,
    /// Rendered on every request instead of being served from the page cache.
    #[serde(default)]
    pub dynamic: bool,
    /// `false` when the route exports `dynamicParams = false`: paths outside `paths` answer 404.
    #[serde(default = "enabled", skip_serializing_if = "Clone::clone")]
    pub dynamic_params: bool,
    /// Rendered by the Pages Router: data requests also run `getServerSideProps` / `getStaticProps`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pages: bool,
    /// Pages Router `getStaticProps` page: HTML and props are cached per pathname and shared by every visitor.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub static_props: bool,
    /// `dynamic = "force-static"` (or `"error"`): one cached render per pathname, even with a Rust loader,
    /// and empty `searchParams`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub force_static: bool,
    /// Keys of the `app/` directories from the root to the page (`/`, `/dashboard`, ...), whose slots it renders.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub segments: Vec<String>,
    /// Set on intercepting routes (`(.)x`, `(..)x`, `(...)x`), which only render on client navigations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intercept: Option<Intercept>,
    /// Key of the directory whose `default` page this route renders: the URL only exists in its slots.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback: Option<String>,
}

/// What an intercepting route replaces during client-side navigation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Intercept {
    /// Pattern of the intercepted URL, e.g. `/photos/[id]`.
    pub target: String,
    /// Directory key the navigation must start under for the interception to apply.
    pub scope: String,
}

/// A route handler: the `route.ts` of an `app/` directory.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HandlerEntry {
    /// URL pattern, e.g. `/api/posts/[id]`.
    pub id: String,
    /// The `GET` response written by `volt build` for `dynamic = "force-static"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prerendered: Option<PrerenderedResponse>,
}

/// A response written at build time, with its body in a file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PrerenderedResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    /// Body file relative to the out dir, see [`prerender_file`].
    pub file: String,
}

/// `middleware.ts`: which paths it runs on.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MiddlewareEntry {
    /// `config.matcher` rules; empty runs it on every path.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub matcher: Vec<crate::matcher::MatcherRule>,
}

/// A parallel route slot: the `@name` folder of a layout.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SlotEntry {
    /// Directory key of the slot, e.g. `/dashboard/@team`.
    pub key: String,
    /// Directory key of the layout that receives the slot.
    pub segment: String,
    /// Whether the slot has a `default` page for URLs it does not match.
    pub default: bool,
    pub routes: Vec<SlotRoute>,
}

/// A page inside a slot.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SlotRoute {
    /// Directory path of the page inside `app/` with groups removed, e.g. `/@modal/(.)photos/[id]`; also its loader route.
    pub id: String,
    /// URL pattern the page matches, e.g. `/photos/[id]`.
    pub pattern: String,
    /// Matches only on client navigations from inside the slot's layout.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub intercept: bool,
    /// Directory keys of the page inside the slot, for slots nested in it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub segments: Vec<String>,
    /// A `not-found` file inside the slot renders when the page's loader fails with `not_found()`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub not_found: bool,
    /// An `error` file inside the slot renders when the page's loader fails with any other error.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub error: bool,
}

impl Default for RouteEntry {
    fn default() -> Self {
        Self {
            id: String::new(),
            chunks: Vec::new(),
            paths: Vec::new(),
            prerendered: Vec::new(),
            revalidate: None,
            dynamic: false,
            dynamic_params: true,
            pages: false,
            static_props: false,
            force_static: false,
            segments: Vec::new(),
            intercept: None,
            fallback: None,
        }
    }
}

fn enabled() -> bool {
    true
}

impl RouteEntry {
    /// Whether responses of this route are kept in the page cache.
    pub fn is_cached(&self) -> bool {
        !self.dynamic || self.revalidate.is_some()
    }

    /// Whether the route answers its own URL: not a pseudo route like `/_not-found` and not an intercepting route.
    pub fn is_page(&self) -> bool {
        !self.id.starts_with("/_") && self.intercept.is_none()
    }

    /// Whether `path` is allowed by `dynamicParams`.
    pub fn accepts(&self, path: &str) -> bool {
        self.dynamic_params || self.paths.iter().any(|known| same_path(known, path))
    }
}

pub const PRERENDER_DIR: &str = "prerender";

/// Decoded file stem of a concrete path: `/` is `index`, `/blog/a%20b` is `blog/a b`.
/// Returns `None` for paths that would escape the output directory.
pub fn file_stem(path: &str) -> Option<String> {
    let decoded = percent_encoding::percent_decode_str(path).decode_utf8().ok()?;
    let parts: Vec<&str> = decoded.split('/').filter(|s| !s.is_empty()).collect();
    if parts.iter().any(|p| *p == "." || *p == ".." || p.contains('\\') || p.contains('\0')) {
        return None;
    }
    Some(if parts.is_empty() { "index".to_owned() } else { parts.join("/") })
}

/// Prerendered file of `path` relative to the out dir, `extension` being `html` or `json`.
pub fn prerender_file(path: &str, extension: &str) -> Option<String> {
    file_stem(path).map(|stem| format!("{PRERENDER_DIR}/{stem}.{extension}"))
}

/// Compares two URL paths after percent-decoding and trimming trailing slashes.
pub fn same_path(a: &str, b: &str) -> bool {
    let normalize = |p: &str| {
        let decoded = percent_encoding::percent_decode_str(p).decode_utf8_lossy().into_owned();
        let trimmed = decoded.trim_end_matches('/').to_owned();
        if trimmed.is_empty() { "/".to_owned() } else { trimmed }
    };
    normalize(a) == normalize(b)
}

impl Manifest {
    pub fn load(out_dir: &Path) -> std::io::Result<Self> {
        let text = std::fs::read_to_string(out_dir.join(MANIFEST_FILE))?;
        serde_json::from_str(&text).map_err(std::io::Error::other)
    }

    pub fn save(&self, out_dir: &Path) -> std::io::Result<()> {
        let text = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        // Renamed into place so the dev server, which polls the file, never reads a partial manifest.
        let partial = out_dir.join(format!("{MANIFEST_FILE}.tmp"));
        std::fs::write(&partial, text)?;
        std::fs::rename(partial, out_dir.join(MANIFEST_FILE))
    }

    pub fn route(&self, id: &str) -> Option<&RouteEntry> {
        self.routes.iter().find(|r| r.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_prerendered_files() {
        assert_eq!(prerender_file("/", "html").as_deref(), Some("prerender/index.html"));
        assert_eq!(prerender_file("/blog/a%20b/", "json").as_deref(), Some("prerender/blog/a b.json"));
        assert!(file_stem("/a/%2e%2e/b").is_none());
        assert!(same_path("/blog/a%20b/", "/blog/a b"));
    }

    #[test]
    fn enforces_dynamic_params() {
        let entry = RouteEntry { paths: vec!["/blog/a".into()], dynamic_params: false, ..RouteEntry::default() };
        assert!(entry.accepts("/blog/a/"));
        assert!(!entry.accepts("/blog/b"));
        let json = serde_json::to_string(&RouteEntry::default()).unwrap();
        assert!(!json.contains("dynamicParams"));
        let back: RouteEntry = serde_json::from_str(&json).unwrap();
        assert!(back.dynamic_params);
    }
}
