//! Renderer preferences. Per-wallpaper user settings override theme settings, then globals.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Which GPU the engine renders on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GpuPreference {
    /// The low-power GPU.
    Integrated,
    /// The discrete GPU.
    HighPerformance,
    /// The GPU that drives the screens (docs/glass.md, GPU default).
    #[default]
    Auto,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RenderOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpu: Option<GpuPreference>,
    /// Maximum wallpaper presents per second. Null inherits the next layer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fps: Option<u32>,
}
impl RenderOptions {
    pub fn validate(&self) -> Result<()> {
        if let Some(fps) = self.fps {
            ensure!(
                (1..=360).contains(&fps),
                "engine.fps must be between 1 and 360"
            );
        }
        Ok(())
    }
}

/// What the wallpaper does while core's game signal is on (docs/system.md, "Games").
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DuringGames {
    /// Every screen pauses on the theme's still, and after 10 s the engine lets go of what it
    /// draws with, as when every screen is covered (Wallpaper Engine's "other app fullscreen:
    /// pause").
    #[default]
    Pause,
    /// Screens the game doesn't cover keep animating.
    Keep,
}

impl DuringGames {
    fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct EngineConfig {
    pub gpu: GpuPreference,
    pub fps: Option<u32>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub wallpapers: BTreeMap<String, RenderOptions>,
    /// Written only when not the default, so configs stay readable by older builds.
    #[serde(skip_serializing_if = "DuringGames::is_default")]
    pub during_games: DuringGames,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderSettings {
    pub gpu: GpuPreference,
    pub fps: Option<u32>,
}
impl EngineConfig {
    pub fn validate(&self) -> Result<()> {
        RenderOptions {
            gpu: Some(self.gpu),
            fps: self.fps,
        }
        .validate()?;
        for (name, options) in &self.wallpapers {
            ensure!(!name.is_empty(), "engine.wallpapers keys must name a theme");
            options.validate()?;
        }
        Ok(())
    }
    pub fn resolve(&self, theme_id: &str, theme: RenderOptions) -> RenderSettings {
        let user = self.wallpapers.get(theme_id).copied().unwrap_or_default();
        RenderSettings {
            gpu: user.gpu.or(theme.gpu).unwrap_or(self.gpu),
            fps: user.fps.or(theme.fps).or(self.fps),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn during_games_defaults_to_pause_and_is_written_only_when_kept() {
        let config: EngineConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(config.during_games, DuringGames::Pause);
        assert!(!serde_json::to_string(&config).unwrap().contains("during_games"));
        let keep: EngineConfig = serde_json::from_str(r#"{"during_games":"keep"}"#).unwrap();
        assert_eq!(keep.during_games, DuringGames::Keep);
        assert!(serde_json::to_string(&keep).unwrap().contains(r#""during_games":"keep""#));
        assert!(serde_json::from_str::<EngineConfig>(r#"{"during_games":"maybe"}"#).is_err());
    }
    #[test]
    fn auto_defaults_and_per_wallpaper_precedence_are_explicit() {
        let mut global = EngineConfig::default();
        assert_eq!(
            global.resolve("rain-city", RenderOptions::default()).gpu,
            GpuPreference::Auto
        );
        global.fps = Some(60);
        let theme = RenderOptions {
            gpu: Some(GpuPreference::HighPerformance),
            fps: Some(30),
        };
        assert_eq!(
            global.resolve("rain-city", theme),
            RenderSettings {
                gpu: GpuPreference::HighPerformance,
                fps: Some(30)
            }
        );
        global.wallpapers.insert(
            "rain-city".into(),
            RenderOptions {
                gpu: Some(GpuPreference::Auto),
                fps: Some(24),
            },
        );
        assert_eq!(
            global.resolve("rain-city", theme),
            RenderSettings {
                gpu: GpuPreference::Auto,
                fps: Some(24)
            }
        );
        assert!(
            RenderOptions {
                gpu: None,
                fps: Some(0)
            }
            .validate()
            .is_err()
        );
        assert!(
            RenderOptions {
                gpu: None,
                fps: Some(361)
            }
            .validate()
            .is_err()
        );
    }
}
