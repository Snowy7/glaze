use std::path::Path;

use anyhow::{Context, Result};

use mypaper_fswatch::{Event, Watch};

/// Keeps a file watch alive; dropping it stops the watch.
pub struct ConfigWatcher {
    _watch: Watch,
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
/// nothing in the folder changes ([`Watch`]).
pub fn watch_file(path: &Path, on_change: impl Fn() + Send + 'static) -> Result<ConfigWatcher> {
    let dir = path.parent().context("watched path has no parent")?;
    let name = path.file_name().context("watched path has no file name")?;
    // The folder must exist to be watched, and may not before the first save.
    std::fs::create_dir_all(dir)?;
    let name = name.to_owned();
    let watch = Watch::directory(dir, false, move |changes| {
        // A rescan includes overflow and recovery after the directory was replaced.
        if match changes {
            Event::Rescan => true,
            Event::Paths(paths) => paths.iter().any(|p| p.file_name() == Some(&name)),
        } {
            on_change();
        }
    })?;
    Ok(ConfigWatcher { _watch: watch })
}
