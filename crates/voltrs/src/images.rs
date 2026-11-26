//! `/_volt/image`: on-demand resizing for `<Image>` with a disk cache.

use std::hash::{Hash, Hasher};
use std::io::Cursor;
use std::path::{Path, PathBuf};

use http::{HeaderValue, StatusCode, header};
use image::{ImageFormat, ImageReader, codecs::jpeg::JpegEncoder};
use tokio::sync::Semaphore;

use crate::assets::{mime, safe_join};
use crate::error::{Error, Result};
use crate::manifest::STATIC_PREFIX;
use crate::response::Response;

/// Widths accepted from `<Image>`, matching `DEVICE_SIZES` and `IMAGE_SIZES` in `volt-rs/image`.
pub const WIDTHS: [u32; 16] = [16, 32, 48, 64, 96, 128, 256, 384, 640, 750, 828, 1080, 1200, 1920, 2048, 3840];

pub struct Images {
    public: PathBuf,
    static_dir: PathBuf,
    cache: PathBuf,
    permits: Semaphore,
}

struct Request {
    source: PathBuf,
    width: u32,
    quality: u8,
}

impl Images {
    pub fn new(public: PathBuf, out_dir: &Path, concurrency: usize) -> Self {
        Self {
            public,
            static_dir: out_dir.join("static"),
            cache: out_dir.join("cache").join("images"),
            permits: Semaphore::new(concurrency.max(1)),
        }
    }

    fn parse(&self, query: &str) -> Result<Request> {
        let pairs = crate::form::parse_pairs(query.as_bytes());
        let get = |key: &str| pairs.iter().find(|(k, _)| k == key).and_then(|(_, v)| v.first()).map(String::as_str);
        let url = get("url").ok_or_else(|| Error::bad_request("missing `url`"))?;
        let width: u32 = get("w").and_then(|w| w.parse().ok()).ok_or_else(|| Error::bad_request("missing `w`"))?;
        let quality: u8 =
            get("q").map_or(Some(75), |q| q.parse().ok()).ok_or_else(|| Error::bad_request("invalid `q`"))?;
        if !WIDTHS.contains(&width) {
            return Err(Error::bad_request(format!("`w` must be one of {WIDTHS:?}")));
        }
        if !(1..=100).contains(&quality) {
            return Err(Error::bad_request("`q` must be between 1 and 100"));
        }
        if !url.starts_with('/') || url.starts_with("//") {
            return Err(Error::bad_request("only local images can be optimized"));
        }
        let source = match url.strip_prefix(STATIC_PREFIX) {
            Some(rest) => safe_join(&self.static_dir, rest),
            None => safe_join(&self.public, url),
        }
        .filter(|p| p.is_file())
        .ok_or_else(Error::not_found)?;
        Ok(Request { source, width, quality })
    }

    pub async fn serve(&self, query: &str) -> Response {
        match self.try_serve(query).await {
            Ok(response) => response,
            Err(error) => Response::text(error.message().to_owned()).status(error.status()),
        }
    }

    async fn try_serve(&self, query: &str) -> Result<Response> {
        let request = self.parse(query)?;
        let modified = std::fs::metadata(&request.source)?.modified()?;
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        (&request.source, request.width, request.quality, modified).hash(&mut hasher);
        let ext = request.source.extension().and_then(|e| e.to_str()).unwrap_or("bin").to_ascii_lowercase();
        let ext = if ext == "gif" { "png".to_owned() } else { ext };
        let cached = self.cache.join(format!("{:016x}.{ext}", hasher.finish()));
        let etag = format!("\"{:016x}\"", hasher.finish());

        let body = match tokio::fs::read(&cached).await {
            Ok(body) => body,
            Err(_) => {
                let _permit = self.permits.acquire().await.map_err(|_| Error::internal("image pool closed"))?;
                let source = request.source.clone();
                let body = tokio::task::spawn_blocking(move || resize(&source, request.width, request.quality))
                    .await
                    .map_err(|e| Error::internal(e.to_string()))??;
                tokio::fs::create_dir_all(&self.cache).await?;
                tokio::fs::write(&cached, &body).await?;
                body
            }
        };
        Ok(Response::with_type(StatusCode::OK, mime(&cached), body)
            .header(
                header::CACHE_CONTROL,
                HeaderValue::from_static("public, max-age=86400, stale-while-revalidate=604800"),
            )
            .header(header::ETAG, HeaderValue::from_str(&etag).map_err(|e| Error::internal(e.to_string()))?))
    }
}

fn resize(source: &Path, width: u32, quality: u8) -> Result<Vec<u8>> {
    let reader = ImageReader::open(source)?.with_guessed_format()?;
    let format = reader.format().ok_or_else(|| Error::bad_request("unsupported image format"))?;
    let image = reader.decode().map_err(|e| Error::bad_request(e.to_string()))?;
    let image = if image.width() > width {
        image.resize(width, u32::MAX, image::imageops::FilterType::Lanczos3)
    } else {
        image
    };
    let mut out = Cursor::new(Vec::new());
    match format {
        ImageFormat::Jpeg => image
            .to_rgb8()
            .write_with_encoder(JpegEncoder::new_with_quality(&mut out, quality))
            .map_err(|e| Error::internal(e.to_string()))?,
        ImageFormat::WebP => image.write_to(&mut out, ImageFormat::WebP).map_err(|e| Error::internal(e.to_string()))?,
        _ => image.write_to(&mut out, ImageFormat::Png).map_err(|e| Error::internal(e.to_string()))?,
    }
    Ok(out.into_inner())
}
