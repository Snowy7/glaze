//! Live configuration: follows `config.json` and the theme folder while the
//! wallpaper runs, so a theme switch or pause toggle needs no restart.

use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::Result;
use mypaper_config::Config;
use mypaper_desktop::{ConfigWatcher, watch_config};
use mypaper_fswatch::Watch;
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::PostThreadMessageW;

use crate::engine::Engine;
use crate::window::{WM_CONFIG, WM_RELOAD};

/// Coalesces the burst of events one config save produces.
const CONFIG_DEBOUNCE: Duration = Duration::from_millis(100);

/// What a config change asks of the engine.
#[derive(Debug, PartialEq, Eq)]
enum Action {
    SwitchTheme(String),
    UpdateRenderer,
    /// Paused / pause-on-battery / pause-on-fullscreen changed.
    UpdateHold,
    /// `static_snapshot` was switched; off puts the user's wallpaper back right away.
    SetSnapshot(bool),
}

/// The actions that take the engine from `old` to `new`.
fn actions(old: &Config, new: &Config) -> Vec<Action> {
    let mut actions = Vec::new();
    if old.engine != new.engine {
        actions.push(Action::UpdateRenderer);
    }
    if old.theme != new.theme {
        actions.push(Action::SwitchTheme(new.theme.clone()));
    }
    if (
        old.paused,
        old.pause_on_battery,
        old.pause_on_fullscreen,
        old.engine.during_games,
    ) != (
        new.paused,
        new.pause_on_battery,
        new.pause_on_fullscreen,
        new.engine.during_games,
    ) {
        actions.push(Action::UpdateHold);
    }
    if old.static_snapshot != new.static_snapshot {
        actions.push(Action::SetSnapshot(new.static_snapshot));
    }
    actions
}

/// Applies config changes to the engine and keeps the file watches.
pub struct Live {
    /// The last config applied. A `THEME` command-line override stays in
    /// effect until the config's theme changes from what it was at startup.
    config: Config,
    apply_at: Option<Instant>,
    theme_watch: Option<Watch>,
    _config_watch: ConfigWatcher,
}

impl Live {
    /// Starts following `config.json` and the folder of the theme in use.
    pub fn new(config: Config, theme_dir: &Path) -> Result<Self> {
        // SAFETY: no preconditions.
        let thread = unsafe { GetCurrentThreadId() };
        let config_watch = watch_config(move || post(thread, WM_CONFIG))?;
        Ok(Self {
            config,
            apply_at: None,
            theme_watch: watch_theme(theme_dir),
            _config_watch: config_watch,
        })
    }

    /// The config file changed; apply it after the debounce.
    pub fn schedule(&mut self) {
        self.apply_at
            .get_or_insert(Instant::now() + CONFIG_DEBOUNCE);
    }

    pub fn deadline(&self) -> Option<Instant> {
        self.apply_at
    }

    /// Applies a pending config change once its debounce has passed.
    pub fn apply_due(&mut self, engine: &mut Engine) {
        // Studio can ask before the config watch/debounce fires. Read its saved selection
        // before answering, or an old wallpaper would be cached under the new geometry.
        if engine.preview_requested() {
            self.apply_at = Some(Instant::now());
        }
        if !self.apply_at.is_some_and(|t| Instant::now() >= t) {
            return;
        }
        self.apply_at = None;
        let new = match Config::load() {
            Ok(config) => config,
            Err(e) => return log::error!("{e:#}\n(keeping the current settings)"),
        };
        for action in actions(&self.config, &new) {
            match action {
                Action::UpdateRenderer => engine.apply_renderer_config(&new.engine),
                Action::SwitchTheme(name) => self.switch_theme(engine, &name),
                Action::UpdateHold => engine.apply_config(&new),
                Action::SetSnapshot(on) => engine.set_static_snapshot(on),
            }
        }
        self.config = new;
    }

    /// Reloads the engine onto another theme and follows its folder. On
    /// failure the current theme keeps running.
    fn switch_theme(&mut self, engine: &mut Engine, name: &str) {
        let switched = mypaper_theme::locate(Some(name)).and_then(|dir| {
            engine.switch_theme(&dir)?;
            Ok(dir)
        });
        match switched {
            Ok(dir) => self.theme_watch = watch_theme(&dir),
            Err(e) => {
                log::error!("cannot switch to theme {name:?}: {e:#}\n(keeping the current one)")
            }
        }
    }
}

/// Wakes the main thread with `WM_RELOAD` whenever a theme file changes, including descendants
/// and recovery after a folder replacement. No wakes while idle.
fn watch_theme(dir: &Path) -> Option<Watch> {
    // SAFETY: no preconditions.
    let thread = unsafe { GetCurrentThreadId() };
    Watch::directory(dir, true, move |_| post(thread, WM_RELOAD))
        .inspect_err(|e| log::warn!("not watching {} for changes: {e:#}", dir.display()))
        .ok()
}

fn post(thread: u32, message: u32) {
    // SAFETY: posting to a thread id is always safe; failure means it is gone.
    let _ = unsafe { PostThreadMessageW(thread, message, WPARAM(0), LPARAM(0)) };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(f: impl FnOnce(&mut Config)) -> Config {
        let mut config = Config::default();
        f(&mut config);
        config
    }

    #[test]
    fn identical_configs_ask_for_nothing() {
        assert!(actions(&Config::default(), &Config::default()).is_empty());
    }

    #[test]
    fn renderer_changes_are_applied_before_a_simultaneous_theme_switch() {
        let new = config(|c| {
            c.engine.fps = Some(24);
            c.theme = "rain-city".into();
        });
        assert_eq!(
            actions(&Config::default(), &new),
            [
                Action::UpdateRenderer,
                Action::SwitchTheme("rain-city".into())
            ]
        );
    }

    #[test]
    fn theme_change_switches_theme_only() {
        let new = config(|c| c.theme = "photo".into());
        assert_eq!(
            actions(&Config::default(), &new),
            [Action::SwitchTheme("photo".into())]
        );
    }

    #[test]
    fn each_pause_setting_updates_hold() {
        for new in [
            config(|c| c.paused = true),
            config(|c| c.pause_on_battery = false),
            config(|c| c.pause_on_fullscreen = false),
        ] {
            assert_eq!(actions(&Config::default(), &new), [Action::UpdateHold]);
        }
    }

    #[test]
    fn switching_the_static_snapshot_is_its_own_action() {
        let new = config(|c| c.static_snapshot = false);
        assert_eq!(
            actions(&Config::default(), &new),
            [Action::SetSnapshot(false)]
        );
    }

    #[test]
    fn simultaneous_changes_do_both() {
        let new = config(|c| {
            c.theme = "photo".into();
            c.paused = true;
        });
        assert_eq!(
            actions(&Config::default(), &new),
            [Action::SwitchTheme("photo".into()), Action::UpdateHold]
        );
    }
}
