//! Watches `%APPDATA%\mypaper` so the UI follows changes made elsewhere
//! (the tray, a text editor, a second Studio). Emits one debounced
//! `studio:changed` event per burst of file events.

use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

use anyhow::{Context, Result};
use notify::{RecursiveMode, Watcher};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

pub const EVENT: &str = "studio:changed";

/// What changed. Several flags can be set when a burst touches several files.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Changes {
    pub config: bool,
    pub themes: bool,
    /// `bars.json`.
    pub bars: bool,
    /// `widgets.json`, or a widget folder.
    pub widgets: bool,
}

impl Changes {
    fn merge(&mut self, other: Self) {
        self.config |= other.config;
        self.themes |= other.themes;
        self.bars |= other.bars;
        self.widgets |= other.widgets;
    }
}

/// Classifies an event path: a file we follow (not its temp file), or anything under `themes\` or `widgets\`.
fn classify(dir: &Path, path: &Path) -> Changes {
    let Ok(relative) = path.strip_prefix(dir) else {
        return Changes::default();
    };
    Changes {
        config: relative == Path::new("config.json"),
        themes: relative.starts_with("themes"),
        bars: relative == Path::new("bars.json"),
        widgets: relative == Path::new("widgets.json") || relative.starts_with("widgets"),
    }
}

/// Starts the watcher on a background thread that owns it for the life of the app.
pub fn start(app: AppHandle) -> Result<()> {
    let dir = mypaper_config::dir()?;
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;

    let (tx, rx) = mpsc::channel();
    let root = dir.clone();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        let Ok(event) = event else { return };
        let mut changes = Changes::default();
        for path in &event.paths {
            changes.merge(classify(&root, path));
        }
        if changes != Changes::default() {
            let _ = tx.send(changes);
        }
    })?;
    watcher.watch(&dir, RecursiveMode::Recursive)?;

    std::thread::spawn(move || {
        let _watcher = watcher;
        while let Ok(mut changes) = rx.recv() {
            // Saving is write-temp-then-rename, so wait for the burst to settle.
            while let Ok(more) = rx.recv_timeout(Duration::from_millis(80)) {
                changes.merge(more);
            }
            let _ = app.emit(EVENT, changes);
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_files_studio_follows_count() {
        let dir = Path::new(r"C:\Users\me\AppData\Roaming\mypaper");
        let at = |rel: &str| classify(dir, &dir.join(rel));
        assert_eq!(at("config.json"), Changes { config: true, ..Changes::default() });
        assert_eq!(at("config.json.tmp"), Changes::default());
        assert_eq!(at(r"themes\neon\theme.json"), Changes { themes: true, ..Changes::default() });
        assert_eq!(at("themes"), Changes { themes: true, ..Changes::default() });
        assert_eq!(at("themes-backup"), Changes::default());
        assert_eq!(at("bars.json"), Changes { bars: true, ..Changes::default() });
        assert_eq!(at("bars.json.studio.tmp"), Changes::default());
        assert_eq!(at("widgets.json"), Changes { widgets: true, ..Changes::default() });
        assert_eq!(at(r"widgets\mine\widget.json"), Changes { widgets: true, ..Changes::default() });
        assert_eq!(at(r"widget-data\clock-1.json"), Changes::default());
        assert_eq!(classify(dir, Path::new(r"D:\elsewhere\config.json")), Changes::default());
    }
}
