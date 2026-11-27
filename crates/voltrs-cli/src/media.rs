//! Content-hashed copies of imported files under `static/media`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result};

use crate::util::hash;

static WRITES: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone)]
pub struct Media {
    dir: PathBuf,
    /// Public URL prefix of the static dir, including the base path.
    prefix: String,
}

impl Media {
    pub fn new(dir: PathBuf, prefix: impl Into<String>) -> Self {
        Self { dir, prefix: prefix.into() }
    }

    /// Copies a file and returns its public URL.
    pub fn copy(&self, path: &Path) -> Result<String> {
        let bytes = std::fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "asset".into());
        let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        self.write(&stem, &ext, &bytes)
    }

    /// Writes bytes as `<stem>-<hash>.<ext>` and returns the public URL.
    pub fn write(&self, stem: &str, ext: &str, bytes: &[u8]) -> Result<String> {
        let name =
            if ext.is_empty() { format!("{stem}-{}", hash(bytes)) } else { format!("{stem}-{}.{ext}", hash(bytes)) };
        let target = self.dir.join(&name);
        if !target.exists() {
            std::fs::create_dir_all(&self.dir)?;
            // Both bundles may emit the same file concurrently, so each writes its own temp file first.
            let partial =
                self.dir.join(format!(".{name}.{}.{}", std::process::id(), WRITES.fetch_add(1, Ordering::Relaxed)));
            std::fs::write(&partial, bytes)?;
            std::fs::rename(&partial, &target)?;
        }
        Ok(format!("{}media/{name}", self.prefix))
    }
}
