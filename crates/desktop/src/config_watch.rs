use std::path::Path;

use anyhow::{Context, Result};

use crate::dir_watch::DirWatch;

/// Keeps a file watch alive; dropping it stops the watch.
pub struct ConfigWatcher {
    _watch: DirWatch,
}

/// Calls `on_change` (on the watcher's thread) whenever `config.json` is
/// written, replaced or removed. See [`watch_file`].
pub fn watch_config(on_change: impl Fn() + Send + 'static) -> Result<ConfigWatcher> {
    watch_file(&mypaper_config::path()?, on_change)
}

/// Calls `on_change` (on the watcher's thread) whenever the file at `path`
/// is written, replaced or removed. Atomic saves write a temp file and rename
/// it, so this watches the folder and filters by file name. Keep `on_change`
/// tiny, e.g. post a message to the UI thread. The watch costs no wakes while
/// nothing in the folder changes ([`DirWatch`]).
pub fn watch_file(path: &Path, on_change: impl Fn() + Send + 'static) -> Result<ConfigWatcher> {
    let dir = path.parent().context("watched path has no parent")?;
    let name = path.file_name().context("watched path has no file name")?;
    // The folder must exist to be watched, and may not before the first save.
    std::fs::create_dir_all(dir)?;
    let name = name.to_owned();
    let watch = DirWatch::new(dir, false, move |changes| {
        // `None`: too much changed to list, so maybe this file too.
        if changes.is_none_or(|paths| paths.iter().any(|p| p.as_os_str() == name)) {
            on_change();
        }
    })?;
    Ok(ConfigWatcher { _watch: watch })
}
