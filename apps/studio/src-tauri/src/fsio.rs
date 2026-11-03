//! Small file helpers shared by the files Studio edits next to other programs.

use std::path::Path;

use anyhow::{Context, Result};

/// The file's text, or `None` if it doesn't exist.
pub fn read_optional(path: &Path) -> Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

/// Writes through a temp file and a rename, so a watcher never sees half a file.
/// The temp name is ours alone: the program that owns the file uses its own
/// `.tmp`, and two writers must not share one.
pub fn write_atomic(path: &Path, contents: impl AsRef<[u8]>) -> Result<()> {
    let dir = path.parent().context("the path has no parent folder")?;
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let mut name = path.file_name().context("the path has no file name")?.to_os_string();
    name.push(".studio.tmp");
    let temp = path.with_file_name(name);
    std::fs::write(&temp, contents).with_context(|| format!("writing {}", temp.display()))?;
    std::fs::rename(&temp, path).with_context(|| format!("replacing {}", path.display()))?;
    Ok(())
}
