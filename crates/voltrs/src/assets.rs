//! Static file serving: hashed build assets from memory, `public/` from disk.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

use bytes::Bytes;
use http::{HeaderMap, HeaderValue, StatusCode, header};

use crate::response::Response;

pub fn mime(path: &Path) -> &'static str {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
    match ext.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json",
        "webmanifest" => "application/manifest+json",
        "txt" => "text/plain; charset=utf-8",
        "xml" => "application/xml",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "wasm" => "application/wasm",
        "pdf" => "application/pdf",
        _ => "application/octet-stream",
    }
}

pub fn compressible(content_type: &str) -> bool {
    content_type.starts_with("text/")
        || content_type.starts_with("application/json")
        || content_type.starts_with("application/xml")
        || content_type.starts_with("application/manifest")
        || content_type.starts_with("image/svg")
        || content_type.starts_with("application/wasm")
}

/// Joins a URL path onto `root`, rejecting traversal.
pub fn safe_join(root: &Path, url_path: &str) -> Option<PathBuf> {
    let decoded = percent_encoding::percent_decode_str(url_path).decode_utf8().ok()?;
    let relative = Path::new(decoded.trim_start_matches('/'));
    if relative.components().any(|c| !matches!(c, Component::Normal(_))) {
        return None;
    }
    Some(root.join(relative))
}

/// Returns whether the client accepts `(brotli, gzip)`.
pub fn accepts(headers: &HeaderMap) -> (bool, bool) {
    let accept = headers.get(header::ACCEPT_ENCODING).and_then(|v| v.to_str().ok()).unwrap_or_default();
    (accept.contains("br"), accept.contains("gzip"))
}

struct Asset {
    body: Bytes,
    /// Content hash, for revalidated development assets.
    etag: Option<HeaderValue>,
    gzip: Option<Bytes>,
    brotli: Option<Bytes>,
    content_type: &'static str,
}

/// Immutable build assets kept in memory with their precompressed variants.
#[derive(Default)]
pub struct StaticAssets {
    files: HashMap<String, Asset>,
    /// Development builds keep their file names across rebuilds, so browsers must revalidate them.
    revalidate: bool,
}

impl StaticAssets {
    pub fn load(dir: &Path, dev: bool) -> Self {
        let mut assets = Self { revalidate: dev, ..Self::default() };
        assets.collect(dir, dir);
        assets
    }

    fn collect(&mut self, root: &Path, dir: &Path) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                self.collect(root, &path);
                continue;
            }
            let ext = path.extension().and_then(|e| e.to_str());
            if matches!(ext, Some("gz" | "br")) {
                continue;
            }
            let Ok(body) = std::fs::read(&path) else {
                continue;
            };
            let sibling = |suffix: &str| {
                let mut name = path.clone().into_os_string();
                name.push(suffix);
                std::fs::read(PathBuf::from(name)).ok().map(Bytes::from)
            };
            let key = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
            let etag = self.revalidate.then(|| {
                let mut hasher = DefaultHasher::new();
                body.hash(&mut hasher);
                HeaderValue::from_str(&format!("W/\"{:016x}\"", hasher.finish())).ok()
            });
            self.files.insert(
                key,
                Asset {
                    gzip: sibling(".gz"),
                    brotli: sibling(".br"),
                    content_type: mime(&path),
                    etag: etag.flatten(),
                    body: body.into(),
                },
            );
        }
    }

    pub fn serve(&self, key: &str, headers: &HeaderMap) -> Option<Response> {
        let asset = self.files.get(key)?;
        if let Some(etag) = &asset.etag
            && headers.get(header::IF_NONE_MATCH) == Some(etag)
        {
            return Some(
                Response::empty(StatusCode::NOT_MODIFIED)
                    .header(header::ETAG, etag.clone())
                    .header(header::CACHE_CONTROL, HeaderValue::from_static("no-cache")),
            );
        }
        let (br, gzip) = accepts(headers);
        let (body, encoding) = match (&asset.brotli, &asset.gzip) {
            (Some(body), _) if br => (body.clone(), Some("br")),
            (_, Some(body)) if gzip => (body.clone(), Some("gzip")),
            _ => (asset.body.clone(), None),
        };
        let mut response = Response::with_type(StatusCode::OK, asset.content_type, body)
            .header(
                header::CACHE_CONTROL,
                HeaderValue::from_static(if self.revalidate {
                    "no-cache"
                } else {
                    "public, max-age=31536000, immutable"
                }),
            )
            .header(header::VARY, HeaderValue::from_static("Accept-Encoding"));
        if let Some(encoding) = encoding {
            response.headers.insert(header::CONTENT_ENCODING, HeaderValue::from_static(encoding));
        }
        if let Some(etag) = &asset.etag {
            response.headers.insert(header::ETAG, etag.clone());
        }
        Some(response)
    }
}

/// Serves a file from `public/` with ETag revalidation.
pub async fn serve_public(root: &Path, url_path: &str, headers: &HeaderMap) -> Option<Response> {
    let path = safe_join(root, url_path)?;
    let metadata = tokio::fs::metadata(&path).await.ok()?;
    if !metadata.is_file() {
        return None;
    }
    let modified = metadata.modified().ok().and_then(|m| m.duration_since(UNIX_EPOCH).ok()).unwrap_or_default();
    let etag = format!("W/\"{:x}-{:x}\"", metadata.len(), modified.as_secs());
    let cache = HeaderValue::from_static("public, max-age=0, must-revalidate");
    let tag = HeaderValue::from_str(&etag).ok()?;
    if headers.get(header::IF_NONE_MATCH).is_some_and(|v| v.as_bytes() == etag.as_bytes()) {
        return Some(
            Response::empty(StatusCode::NOT_MODIFIED).header(header::ETAG, tag).header(header::CACHE_CONTROL, cache),
        );
    }
    let body = tokio::fs::read(&path).await.ok()?;
    Some(
        Response::with_type(StatusCode::OK, mime(&path), body)
            .header(header::ETAG, tag)
            .header(header::CACHE_CONTROL, cache),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revalidates_development_assets() {
        let dir = std::env::temp_dir().join(format!("volt-assets-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("js")).unwrap();
        std::fs::write(dir.join("js/main.js"), "export {};").unwrap();
        let none = HeaderMap::new();
        let production = StaticAssets::load(&dir, false).serve("js/main.js", &none).unwrap();
        assert!(production.headers.get(header::ETAG).is_none());

        let assets = StaticAssets::load(&dir, true);
        let first = assets.serve("js/main.js", &none).unwrap();
        assert_eq!(first.status, StatusCode::OK);
        assert_eq!(first.headers[header::CACHE_CONTROL], "no-cache");
        let mut headers = HeaderMap::new();
        headers.insert(header::IF_NONE_MATCH, first.headers[header::ETAG].clone());
        assert_eq!(assets.serve("js/main.js", &headers).unwrap().status, StatusCode::NOT_MODIFIED);
        headers.insert(header::IF_NONE_MATCH, HeaderValue::from_static("\"other\""));
        assert_eq!(assets.serve("js/main.js", &headers).unwrap().status, StatusCode::OK);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rejects_traversal() {
        let root = Path::new("/srv/public");
        assert!(safe_join(root, "/../etc/passwd").is_none());
        assert!(safe_join(root, "/%2e%2e/etc/passwd").is_none());
        assert_eq!(safe_join(root, "/img/a.png").unwrap(), PathBuf::from("/srv/public/img/a.png"));
    }
}
