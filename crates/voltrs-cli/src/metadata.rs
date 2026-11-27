//! App Router metadata conventions: metadata files (`icon.png`, `opengraph-image.jpg`, `robots.txt`, ...) at any
//! segment and code-defined metadata routes (`app/sitemap.ts`, `app/robots.ts`, `app/manifest.ts`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde_json::{Map, Value, json};
use voltrs::manifest::{METADATA_DIR, MetadataFile};
use voltrs::router::RoutePattern;
use voltrs::seo::MetadataRoute;

use crate::util::hash;

/// What a metadata file is, from its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Favicon,
    Icon,
    Apple,
    OpenGraph,
    Twitter,
    Manifest,
    Robots,
    Sitemap,
}

impl Kind {
    /// Recognizes `name.ext`; numbered variants (`icon1.png`) are allowed for images, root-only files only at the root.
    fn of(stem: &str, ext: &str, root: bool) -> Option<Self> {
        let numbered = |prefix: &str| stem.strip_prefix(prefix).is_some_and(|n| n.chars().all(|c| c.is_ascii_digit()));
        let kind = match ext {
            "ico" if stem == "favicon" => Self::Favicon,
            "ico" | "jpg" | "jpeg" | "png" | "svg" if numbered("icon") => Self::Icon,
            "jpg" | "jpeg" | "png" if numbered("apple-icon") => Self::Apple,
            "jpg" | "jpeg" | "png" | "gif" | "webp" if numbered("opengraph-image") => Self::OpenGraph,
            "jpg" | "jpeg" | "png" | "gif" | "webp" if numbered("twitter-image") => Self::Twitter,
            "json" | "webmanifest" if stem == "manifest" => Self::Manifest,
            "txt" if stem == "robots" => Self::Robots,
            "xml" if stem == "sitemap" => Self::Sitemap,
            _ => return None,
        };
        let root_only = matches!(kind, Self::Favicon | Self::Manifest | Self::Robots | Self::Sitemap);
        (root || !root_only).then_some(kind)
    }

    /// Images referenced from the head with a `?<hash>` URL, cached forever.
    fn hashed(self) -> bool {
        matches!(self, Self::Icon | Self::Apple | Self::OpenGraph | Self::Twitter)
    }
}

/// A metadata file found in a segment directory.
#[derive(Debug, Clone)]
pub struct File {
    pub kind: Kind,
    pub source: PathBuf,
    /// URL pattern relative to the base path, e.g. `/blog/[slug]/opengraph-image.png`.
    pub route: String,
    pub hash: String,
    ext: String,
    size: Option<(usize, usize)>,
    /// Contents of `<name>.alt.txt` next to an Open Graph or Twitter image.
    alt: Option<String>,
}

impl File {
    /// The metadata files of a segment directory whose URL (with groups and dynamic segments) is `url`.
    pub fn scan(dir: &Path, url: &str, root: bool) -> Result<Vec<Self>> {
        let mut files = Vec::new();
        let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)?.flatten().map(|e| e.path()).collect();
        entries.sort();
        for path in entries.into_iter().filter(|p| p.is_file()) {
            let (Some(stem), Some(ext)) = (path.file_stem(), path.extension()) else { continue };
            let (stem, ext) = (stem.to_string_lossy(), ext.to_string_lossy().to_ascii_lowercase());
            let Some(kind) = Kind::of(&stem, &ext, root) else { continue };
            let bytes = std::fs::read(&path).with_context(|| format!("cannot read {}", path.display()))?;
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let alt = matches!(kind, Kind::OpenGraph | Kind::Twitter)
                .then(|| std::fs::read_to_string(dir.join(format!("{stem}.alt.txt"))).ok())
                .flatten()
                .map(|text| text.trim().to_owned());
            files.push(Self {
                kind,
                route: RoutePattern::parse(&format!("{url}/{name}")).as_str().to_owned(),
                hash: hash(&bytes),
                size: imagesize::blob_size(&bytes).ok().map(|size| (size.width, size.height)),
                ext,
                alt,
                source: path,
            });
        }
        Ok(files)
    }

    fn url(&self, base_path: &str) -> String {
        let url = format!("{base_path}{}", self.route);
        if self.kind.hashed() { format!("{url}?{}", self.hash) } else { url }
    }

    fn content_type(&self) -> &'static str {
        voltrs::assets::mime(Path::new(&format!("x.{}", self.ext)))
    }

    fn icon(&self, base_path: &str) -> Value {
        let sizes = match self.size {
            _ if self.ext == "svg" => "any".to_owned(),
            Some((width, height)) => format!("{width}x{height}"),
            None => "any".to_owned(),
        };
        json!({ "url": self.url(base_path), "type": self.content_type(), "sizes": sizes })
    }

    fn image(&self, base_path: &str) -> Value {
        let mut image = json!({ "url": self.url(base_path), "type": self.content_type() });
        if let Some((width, height)) = self.size {
            image["width"] = json!(width);
            image["height"] = json!(height);
        }
        if let Some(alt) = &self.alt {
            image["alt"] = json!(alt);
        }
        image
    }
}

/// Metadata the files of one segment contribute to every route below it, as read by `resolveMetadata`
/// (`volt-rs/internal/metadata`). URLs may contain `[param]` segments, filled per request.
pub fn head(files: &[File], base_path: &str, manifest: Option<MetadataRoute>) -> Option<Value> {
    let mut head = Map::new();
    let mut add = |key: &str, kinds: &[Kind], render: &dyn Fn(&File) -> Value| {
        let values: Vec<Value> = files.iter().filter(|f| kinds.contains(&f.kind)).map(render).collect();
        if !values.is_empty() {
            head.insert(key.to_owned(), Value::Array(values));
        }
    };
    add("icon", &[Kind::Favicon, Kind::Icon], &|f| f.icon(base_path));
    add("apple", &[Kind::Apple], &|f| f.icon(base_path));
    add("openGraph", &[Kind::OpenGraph], &|f| f.image(base_path));
    add("twitter", &[Kind::Twitter], &|f| f.image(base_path));
    let manifest = files
        .iter()
        .find(|f| f.kind == Kind::Manifest)
        .map(|f| f.url(base_path))
        .or_else(|| manifest.map(|route| format!("{base_path}{}", route.path())));
    if let Some(url) = manifest {
        head.insert("manifest".into(), json!(url));
    }
    (!head.is_empty()).then_some(Value::Object(head))
}

/// Copies the files into `<out>/metadata` and returns their manifest entries, one per route.
pub fn write<'a>(files: impl IntoIterator<Item = &'a File>, out_dir: &Path) -> Result<Vec<MetadataFile>> {
    let dir = out_dir.join(METADATA_DIR);
    crate::util::reset_dir(&dir)?;
    let mut entries = BTreeMap::new();
    for file in files {
        if entries.contains_key(&file.route) {
            continue;
        }
        let name = format!("{}.{}", file.hash, file.ext);
        std::fs::copy(&file.source, dir.join(&name))
            .with_context(|| format!("cannot copy {}", file.source.display()))?;
        entries.insert(
            file.route.clone(),
            MetadataFile { route: file.route.clone(), file: format!("{METADATA_DIR}/{name}"), hash: file.hash.clone() },
        );
    }
    Ok(entries.into_values().collect())
}

/// Code-defined metadata routes of the app directory: `sitemap`, `robots` and `manifest` scripts.
pub fn routes(app_dir: Option<&Path>) -> Result<Vec<(MetadataRoute, PathBuf)>> {
    let Some(dir) = app_dir else { return Ok(Vec::new()) };
    let mut found = Vec::new();
    for route in MetadataRoute::ALL {
        let Some(script) = crate::routes::find_file(dir, route.name()) else { continue };
        let fixed: &[&str] = match route {
            MetadataRoute::Sitemap => &["sitemap.xml"],
            MetadataRoute::Robots => &["robots.txt"],
            MetadataRoute::Manifest => &["manifest.json", "manifest.webmanifest"],
        };
        if let Some(file) = fixed.iter().find(|f| dir.join(f).is_file()) {
            bail!("app/{file} and {} both define {}; keep one", script.display(), route.path());
        }
        found.push((route, script));
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_metadata_files() {
        assert_eq!(Kind::of("favicon", "ico", true), Some(Kind::Favicon));
        assert_eq!(Kind::of("favicon", "ico", false), None);
        assert_eq!(Kind::of("icon2", "png", false), Some(Kind::Icon));
        assert_eq!(Kind::of("icons", "png", false), None);
        assert_eq!(Kind::of("opengraph-image", "webp", false), Some(Kind::OpenGraph));
        assert_eq!(Kind::of("apple-icon", "svg", true), None);
        assert_eq!(Kind::of("robots", "txt", false), None);
    }

    #[test]
    fn describes_head_tags() {
        let dir = std::env::temp_dir().join(format!("volt-metadata-files-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("icon.svg"), "<svg xmlns=\"http://www.w3.org/2000/svg\"/>").unwrap();
        std::fs::write(dir.join("opengraph-image.alt.txt"), " A cover \n").unwrap();
        std::fs::write(dir.join("opengraph-image.gif"), b"GIF89a\x02\x00\x03\x00\x00\x00\x00;").unwrap();
        std::fs::write(dir.join("page.tsx"), "").unwrap();
        let files = File::scan(&dir, "/(site)/blog/[slug]", false).unwrap();
        assert_eq!(files.len(), 2);
        let head = head(&files, "/docs", Some(MetadataRoute::Manifest)).unwrap();
        let og = &head["openGraph"][0];
        assert_eq!(og["url"], format!("/docs/blog/[slug]/opengraph-image.gif?{}", files[1].hash));
        assert_eq!((og["width"].as_u64(), og["height"].as_u64()), (Some(2), Some(3)));
        assert_eq!(og["alt"], "A cover");
        assert_eq!(head["icon"][0]["sizes"], "any");
        assert_eq!(head["icon"][0]["type"], "image/svg+xml");
        assert_eq!(head["manifest"], "/docs/manifest.webmanifest");
        let out = dir.join("out");
        let entries = write(&files, &out).unwrap();
        assert_eq!(entries[0].route, "/blog/[slug]/icon.svg");
        assert!(out.join(&entries[0].file).is_file());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
