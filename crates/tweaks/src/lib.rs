//! Native Windows styling for the active theme's `windows` section, using
//! supported public APIs only:
//!
//! - **Windows (DWM)**: border, title bar, corners, backdrop and dark title
//!   bar on allowlisted windows ([`dwm`]).
//! - **System**: accent color and dark mode, as the same per-user registry
//!   values the Settings app writes, announced with `WM_SETTINGCHANGE`
//!   ("ImmersiveColorSet") so Windows applies them live ([`plan`], [`registry`]).
//! - **`"auto"` colors**: taken from the wallpaper's [`palette`].
//!
//! Everything is reversible. Before the first registry change the original
//! values (including "absent") are journaled to `%LOCALAPPDATA%\mypaper\undo.json`
//! ([`undo`]); [`Tweaks::deactivate`] (also run on drop) and the next
//! start after a crash both undo from it.
//!
//! [`Tweaks`] must live on a thread that pumps messages (window events are
//! delivered through its queue), like core's main thread.

pub mod color;
pub mod dwm;
pub mod palette;
pub mod plan;
pub mod registry;
pub mod undo;

use std::time::Duration;

use anyhow::Result;
use mypaper_theme::WindowsStyle;
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    HWND_BROADCAST, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_SETTINGCHANGE,
};
use windows::core::HSTRING;

use crate::dwm::Styler;
use crate::palette::Palette;
use crate::registry::Registry;
use crate::undo::Undo;

/// How long to wait for each top-level window to take the settings broadcast.
const BROADCAST_TIMEOUT_MS: u32 = 500;

/// Pause between restore passes: Windows rewrites related accent values a
/// few milliseconds after we change one, and the restore waits that out.
const SETTLE: Duration = Duration::from_millis(100);

/// A style and the palette its `"auto"` colors were resolved with.
#[derive(PartialEq)]
struct Applied {
    style: WindowsStyle,
    palette: Option<Palette>,
}

/// Applies a theme's Windows styling and takes it back.
pub struct Tweaks {
    system: Undo<Registry>,
    styler: Option<Styler>,
    active: Option<Applied>,
}

impl Tweaks {
    /// Creates the controller. If a journal is left over from a run that
    /// crashed, that run's changes are undone first.
    pub fn new() -> Result<Self> {
        let journal = palette::local_dir()?.join("undo.json");
        let mut tweaks = Self {
            system: Undo::new(Registry, journal, SETTLE),
            styler: None,
            active: None,
        };
        if tweaks.system.pending() {
            log::warn!("undoing Windows styling left behind by an unclean exit");
            // Did the crashed run force dark mode? Then its windows may be
            // stuck dark (or light) and need the system's value back.
            let forced_dark = tweaks
                .system
                .journaled(plan::PERSONALIZE_KEY, plan::APPS_THEME);
            tweaks.undo_system();
            dwm::reset_all(forced_dark.then(plan::system_prefers_dark));
        }
        Ok(tweaks)
    }

    /// Applies `style`. `palette` resolves `"auto"` colors; without one,
    /// those are skipped until [`activate`](Self::activate) is called again
    /// with it. Calling again with the same style only updates colors, and a
    /// different style first restores everything. Failures are logged, not
    /// returned: styling is cosmetic and must never stop the wallpaper.
    pub fn activate(&mut self, style: &WindowsStyle, palette: Option<&Palette>) {
        let applied = Applied {
            style: style.clone(),
            palette: palette.copied(),
        };
        if self.active.as_ref() == Some(&applied) {
            return;
        }
        if self.active.as_ref().map(|a| &a.style) != Some(style) {
            self.deactivate();
        }
        // The journal is written before anything is changed (even for a
        // theme that only styles windows), so a crash is always recoverable.
        match self.system.apply(&plan::system_writes(style, palette)) {
            Ok(true) => broadcast_settings_changed(),
            Ok(false) => {}
            Err(e) => log::warn!("system accent/dark mode not applied: {e:#}"),
        }
        match &self.styler {
            Some(styler) => styler.restyle(palette),
            None => match Styler::start(style, palette) {
                Ok(styler) => self.styler = Some(styler),
                Err(e) => log::warn!("window styling not applied: {e:#}"),
            },
        }
        self.active = Some(applied);
    }

    /// Restores every window and registry value, and deletes the journal.
    /// Safe to call when nothing is active.
    pub fn deactivate(&mut self) {
        self.styler = None; // drop restores the windows
        self.active = None;
        self.undo_system();
    }

    fn undo_system(&mut self) {
        if let Err(e) = self.system.restore(broadcast_settings_changed) {
            log::error!("could not restore system settings: {e:#}");
        }
    }
}

impl Drop for Tweaks {
    fn drop(&mut self) {
        self.deactivate();
    }
}

/// Tells running apps and the shell that colors and theme changed, the way
/// the Settings app does after it writes them.
fn broadcast_settings_changed() {
    let area = HSTRING::from("ImmersiveColorSet");
    // SAFETY: `area` is a NUL-terminated wide string that outlives the call,
    // which waits for (or times out on) every receiver.
    unsafe {
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            WPARAM(0),
            LPARAM(area.as_ptr() as isize),
            SMTO_ABORTIFHUNG,
            BROADCAST_TIMEOUT_MS,
            None,
        );
    }
}
