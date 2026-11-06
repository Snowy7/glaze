//! Native Windows styling for the active theme: keeps `mypaper-tweaks`
//! in step with `config.json` (which theme) and `palette.json` (what the
//! wallpaper's `"auto"` colors resolve to). Both files are watched, so core
//! stays idle until one changes.
//!
//! The wallpaper rewrites `palette.json` every time it (re)loads a theme,
//! including hot reloads after a file edit, so that signal also re-reads the
//! theme's `windows` section: edits to it apply live.

use std::path::PathBuf;

use anyhow::{Context, Result};
use mypaper_config::Config;
use mypaper_desktop::{ConfigWatcher, watch_config};
use mypaper_theme::{LoadedTheme, WindowsStyle};
use mypaper_tweaks::Tweaks;
use mypaper_tweaks::palette::{self, PaletteWatcher};
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{PostThreadMessageW, WM_APP};

/// Posted to the supervisor thread when `config.json` changes.
pub const WM_STYLE_CONFIG: u32 = WM_APP + 4;
/// Posted to the supervisor thread when `palette.json` changes.
pub const WM_STYLE_PALETTE: u32 = WM_APP + 5;

pub struct Styling {
    tweaks: Tweaks,
    /// The folder of the theme being styled, and the styling in effect: the user's look
    /// (`look.windows`, D2) over the theme's `windows` section.
    theme: Option<(PathBuf, Option<WindowsStyle>)>,
    /// The user's look, as last read from `config.json`.
    look: mypaper_config::Look,
    /// The config's theme the last time we looked. Like the wallpaper, we
    /// only switch when this changes, so a `THEME` override on the command
    /// line stays in effect through unrelated config edits.
    config_theme: String,
    // Dropped with `Styling`: stops the watches.
    _config: Option<ConfigWatcher>,
    _palette: Option<PaletteWatcher>,
}

impl Styling {
    /// Undoes any styling a crashed run left behind, then starts watching.
    /// `config_theme` is the config's current theme name. The watchers post
    /// thread messages to the calling thread, which must handle
    /// [`WM_STYLE_CONFIG`] and [`WM_STYLE_PALETTE`].
    pub fn new(config_theme: &str) -> Result<Self> {
        let tweaks = Tweaks::new().context("undo journal")?;
        // SAFETY: no preconditions.
        let thread = unsafe { GetCurrentThreadId() };
        let post = move |message: u32| {
            move || {
                // SAFETY: posting to a thread id is always safe; failure means it is gone.
                let _ = unsafe { PostThreadMessageW(thread, message, WPARAM(0), LPARAM(0)) };
            }
        };
        Ok(Self {
            tweaks,
            theme: None,
            look: Config::load().map(|c| c.look).unwrap_or_default(),
            config_theme: config_theme.to_owned(),
            _config: watch_config(post(WM_STYLE_CONFIG))
                .inspect_err(|e| log::warn!("styling will not follow config changes: {e:#}"))
                .ok(),
            _palette: palette::watch(post(WM_STYLE_PALETTE))
                .inspect_err(|e| log::warn!("\"auto\" colors will not follow the wallpaper: {e:#}"))
                .ok(),
        })
    }

    /// Styles for the theme `name` (a name or folder, as in the config). A
    /// theme that fails to load leaves the current styling in place, like
    /// the wallpaper does.
    pub fn set_theme(&mut self, name: &str) {
        match mypaper_theme::locate(Some(name)).and_then(LoadedTheme::load) {
            Ok(loaded) => self.use_theme(loaded),
            Err(e) => log::warn!("styling unchanged: {e:#}"),
        }
    }

    /// `config.json` changed: switch if its theme did.
    pub fn follow_config(&mut self) {
        match Config::load() {
            Ok(config) if config.theme != self.config_theme => {
                self.look = config.look;
                self.set_theme(&config.theme);
                self.config_theme = config.theme;
            }
            Ok(config) if config.look != self.look => {
                // The user's own Windows styling changed (D2): re-layer it over the theme's.
                self.look = config.look;
                self.palette_changed();
            }
            Ok(_) => {}
            Err(e) => log::warn!("styling unchanged: {e:#}"),
        }
    }

    /// The wallpaper wrote a palette (it loaded or reloaded its theme):
    /// re-read the theme's styling and re-resolve the `"auto"` colors.
    pub fn palette_changed(&mut self) {
        let Some((dir, _)) = &self.theme else { return };
        match LoadedTheme::load(dir) {
            Ok(loaded) => self.use_theme(loaded),
            // Mid-edit: the next write will fix it.
            Err(e) => log::warn!("styling unchanged: {e:#}"),
        }
    }

    /// Restores everything. Also happens on drop, so every exit path of
    /// the supervisor undoes styling.
    pub fn stop(&mut self) {
        self.theme = None;
        self.tweaks.deactivate();
    }

    fn use_theme(&mut self, loaded: LoadedTheme) {
        let palette = palette::read()
            .filter(|snapshot| snapshot.is_for(&loaded.dir))
            .map(|snapshot| snapshot.palette);
        let resolved = mypaper_theme::windows::resolve(
            &self.look,
            Some(&loaded.theme),
            palette,
            self.tweaks.unmanaged_dark_mode(),
        );
        self.theme = Some((loaded.dir, resolved.windows));
        self.apply();
    }

    fn apply(&mut self) {
        match &self.theme {
            Some((dir, Some(style))) => {
                // Only a palette computed for this very theme counts.
                let palette = palette::read()
                    .filter(|snapshot| snapshot.is_for(dir))
                    .map(|snapshot| snapshot.palette);
                self.tweaks.activate(style, palette.as_ref(), dir);
            }
            _ => self.tweaks.deactivate(),
        }
    }
}
