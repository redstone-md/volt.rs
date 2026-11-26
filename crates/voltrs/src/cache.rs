//! In-memory cache for prerendered and revalidated pages.

use std::collections::HashMap;
use std::path::Path;
use std::sync::RwLock;
use std::time::{Duration, Instant};

use http::{HeaderValue, header};

use crate::manifest::RouteEntry;
use crate::response::Response;

struct Entry {
    response: Response,
    expires: Option<Instant>,
}

#[derive(Default)]
pub struct PageCache {
    entries: RwLock<HashMap<String, Entry>>,
}

impl PageCache {
    pub fn get(&self, key: &str) -> Option<Response> {
        let entries = self.entries.read().ok()?;
        let entry = entries.get(key)?;
        match entry.expires {
            Some(expires) if Instant::now() >= expires => None,
            _ => Some(entry.response.clone()),
        }
    }

    pub fn put(&self, key: String, response: &Response, ttl: Option<Duration>) {
        if let Ok(mut entries) = self.entries.write() {
            entries.insert(key, Entry { response: response.clone(), expires: ttl.map(|ttl| Instant::now() + ttl) });
        }
    }

    /// Loads HTML that `volt build` prerendered for static routes.
    pub fn preload(&self, routes: &[RouteEntry], out_dir: &Path) {
        for route in routes {
            let Some(file) = &route.prerendered else {
                continue;
            };
            let Ok(html) = std::fs::read_to_string(out_dir.join(file)) else {
                continue;
            };
            let response = Response::html(html).header(
                header::CACHE_CONTROL,
                HeaderValue::from_static("public, max-age=0, s-maxage=31536000, stale-while-revalidate"),
            );
            self.put(format!("{}?", route.id), &response, route.revalidate.map(Duration::from_secs));
        }
    }
}
