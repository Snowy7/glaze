//! The wallpaper's palette: the engine writes it after the first frame of
//! each newly loaded theme, and core reads it to resolve `"auto"` colors.
//!
//! ```json
//! { "theme": "D:\\themes\\aurora", "dominant": "#1a2b3c", "accent": "#33aaff" }
//! ```
//!
//! `theme` is the theme folder it was computed for, so a palette left over
//! from another theme is never applied.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use mypaper_theme::Color;
use serde::{Deserialize, Serialize};

use crate::color::hex;

/// `%LOCALAPPDATA%\mypaper`: machine-local state (palette, undo journal, log).
pub fn local_dir() -> Result<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA").context("LOCALAPPDATA is not set")?;
    Ok(PathBuf::from(base).join("mypaper"))
}

const FILE: &str = "palette.json";

pub fn path() -> Result<PathBuf> {
    Ok(local_dir()?.join(FILE))
}

pub use mypaper_theme::windows::Palette;

/// A palette and the theme folder it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub theme: PathBuf,
    pub palette: Palette,
}

/// The JSON form, with colors as `"#rrggbb"`.
#[derive(Serialize, Deserialize)]
struct Wire {
    theme: PathBuf,
    dominant: String,
    accent: String,
}

impl Snapshot {
    fn to_json(&self) -> Result<String> {
        let wire = Wire {
            theme: self.theme.clone(),
            dominant: hex(self.palette.dominant),
            accent: hex(self.palette.accent),
        };
        Ok(serde_json::to_string_pretty(&wire)? + "\n")
    }

    fn from_json(json: &str) -> Option<Self> {
        let wire: Wire = serde_json::from_str(json).ok()?;
        Some(Self {
            theme: wire.theme,
            palette: Palette {
                dominant: Color::parse(&wire.dominant)?,
                accent: Color::parse(&wire.accent)?,
            },
        })
    }

    /// Was this palette computed for the theme in `dir`?
    pub fn is_for(&self, dir: &Path) -> bool {
        normalize(&self.theme) == normalize(dir)
    }
}

/// Case-insensitive, separator-insensitive form for comparing folders.
fn normalize(path: &Path) -> String {
    path.to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase()
}

/// Writes the palette atomically (temp file + rename), so watchers never
/// see a half-written file.
pub fn write(snapshot: &Snapshot) -> Result<()> {
    let path = path()?;
    std::fs::create_dir_all(path.parent().context("palette path has no parent")?)?;
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, snapshot.to_json()?)?;
    std::fs::rename(&temp, &path)?;
    Ok(())
}

/// The current palette, or `None` if there is none or it is unreadable.
pub fn read() -> Option<Snapshot> {
    let text = std::fs::read_to_string(path().ok()?).ok()?;
    Snapshot::from_json(&text)
}

/// Keeps the palette file watch alive; dropping it stops the watch.
pub struct PaletteWatcher {
    _watcher: mypaper_fswatch::Watch,
}

/// Calls `on_change` (on the watcher's thread) whenever `palette.json` is
/// written or replaced. Keep it tiny, e.g. post a message to the UI thread.
pub fn watch(on_change: impl Fn() + Send + 'static) -> Result<PaletteWatcher> {
    let dir = local_dir()?;
    std::fs::create_dir_all(&dir)?;
    let watcher = mypaper_fswatch::Watch::files(&dir, &[FILE], on_change)?;
    Ok(PaletteWatcher { _watcher: watcher })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_round_trips_and_names_the_theme() {
        let snapshot = Snapshot {
            theme: PathBuf::from(r"D:\themes\aurora"),
            palette: Palette {
                dominant: Color([0x1a, 0x2b, 0x3c]),
                accent: Color([0x33, 0xaa, 0xff]),
            },
        };
        let json = snapshot.to_json().unwrap();
        assert!(json.contains("\"#33aaff\""), "{json}");
        assert_eq!(Snapshot::from_json(&json), Some(snapshot));
        assert_eq!(
            Snapshot::from_json(r#"{ "theme": "x", "dominant": "red" }"#),
            None
        );
    }

    #[test]
    fn theme_match_ignores_case_and_separators() {
        let snapshot = Snapshot::from_json(
            r##"{ "theme": "D:\\Themes\\Aurora", "dominant": "#000001", "accent": "#000002" }"##,
        )
        .unwrap();
        assert!(snapshot.is_for(Path::new("d:/themes/aurora/")));
        assert!(!snapshot.is_for(Path::new(r"D:\themes\midnight")));
    }
}
