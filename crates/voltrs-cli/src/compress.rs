//! Precompresses static assets so the server can send them without CPU work.

use std::io::Write;
use std::path::Path;

use anyhow::Result;

const MIN_SIZE: u64 = 1024;
const EXTENSIONS: [&str; 6] = ["js", "css", "html", "svg", "json", "map"];

pub fn precompress(dir: &Path) -> Result<usize> {
    let mut count = 0;
    let Ok(entries) = std::fs::read_dir(dir) else { return Ok(0) };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            count += precompress(&path)?;
            continue;
        }
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or_default();
        if !EXTENSIONS.contains(&ext) || entry.metadata().map(|m| m.len()).unwrap_or(0) < MIN_SIZE {
            continue;
        }
        let bytes = std::fs::read(&path)?;
        let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
        gzip.write_all(&bytes)?;
        std::fs::write(with_suffix(&path, "gz"), gzip.finish()?)?;
        let mut brotli = Vec::new();
        let params = brotli::enc::BrotliEncoderParams { quality: 11, lgwin: 22, ..Default::default() };
        brotli::BrotliCompress(&mut bytes.as_slice(), &mut brotli, &params)?;
        std::fs::write(with_suffix(&path, "br"), brotli)?;
        count += 1;
    }
    Ok(count)
}

fn with_suffix(path: &Path, suffix: &str) -> std::path::PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".");
    name.push(suffix);
    name.into()
}
