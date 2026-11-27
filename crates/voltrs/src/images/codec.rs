//! Decoding, resizing and encoding for the optimizer and for build-time blur placeholders.

use std::io::Cursor;

use base64::Engine as _;
use image::codecs::avif::AvifEncoder;
use image::codecs::gif::GifDecoder;
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngDecoder;
use image::codecs::webp::{WebPDecoder, WebPEncoder};
use image::imageops::FilterType;
use image::{AnimationDecoder, DynamicImage, ImageDecoder, ImageFormat, ImageReader};

use crate::error::{Error, Result};

/// rav1e speed (1 slowest, 10 fastest): images are encoded on request, so speed matters more than the last
/// few percent of size.
const AVIF_SPEED: u8 = 8;
/// Longer side of build-time blur placeholders, as in Next.js.
const BLUR_SIZE: u32 = 8;

/// Output format chosen from `[images] formats` and the request's `Accept` header.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Target {
    Avif,
    WebP,
    /// The source format: JPEG stays JPEG, everything else becomes PNG.
    Source,
}

impl Target {
    pub(super) fn from_mime(mime: &str) -> Option<Self> {
        match mime {
            "image/avif" => Some(Self::Avif),
            "image/webp" => Some(Self::WebP),
            _ => None,
        }
    }
}

/// Bounds that keep a hostile source from exhausting memory.
#[derive(Debug, Clone, Copy)]
pub(super) struct Limits {
    pub max_pixels: u64,
    pub max_dimension: u32,
}

impl Limits {
    fn reader<'a>(&self, bytes: &'a [u8]) -> Result<ImageReader<Cursor<&'a [u8]>>> {
        let mut reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(self.max_dimension);
        limits.max_image_height = Some(self.max_dimension);
        // Eight bytes per pixel covers 16-bit RGBA, the widest buffer a decoder allocates.
        limits.max_alloc = Some(self.max_pixels.saturating_mul(8).max(64 * 1024 * 1024));
        reader.limits(limits);
        Ok(reader)
    }

    /// Reads only the header, so a decompression bomb is refused before any pixel is allocated.
    fn check(&self, bytes: &[u8]) -> Result<()> {
        let (width, height) = ImageReader::new(Cursor::new(bytes))
            .with_guessed_format()?
            .into_dimensions()
            .map_err(|e| Error::bad_request(format!("cannot read the image: {e}")))?;
        if width > self.max_dimension || height > self.max_dimension {
            return Err(Error::bad_request(format!(
                "the image is {width}x{height}, larger than images.max_dimension ({})",
                self.max_dimension
            )));
        }
        if u64::from(width) * u64::from(height) > self.max_pixels {
            return Err(Error::bad_request(format!(
                "the image has {} pixels, more than images.max_pixels ({})",
                u64::from(width) * u64::from(height),
                self.max_pixels
            )));
        }
        Ok(())
    }

    /// Decodes with the EXIF orientation applied. Callers run [`Limits::check`] first.
    fn decode(&self, bytes: &[u8]) -> Result<DynamicImage> {
        let mut decoder = self.reader(bytes)?.into_decoder().map_err(|e| Error::bad_request(e.to_string()))?;
        let orientation = decoder.orientation().map_err(|e| Error::bad_request(e.to_string()))?;
        let mut image = DynamicImage::from_decoder(decoder).map_err(|e| Error::bad_request(e.to_string()))?;
        image.apply_orientation(orientation);
        Ok(image)
    }
}

/// MIME type of an image recognized by its content, SVG included.
pub(super) fn content_type(bytes: &[u8]) -> Option<&'static str> {
    if is_svg(bytes) {
        return Some("image/svg+xml");
    }
    Some(match image::guess_format(bytes).ok()? {
        ImageFormat::Png => "image/png",
        ImageFormat::Jpeg => "image/jpeg",
        ImageFormat::Gif => "image/gif",
        ImageFormat::WebP => "image/webp",
        ImageFormat::Avif => "image/avif",
        ImageFormat::Ico => "image/x-icon",
        ImageFormat::Bmp => "image/bmp",
        ImageFormat::Tiff => "image/tiff",
        _ => return None,
    })
}

pub(super) fn is_svg(bytes: &[u8]) -> bool {
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(4096)]).to_ascii_lowercase();
    let trimmed = head.trim_start_matches('\u{feff}').trim_start();
    trimmed.starts_with('<') && head.contains("<svg")
}

/// File extension for a MIME type returned by [`content_type`].
pub(super) fn extension(content_type: &str) -> &'static str {
    match content_type {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/avif" => "avif",
        "image/svg+xml" => "svg",
        "image/x-icon" => "ico",
        "image/bmp" => "bmp",
        "image/tiff" => "tiff",
        _ => "bin",
    }
}

/// Animated sources are served unchanged: resizing would keep only the first frame.
fn animated(bytes: &[u8], format: ImageFormat) -> bool {
    match format {
        ImageFormat::Gif => {
            GifDecoder::new(Cursor::new(bytes)).is_ok_and(|decoder| decoder.into_frames().take(2).count() > 1)
        }
        ImageFormat::Png => PngDecoder::new(Cursor::new(bytes)).is_ok_and(|d| d.is_apng().unwrap_or(false)),
        ImageFormat::WebP => WebPDecoder::new(Cursor::new(bytes)).is_ok_and(|d| d.has_animation()),
        _ => false,
    }
}

/// One resize and re-encode of a source image.
#[derive(Debug, Clone, Copy)]
pub(super) struct Transform {
    pub width: u32,
    pub quality: u8,
    pub target: Target,
    pub limits: Limits,
}

impl Transform {
    /// The optimized bytes, or the source itself when it cannot be improved (animations, AVIF and other
    /// formats this build does not decode). Callers handle SVG before this.
    pub(super) fn apply(&self, source: Vec<u8>) -> Result<Vec<u8>> {
        let format = image::guess_format(&source).map_err(|_| Error::bad_request("unsupported image format"))?;
        if !matches!(format, ImageFormat::Jpeg | ImageFormat::Png | ImageFormat::WebP | ImageFormat::Gif) {
            return content_type(&source).map(|_| source).ok_or_else(|| Error::bad_request("unsupported image format"));
        }
        self.limits.check(&source)?;
        if animated(&source, format) {
            return Ok(source);
        }
        let image = self.limits.decode(&source)?;
        let image =
            if image.width() > self.width { image.resize(self.width, u32::MAX, FilterType::Lanczos3) } else { image };
        match self.target {
            Target::Avif => encode_avif(&image, self.quality),
            Target::WebP => {
                // The pure-Rust WebP encoder is lossless: photos can come out larger than a JPEG, so the smaller
                // of the two is kept.
                let webp = encode_webp(&image)?;
                let fallback = encode_source(&image, format, self.quality)?;
                Ok(if webp.len() <= fallback.len() { webp } else { fallback })
            }
            Target::Source => encode_source(&image, format, self.quality),
        }
    }
}

fn encoded(result: image::ImageResult<()>, out: Cursor<Vec<u8>>) -> Result<Vec<u8>> {
    result.map_err(|e| Error::internal(e.to_string()))?;
    Ok(out.into_inner())
}

fn encode_avif(image: &DynamicImage, quality: u8) -> Result<Vec<u8>> {
    let mut out = Cursor::new(Vec::new());
    let encoder = AvifEncoder::new_with_speed_quality(&mut out, AVIF_SPEED, quality);
    let result = if image.color().has_alpha() {
        image.to_rgba8().write_with_encoder(encoder)
    } else {
        image.to_rgb8().write_with_encoder(encoder)
    };
    encoded(result, out)
}

fn encode_webp(image: &DynamicImage) -> Result<Vec<u8>> {
    let mut out = Cursor::new(Vec::new());
    let encoder = WebPEncoder::new_lossless(&mut out);
    let result = if image.color().has_alpha() {
        image.to_rgba8().write_with_encoder(encoder)
    } else {
        image.to_rgb8().write_with_encoder(encoder)
    };
    encoded(result, out)
}

fn encode_source(image: &DynamicImage, format: ImageFormat, quality: u8) -> Result<Vec<u8>> {
    let mut out = Cursor::new(Vec::new());
    let result = match format {
        ImageFormat::Jpeg => image.to_rgb8().write_with_encoder(JpegEncoder::new_with_quality(&mut out, quality)),
        _ => image.write_to(&mut out, ImageFormat::Png),
    };
    encoded(result, out)
}

/// A tiny preview of an image for `placeholder="blur"`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlurPlaceholder {
    /// `data:image/png;base64,...`
    pub data_url: String,
    pub width: u32,
    pub height: u32,
}

/// Scales a raster image down to at most 8 pixels on its longer side and encodes it as a PNG data URL.
/// Returns `None` for formats this build cannot decode (SVG, AVIF, fonts, ...).
pub fn blur_placeholder(bytes: &[u8]) -> Option<BlurPlaceholder> {
    let format = image::guess_format(bytes).ok()?;
    if !matches!(format, ImageFormat::Jpeg | ImageFormat::Png | ImageFormat::WebP | ImageFormat::Gif) {
        return None;
    }
    let limits = Limits { max_pixels: 100_000_000, max_dimension: 20_000 };
    limits.check(bytes).ok()?;
    let image = limits.decode(bytes).ok()?.thumbnail(BLUR_SIZE, BLUR_SIZE);
    let mut out = Cursor::new(Vec::new());
    image.write_to(&mut out, ImageFormat::Png).ok()?;
    let data = base64::engine::general_purpose::STANDARD.encode(out.into_inner());
    Some(BlurPlaceholder {
        data_url: format!("data:image/png;base64,{data}"),
        width: image.width(),
        height: image.height(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage, Rgba, RgbaImage};

    const LIMITS: Limits = Limits { max_pixels: 10_000_000, max_dimension: 8192 };

    fn png(width: u32, height: u32) -> Vec<u8> {
        let image = RgbaImage::from_fn(width, height, |x, y| Rgba([(x % 256) as u8, (y % 256) as u8, 90, 255]));
        let mut out = Cursor::new(Vec::new());
        image.write_to(&mut out, ImageFormat::Png).unwrap();
        out.into_inner()
    }

    fn jpeg(width: u32, height: u32) -> Vec<u8> {
        let image = RgbImage::from_fn(width, height, |x, y| Rgb([(x * 7 % 256) as u8, (y * 3 % 256) as u8, 128]));
        let mut out = Cursor::new(Vec::new());
        image.write_with_encoder(JpegEncoder::new_with_quality(&mut out, 80)).unwrap();
        out.into_inner()
    }

    fn transform(width: u32, target: Target) -> Transform {
        Transform { width, quality: 75, target, limits: LIMITS }
    }

    fn size(bytes: &[u8]) -> (u32, u32) {
        ImageReader::new(Cursor::new(bytes)).with_guessed_format().unwrap().into_dimensions().unwrap()
    }

    #[test]
    fn resizes_and_converts() {
        let webp = transform(64, Target::WebP).apply(png(200, 100)).unwrap();
        assert_eq!(content_type(&webp), Some("image/webp"));
        assert_eq!(size(&webp), (64, 32));
        let avif = transform(64, Target::Avif).apply(png(200, 100)).unwrap();
        assert_eq!(content_type(&avif), Some("image/avif"));
        let same = transform(640, Target::Source).apply(jpeg(100, 50)).unwrap();
        assert_eq!(content_type(&same), Some("image/jpeg"));
        assert_eq!(size(&same), (100, 50));
    }

    #[test]
    fn refuses_oversized_sources() {
        let small = Limits { max_pixels: 1000, max_dimension: 8192 };
        let error = Transform { limits: small, ..transform(64, Target::Source) }.apply(png(100, 100)).unwrap_err();
        assert_eq!(error.status(), http::StatusCode::BAD_REQUEST);
        let narrow = Limits { max_pixels: 1_000_000, max_dimension: 50 };
        assert!(Transform { limits: narrow, ..transform(64, Target::Source) }.apply(png(100, 10)).is_err());
    }

    #[test]
    fn passes_through_what_it_cannot_improve() {
        let svg = b"<?xml version=\"1.0\"?><svg xmlns=\"http://www.w3.org/2000/svg\"/>".to_vec();
        assert!(is_svg(&svg));
        assert_eq!(content_type(&svg), Some("image/svg+xml"));
        assert!(transform(64, Target::WebP).apply(b"not an image".to_vec()).is_err());
    }

    #[test]
    fn builds_blur_placeholders() {
        let blur = blur_placeholder(&png(400, 200)).unwrap();
        assert_eq!((blur.width, blur.height), (8, 4));
        assert!(blur.data_url.starts_with("data:image/png;base64,"));
        assert!(blur_placeholder(b"<svg/>").is_none());
    }
}
