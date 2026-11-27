//! Decides which `volt dev` rebuilds the browser can apply in place with React Fast Refresh.

use std::collections::{HashMap, VecDeque};

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
    /// The modules each recent hot revision changed, oldest first, since the last reload.
    history: VecDeque<(u64, Vec<HotModule>)>,
}

impl Hot {
    /// Records a successful build. It is hot when every file that changed holds an instrumented app module;
    /// files that did not exist before were never loaded, so they need nothing. Installed packages do not
    /// change while `volt dev` runs (it does not watch them): their output only varies with what the app
    /// uses of them and with rolldown's interop details, and the loaded instances keep working, or fail to
    /// link the new code, which reloads in the browser. A change anywhere else (the entry, route tables,
    /// CSS module class names) reloads the page.
    pub fn record(&mut self, modules: &[ClientModule], build_id: String) -> Revision {
        let mut changed = Vec::new();
        let mut hot = self.revision > 0;
        for module in modules {
            let Some(previous) = self.digests.get(&module.url) else { continue };
            if *previous == module.digest || module.dependency {
                continue;
            }
            match &module.refresh {
                Some(id) => changed.push(HotModule { id: id.clone(), url: module.url.clone() }),
                None => hot = false,
            }
        }
        self.revision += 1;
        self.digests = modules.iter().map(|m| (m.url.clone(), m.digest.clone())).collect();
        if !hot {
            self.build_id = build_id;
            self.history.clear();
            return Revision { number: self.revision, hot: None, build_id: self.build_id.clone() };
        }
        if self.history.len() == WINDOW {
            self.history.pop_front();
        }
        self.history.push_back((self.revision, changed));
        let base = self.history.front().map_or(self.revision, |(revision, _)| revision - 1);
        let mut modules: Vec<HotModule> = Vec::new();
        for module in self.history.iter().flat_map(|(_, changed)| changed) {
            if !modules.contains(module) {
                modules.push(module.clone());
            }
        }
        modules.sort_by(|a, b| a.id.cmp(&b.id));
        Revision { number: self.revision, hot: Some(HotUpdate { base, modules }), build_id: self.build_id.clone() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn module(url: &str, digest: &str, refresh: bool) -> ClientModule {
        ClientModule {
            url: url.into(),
            digest: digest.into(),
            refresh: refresh.then(|| format!("{url}.tsx")),
            dependency: url.starts_with("node_modules/"),
        }
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
        let update = second.hot.unwrap();
        assert_eq!(update.base, 1);
        assert_eq!(update.modules, vec![HotModule { id: "page.tsx".into(), url: "page".into() }]);

        // Updates cover the recent hot revisions, for a server that missed one.
        let unchanged = hot.record(&[module("main", "1", false), module("page", "2", true)], "c".into());
        assert_eq!(unchanged.hot, Some(HotUpdate { base: 1, modules: update.modules }));
    }

    #[test]
    fn covers_a_window_of_revisions() {
        let mut hot = Hot::default();
        hot.record(&[module("page", "0", true)], "a".into());
        for digest in 1..=WINDOW + 4 {
            hot.record(&[module("page", &digest.to_string(), true)], "a".into());
        }
        let last = hot.record(&[module("page", "x", true)], "a".into());
        assert_eq!(last.number, WINDOW as u64 + 6);
        assert_eq!(last.hot.unwrap().base, last.number - WINDOW as u64);
    }

    #[test]
    fn reloads_for_other_changes() {
        let mut hot = Hot::default();
        hot.record(&[module("main", "1", false), module("page", "1", true)], "a".into());
        let next = hot.record(&[module("main", "2", false), module("page", "2", true)], "b".into());
        assert_eq!(next, Revision { number: 2, hot: None, build_id: "b".into() });
    }

    #[test]
    fn ignores_dependency_output() {
        let mut hot = Hot::default();
        hot.record(&[module("node_modules/react.js", "1", false)], "a".into());
        let next = hot.record(&[module("node_modules/react.js", "2", false)], "b".into());
        assert_eq!(next.hot, Some(HotUpdate { base: 1, modules: Vec::new() }));
    }
}
