//! The fixed test setups: which theme, whether the widget host and the bars run, and the
//! temporary `%APPDATA%` / `%LOCALAPPDATA%` each one runs in, so a run never reads or writes the
//! user's settings.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use mypaper_desktop::{covered_monitors, monitor_infos};
use serde_json::{Value, json};

/// The shell bars in a scenario.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bars {
    Off,
    /// Top bar and dock on every monitor; the native taskbar kept.
    On,
    /// The same, with the native taskbar hidden (measures how long it stays visible).
    HideTaskbar,
}

#[derive(Debug, Clone, Copy)]
pub struct Scenario {
    pub name: &'static str,
    pub about: &'static str,
    /// A bundled theme folder (`themes/<theme>`).
    pub theme: &'static str,
    pub widgets: bool,
    pub bars: Bars,
}

/// Every scenario, in run order: one theme of each kind alone, then the shell on a still
/// wallpaper (so its own cost shows), then everything.
pub const ALL: &[Scenario] = &[
    Scenario {
        name: "shader",
        about: "aurora shader at 60 fps, wallpaper only",
        theme: "aurora",
        widgets: false,
        bars: Bars::Off,
    },
    Scenario {
        name: "image",
        about: "photo, wallpaper only",
        theme: "photo",
        widgets: false,
        bars: Bars::Off,
    },
    Scenario {
        name: "slideshow",
        about: "gallery slideshow, wallpaper only",
        theme: "gallery",
        widgets: false,
        bars: Bars::Off,
    },
    Scenario {
        name: "video",
        about: "earth-orbit video, wallpaper only",
        theme: "earth-orbit",
        widgets: false,
        bars: Bars::Off,
    },
    Scenario {
        name: "scene",
        about: "islands 3D scene at 30 fps, wallpaper only",
        theme: "islands",
        widgets: false,
        bars: Bars::Off,
    },
    Scenario {
        name: "web",
        about: "webdemo web page, wallpaper only",
        theme: "webdemo",
        widgets: false,
        bars: Bars::Off,
    },
    Scenario {
        name: "widgets",
        about: "photo + 3 widgets (clock, system monitor, now playing)",
        theme: "photo",
        widgets: true,
        bars: Bars::Off,
    },
    Scenario {
        name: "bars",
        about: "photo + top bar and dock, taskbar kept",
        theme: "photo",
        widgets: false,
        bars: Bars::On,
    },
    Scenario {
        name: "bars-hide",
        about: "photo + top bar and dock, taskbar hidden",
        theme: "photo",
        widgets: false,
        bars: Bars::HideTaskbar,
    },
    Scenario {
        name: "full",
        about: "aurora + widgets + top bar and dock",
        theme: "aurora",
        widgets: true,
        bars: Bars::On,
    },
];

impl Scenario {
    /// The executables this scenario runs (core starts whichever are next to it).
    pub fn exes(&self) -> Vec<&'static str> {
        let mut exes = vec!["mypaper-core.exe", "mypaper-wallpaper.exe"];
        if self.widgets {
            exes.push("mypaper-widgets.exe");
        }
        if self.bars != Bars::Off {
            exes.push("mypaper-bars.exe");
        }
        exes
    }

    /// The `READY` markers a complete start logs, as `(component, metric)`.
    pub fn expected(&self) -> Vec<(&'static str, &'static str)> {
        let mut expected = vec![("core", "spawn"), ("wallpaper", "first_frame")];
        if self.widgets {
            expected.push(("widgets", "first_paint"));
        }
        if self.bars != Bars::Off {
            expected.push(("bars", "first_paint"));
        }
        expected
    }
}

/// A scenario's private `%APPDATA%` and `%LOCALAPPDATA%` (under the harness's temp folder).
pub struct Env {
    pub appdata: PathBuf,
    pub local: PathBuf,
}

impl Env {
    /// The log every mypaper process of the run appends to.
    pub fn log(&self) -> PathBuf {
        self.local.join("mypaper").join("mypaper.log")
    }

    pub fn config(&self) -> PathBuf {
        self.appdata.join("mypaper").join("config.json")
    }

    /// Where a test run's bars would journal the native taskbar.
    pub fn taskbar_journal(&self) -> PathBuf {
        self.local
            .join("mypaper")
            .join(mypaper_bars::taskbar::JOURNAL)
    }

    /// Creates a fresh environment for `scenario` under `root`: a theme copy that points at the
    /// bundled assets (without the `windows` styling section, which would change the user's
    /// registry), `config.json`, `bars.json` and `widgets.json`.
    pub fn fresh(root: &Path, scenario: &Scenario, content: &Path) -> Result<Self> {
        let _ = std::fs::remove_dir_all(root);
        let env = Self {
            appdata: root.join("appdata"),
            local: root.join("local"),
        };
        let settings = env.appdata.join("mypaper");
        std::fs::create_dir_all(&settings)?;
        std::fs::create_dir_all(env.local.join("mypaper"))?;

        let theme_name = format!("perf-{}", scenario.name);
        let bundled = content.join("themes").join(scenario.theme);
        let theme = perf_theme(&bundled)?;
        let theme_dir = settings.join("themes").join(&theme_name);
        std::fs::create_dir_all(&theme_dir)?;
        write_json(&theme_dir.join("theme.json"), &theme)?;
        env.write_config(&theme_name, false)?;

        let bars = match scenario.bars {
            Bars::Off | Bars::On => "keep",
            Bars::HideTaskbar => "hide",
        };
        write_json(
            &settings.join("bars.json"),
            &json!({
                "top": { "enabled": true },
                "dock": { "enabled": true },
                "taskbar": { "mode": bars },
            }),
        )?;
        write_json(&settings.join("widgets.json"), &starter_widgets())?;
        Ok(env)
    }

    /// Writes `config.json`. Fullscreen and battery pausing are off so the user's windows and
    /// power source can't hold a run; the static snapshot is off so runs never touch the
    /// Windows wallpaper.
    pub fn write_config(&self, theme: &str, paused: bool) -> Result<()> {
        write_json(
            &self.config(),
            &json!({
                "theme": theme,
                "paused": paused,
                "pause_on_battery": false,
                "pause_on_fullscreen": false,
                "static_snapshot": false,
            }),
        )
    }

    /// Pauses (or resumes) the running scenario through its `config.json`, like the tray does.
    pub fn set_paused(&self, paused: bool) -> Result<()> {
        let text = std::fs::read_to_string(self.config())?;
        let mut config: Value = serde_json::from_str(&text)?;
        config["paused"] = Value::Bool(paused);
        write_json(&self.config(), &config)
    }
}

/// The bundled theme's `theme.json`, styling removed and asset paths made absolute.
fn perf_theme(bundled: &Path) -> Result<Value> {
    let path = bundled.join("theme.json");
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let mut theme: Value = serde_json::from_str(&text)?;
    let object = theme
        .as_object_mut()
        .context("theme.json is not an object")?;
    object.remove("windows");
    if let Some(wallpaper) = object.get_mut("wallpaper").and_then(Value::as_object_mut) {
        for key in ["source", "folder", "url"] {
            if let Some(Value::String(value)) = wallpaper.get_mut(key)
                && !value.contains("://")
                && Path::new(value.as_str()).is_relative()
            {
                *value = bundled.join(&*value).display().to_string();
            }
        }
    }
    Ok(theme)
}

/// Three starter widgets on the first monitor no window covers (a covered monitor hides its
/// widgets, and a hidden layer never paints), else on the primary.
fn starter_widgets() -> Value {
    let monitors = monitor_infos();
    let covered = covered_monitors(&monitors.iter().map(|m| m.rect).collect::<Vec<_>>());
    let device = monitors
        .iter()
        .zip(&covered)
        .find(|(_, covered)| !**covered)
        .map(|(m, _)| m)
        .or_else(|| monitors.iter().find(|m| m.primary))
        .map_or(r"\\.\DISPLAY1".to_owned(), |m| m.device.clone());
    let instance = |id: &str, cell: [u32; 2], size: [u32; 2]| json!({ "instance": format!("{id}-1"), "widget": id, "monitor": device, "cell": cell, "size": size });
    json!({
        "version": 1,
        "instances": [
            instance("clock", [0, 0], [2, 1]),
            instance("system-monitor", [0, 1], [2, 2]),
            instance("now-playing", [0, 3], [3, 1]),
        ],
    })
}

/// Writes JSON atomically, as mypaper's own writers do, so watchers never see half a file.
fn write_json(path: &Path, value: &Value) -> Result<()> {
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, serde_json::to_string_pretty(value)? + "\n")?;
    std::fs::rename(&temp, path).with_context(|| format!("writing {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scenario_names_are_unique() {
        let mut names: Vec<_> = ALL.iter().map(|s| s.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), ALL.len());
    }

    #[test]
    fn perf_themes_drop_styling_and_point_at_the_bundled_assets() {
        let dir = std::env::temp_dir().join(format!("mypaper-perf-theme-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("theme.json"),
            r#"{ "name": "T", "wallpaper": { "kind": "web", "url": "index.html", "fps": 30 },
                 "windows": { "dark_mode": true } }"#,
        )
        .unwrap();
        let theme = perf_theme(&dir).unwrap();
        assert!(theme.get("windows").is_none());
        assert_eq!(
            theme["wallpaper"]["url"].as_str().unwrap(),
            dir.join("index.html").display().to_string()
        );
        assert_eq!(theme["wallpaper"]["fps"], 30);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
