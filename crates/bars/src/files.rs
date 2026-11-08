//! Settings and native-service caches: APPDATA for configuration, LOCALAPPDATA for the taskbar
//! journal and app icons.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// `%LOCALAPPDATA%\glaze\<name>`.
pub fn local(name: &str) -> Result<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA").context("LOCALAPPDATA is not set")?;
    Ok(PathBuf::from(local).join("glaze").join(name))
}

/// Writes a file atomically (temp file plus rename), so readers and watchers never see it
/// half-written.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    std::fs::create_dir_all(path.parent().context("path has no parent")?)?;
    let mut temp = path.as_os_str().to_owned();
    temp.push(".tmp");
    let temp = PathBuf::from(temp);
    std::fs::write(&temp, bytes).with_context(|| format!("writing {}", temp.display()))?;
    std::fs::rename(&temp, path).with_context(|| format!("replacing {}", path.display()))
}

#[cfg(test)]
pub(crate) fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("glaze-bars-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
