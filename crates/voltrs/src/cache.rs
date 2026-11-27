//! In-memory cache for prerendered and revalidated pages, with stale-while-revalidate,
//! on-demand invalidation and a byte budget for pages rendered on demand.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock, Weak};
use std::time::{Duration, Instant};

use http::{HeaderValue, header};
use serde_json::Value;

use crate::config::Config;
use crate::manifest::{RouteEntry, prerender_file, same_path};
use crate::response::Response;
use crate::router::RoutePattern;

/// Caches of the running servers, reached by [`revalidate_path`] and [`revalidate_prefix`].
static CACHES: Mutex<Vec<Weak<PageCache>>> = Mutex::new(Vec::new());

/// A cached page and the Pages Router props it was rendered with.
#[derive(Clone)]
pub struct CachedPage {
    pub response: Response,
    pub data: Option<Value>,
}

struct Entry {
    page: CachedPage,
    expires: Option<Instant>,
    size: usize,
    used: AtomicU64,
}

#[derive(Default)]
struct Entries {
    map: HashMap<String, Entry>,
    /// Keys of build-time paths: never evicted and not counted against the budget.
    pinned: HashSet<String>,
    /// Bytes held by entries that are not pinned.
    bytes: usize,
}

impl Entries {
    fn remove(&mut self, key: &str) {
        if let Some(entry) = self.map.remove(key)
            && !self.pinned.contains(key)
        {
            self.bytes -= entry.size;
        }
    }

    fn insert(&mut self, key: String, entry: Entry, capacity: usize) {
        self.remove(&key);
        if !self.pinned.contains(&key) {
            if entry.size > capacity {
                return;
            }
            self.bytes += entry.size;
        }
        self.map.insert(key, entry);
        while self.bytes > capacity && self.evict_one() {}
    }

    fn evict_one(&mut self) -> bool {
        let oldest = self
            .map
            .iter()
            .filter(|(key, _)| !self.pinned.contains(*key))
            .min_by_key(|(_, entry)| entry.used.load(Ordering::Relaxed))
            .map(|(key, _)| key.clone());
        match oldest {
            Some(key) => {
                self.remove(&key);
                true
            }
            None => false,
        }
    }
}

/// Result of a cache lookup.
pub enum Lookup {
    Fresh(CachedPage),
    /// Past its `revalidate` time. `refresh` is true for exactly one caller until the entry is replaced.
    Stale {
        page: CachedPage,
        refresh: bool,
    },
    Miss,
}

/// Taken before rendering, so a render that started before an invalidation is not stored.
#[derive(Clone, Copy)]
pub struct Ticket(u64);

/// Pages keyed by path and query. Build-time paths stay resident; pages rendered on demand share
/// a byte budget and the least recently used ones are evicted first.
pub struct PageCache {
    entries: RwLock<Entries>,
    refreshing: Mutex<HashSet<String>>,
    capacity: usize,
    clock: AtomicU64,
    epoch: AtomicU64,
}

/// Cache key of a request: route path (without its locale prefix), raw query string and locale.
/// Keeping the locale out of the path lets revalidation by path purge every locale variant.
pub fn key(path: &str, query: &str, locale: &str) -> String {
    format!("{path}?{query}#{locale}")
}

fn key_path(key: &str) -> &str {
    key.split_once('?').map_or(key, |(path, _)| path)
}

/// `Cache-Control` of a cached page: long-lived when static, `s-maxage` when revalidated every `revalidate` seconds.
pub fn cache_control(revalidate: Option<u64>) -> HeaderValue {
    match revalidate {
        Some(seconds) => {
            HeaderValue::from_str(&format!("public, max-age=0, s-maxage={seconds}, stale-while-revalidate"))
                .unwrap_or(HeaderValue::from_static("public, max-age=0, must-revalidate"))
        }
        None => HeaderValue::from_static("public, max-age=0, s-maxage=31536000, stale-while-revalidate"),
    }
}

impl PageCache {
    /// A cache holding at most `capacity` bytes of pages rendered on demand.
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: RwLock::default(),
            refreshing: Mutex::default(),
            capacity,
            clock: AtomicU64::new(0),
            epoch: AtomicU64::new(0),
        }
    }

    /// Creates a cache reachable from the global revalidation functions.
    pub fn shared(capacity: usize) -> Arc<Self> {
        let cache = Arc::new(Self::new(capacity));
        if let Ok(mut caches) = CACHES.lock() {
            caches.retain(|c| c.strong_count() > 0);
            caches.push(Arc::downgrade(&cache));
        }
        cache
    }

    pub fn get(&self, key: &str) -> Lookup {
        let Ok(entries) = self.entries.read() else {
            return Lookup::Miss;
        };
        let Some(entry) = entries.map.get(key) else {
            return Lookup::Miss;
        };
        entry.used.store(self.tick(), Ordering::Relaxed);
        match entry.expires {
            Some(expires) if Instant::now() >= expires => {
                let refresh = self.refreshing.lock().is_ok_and(|mut set| set.insert(key.to_owned()));
                Lookup::Stale { page: entry.page.clone(), refresh }
            }
            _ => Lookup::Fresh(entry.page.clone()),
        }
    }

    pub fn ticket(&self) -> Ticket {
        Ticket(self.epoch.load(Ordering::Acquire))
    }

    /// Stores a rendered page unless the cache was invalidated after `ticket` was taken.
    pub fn put(&self, key: String, page: &CachedPage, ttl: Option<Duration>, ticket: Ticket) {
        self.release(&key);
        let Ok(mut entries) = self.entries.write() else {
            return;
        };
        if self.epoch.load(Ordering::Acquire) == ticket.0 {
            let entry = self.entry(&key, page, ttl);
            entries.insert(key, entry, self.capacity);
        }
    }

    pub fn remove(&self, key: &str) {
        self.release(key);
        if let Ok(mut entries) = self.entries.write() {
            entries.remove(key);
        }
    }

    /// Lets the next stale lookup trigger a refresh again, e.g. after a failed regeneration.
    pub fn release(&self, key: &str) {
        if let Ok(mut set) = self.refreshing.lock() {
            set.remove(key);
        }
    }

    /// Drops every entry whose path satisfies `matches`; the next request renders it again.
    pub fn invalidate(&self, matches: impl Fn(&str) -> bool) -> usize {
        let Ok(mut entries) = self.entries.write() else {
            return 0;
        };
        self.epoch.fetch_add(1, Ordering::AcqRel);
        let keys: Vec<String> = entries.map.keys().filter(|key| matches(key_path(key))).cloned().collect();
        for key in &keys {
            entries.remove(key);
        }
        keys.len()
    }

    /// Loads the HTML (and Pages Router props) that `volt build` prerendered and pins it.
    pub fn preload(&self, routes: &[RouteEntry], out_dir: &Path, config: &Config) {
        let Ok(mut entries) = self.entries.write() else {
            return;
        };
        for route in routes {
            for path in &route.prerendered {
                let Some(file) = prerender_file(path, "html") else {
                    continue;
                };
                let Ok(html) = std::fs::read_to_string(out_dir.join(file)) else {
                    continue;
                };
                let response = Response::html(html).header(header::CACHE_CONTROL, cache_control(route.revalidate));
                let data = route.static_props.then(|| Self::prerendered_props(out_dir, path)).flatten();
                let (locale, route_path) = config.split_locale(path);
                let key = key(route_path, "", locale);
                let entry = self.entry(&key, &CachedPage { response, data }, route.revalidate.map(Duration::from_secs));
                entries.remove(&key);
                entries.pinned.insert(key.clone());
                entries.insert(key, entry, self.capacity);
            }
        }
    }

    /// Props stored in the route data file written next to a prerendered page.
    fn prerendered_props(out_dir: &Path, path: &str) -> Option<Value> {
        let bytes = std::fs::read(out_dir.join(prerender_file(path, "json")?)).ok()?;
        let mut state: Value = serde_json::from_slice(&bytes).ok()?;
        Some(state.get_mut("data")?.take())
    }

    fn entry(&self, key: &str, page: &CachedPage, ttl: Option<Duration>) -> Entry {
        let props = page.data.as_ref().map_or(0, |data| data.to_string().len());
        Entry {
            page: page.clone(),
            expires: ttl.map(|ttl| Instant::now() + ttl),
            size: key.len() + page.response.body.len() + props,
            used: AtomicU64::new(self.tick()),
        }
    }

    fn tick(&self) -> u64 {
        self.clock.fetch_add(1, Ordering::Relaxed)
    }
}

/// What a revalidation call targets.
pub(crate) enum Target {
    /// One path, or every path of a route pattern such as `/blog/[slug]`.
    Path(String),
    /// A path and everything below it.
    Prefix(String),
}

impl Target {
    fn matches(&self, path: &str) -> bool {
        match self {
            Self::Path(target) if target.contains('[') => RoutePattern::parse(target).matches(path).is_some(),
            Self::Path(target) => same_path(target, path),
            Self::Prefix(prefix) => {
                let prefix = prefix.trim_end_matches('/');
                prefix.is_empty() || same_path(prefix, path) || path.starts_with(&format!("{prefix}/"))
            }
        }
    }

    pub(crate) fn apply(&self, cache: &PageCache) -> usize {
        cache.invalidate(|path| self.matches(path))
    }

    fn apply_all(&self) -> usize {
        let caches: Vec<Arc<PageCache>> =
            CACHES.lock().map(|c| c.iter().filter_map(Weak::upgrade).collect()).unwrap_or_default();
        caches.iter().map(|cache| self.apply(cache)).sum()
    }
}

/// Purges the cached page at `path` (an app path such as `/blog/hello`, without the base path)
/// so the next request renders it again. A route pattern like `/blog/[slug]` purges all its pages.
/// Returns the number of purged entries.
pub fn revalidate_path(path: &str) -> usize {
    Target::Path(path.to_owned()).apply_all()
}

/// Purges every cached page at or below `prefix`, e.g. everything under a layout.
pub fn revalidate_prefix(prefix: &str) -> usize {
    Target::Prefix(prefix.to_owned()).apply_all()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(body: &str) -> CachedPage {
        CachedPage { response: Response::html(body.to_owned()), data: None }
    }

    fn put(cache: &PageCache, path: &str, query: &str, body: &str, ttl: Option<Duration>) {
        cache.put(key(path, query, "en"), &page(body), ttl, cache.ticket());
    }

    fn cached(cache: &PageCache, key: &str) -> bool {
        matches!(cache.get(key), Lookup::Fresh(_))
    }

    #[test]
    fn serves_stale_once_per_refresh() {
        let cache = PageCache::new(1 << 20);
        put(&cache, "/a", "", "a", Some(Duration::ZERO));
        assert!(matches!(cache.get("/a?#en"), Lookup::Stale { refresh: true, .. }));
        assert!(matches!(cache.get("/a?#en"), Lookup::Stale { refresh: false, .. }));
        put(&cache, "/a", "", "b", Some(Duration::from_secs(60)));
        assert!(cached(&cache, "/a?#en"));
        assert!(matches!(cache.get("/b?#en"), Lookup::Miss));
    }

    #[test]
    fn invalidates_paths_patterns_and_prefixes() {
        let cache = PageCache::new(1 << 20);
        for path in ["/", "/blog", "/blog/a", "/blog/b", "/blogger"] {
            put(&cache, path, "", path, None);
        }
        put(&cache, "/blog/a", "x=1", "q", None);
        assert_eq!(Target::Path("/blog/a/".into()).apply(&cache), 2);
        assert_eq!(Target::Path("/blog/[slug]".into()).apply(&cache), 1);
        assert_eq!(Target::Prefix("/blog".into()).apply(&cache), 1);
        assert!(cached(&cache, "/blogger?#en"));
        assert_eq!(Target::Prefix("/".into()).apply(&cache), 2);
    }

    #[test]
    fn revalidating_a_path_purges_every_locale() {
        let cache = PageCache::new(1 << 20);
        for locale in ["en", "de"] {
            cache.put(key("/about", "", locale), &page(locale), None, cache.ticket());
        }
        assert!(cached(&cache, "/about?#de"));
        assert_eq!(Target::Path("/about".into()).apply(&cache), 2);
    }

    #[test]
    fn evicts_least_recently_used_within_budget() {
        let body = "x".repeat(100);
        let cache = PageCache::new(350);
        for path in ["/a", "/b", "/c"] {
            put(&cache, path, "", &body, None);
        }
        assert!(cached(&cache, "/a?#en"));
        put(&cache, "/d", "", &body, None);
        assert!(matches!(cache.get("/b?#en"), Lookup::Miss));
        for kept in ["/a?#en", "/c?#en", "/d?#en"] {
            assert!(cached(&cache, kept), "{kept}");
        }
        put(&cache, "/huge", "", &"x".repeat(400), None);
        assert!(matches!(cache.get("/huge?#en"), Lookup::Miss));
        assert!(cache.entries.read().unwrap().bytes <= 350);
    }

    #[test]
    fn keeps_pinned_paths_and_drops_renders_older_than_a_purge() {
        let cache = PageCache::new(0);
        cache.entries.write().unwrap().pinned.insert(key("/a", "", "en"));
        put(&cache, "/a", "", "a", None);
        put(&cache, "/b", "", "b", None);
        assert!(cached(&cache, "/a?#en"));
        assert!(matches!(cache.get("/b?#en"), Lookup::Miss));

        let ticket = cache.ticket();
        assert_eq!(Target::Path("/a".into()).apply(&cache), 1);
        cache.put(key("/a", "", "en"), &page("old"), None, ticket);
        assert!(matches!(cache.get("/a?#en"), Lookup::Miss));
        put(&cache, "/a", "", "new", None);
        assert!(cached(&cache, "/a?#en"));
    }
}
