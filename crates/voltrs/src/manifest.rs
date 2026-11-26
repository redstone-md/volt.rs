//! Build manifest written by `volt build` and read by the server.

use std::path::Path;

use serde::{Deserialize, Serialize};

pub const MANIFEST_FILE: &str = "manifest.json";
pub const STATIC_PREFIX: &str = "/_volt/static/";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub build_id: String,
    pub dev: bool,
    /// Server bundle path relative to the out dir.
    pub server: String,
    pub client: ClientAssets,
    pub routes: Vec<RouteEntry>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientAssets {
    pub entry: String,
    pub css: Vec<String>,
    pub preload: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteEntry {
    /// Route pattern, e.g. `/blog/[slug]`.
    pub id: String,
    pub chunks: Vec<String>,
    /// Prerendered HTML path relative to the out dir.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prerendered: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revalidate: Option<u64>,
    #[serde(default)]
    pub dynamic: bool,
}

impl Manifest {
    pub fn load(out_dir: &Path) -> std::io::Result<Self> {
        let text = std::fs::read_to_string(out_dir.join(MANIFEST_FILE))?;
        serde_json::from_str(&text).map_err(std::io::Error::other)
    }

    pub fn save(&self, out_dir: &Path) -> std::io::Result<()> {
        let text = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(out_dir.join(MANIFEST_FILE), text)
    }

    pub fn route(&self, id: &str) -> Option<&RouteEntry> {
        self.routes.iter().find(|r| r.id == id)
    }
}
