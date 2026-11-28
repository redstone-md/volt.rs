use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use voltrs::config::{CONFIG_FILE, Config};

pub const APP_DIRS: [&str; 2] = ["app", "src/app"];
pub const PAGES_DIRS: [&str; 2] = ["pages", "src/pages"];

/// A volt project on disk.
pub struct Project {
    pub root: PathBuf,
    pub config: Config,
    /// App Router directory, when present.
    pub app_dir: Option<PathBuf>,
    /// Pages Router directory, when present.
    pub pages_dir: Option<PathBuf>,
    pub out_dir: PathBuf,
    pub rust: bool,
}

impl Project {
    /// Finds the project containing `start` by walking up to a `volt.toml` or `package.json`.
    pub fn discover(start: &Path) -> Result<Self> {
        let start = crate::util::canonical(start).with_context(|| format!("cannot open {}", start.display()))?;
        let root = start
            .ancestors()
            .find(|dir| {
                dir.join(CONFIG_FILE).is_file()
                    || (dir.join("package.json").is_file()
                        && (first_dir(dir, &APP_DIRS).is_some() || first_dir(dir, &PAGES_DIRS).is_some()))
            })
            .map(Path::to_path_buf)
            .with_context(|| format!("no volt project found in {} or its parents", start.display()))?;
        let app_dir = first_dir(&root, &APP_DIRS);
        let pages_dir = first_dir(&root, &PAGES_DIRS);
        if app_dir.is_none() && pages_dir.is_none() {
            bail!("{} has no `app/` or `pages/` directory", root.display());
        }
        crate::env::load(&root);
        let config = Config::load(&root)?;
        let out_dir = config.out_dir();
        let rust = std::fs::read_to_string(root.join("Cargo.toml")).is_ok_and(|manifest| manifest.contains("voltrs"));
        Ok(Self { root, config, app_dir, pages_dir, out_dir, rust })
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

    /// Scans both routers' directories.
    pub fn routes(&self) -> Result<Vec<crate::routes::Route>> {
        crate::routes::scan(self.app_dir.as_deref(), self.pages_dir.as_deref())
    }

    pub fn relative<'a>(&self, path: &'a Path) -> std::borrow::Cow<'a, str> {
        path.strip_prefix(&self.root).unwrap_or(path).to_string_lossy()
    }
}

fn first_dir(root: &Path, candidates: &[&str]) -> Option<PathBuf> {
    candidates.iter().map(|dir| root.join(dir)).find(|dir| dir.is_dir())
}
