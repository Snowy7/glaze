use anyhow::{Context, Result};
use notify::Watcher;

/// Keeps the config file watch alive; dropping it stops the watch.
pub struct ConfigWatcher {
    _watcher: notify::RecommendedWatcher,
}

/// Calls `on_change` (on the watcher's thread) whenever `config.json` is
/// written, replaced or removed. `Config::save` writes a temp file and
/// renames it, so this watches the folder and filters by file name. Keep
/// `on_change` tiny, e.g. post a message to the UI thread.
pub fn watch_config(on_change: impl Fn() + Send + 'static) -> Result<ConfigWatcher> {
    let path = mypaper_config::path()?;
    let dir = path.parent().context("config path has no parent")?;
    let name = path.file_name().context("config path has no file name")?;
    // The folder must exist to be watched, and may not before the first save.
    std::fs::create_dir_all(dir)?;
    let name = name.to_owned();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if event.is_ok_and(|e| {
            !e.kind.is_access() && e.paths.iter().any(|p| p.file_name() == Some(&name))
        }) {
            on_change();
        }
    })?;
    watcher.watch(dir, notify::RecursiveMode::NonRecursive)?;
    Ok(ConfigWatcher { _watcher: watcher })
}
