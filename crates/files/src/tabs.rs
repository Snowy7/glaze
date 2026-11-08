//! Inactive tabs keep immutable row snapshots and their own history, selection and scroll.
//! Only the active tab has a directory watcher or background requests.
use crate::{
    model::{Entry, History, Selection, Sort},
    shell::Location,
};
use glaze_ui::Scroll;
use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};

#[derive(Default, Clone)]
pub struct Tab {
    pub path: String,
    pub location: Option<Location>,
    pub entries: Arc<Vec<Entry>>,
    pub shown: Arc<Vec<Entry>>,
    pub history: History,
    pub selection: Selection,
    pub sort: Sort,
    pub scroll: Scroll,
    pub query_text: String,
    pub indexed: bool,
    pub sizes: HashMap<String, crate::model::Size>,
    pub loaded: bool,
}

impl Tab {
    pub fn new(path: String) -> Self {
        Self {
            path,
            ..Self::default()
        }
    }
    pub fn label(&self) -> &str {
        self.location
            .as_ref()
            .map(|l| l.name.as_str())
            .unwrap_or_else(|| {
                std::path::Path::new(&self.path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(&self.path)
            })
    }
}

#[derive(Clone)]
pub struct Folder {
    pub location: Location,
    pub entries: Arc<Vec<Entry>>,
    pub sort: Sort,
    pub sizes: HashMap<String, crate::model::Size>,
}

/// A bounded LRU by entries as well as folder count. No folder scan on a cache hit.
#[derive(Default)]
pub struct FolderCache(VecDeque<Folder>);
impl FolderCache {
    pub fn insert(&mut self, folder: Folder) {
        self.0
            .retain(|f| !f.location.path.eq_ignore_ascii_case(&folder.location.path));
        if folder.entries.len() > 120_000 {
            return;
        }
        self.0.push_back(folder);
        while self.0.len() > 8 || self.0.iter().map(|f| f.entries.len()).sum::<usize>() > 120_000 {
            self.0.pop_front();
        }
    }
    pub fn get(&mut self, path: &str, sort: Sort) -> Option<Folder> {
        let at = self
            .0
            .iter()
            .position(|f| f.location.path.eq_ignore_ascii_case(path) && f.sort == sort)?;
        let folder = self.0.remove(at)?;
        self.0.push_back(folder.clone());
        Some(folder)
    }
    pub fn invalidate(&mut self, path: &str) {
        self.0
            .retain(|f| !f.location.path.eq_ignore_ascii_case(path));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cache_respects_sort_and_invalidates_changes() {
        let mut cache = FolderCache::default();
        for n in 0..10 {
            cache.insert(Folder {
                location: Location {
                    path: format!("C:\\{n}"),
                    name: n.to_string(),
                    parent: None,
                    filesystem: true,
                },
                entries: Arc::default(),
                sort: Sort::default(),
                sizes: HashMap::new(),
            });
        }
        assert!(cache.get("C:\\0", Sort::default()).is_none());
        assert!(cache.get("c:\\9", Sort::default()).is_some());
        assert!(
            cache
                .get(
                    "C:\\9",
                    Sort {
                        descending: true,
                        ..Default::default()
                    }
                )
                .is_none()
        );
        cache.invalidate("C:\\9");
        assert!(cache.get("C:\\9", Sort::default()).is_none());
    }
}
