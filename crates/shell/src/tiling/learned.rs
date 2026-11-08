//! App behavior learned from actual post-placement bounds, saved only when it changes.
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf};
#[derive(Clone, Copy, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Behavior {
    pub min_w: f64,
    pub min_h: f64,
    pub floating: bool,
}
pub struct Learned {
    path: PathBuf,
    apps: HashMap<String, Behavior>,
}
impl Learned {
    pub fn load() -> Result<Self> {
        let path = glaze_config::dir()?.join("tiling-learned.json");
        let apps = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => HashMap::new(),
            Err(e) => return Err(e.into()),
        };
        Ok(Self { path, apps })
    }
    pub fn get(&self, exe: &str) -> Behavior {
        self.apps
            .get(&exe.to_lowercase())
            .copied()
            .unwrap_or_default()
    }
    pub fn record(&mut self, exe: &str, min_w: f64, min_h: f64, floating: bool) -> Result<()> {
        let entry = self.apps.entry(exe.to_lowercase()).or_default();
        if entry.min_w >= min_w && entry.min_h >= min_h && (!floating || entry.floating) {
            return Ok(());
        }
        entry.min_w = entry.min_w.max(min_w);
        entry.min_h = entry.min_h.max(min_h);
        entry.floating |= floating;
        std::fs::create_dir_all(self.path.parent().unwrap())?;
        let temp = self.path.with_extension("json.tmp");
        std::fs::write(&temp, serde_json::to_vec_pretty(&self.apps)?)?;
        std::fs::rename(temp, &self.path)?;
        Ok(())
    }
}
