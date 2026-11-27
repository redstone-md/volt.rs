//! `/_volt/image`: on-demand resizing and format conversion for `<Image>`, with a disk cache.

mod codec;
mod remote;

use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header};
use tokio::sync::Semaphore;

pub use codec::{BlurPlaceholder, blur_placeholder};
use codec::{Limits, Target, Transform};
use remote::Remote;

use crate::assets::safe_join;
use crate::config::{Config, ImageConfig, RemoteUrl};
use crate::error::{Error, Result};
use crate::manifest::STATIC_PREFIX;
use crate::response::Response;

static WRITES: AtomicU64 = AtomicU64::new(0);

/// The image optimizer: validates requests, fetches remote originals and caches every variant on disk.
pub struct Images {
    config: ImageConfig,
    widths: Vec<u32>,
    public: PathBuf,
    base_path: String,
    static_dir: PathBuf,
    cache: PathBuf,
    permits: Semaphore,
    remote: Option<Remote>,
}

enum Source {
    Local(PathBuf),
    Remote(RemoteUrl),
}

struct Request {
    source: Source,
    width: u32,
    quality: u8,
}

impl Images {
    pub fn new(config: &Config, out_dir: &Path, concurrency: usize) -> Result<Self> {
        let images = config.images.clone();
        let remote =
            (!images.remote_patterns.is_empty()).then(|| Remote::new(images.dangerously_allow_local_ip)).transpose()?;
        Ok(Self {
            widths: images.widths(),
            config: images,
            public: config.public_dir(),
            base_path: config.base_path().to_owned(),
            static_dir: out_dir.join("static"),
            cache: out_dir.join("cache").join("images"),
            permits: Semaphore::new(concurrency.max(1)),
            remote,
        })
    }

    fn parse(&self, query: &str) -> Result<Request> {
        let pairs = crate::form::parse_pairs(query.as_bytes());
        let get = |key: &str| pairs.iter().find(|(k, _)| k == key).and_then(|(_, v)| v.first()).map(String::as_str);
        let url = get("url").ok_or_else(|| Error::bad_request("missing `url`"))?;
        let width: u32 = get("w").and_then(|w| w.parse().ok()).ok_or_else(|| Error::bad_request("missing `w`"))?;
        let quality: u8 =
            get("q").map_or(Some(75), |q| q.parse().ok()).ok_or_else(|| Error::bad_request("invalid `q`"))?;
        if !self.widths.contains(&width) {
            return Err(Error::bad_request(format!("`w` must be one of {:?}", self.widths)));
        }
        if !(1..=100).contains(&quality) {
            return Err(Error::bad_request("`q` must be between 1 and 100"));
        }
        if url.starts_with("http://") || url.starts_with("https://") {
            let remote = RemoteUrl::parse(url).ok_or_else(|| Error::bad_request("invalid remote `url`"))?;
            if !self.config.allows_remote(&remote) {
                return Err(Error::bad_request("`url` is not allowed by images.remote_patterns"));
            }
            return Ok(Request { source: Source::Remote(remote), width, quality });
        }
        if !url.starts_with('/') || url.starts_with("//") {
            return Err(Error::bad_request("`url` must be a local path or an allowed remote URL"));
        }
        let url = url.strip_prefix(self.base_path.as_str()).filter(|rest| rest.starts_with('/')).unwrap_or(url);
        let source = match url.strip_prefix(STATIC_PREFIX) {
            Some(rest) => safe_join(&self.static_dir, rest),
            None => safe_join(&self.public, url),
        }
        .filter(|p| p.is_file())
        .ok_or_else(Error::not_found)?;
        Ok(Request { source: Source::Local(source), width, quality })
    }

    pub async fn serve(&self, query: &str, headers: &HeaderMap) -> Response {
        match self.try_serve(query, headers).await {
            Ok(response) => response,
            Err(error) => Response::text(error.message().to_owned()).status(error.status()),
        }
    }

    /// The first configured format the client accepts. Only explicit types count, as browsers list the
    /// modern formats they decode and `*/*` says nothing about them.
    fn negotiate(&self, headers: &HeaderMap) -> Target {
        let accept = headers.get(header::ACCEPT).and_then(|v| v.to_str().ok()).unwrap_or_default();
        let accepted = |mime: &str| {
            accept.split(',').any(|part| {
                let mut pieces = part.split(';');
                pieces.next().is_some_and(|t| t.trim().eq_ignore_ascii_case(mime))
                    && !pieces.any(|p| p.trim().strip_prefix("q=").is_some_and(|q| q.trim().parse() == Ok(0.0)))
            })
        };
        self.config.formats.iter().find(|f| accepted(f)).and_then(|f| Target::from_mime(f)).unwrap_or(Target::Source)
    }

    async fn try_serve(&self, query: &str, headers: &HeaderMap) -> Result<Response> {
        let request = self.parse(query)?;
        let (path, name, remote) = match &request.source {
            Source::Local(path) => (path.clone(), stem(path.file_stem().and_then(|s| s.to_str())), false),
            Source::Remote(url) => {
                (self.original(url).await?, stem(Path::new(&url.pathname).file_stem().and_then(|s| s.to_str())), true)
            }
        };
        let modified = std::fs::metadata(&path)?.modified()?;
        let target = self.negotiate(headers);
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        (&path, modified, request.width, request.quality, target).hash(&mut hasher);
        let key = format!("{:016x}", hasher.finish());
        let cached = self.cache.join(&key);
        let etag = format!("\"{key}\"");

        let (body, hit) = match tokio::fs::read(&cached).await {
            Ok(body) => (body, true),
            Err(_) => (self.optimize(&path, &request, target).await?, false),
        };
        if !hit {
            write_atomic(&cached, &body).await?;
        }
        let content_type = codec::content_type(&body).unwrap_or("application/octet-stream");
        let cache_control = if remote {
            format!("public, max-age={}, stale-while-revalidate=604800", self.config.minimum_cache_ttl)
        } else {
            "public, max-age=86400, stale-while-revalidate=604800".to_owned()
        };
        let disposition =
            format!("{}; filename=\"{name}.{}\"", self.config.content_disposition_type, codec::extension(content_type));
        let not_modified = headers.get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok()) == Some(etag.as_str());
        let response = if not_modified {
            Response::empty(StatusCode::NOT_MODIFIED)
        } else {
            Response::with_type(StatusCode::OK, content_type, body)
        };
        Ok(response
            .header(header::CACHE_CONTROL, value(&cache_control)?)
            .header(header::ETAG, value(&etag)?)
            .header(header::VARY, HeaderValue::from_static("Accept"))
            .header(header::CONTENT_SECURITY_POLICY, value(&self.config.content_security_policy)?)
            .header(header::CONTENT_DISPOSITION, value(&disposition)?)
            .header(
                HeaderName::from_static("x-volt-cache"),
                HeaderValue::from_static(if hit { "HIT" } else { "MISS" }),
            ))
    }

    async fn optimize(&self, path: &Path, request: &Request, target: Target) -> Result<Vec<u8>> {
        let source = tokio::fs::read(path).await?;
        if codec::is_svg(&source) {
            if !self.config.dangerously_allow_svg {
                return Err(Error::bad_request(
                    "SVG images are not optimized; set images.dangerously_allow_svg or use `unoptimized`",
                ));
            }
            return Ok(source);
        }
        let transform = Transform {
            width: request.width,
            quality: request.quality,
            target,
            limits: Limits { max_pixels: self.config.max_pixels, max_dimension: self.config.max_dimension },
        };
        let _permit = self.permits.acquire().await.map_err(|_| Error::internal("image pool closed"))?;
        tokio::task::spawn_blocking(move || transform.apply(source))
            .await
            .map_err(|e| Error::internal(e.to_string()))?
    }

    /// Path of the cached original of a remote image, downloaded again once `minimum_cache_ttl` has passed.
    /// A failed refresh keeps serving the previous copy.
    async fn original(&self, url: &RemoteUrl) -> Result<PathBuf> {
        let remote = self.remote.as_ref().ok_or_else(|| Error::bad_request("no remote images are allowed"))?;
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        url.href().hash(&mut hasher);
        let path = self.cache.join("remote").join(format!("{:016x}", hasher.finish()));
        let age = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .map(|modified| SystemTime::now().duration_since(modified).unwrap_or_default());
        if age.is_some_and(|age| age < Duration::from_secs(self.config.minimum_cache_ttl)) {
            return Ok(path);
        }
        let fetched = remote.fetch(url, &self.config).await.and_then(|bytes| match codec::content_type(&bytes) {
            Some(_) => Ok(bytes),
            None => Err(Error::bad_request(format!("{} is not an image", url.href()))),
        });
        match fetched {
            Ok(bytes) => {
                write_atomic(&path, &bytes).await?;
                Ok(path)
            }
            Err(error) if age.is_some() => {
                crate::log::warn(&format!("serving a stale copy of {}: {}", url.href(), error.message()));
                Ok(path)
            }
            Err(error) => Err(error),
        }
    }
}

/// A file name for `Content-Disposition`, restricted to characters that need no quoting.
fn stem(raw: Option<&str>) -> String {
    let clean: String =
        raw.unwrap_or_default().chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_')).collect();
    if clean.is_empty() { "image".into() } else { clean }
}

fn value(raw: &str) -> Result<HeaderValue> {
    HeaderValue::from_str(raw).map_err(|e| Error::internal(e.to_string()))
}

/// Concurrent requests may produce the same file, so each writes its own temp file and renames it into place.
async fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent() {
        tokio::fs::create_dir_all(dir).await?;
    }
    let partial = path.with_extension(format!("{}.{}.tmp", std::process::id(), WRITES.fetch_add(1, Ordering::Relaxed)));
    tokio::fs::write(&partial, bytes).await?;
    tokio::fs::rename(&partial, path).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn images(formats: &[&str]) -> Images {
        let mut config = Config::default();
        config.images.formats = formats.iter().map(|f| (*f).to_owned()).collect();
        Images::new(&config, Path::new("/nonexistent"), 1).unwrap()
    }

    fn accept(value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(header::ACCEPT, HeaderValue::from_str(value).unwrap());
        headers
    }

    #[test]
    fn negotiates_formats_in_configured_order() {
        let both = images(&["image/avif", "image/webp"]);
        assert_eq!(both.negotiate(&accept("image/avif,image/webp,*/*")), Target::Avif);
        assert_eq!(both.negotiate(&accept("image/webp,*/*")), Target::WebP);
        assert_eq!(both.negotiate(&accept("image/avif;q=0, image/webp")), Target::WebP);
        assert_eq!(both.negotiate(&accept("*/*")), Target::Source);
        assert_eq!(both.negotiate(&HeaderMap::new()), Target::Source);
        assert_eq!(images(&["image/webp"]).negotiate(&accept("image/avif,image/webp")), Target::WebP);
        assert_eq!(images(&[]).negotiate(&accept("image/avif,image/webp")), Target::Source);
    }

    #[test]
    fn validates_requests() {
        let images = images(&["image/webp"]);
        assert!(images.parse("url=%2Fa.png&w=100").is_err());
        assert!(images.parse("url=%2F%2Fevil.dev%2Fa.png&w=64").is_err());
        assert!(images.parse("url=https%3A%2F%2Fcdn.example.com%2Fa.png&w=64").is_err());
        assert!(images.parse("url=%2Fa.png&w=64&q=0").is_err());
        assert_eq!(stem(Some("my photo\"; x")), "myphotox");
        assert_eq!(stem(None), "image");
    }
}
