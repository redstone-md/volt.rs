//! In-memory cache for prerendered and revalidated pages.

use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::RwLock;
use std::time::{Duration, Instant};

use http::{HeaderValue, header};
use serde_json::Value;

use crate::manifest::{RouteEntry, props_file};
use crate::response::Response;

/// Cache-Control of pages that are the same for every visitor.
pub const PUBLIC_PAGE: &str = "public, max-age=0, s-maxage=31536000, stale-while-revalidate";

/// A cached page and the Pages Router props it was rendered with.
#[derive(Clone)]
pub struct CachedPage {
    pub response: Response,
    pub data: Option<Value>,
}

struct Entry {
    page: CachedPage,
    expires: Option<Instant>,
}

#[derive(Default)]
struct Entries {
    map: HashMap<String, Entry>,
    /// Insertion order of the evictable keys. Prerendered pages are bounded by the build and never evicted.
    order: VecDeque<String>,
}

/// Page cache holding prerendered pages plus at most `capacity` pages rendered at runtime.
pub struct PageCache {
    entries: RwLock<Entries>,
    capacity: usize,
}

impl PageCache {
    pub fn new(capacity: usize) -> Self {
        Self { entries: RwLock::new(Entries::default()), capacity }
    }

    pub fn get(&self, key: &str) -> Option<CachedPage> {
        let entries = self.entries.read().ok()?;
        let entry = entries.map.get(key)?;
        match entry.expires {
            Some(expires) if Instant::now() >= expires => None,
            _ => Some(entry.page.clone()),
        }
    }

    pub fn put(&self, key: String, page: CachedPage, ttl: Option<Duration>) {
        self.insert(key, page, ttl, false);
    }

    fn insert(&self, key: String, page: CachedPage, ttl: Option<Duration>, pin: bool) {
        let Ok(mut entries) = self.entries.write() else {
            return;
        };
        let expires = ttl.map(|ttl| Instant::now() + ttl);
        if let Some(entry) = entries.map.get_mut(&key) {
            entry.page = page;
            entry.expires = expires;
            return;
        }
        if !pin {
            if self.capacity == 0 {
                return;
            }
            while entries.order.len() >= self.capacity {
                if let Some(oldest) = entries.order.pop_front() {
                    entries.map.remove(&oldest);
                }
            }
            entries.order.push_back(key.clone());
        }
        entries.map.insert(key, Entry { page, expires });
    }

    /// Loads HTML and props that `volt build` prerendered for static routes.
    pub fn preload(&self, routes: &[RouteEntry], out_dir: &Path) {
        for route in routes {
            for (path, file) in route.prerendered_files() {
                let Ok(html) = std::fs::read_to_string(out_dir.join(file)) else {
                    continue;
                };
                let data = std::fs::read(out_dir.join(props_file(file)))
                    .ok()
                    .and_then(|bytes| serde_json::from_slice(&bytes).ok());
                let response =
                    Response::html(html).header(header::CACHE_CONTROL, HeaderValue::from_static(PUBLIC_PAGE));
                let ttl = route.revalidate.map(Duration::from_secs);
                self.insert(format!("{path}?"), CachedPage { response, data }, ttl, true);
            }
        }
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries.read().map(|e| e.map.len()).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(body: &str) -> CachedPage {
        CachedPage { response: Response::html(body.to_owned()), data: None }
    }

    #[test]
    fn evicts_oldest_runtime_pages_beyond_capacity() {
        let cache = PageCache::new(2);
        cache.insert("/pinned?".into(), page("pinned"), None, true);
        for key in ["/a?", "/b?", "/c?"] {
            cache.put(key.into(), page(key), None);
        }
        assert!(cache.get("/a?").is_none());
        assert!(cache.get("/b?").is_some() && cache.get("/c?").is_some());
        assert!(cache.get("/pinned?").is_some());
        assert_eq!(cache.len(), 3);
    }

    #[test]
    fn replacing_a_key_keeps_its_slot() {
        let cache = PageCache::new(2);
        cache.put("/a?".into(), page("1"), None);
        cache.put("/a?".into(), page("2"), None);
        cache.put("/b?".into(), page("b"), None);
        assert_eq!(cache.get("/a?").map(|p| p.response.body), Some("2".into()));
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn zero_capacity_keeps_only_prerendered_pages() {
        let cache = PageCache::new(0);
        cache.put("/a?".into(), page("a"), None);
        assert!(cache.get("/a?").is_none());
    }
}
