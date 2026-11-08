//! Per-volume offline recovery policy. Checkpoints/live writes never refresh scan age.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use glaze_config::IndexConfig;
use serde::{Deserialize, Serialize};

use crate::store::{DIRECTORY, NONE, Store};

#[derive(Default, Serialize, Deserialize)]
struct Volume {
    verified: u64,
    generation: u64,
    /// Direct child directory timestamps, not recursive change tokens.
    directories: BTreeMap<String, u32>,
}

#[derive(Default, Serialize, Deserialize)]
pub(crate) struct Freshness {
    version: u32,
    exclude: Vec<String>,
    include: Vec<String>,
    volumes: BTreeMap<PathBuf, Volume>,
}

pub(crate) fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

impl Freshness {
    pub(crate) fn load(cache: &Path, config: &IndexConfig) -> Self {
        let saved = std::fs::read(cache.join("freshness.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Self>(&bytes).ok());
        saved
            .filter(|saved| {
                saved.version == 1
                    && saved.exclude == config.exclude
                    && saved.include == config.include
            })
            .unwrap_or_else(|| Self {
                version: 1,
                exclude: config.exclude.clone(),
                include: config.include.clone(),
                ..Self::default()
            })
    }

    pub(crate) fn due(&self, root: &Path, generation: u64, now: u64, after: u64) -> bool {
        self.volumes.get(root).is_none_or(|volume| {
            generation < volume.generation
                || now < volume.verified
                || now - volume.verified >= after
        })
    }

    /// Recent volumes wait until their age limit, so offline drift cannot remain forever
    /// merely because this login keeps the service running for several days.
    pub(crate) fn remaining(&self, root: &Path, generation: u64, now: u64, after: u64) -> u64 {
        if self.due(root, generation, now, after) {
            return 0;
        }
        self.volumes[root]
            .verified
            .saturating_add(after)
            .saturating_sub(now)
    }

    /// Enumerates only the volume root. A matching timestamp cannot certify descendants:
    /// Windows does not propagate nested changes or in-place file writes to ancestors.
    pub(crate) fn changed_top_level(&self, root: &Path) -> Result<Vec<PathBuf>> {
        let entries = crate::scan::enumerate(root, crate::scan::Fetch::Basic)?;
        let previous = self.volumes.get(root);
        Ok(entries
            .into_iter()
            .filter(|entry| entry.attributes & DIRECTORY != 0)
            .filter(|entry| {
                previous.and_then(|volume| volume.directories.get(&entry.name))
                    != Some(&entry.modified)
            })
            .map(|entry| root.join(entry.name))
            .collect())
    }

    pub(crate) fn complete(&mut self, root: &Path, store: &Store, verified: u64) {
        let Some(id) = store.find(&root.to_string_lossy()) else {
            return;
        };
        let mut directories = BTreeMap::new();
        let mut child = store.record(id).first;
        while child != NONE {
            let record = store.record(child);
            if record.live() && record.attributes & DIRECTORY != 0 {
                directories.insert(store.name(record).to_owned(), record.modified);
            }
            child = record.next;
        }
        self.volumes.insert(
            root.to_owned(),
            Volume {
                verified,
                generation: store.generation(),
                directories,
            },
        );
    }

    pub(crate) fn save(&self, cache: &Path) -> Result<()> {
        use std::io::Write;
        let temporary = cache.join("freshness.json.tmp");
        let mut file = std::fs::File::create(&temporary)?;
        file.write_all(&serde_json::to_vec(self)?)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(temporary, cache.join("freshness.json"))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freshness_is_per_volume_and_unknown_future_or_older_generations_are_stale() {
        let mut saved = Freshness::default();
        saved.volumes.insert(
            PathBuf::from("C:/"),
            Volume {
                verified: 100,
                generation: 10,
                ..Default::default()
            },
        );
        let root = Path::new("C:/");
        assert!(!saved.due(root, 11, 109, 10));
        assert!(saved.due(root, 11, 110, 10));
        assert!(saved.due(Path::new("D:/"), 11, 109, 10));
        assert!(saved.due(root, 9, 109, 10));
        assert!(saved.due(root, 11, 99, 10));
        assert!(saved.due(root, 11, 100, 0));
        assert_eq!(saved.remaining(root, 11, 109, 10), 1);
        assert_eq!(saved.remaining(root, 11, 110, 10), 0);
    }

    #[test]
    fn persisted_age_survives_checkpoints_and_policy_edits_but_rules_invalidate_it() {
        let cache = std::env::temp_dir().join(format!("glaze-freshness-{}", std::process::id()));
        std::fs::create_dir_all(&cache).unwrap();
        let config = IndexConfig::default();
        let root = Path::new("C:/");
        let mut saved = Freshness::load(&cache, &config);
        let mut store = Store::new();
        store.insert(0, "C:", DIRECTORY, 0, 0);
        saved.complete(root, &store, 100);
        saved.save(&cache).unwrap();
        // Advancing the snapshot generation does not extend this volume's verified time.
        assert!(!Freshness::load(&cache, &config).due(root, 200, 101, 10));
        assert!(Freshness::load(&cache, &config).due(root, 200, 110, 10));
        let mut changed = config.clone();
        changed.startup_rescan_delay = 0;
        assert!(!Freshness::load(&cache, &changed).due(root, 200, 101, 10));
        changed.exclude.push("D:/private/**".into());
        assert!(Freshness::load(&cache, &changed).due(root, 200, 101, 10));
        std::fs::write(cache.join("freshness.json"), b"broken").unwrap();
        assert!(Freshness::load(&cache, &config).due(root, 200, 101, 10));
        std::fs::remove_dir_all(cache).unwrap();
    }
}
