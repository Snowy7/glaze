//! Follows edits made outside Studio (parity S32): the settings files and the user's wallpaper
//! folders. The watcher's thread only posts; the UI thread re-reads after a short quiet spell.

use std::path::Path;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Result;
use notify::Watcher;

use crate::docs::DocId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    Doc(DocId),
    Themes,
}

/// Keeps the watch alive.
pub struct Watch {
    _watcher: notify::RecommendedWatcher,
}

/// Watches `%APPDATA%\glaze` (the files, and `themes\` below it).
pub fn start(inbox: Sender<crate::app::Msg>, thread: u32) -> Result<Watch> {
    let dir = glaze_config::dir()?;
    std::fs::create_dir_all(&dir)?;
    let files: Vec<(DocId, std::ffi::OsString)> = DocId::ALL
        .iter()
        .zip(crate::docs::paths())
        .filter_map(|(id, p)| Some((*id, p?.file_name()?.to_owned())))
        .collect();
    let themes = dir.join("themes");
    // Bursts (a save is a write plus a rename) collapse into one message.
    let pending: Arc<Mutex<Vec<Change>>> = Arc::default();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        let Ok(event) = event else { return };
        if event.kind.is_access() {
            return;
        }
        let mut changes = Vec::new();
        for path in &event.paths {
            if path.starts_with(&themes) {
                // Draft previews update continuously; their editor owns output polling.
                if path.strip_prefix(&themes).ok().and_then(|relative| relative.components().next())
                    .is_some_and(|child| child.as_os_str().to_string_lossy().starts_with(".draft-")) {
                    continue;
                }
                changes.push(Change::Themes);
            } else if let Some((id, _)) = files.iter().find(|(_, n)| Some(n.as_os_str()) == path.file_name()) {
                changes.push(Change::Doc(*id));
            }
        }
        if changes.is_empty() {
            return;
        }
        let first = {
            let mut p = pending.lock().unwrap_or_else(|e| e.into_inner());
            let first = p.is_empty();
            for c in changes {
                if !p.contains(&c) {
                    p.push(c);
                }
            }
            first
        };
        if first {
            let (pending, inbox) = (pending.clone(), inbox.clone());
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(80));
                let changes = std::mem::take(&mut *pending.lock().unwrap_or_else(|e| e.into_inner()));
                let _ = inbox.send(crate::app::Msg::Files(changes));
                crate::wake(thread);
            });
        }
    })?;
    watcher.watch(&dir, notify::RecursiveMode::NonRecursive)?;
    let themes_dir = dir.join("themes");
    if themes_dir.is_dir() {
        watcher.watch(&themes_dir, notify::RecursiveMode::Recursive)?;
    }
    let _ = Path::new("");
    Ok(Watch { _watcher: watcher })
}
