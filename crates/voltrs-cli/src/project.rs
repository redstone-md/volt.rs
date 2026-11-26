use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use voltrs::config::{CONFIG_FILE, Config};

pub const APP_DIRS: [&str; 2] = ["app", "src/app"];

/// A volt project on disk.
pub struct Project {
    pub root: PathBuf,
    pub config: Config,
    pub app_dir: PathBuf,
    pub out_dir: PathBuf,
    pub rust: bool,
}

impl Project {
    /// Finds the project containing `start` by walking up to a `volt.toml` or `package.json`.
    pub fn discover(start: &Path) -> Result<Self> {
        let start = start.canonicalize().with_context(|| format!("cannot open {}", start.display()))?;
        let root = start
            .ancestors()
            .find(|dir| {
                dir.join(CONFIG_FILE).is_file() || (dir.join("package.json").is_file() && app_dir(dir).is_some())
            })
            .map(Path::to_path_buf)
            .with_context(|| format!("no volt project found in {} or its parents", start.display()))?;
        let Some(app_dir) = app_dir(&root) else {
            bail!("{} has no `app/` directory", root.display());
        };
        crate::env::load(&root);
        let config = Config::load(&root)?;
        let out_dir = config.out_dir();
        let rust = std::fs::read_to_string(root.join("Cargo.toml")).is_ok_and(|manifest| manifest.contains("voltrs"));
        Ok(Self { root, config, app_dir, out_dir, rust })
    }

    pub fn static_dir(&self) -> PathBuf {
        self.out_dir.join("static")
    }

    pub fn server_dir(&self) -> PathBuf {
        self.out_dir.join("server")
    }

    pub fn types_dir(&self) -> PathBuf {
        self.out_dir.join("types")
    }

    pub fn cache_dir(&self) -> PathBuf {
        self.out_dir.join("cache")
    }

    pub fn sdk_file(&self) -> PathBuf {
        self.types_dir().join(voltrs::sdk::SDK_FILE)
    }

    pub fn node_modules(&self) -> PathBuf {
        self.root.join("node_modules")
    }

    pub fn relative<'a>(&self, path: &'a Path) -> std::borrow::Cow<'a, str> {
        path.strip_prefix(&self.root).unwrap_or(path).to_string_lossy()
    }
}

fn app_dir(root: &Path) -> Option<PathBuf> {
    APP_DIRS.iter().map(|dir| root.join(dir)).find(|dir| dir.is_dir())
}
