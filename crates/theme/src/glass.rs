//! Liquid glass plates: what UI processes (widgets, bars) ask the wallpaper engine to draw
//! under their windows. Each writer keeps one file, `%LOCALAPPDATA%\mypaper\glass\<source>.json`,
//! holding its process id and its plates, then posts the registered window message [`MESSAGE`]
//! to the wallpaper's control window; the engine reads every file whose writer is still alive.
//! Coordinates are physical pixels of the virtual screen (what `GetMonitorInfo` reports).

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// The registered window message that says "a plates file changed".
pub const MESSAGE: &str = "mypaper.glass";

/// One glass plate: a rounded rectangle and how it looks (the bars' `look.glass` names).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plate {
    /// Stable per element (an instance id), so the engine can animate a plate that moves.
    pub id: String,
    /// `[x, y, width, height]`, physical px of the virtual screen.
    pub rect: [i32; 4],
    /// Corner radius, physical px.
    pub radius: f32,
    /// 0..=1: the whole effect (shadow, rim, bend).
    pub intensity: f32,
    /// 0..=1: how strongly the rim bends what is behind it.
    pub refraction: f32,
    /// 0..=1: the specular rim.
    pub highlight: f32,
    /// `[r, g, b, a]`, 0..=255: a wash over the plate (the widgets paint theirs themselves: a = 0).
    pub tint: [u8; 4],
    /// Background blur in physical px (liquid glass wants little or none).
    pub blur: f32,
}

/// A writer's file: who wrote it, and its plates.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plates {
    pub pid: u32,
    pub plates: Vec<Plate>,
}

/// `%LOCALAPPDATA%\mypaper\glass`.
pub fn dir() -> Result<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA").context("LOCALAPPDATA is not set")?;
    Ok(PathBuf::from(local).join("mypaper").join("glass"))
}

/// Writes `source`'s plates (replacing the file atomically, so a reader never sees half of it).
pub fn write(source: &str, plates: &Plates) -> Result<()> {
    let dir = dir()?;
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let path = dir.join(format!("{source}.json"));
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, serde_json::to_vec(plates)?)?;
    std::fs::rename(&temp, &path).with_context(|| format!("writing {}", path.display()))
}

/// Deletes `source`'s file, for a writer that stops (a crashed writer's file is ignored anyway,
/// by its pid). A missing file is fine.
pub fn remove(source: &str) -> Result<()> {
    let path = dir()?.join(format!("{source}.json"));
    match std::fs::remove_file(&path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            Err(e).with_context(|| format!("removing {}", path.display()))
        }
        _ => Ok(()),
    }
}

/// Every plate of every writer for whom `alive(pid)` holds; unreadable files are skipped.
pub fn read_all(alive: impl Fn(u32) -> bool) -> Vec<Plate> {
    let Ok(entries) = dir().and_then(|d| Ok(std::fs::read_dir(d)?)) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .filter_map(|p| serde_json::from_slice::<Plates>(&std::fs::read(&p).ok()?).ok())
        .filter(|file| alive(file.pid))
        .flat_map(|file| file.plates)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plates_round_trip_and_reject_unknown_fields() {
        let plates = Plates {
            pid: 7,
            plates: vec![Plate {
                id: "clock-1".into(),
                rect: [-2560, 9, 300, 200],
                radius: 18.0,
                intensity: 0.8,
                refraction: 0.6,
                highlight: 0.7,
                tint: [0, 0, 0, 0],
                blur: 0.0,
            }],
        };
        let json = serde_json::to_string(&plates).unwrap();
        assert_eq!(serde_json::from_str::<Plates>(&json).unwrap(), plates);
        assert!(
            serde_json::from_str::<Plates>(r#"{ "pid": 1, "plates": [], "extra": 1 }"#).is_err()
        );
    }
}
