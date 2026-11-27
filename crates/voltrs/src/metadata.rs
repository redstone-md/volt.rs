//! Metadata files of `app/` (`favicon.ico`, `icon.png`, `opengraph-image.jpg`, `robots.txt`, ...) served
//! at their Next.js URLs.

use std::path::Path;

use bytes::Bytes;
use http::{HeaderMap, HeaderValue, StatusCode, header};

use crate::assets::mime;
use crate::manifest::MetadataFile;
use crate::response::Response;
use crate::router::Router;

struct Asset {
    body: Bytes,
    content_type: &'static str,
    hash: String,
}

/// The metadata files of a build, kept in memory.
#[derive(Default)]
pub struct MetadataAssets {
    routes: Router<usize>,
    files: Vec<Asset>,
}

impl MetadataAssets {
    pub fn load(out_dir: &Path, entries: &[MetadataFile]) -> Self {
        let mut assets = Self::default();
        for entry in entries {
            let path = out_dir.join(&entry.file);
            let Ok(body) = std::fs::read(&path) else {
                crate::log::warn(&format!("metadata file {} is missing", path.display()));
                continue;
            };
            assets.routes.insert(&entry.route, assets.files.len());
            assets.files.push(Asset { body: body.into(), content_type: mime(&path), hash: entry.hash.clone() });
        }
        assets
    }

    /// Answers a request for a metadata file. Head tags link `?<hash>` URLs, which never change; plain URLs
    /// (`/favicon.ico`, `/robots.txt`) revalidate with an ETag.
    pub fn serve(&self, path: &str, query: Option<&str>, headers: &HeaderMap) -> Option<Response> {
        let (_, index, _) = self.routes.find(path)?;
        let asset = &self.files[*index];
        let etag = HeaderValue::from_str(&format!("\"{}\"", asset.hash)).ok()?;
        let cache = if query == Some(asset.hash.as_str()) {
            HeaderValue::from_static("public, max-age=31536000, immutable")
        } else {
            HeaderValue::from_static("public, max-age=0, must-revalidate")
        };
        if headers.get(header::IF_NONE_MATCH) == Some(&etag) {
            return Some(
                Response::empty(StatusCode::NOT_MODIFIED)
                    .header(header::ETAG, etag)
                    .header(header::CACHE_CONTROL, cache),
            );
        }
        Some(
            Response::with_type(StatusCode::OK, asset.content_type, asset.body.clone())
                .header(header::ETAG, etag)
                .header(header::CACHE_CONTROL, cache),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serves_files_by_route() {
        let dir = std::env::temp_dir().join(format!("volt-metadata-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("metadata")).unwrap();
        std::fs::write(dir.join("metadata/abc.png"), b"png").unwrap();
        let entries = [MetadataFile {
            route: "/blog/[slug]/opengraph-image.png".into(),
            file: "metadata/abc.png".into(),
            hash: "abc".into(),
        }];
        let assets = MetadataAssets::load(&dir, &entries);
        let headers = HeaderMap::new();
        let fresh = assets.serve("/blog/hello/opengraph-image.png", Some("abc"), &headers).unwrap();
        assert_eq!(fresh.headers[header::CONTENT_TYPE], "image/png");
        assert!(fresh.headers[header::CACHE_CONTROL].to_str().unwrap().contains("immutable"));
        let plain = assets.serve("/blog/hello/opengraph-image.png", None, &headers).unwrap();
        assert!(plain.headers[header::CACHE_CONTROL].to_str().unwrap().contains("must-revalidate"));
        assert!(assets.serve("/blog/opengraph-image.png", None, &headers).is_none());
        let mut conditional = HeaderMap::new();
        conditional.insert(header::IF_NONE_MATCH, HeaderValue::from_static("\"abc\""));
        assert_eq!(assets.serve("/blog/x/opengraph-image.png", None, &conditional).unwrap().status, 304);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
