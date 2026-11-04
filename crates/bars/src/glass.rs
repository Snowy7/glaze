//! Wallpaper glass, the bars' side (docs/bars.md, "Wallpaper glass"). Only mypaper owns the
//! wallpaper's pixels: the engine writes a blurred 1/8-size snapshot of each monitor to
//! `%LOCALAPPDATA%\mypaper\backdrop\<DEVICE>.bmp` once per theme load (and, while an animated
//! wallpaper runs, every `interval` seconds as long as `request.json` asks for it), replacing it
//! by atomic rename. The bars write and delete that request, watch the folder, and give each bar
//! the snapshot's URL (with its modification time, so a new one reloads) and where the bar's
//! window sits on its monitor, so the page lines the image up with the screen.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use notify::Watcher;
use serde::Serialize;

use crate::wake::{Msg, Waker};

/// The virtual host the snapshot folder is mapped to.
pub const BACKDROP_HOST: &str = "backdrop.bars.mypaper";
const REQUEST: &str = "request.json";

/// What a bar's page gets as `backdrop`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Backdrop {
    pub url: String,
    /// The monitor's size and the window's offset on it, DIPs.
    pub width: f64,
    pub height: f64,
    pub x: f64,
    pub y: f64,
}

/// `\\.\DISPLAY1` → `DISPLAY1`, the engine's file name.
pub fn file_stem(device: &str) -> &str {
    device.strip_prefix(r"\\.\").unwrap_or(device)
}

pub struct Glass {
    dir: PathBuf,
    /// Device file stem → the snapshot's modification time (ms).
    versions: HashMap<String, u64>,
    /// The interval asked for in `request.json`, if any.
    requested: Option<u32>,
    _watch: Option<notify::RecommendedWatcher>,
}

impl Glass {
    pub fn new(dir: PathBuf, waker: Waker) -> Self {
        let _ = std::fs::create_dir_all(&dir);
        let watch = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
            let snapshot = |p: &PathBuf| p.extension().is_some_and(|e| e == "bmp");
            if event.is_ok_and(|e| !e.kind.is_access() && e.paths.iter().any(snapshot)) {
                waker.send(Msg::Backdrop);
            }
        })
        .and_then(|mut w| {
            w.watch(&dir, notify::RecursiveMode::NonRecursive)
                .map(|()| w)
        })
        .inspect_err(|e| log::warn!("not watching the wallpaper snapshots: {e}"))
        .ok();
        let mut glass = Self {
            dir,
            versions: HashMap::new(),
            requested: None,
            _watch: watch,
        };
        // A request left by a crash would keep the engine publishing for nobody.
        let _ = std::fs::remove_file(glass.dir.join(REQUEST));
        glass.rescan();
        glass
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Re-reads which snapshots exist. Returns whether any changed.
    pub fn rescan(&mut self) -> bool {
        let versions: HashMap<String, u64> = std::fs::read_dir(&self.dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                (path.extension()? == "bmp").then_some(())?;
                let modified = entry.metadata().ok()?.modified().ok()?;
                let ms = modified.duration_since(UNIX_EPOCH).ok()?.as_millis() as u64;
                Some((path.file_stem()?.to_str()?.to_ascii_uppercase(), ms))
            })
            .collect();
        let changed = versions != self.versions;
        self.versions = versions;
        changed
    }

    /// The snapshot of `device`, if the engine wrote one.
    pub fn url(&self, device: &str) -> Option<String> {
        let stem = file_stem(device).to_ascii_uppercase();
        let version = self.versions.get(&stem)?;
        Some(format!("https://{BACKDROP_HOST}/{stem}.bmp?v={version}"))
    }

    /// Asks the engine for snapshots every `interval` seconds (`None`: only once per theme
    /// load). Writes or deletes `request.json` only when the answer changes.
    pub fn request(&mut self, interval: Option<u32>) {
        if interval == self.requested {
            return;
        }
        self.requested = interval;
        let path = self.dir.join(REQUEST);
        let result = match interval {
            Some(interval) => mypaper_bars::files::write_atomic(
                &path,
                format!("{{ \"interval\": {interval} }}\n").as_bytes(),
            ),
            None => std::fs::remove_file(&path)
                .or_else(|e| {
                    if e.kind() == std::io::ErrorKind::NotFound {
                        Ok(())
                    } else {
                        Err(e)
                    }
                })
                .map_err(Into::into),
        };
        if let Err(e) = result {
            log::warn!("wallpaper snapshot request: {e:#}");
        }
    }
}

impl Drop for Glass {
    fn drop(&mut self) {
        self.request(None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_names_become_file_stems() {
        assert_eq!(file_stem(r"\\.\DISPLAY2"), "DISPLAY2");
        assert_eq!(file_stem("dev"), "dev");
    }
}
