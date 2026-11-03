//! A theme is a folder: `theme.json` plus the assets it references.
//!
//! ```json
//! { "name": "Aurora", "wallpaper": { "kind": "shader", "source": "aurora.wgsl", "fps": 60 } }
//! ```
//!
//! Asset paths are relative to the theme folder and may not escape it.

use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

pub const MANIFEST: &str = "theme.json";

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Theme {
    pub name: String,
    pub wallpaper: Wallpaper,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Wallpaper {
    /// A WGSL fragment shader defining `fn wallpaper(frag: vec2f) -> vec4f`.
    Shader(ShaderWallpaper),
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShaderWallpaper {
    pub source: PathBuf,
    /// Frame cap. Lower = lighter; shaders that move slowly look identical at 30.
    #[serde(default = "default_fps")]
    pub fps: u32,
}

fn default_fps() -> u32 {
    60
}

/// A parsed theme together with the folder it was loaded from.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedTheme {
    pub dir: PathBuf,
    pub theme: Theme,
}

impl LoadedTheme {
    pub fn load(dir: impl Into<PathBuf>) -> Result<Self> {
        let dir = dir.into();
        let manifest = dir.join(MANIFEST);
        let text = std::fs::read_to_string(&manifest)
            .with_context(|| format!("reading {}", manifest.display()))?;
        Self::parse(dir, &text).with_context(|| format!("in {}", manifest.display()))
    }

    pub fn parse(dir: PathBuf, json: &str) -> Result<Self> {
        let theme: Theme = serde_json::from_str(json)?;
        match &theme.wallpaper {
            Wallpaper::Shader(s) => {
                ensure_inside(&s.source)?;
                if !(1..=240).contains(&s.fps) {
                    bail!("wallpaper.fps must be between 1 and 240, got {}", s.fps);
                }
            }
        }
        Ok(Self { dir, theme })
    }

    /// Resolves a theme-relative asset path.
    pub fn asset(&self, relative: &Path) -> PathBuf {
        self.dir.join(relative)
    }
}

fn ensure_inside(path: &Path) -> Result<()> {
    let escapes = path
        .components()
        .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir));
    if escapes {
        bail!(
            "asset path {} must stay inside the theme folder",
            path.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> Result<LoadedTheme> {
        LoadedTheme::parse(PathBuf::from("themes/t"), json)
    }

    #[test]
    fn parses_shader_theme_with_default_fps() {
        let t = parse(r#"{ "name": "A", "wallpaper": { "kind": "shader", "source": "a.wgsl" } }"#)
            .unwrap();
        assert_eq!(
            t.theme.wallpaper,
            Wallpaper::Shader(ShaderWallpaper {
                source: "a.wgsl".into(),
                fps: 60
            })
        );
        assert_eq!(
            t.asset(Path::new("a.wgsl")),
            PathBuf::from("themes/t/a.wgsl")
        );
    }

    #[test]
    fn rejects_typos_and_unknown_kinds() {
        assert!(
            parse(r#"{ "name": "A", "wallpaper": { "kind": "shader", "sorce": "a.wgsl" } }"#)
                .is_err()
        );
        assert!(parse(r#"{ "name": "A", "wallpaper": { "kind": "hologram" } }"#).is_err());
    }

    #[test]
    fn rejects_assets_outside_the_theme() {
        for source in ["../x.wgsl", r"C:\\x.wgsl", "/x.wgsl"] {
            let json = format!(
                r#"{{ "name": "A", "wallpaper": {{ "kind": "shader", "source": "{source}" }} }}"#
            );
            assert!(parse(&json).is_err(), "{source} should be rejected");
        }
    }

    #[test]
    fn rejects_out_of_range_fps() {
        assert!(parse(r#"{ "name": "A", "wallpaper": { "kind": "shader", "source": "a.wgsl", "fps": 0 } }"#).is_err());
    }
}
