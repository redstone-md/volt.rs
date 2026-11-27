//! Decides which `volt dev` rebuilds the browser can apply in place with React Fast Refresh.

use std::collections::{BTreeMap, HashMap, VecDeque};

use voltrs::manifest::{HotModule, HotUpdate};

use crate::bundler::ClientModule;

/// The outcome of a rebuild for the browsers showing the previous one.
#[derive(Debug, PartialEq, Eq)]
pub struct Revision {
    pub number: u64,
    pub hot: Option<HotUpdate>,
    /// Build id to publish: the previous one for hot updates, so client navigations stay soft.
    pub build_id: String,
}

/// How many recent hot revisions an update covers. The server polls the manifest, so it can miss a
/// revision when rebuilds follow each other quickly; a page behind by fewer revisions still updates in place.
const WINDOW: usize = 16;

/// The client files of the last successful development build and the recent hot changes.
#[derive(Default)]
pub struct Hot {
    revision: u64,
    build_id: String,
    digests: HashMap<String, String>,
    /// The recent hot revisions, oldest first, since the last reload.
    window: VecDeque<u64>,
    /// The app modules those revisions changed, by URL, each with the last revision it changed in.
    changed: BTreeMap<String, u64>,
}

impl Hot {
    /// Records a successful build. It is hot when every file that changed holds an instrumented app module;
    /// files that did not exist before were never loaded, so they need nothing. A change anywhere else (the
    /// entry, route tables, CSS module class names, an installed package) reloads the page.
    pub fn record(&mut self, modules: &[ClientModule], build_id: String) -> Revision {
        let mut changed = Vec::new();
        let mut hot = self.revision > 0;
        for module in modules {
            let Some(previous) = self.digests.get(&module.url) else { continue };
            if *previous == module.digest {
                continue;
            }
            if module.refresh {
                changed.push(module.url.clone());
            } else {
                hot = false;
            }
        }
        self.revision += 1;
        self.digests = modules.iter().map(|m| (m.url.clone(), m.digest.clone())).collect();
        if !hot {
            self.build_id = build_id;
            self.window.clear();
            self.changed.clear();
            return Revision { number: self.revision, hot: None, build_id: self.build_id.clone() };
        }
        if self.window.len() == WINDOW {
            self.window.pop_front();
        }
        self.window.push_back(self.revision);
        let oldest = self.window.front().copied().unwrap_or(self.revision);
        self.changed.retain(|_, revision| *revision >= oldest);
        for url in changed {
            self.changed.insert(url, self.revision);
        }
        let update = HotUpdate {
            base: oldest - 1,
            modules: self
                .changed
                .iter()
                .map(|(url, revision)| HotModule { url: url.clone(), revision: *revision })
                .collect(),
            importers: Self::importers(modules),
        };
        Revision { number: self.revision, hot: Some(update), build_id: self.build_id.clone() }
    }

    /// The importers of every app module.
    fn importers(modules: &[ClientModule]) -> BTreeMap<String, Vec<String>> {
        let mut importers: BTreeMap<String, Vec<String>> =
            modules.iter().filter(|m| m.refresh).map(|m| (m.url.clone(), Vec::new())).collect();
        for module in modules {
            for import in &module.imports {
                if let Some(list) = importers.get_mut(import) {
                    list.push(module.url.clone());
                }
            }
        }
        for list in importers.values_mut() {
            list.sort();
            list.dedup();
        }
        importers
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn module(url: &str, digest: &str, refresh: bool) -> ClientModule {
        ClientModule { url: url.into(), digest: digest.into(), refresh, imports: Vec::new() }
    }

    fn changed(revision: &Revision) -> Vec<(&str, u64)> {
        let modules = &revision.hot.as_ref().unwrap().modules;
        modules.iter().map(|m| (m.url.as_str(), m.revision)).collect()
    }

    #[test]
    fn applies_app_module_changes_in_place() {
        let mut hot = Hot::default();
        let first = hot.record(&[module("main", "1", false), module("page", "1", true)], "a".into());
        assert_eq!(first, Revision { number: 1, hot: None, build_id: "a".into() });

        let second =
            hot.record(&[module("main", "1", false), module("page", "2", true), module("new", "1", false)], "b".into());
        assert_eq!(second.number, 2);
        assert_eq!(second.build_id, "a");
        assert_eq!(second.hot.as_ref().unwrap().base, 1);
        assert_eq!(changed(&second), [("page", 2)]);

        // Each module carries the revision it last changed in, so a page evaluates only what is newer than its copy.
        let third =
            hot.record(&[module("main", "1", false), module("page", "2", true), module("b", "1", true)], "c".into());
        assert_eq!(changed(&third), [("page", 2)]);
        let fourth =
            hot.record(&[module("main", "1", false), module("page", "2", true), module("b", "2", true)], "c".into());
        assert_eq!(changed(&fourth), [("b", 4), ("page", 2)]);
        assert_eq!(fourth.hot.unwrap().base, 1);
    }

    #[test]
    fn lists_the_importers_of_app_modules() {
        let mut hot = Hot::default();
        let mut page = module("page", "1", true);
        page.imports = vec!["label".into(), "react".into()];
        let mut route = module("route", "1", false);
        route.imports = vec!["page".into()];
        let react = module("react", "1", false);
        hot.record(&[page.clone(), route.clone(), module("label", "1", true), react.clone()], "a".into());
        let next = hot.record(&[page, route, module("label", "2", true), react], "a".into());
        let importers = next.hot.unwrap().importers;
        assert_eq!(importers["label"], ["page"]);
        assert_eq!(importers["page"], ["route"]);
        assert!(!importers.contains_key("react"));
    }

    #[test]
    fn covers_a_window_of_revisions() {
        let mut hot = Hot::default();
        hot.record(&[module("page", "0", true), module("old", "0", true)], "a".into());
        hot.record(&[module("page", "0", true), module("old", "1", true)], "a".into());
        for digest in 1..=WINDOW + 4 {
            hot.record(&[module("page", &digest.to_string(), true), module("old", "1", true)], "a".into());
        }
        let last = hot.record(&[module("page", "x", true), module("old", "1", true)], "a".into());
        assert_eq!(last.number, WINDOW as u64 + 7);
        assert_eq!(last.hot.as_ref().unwrap().base, last.number - WINDOW as u64);
        assert_eq!(changed(&last), [("page", last.number)]);
    }

    #[test]
    fn reloads_for_other_changes() {
        let mut hot = Hot::default();
        hot.record(&[module("main", "1", false), module("page", "1", true)], "a".into());
        let next = hot.record(&[module("main", "2", false), module("page", "2", true)], "b".into());
        assert_eq!(next, Revision { number: 2, hot: None, build_id: "b".into() });
    }
}
