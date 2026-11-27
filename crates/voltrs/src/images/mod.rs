//! `/_volt/image`: on-demand resizing and format conversion for `<Image>`, with a disk cache.

mod codec;
mod remote;

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header};
use tokio::sync::{OwnedMutexGuard, Semaphore};

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
    /// AVIF encodes take seconds, so they get at most half of `permits` and cannot starve the other formats.
    avif_permits: Semaphore,
    flights: Flights,
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
            avif_permits: Semaphore::new((concurrency / 2).max(1)),
            flights: Flights::default(),
            remote,
        })
    }

    fn parse(&self, query: &str) -> Result<Request> {
        let pairs = crate::form::parse_pairs(query.as_bytes());
        let get = |key: &str| pairs.iter().find(|(k, _)| k == key).and_then(|(_, v)| v.first()).map(String::as_str);
        let url = get("url").ok_or_else(|| Error::bad_request("missing `url`"))?;
        let width: u32 = get("w").and_then(|w| w.parse().ok()).ok_or_else(|| Error::bad_request("missing `w`"))?;
        let quality: u8 = get("q")
            .map_or(Some(self.config.closest_quality(75)), |q| q.parse().ok())
            .ok_or_else(|| Error::bad_request("invalid `q`"))?;
        if !self.widths.contains(&width) {
            return Err(Error::bad_request(format!("`w` must be one of {:?}", self.widths)));
        }
        if !self.config.qualities.contains(&quality) {
            return Err(Error::bad_request(format!("`q` must be one of images.qualities {:?}", self.config.qualities)));
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
        self.config
            .formats
            .iter()
            .find(|f| accepts(headers, f))
            .and_then(|f| Target::from_mime(f))
            .unwrap_or(Target::Source)
    }

    /// Cache key of a variant. The policy settings are part of it, so tightening them in `volt.toml` does not
    /// keep serving variants produced under the old rules.
    fn variant_key(&self, path: &Path, modified: SystemTime, request: &Request, target: Target) -> String {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        (path, modified, request.width, request.quality, target).hash(&mut hasher);
        (self.config.dangerously_allow_svg, self.config.max_pixels, self.config.max_dimension).hash(&mut hasher);
        format!("{:016x}", hasher.finish())
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
        let key = self.variant_key(&path, modified, &request, target);
        let cached = self.cache.join(&key);
        let etag = format!("\"{key}\"");

        let (body, hit) = match tokio::fs::read(&cached).await {
            Ok(body) => (body, true),
            Err(_) => {
                // Concurrent requests for one variant wait for the first instead of encoding it again.
                let _flight = self.flights.join(key.clone()).await;
                match tokio::fs::read(&cached).await {
                    Ok(body) => (body, true),
                    Err(_) => {
                        let body = self.optimize(&path, &request, target).await?;
                        write_atomic(&cached, &body).await?;
                        (body, false)
                    }
                }
            }
        };
        let content_type = codec::content_type(&body).unwrap_or("application/octet-stream");
        if content_type == "image/svg+xml" && !self.config.dangerously_allow_svg {
            return Err(svg_refused());
        }
        // AVIF sources pass through unresized; a client that did not ask for AVIF may not be able to show them.
        if content_type == "image/avif" && !accepts(headers, "image/avif") {
            return Err(Error::new(
                StatusCode::NOT_ACCEPTABLE,
                "the source is AVIF, which this build cannot convert, and the request does not accept image/avif",
            ));
        }
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
                return Err(svg_refused());
            }
            return Ok(source);
        }
        let transform = Transform {
            width: request.width,
            quality: request.quality,
            target,
            limits: Limits { max_pixels: self.config.max_pixels, max_dimension: self.config.max_dimension },
        };
        let closed = |_| Error::internal("image pool closed");
        let _avif = match target {
            Target::Avif => Some(self.avif_permits.acquire().await.map_err(closed)?),
            _ => None,
        };
        let _permit = self.permits.acquire().await.map_err(closed)?;
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
        let key = format!("remote/{:016x}", hasher.finish());
        let path = self.cache.join(&key);
        let ttl = Duration::from_secs(self.config.minimum_cache_ttl);
        if age(&path).is_some_and(|age| age < ttl) {
            return Ok(path);
        }
        // One download per original: requests that waited here find the copy the first one stored.
        let _flight = self.flights.join(key).await;
        let age = age(&path);
        if age.is_some_and(|age| age < ttl) {
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

/// Time since a cached file was written, `None` when it does not exist.
fn age(path: &Path) -> Option<Duration> {
    let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok()?;
    Some(SystemTime::now().duration_since(modified).unwrap_or_default())
}

/// Whether `Accept` lists `mime` explicitly with a non-zero quality. `*/*` and `image/*` do not count, as browsers
/// list the modern formats they decode.
fn accepts(headers: &HeaderMap, mime: &str) -> bool {
    let accept = headers.get(header::ACCEPT).and_then(|v| v.to_str().ok()).unwrap_or_default();
    accept.split(',').any(|part| {
        let mut pieces = part.split(';');
        pieces.next().is_some_and(|t| t.trim().eq_ignore_ascii_case(mime))
            && !pieces.any(|p| p.trim().strip_prefix("q=").is_some_and(|q| q.trim().parse() == Ok(0.0)))
    })
}

fn svg_refused() -> Error {
    Error::bad_request("SVG images are not optimized; set images.dangerously_allow_svg or use `unoptimized`")
}

/// Per-key locks that let one request produce a file while concurrent requests for it wait.
#[derive(Default)]
struct Flights(Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>);

impl Flights {
    async fn join(&self, key: String) -> Flight<'_> {
        let lock = self.0.lock().unwrap_or_else(|e| e.into_inner()).entry(key.clone()).or_default().clone();
        Flight { flights: self, key, guard: Some(lock.lock_owned().await) }
    }
}

struct Flight<'a> {
    flights: &'a Flights,
    key: String,
    guard: Option<OwnedMutexGuard<()>>,
}

impl Drop for Flight<'_> {
    fn drop(&mut self) {
        let mut map = self.flights.0.lock().unwrap_or_else(|e| e.into_inner());
        drop(self.guard.take());
        // Nobody else holds or waits for the lock once the map owns the only reference.
        if map.get(&self.key).is_some_and(|lock| Arc::strong_count(lock) == 1) {
            map.remove(&self.key);
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
    fn keys_variants_by_policy() {
        let open = images(&["image/webp"]);
        let request = Request { source: Source::Local("/a.svg".into()), width: 64, quality: 75 };
        let key =
            |images: &Images| images.variant_key(Path::new("/a.svg"), SystemTime::UNIX_EPOCH, &request, Target::WebP);
        let mut config = Config::default();
        config.images.dangerously_allow_svg = true;
        let svg = Images::new(&config, Path::new("/nonexistent"), 1).unwrap();
        assert_ne!(key(&open), key(&svg));
        assert_eq!(key(&open), key(&images(&["image/webp"])));
    }

    #[test]
    fn accepts_only_explicit_types() {
        assert!(accepts(&accept("image/avif,image/webp"), "image/avif"));
        assert!(!accepts(&accept("image/webp,image/*,*/*"), "image/avif"));
        assert!(!accepts(&accept("image/avif;q=0"), "image/avif"));
    }

    #[tokio::test]
    async fn flights_wait_for_each_other_and_clean_up() {
        let flights = Flights::default();
        let first = flights.join("a".into()).await;
        drop(flights.join("b".into()).await);
        assert!(tokio::time::timeout(Duration::from_millis(50), flights.join("a".into())).await.is_err());
        drop(first);
        drop(flights.join("a".into()).await);
        assert!(flights.0.lock().unwrap().is_empty());
    }

    #[test]
    fn validates_requests() {
        let images = images(&["image/webp"]);
        assert!(images.parse("url=%2Fa.png&w=100").is_err());
        assert!(images.parse("url=%2F%2Fevil.dev%2Fa.png&w=64").is_err());
        assert!(images.parse("url=https%3A%2F%2Fcdn.example.com%2Fa.png&w=64").is_err());
        assert!(images.parse("url=%2Fa.png&w=64&q=0").is_err());
        assert!(images.parse("url=%2Fa.png&w=64&q=61").is_err());
        assert_eq!(stem(Some("my photo\"; x")), "myphotox");
        assert_eq!(stem(None), "image");
    }
}
