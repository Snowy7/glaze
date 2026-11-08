//! Where the widget host finds and keeps things (docs/widgets.md, "Folders").
//!
//! Bundled plugins are found
//! beside the executable or in any parent folder, like themes, so
//! `target\debug\glaze-shell.exe` finds the repo's copies. User folders
//! live under `%APPDATA%\glaze`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Preserve exact bytes beside a configuration. Never replace a previous recovery copy.
/// Identical content reuses its backup, keeping repeated refused writes cheap.
pub fn backup(path: &Path, bytes: &[u8]) -> Result<PathBuf> {
    backup_tagged(path, "bak", bytes)
}

/// [`backup`] as `<file>.<tag>` (then `.<tag>.1`, ...): e.g. `widgets.json.v1`, the layout
/// as it was before its upgrade.
pub fn backup_tagged(path: &Path, tag: &str, bytes: &[u8]) -> Result<PathBuf> {
    use std::io::Write;
    for index in 0.. {
        let mut name = path.as_os_str().to_owned();
        name.push(if index == 0 { format!(".{tag}") } else { format!(".{tag}.{index}") });
        let target = PathBuf::from(name);
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&target) {
            Ok(mut file) => {
                file.write_all(bytes)?;
                file.sync_all()?;
                return Ok(target);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                if std::fs::read(&target).is_ok_and(|saved| saved == bytes) {
                    return Ok(target);
                }
            }
            Err(e) => return Err(e).context("preserving configuration backup"),
        }
    }
    unreachable!()
}

/// The first `<ancestor>/<relative>` folder above `exe` for which `ok` holds.
fn find_up(exe: &Path, relative: &str, ok: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    exe.ancestors()
        .skip(1)
        .map(|dir| dir.join(relative))
        .find(|dir| ok(dir))
}

fn exe() -> Option<PathBuf> {
    std::env::current_exe().ok()
}

/// The bundled `plugins/` folder.
pub fn bundled_plugins() -> Option<PathBuf> {
    find_up(&exe()?, "plugins", |dir| dir.is_dir())
}

/// `%APPDATA%\glaze\<name>`.
pub fn user(name: &str) -> Result<PathBuf> {
    Ok(glaze_config::dir()?.join(name))
}

pub fn layout() -> Result<PathBuf> {
    user("widgets.json")
}

/// `%LOCALAPPDATA%\glaze\<name>`.
pub fn local(name: &str) -> Result<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA").context("LOCALAPPDATA is not set")?;
    Ok(PathBuf::from(local).join("glaze").join(name))
}

/// Writes a file atomically (temp file plus rename), so readers and watchers
/// never see it half-written.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    std::fs::create_dir_all(path.parent().context("path has no parent")?)?;
    let mut temp = path.as_os_str().to_owned();
    temp.push(".tmp");
    let temp = PathBuf::from(temp);
    std::fs::write(&temp, bytes).with_context(|| format!("writing {}", temp.display()))?;
    std::fs::rename(&temp, path).with_context(|| format!("replacing {}", path.display()))
}

/// A path for WebView2 APIs: absolute, without the `\\?\` prefix.
pub fn plain(path: &Path) -> String {
    let path = std::path::absolute(path).unwrap_or_else(|_| path.to_owned());
    let text = path.display().to_string();
    text.strip_prefix(r"\\?\")
        .map(str::to_owned)
        .unwrap_or(text)
}

#[cfg(test)]
pub(crate) fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("glaze-widgets-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_writes_replace_and_leave_no_temp_file() {
        let root = temp_dir("atomic");
        let path = root.join("sub").join("a.json");
        write_atomic(&path, b"1").unwrap();
        write_atomic(&path, b"2").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"2");
        assert!(!root.join("sub").join("a.json.tmp").exists());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
