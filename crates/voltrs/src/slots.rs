//! Parallel route resolution: which page of each `@slot` renders for a URL.

use crate::manifest::{SlotEntry, SlotRoute};
use crate::router::{Params, RoutePattern, Router};

/// The slots of one build, matched with [`Router`] like pages.
#[derive(Default)]
pub(crate) struct Slots {
    tables: Vec<Table>,
}

struct Table {
    entry: SlotEntry,
    routes: Router<usize>,
    intercepts: Router<usize>,
}

/// What one slot renders: a page with its params, or nothing when no page matches.
pub(crate) struct SlotMatch<'a> {
    pub key: &'a str,
    /// Directory key of the layout that receives the slot.
    pub segment: &'a str,
    pub route: Option<(&'a SlotRoute, Params)>,
    pub default: bool,
}

impl Slots {
    pub fn new(entries: &[SlotEntry]) -> Self {
        let tables = entries
            .iter()
            .map(|entry| {
                let pick = |intercept: bool| {
                    entry
                        .routes
                        .iter()
                        .enumerate()
                        .filter(|(_, route)| route.intercept == intercept)
                        .map(|(i, route)| (route.pattern.clone(), i))
                        .collect()
                };
                Table { routes: pick(false), intercepts: pick(true), entry: entry.clone() }
            })
            .collect();
        Self { tables }
    }

    /// Resolves the slots of the layouts in `dirs` and, recursively, of the slot pages that match.
    /// Intercepting slot pages only take part when `intercept` is set (a client navigation from inside `dirs`).
    pub fn resolve(&self, dirs: &[String], path: &str, intercept: bool) -> Vec<SlotMatch<'_>> {
        let mut pending: Vec<&str> = dirs.iter().map(String::as_str).collect();
        let mut out = Vec::new();
        let mut index = 0;
        while index < pending.len() {
            let dir = pending[index];
            index += 1;
            for table in self.tables.iter().filter(|t| t.entry.segment == dir) {
                let found = intercept
                    .then(|| table.intercepts.find(path))
                    .flatten()
                    .or_else(|| table.routes.find(path))
                    .map(|(_, i, params)| (&table.entry.routes[*i], params));
                if let Some((route, _)) = &found {
                    pending.extend(route.segments.iter().map(String::as_str));
                }
                out.push(SlotMatch {
                    key: &table.entry.key,
                    segment: &table.entry.segment,
                    route: found,
                    default: table.entry.default,
                });
            }
        }
        out
    }

    /// Whether `path` exists in a slot of the layouts in `dirs`. Catch-all slot pages, the usual way to clear a
    /// slot on other URLs, render for URLs that exist but do not make every URL exist.
    pub fn matches(&self, dirs: &[String], path: &str) -> bool {
        self.resolve(dirs, path, false)
            .iter()
            .filter_map(|m| m.route.as_ref())
            .any(|(route, _)| !RoutePattern::parse(&route.pattern).is_catch_all())
    }

    /// Whether an intercepting slot page of the layouts in `dirs` matches `path`.
    pub fn intercepts(&self, dirs: &[String], path: &str) -> bool {
        self.resolve(dirs, path, true).iter().any(|m| m.route.as_ref().is_some_and(|(route, _)| route.intercept))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route(id: &str, pattern: &str, intercept: bool) -> SlotRoute {
        SlotRoute {
            id: id.into(),
            pattern: pattern.into(),
            intercept,
            segments: vec![id.into()],
            ..SlotRoute::default()
        }
    }

    #[test]
    fn resolves_slots_and_interceptions() {
        let slots = Slots::new(&[
            SlotEntry {
                key: "/@modal".into(),
                segment: "/".into(),
                default: true,
                routes: vec![
                    route("/@modal/(.)photos/[id]", "/photos/[id]", true),
                    route("/@modal/[...rest]", "/[...rest]", false),
                ],
            },
            SlotEntry {
                key: "/dashboard/@team".into(),
                segment: "/dashboard".into(),
                default: false,
                routes: vec![route("/dashboard/@team", "/dashboard", false)],
            },
        ]);
        let root = vec!["/".to_owned()];
        let hard = slots.resolve(&root, "/photos/1", false);
        assert_eq!(hard.len(), 1);
        assert_eq!(hard[0].route.as_ref().unwrap().0.id, "/@modal/[...rest]");
        let soft = slots.resolve(&root, "/photos/1", true);
        let (route, params) = soft[0].route.as_ref().unwrap();
        assert_eq!(route.id, "/@modal/(.)photos/[id]");
        assert_eq!(params.get("id"), Some("1"));
        assert!(slots.intercepts(&root, "/photos/1"));
        assert!(!slots.intercepts(&root, "/photos"));
        assert!(!slots.matches(&root, "/photos/1"));
        assert!(slots.matches(&["/dashboard".to_owned()], "/dashboard"));
        let dashboard = vec!["/".to_owned(), "/dashboard".to_owned()];
        let matched = slots.resolve(&dashboard, "/dashboard", false);
        assert_eq!((matched[1].key, matched[1].segment), ("/dashboard/@team", "/dashboard"));
        assert!(matched[1].route.is_some());
        assert!(slots.resolve(&dashboard, "/dashboard/settings", false)[1].route.is_none());
    }
}
