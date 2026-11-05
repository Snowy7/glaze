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

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct EngineConfig {
    pub gpu: GpuPreference,
    pub fps: Option<u32>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub wallpapers: BTreeMap<String, RenderOptions>,
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
