//! Native Windows styling for the active theme's `windows` section, using
//! supported public APIs only:
//!
//! - **Windows (DWM)**: border, title bar, corners, backdrop and dark title
//!   bar on allowlisted windows ([`dwm`]).
//! - **System**: accent color and dark mode, as the same per-user registry
//!   values the Settings app writes, announced with `WM_SETTINGCHANGE`
//!   ("ImmersiveColorSet") so Windows applies them live ([`plan`], [`registry`]).
//! - **`"auto"` colors**: taken from the wallpaper's [`palette`].
//! - **Cursors**: a cursor scheme in `HKCU\Control Panel\Cursors` plus
//!   `SPI_SETCURSORS`, with two built-in sets drawn in code ([`cursors`],
//!   [`cursor_art`]).
//! - **Classic context menu** (Win11, undocumented, opt-in): [`classic_menu`],
//!   journaled on its own because it outlives a run.
//!
//! Everything is reversible. Before the first registry change the original
//! values (including "absent") are journaled to `%LOCALAPPDATA%\glaze\undo.json`
//! ([`undo`]); [`Tweaks::deactivate`] (also run on drop) and the next
//! start after a crash both undo from it.
//!
//! [`Tweaks`] queues the latest request onto its own message-pumping worker. Registry IO,
//! settings receivers, signature checks and window events never block core's supervisor.

pub mod classic_menu;
pub mod color;
pub mod cursor_art;
pub mod cursors;
pub mod dwm;
pub mod folder_routing;
pub mod game_activity;
pub mod palette;
pub mod plan;
pub mod registry;
pub mod system;
pub mod undo;
#[doc(hidden)]
pub mod worker;

pub use plan::system_prefers_dark;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::Result;
use glaze_theme::WindowsStyle;
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

/// The app scheme before any active Glaze override, for another process resolving auto
/// tokens. Reading the same journal as core avoids a config-event race with mode restoration.
pub fn unmanaged_system_prefers_dark() -> bool {
    let original = palette::local_dir().ok().and_then(|dir| {
        Undo::new(Registry, dir.join("undo.json"), SETTLE)
            .original(plan::PERSONALIZE_KEY, plan::APPS_THEME)
            .ok()
            .flatten()
    });
    original.map_or_else(system_prefers_dark, |value| {
        value == Some(undo::Value::Dword(0))
    })
}

/// A style and the palette its `"auto"` colors were resolved with.
#[derive(Clone, PartialEq)]
struct Applied {
    style: WindowsStyle,
    palette: Option<Palette>,
    theme_dir: PathBuf,
}

/// Nonblocking Windows-style controller. Rapid requests collapse into the latest one;
/// dropping it waits for the worker to restore the desktop and exit.
pub struct Tweaks {
    worker: worker::Worker<Option<Applied>>,
    unmanaged_dark: Arc<AtomicBool>,
    forcing_mode: bool,
}

impl Tweaks {
    pub fn new() -> Result<Self> {
        let unmanaged_dark = Arc::new(AtomicBool::new(false));
        let scheme = unmanaged_dark.clone();
        let worker = worker::Worker::start(move || {
            let mut controller = Controller::new()?;
            scheme.store(controller.unmanaged_dark_mode(), Ordering::Release);
            Ok(move |request: Option<Applied>| {
                match request {
                    Some(applied) => controller.activate(
                        &applied.style,
                        applied.palette.as_ref(),
                        &applied.theme_dir,
                    ),
                    None => controller.deactivate(),
                }
                scheme.store(controller.unmanaged_dark_mode(), Ordering::Release);
            })
        })?;
        Ok(Self {
            worker,
            unmanaged_dark,
            forcing_mode: false,
        })
    }

    /// Pending releases still use the pre-override scheme, until the worker has restored it.
    pub fn unmanaged_dark_mode(&self) -> bool {
        if self.forcing_mode || !self.worker.is_idle() {
            self.unmanaged_dark.load(Ordering::Acquire)
        } else {
            system_prefers_dark()
        }
    }

    pub fn activate(&mut self, style: &WindowsStyle, palette: Option<&Palette>, theme_dir: &Path) {
        if self.worker.is_idle() && !self.forcing_mode {
            self.unmanaged_dark
                .store(system_prefers_dark(), Ordering::Release);
        }
        self.forcing_mode = style.dark_mode.is_some();
        self.worker.submit(Some(Applied {
            style: style.clone(),
            palette: palette.copied(),
            theme_dir: theme_dir.to_owned(),
        }));
    }

    pub fn deactivate(&mut self) {
        self.forcing_mode = false;
        self.worker.submit(None);
    }
}

/// Lives exclusively on the worker, including its WinEvent hook and undo journal.
struct Controller {
    system: Undo<Registry>,
    styler: Option<Styler>,
    active: Option<Applied>,
}

impl Controller {
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

    /// Windows' own app scheme before our override. Automatic styling must not inherit a
    /// previous forced mode when switching from light/dark back to auto.
    pub fn unmanaged_dark_mode(&self) -> bool {
        match self
            .system
            .original(plan::PERSONALIZE_KEY, plan::APPS_THEME)
        {
            Ok(Some(value)) => value == Some(undo::Value::Dword(0)),
            _ => system_prefers_dark(),
        }
    }

    /// Applies `style`. `palette` resolves `"auto"` colors; without one,
    /// those are skipped until [`activate`](Self::activate) is called again
    /// with it. `theme_dir` is the theme's folder, for a relative
    /// `cursors` path. Changes transition directly without tearing down the hook or windows.
    /// Failures are logged,
    /// not returned: styling is cosmetic and must never stop the wallpaper.
    pub fn activate(&mut self, style: &WindowsStyle, palette: Option<&Palette>, theme_dir: &Path) {
        let applied = Applied {
            style: style.clone(),
            palette: palette.copied(),
            theme_dir: theme_dir.to_owned(),
        };
        if self.active.as_ref() == Some(&applied) {
            return;
        }
        // The journal is written before anything is changed (even for a
        // theme that only styles windows), so a crash is always recoverable.
        let mut writes = plan::system_writes(style, palette);
        let scheme = style.cursors.as_deref().and_then(|spec| {
            cursors::resolve_default(spec, theme_dir)
                .inspect_err(|e| log::warn!("cursors not applied: {e:#}"))
                .ok()
        });
        if let Some(scheme) = &scheme {
            writes.extend(cursors::writes(scheme));
        }
        if let Err(e) = self.system.prepare(&writes) {
            log::warn!("Windows styling not applied: could not journal originals: {e:#}");
            return;
        }
        // Capture original window dark mode before a broadcast can make Windows change it.
        match &self.styler {
            Some(styler) => styler.update(style, palette),
            None => match Styler::start(style, palette) {
                Ok(styler) => self.styler = Some(styler),
                Err(e) => log::warn!("window styling not applied: {e:#}"),
            },
        }
        let had_cursors = self.system.journaled_key(cursors::CURSORS_KEY);
        match self.system.transition(&writes, || {
            broadcast_settings_changed();
            if scheme.is_some() || had_cursors {
                cursors::reload();
            }
        }) {
            Ok(_) => self.active = Some(applied),
            Err(e) => {
                // Do not suppress a retry of the same request after a partial transition.
                log::warn!("system accent/dark mode not applied: {e:#}");
                self.active = None;
            }
        }
    }

    /// Restores every window and registry value, and deletes the journal.
    /// Safe to call when nothing is active.
    pub fn deactivate(&mut self) {
        self.styler = None; // drop restores the windows
        self.active = None;
        self.undo_system();
    }

    fn undo_system(&mut self) {
        let had_cursors = self.system.journaled_key(cursors::CURSORS_KEY);
        let announce = || {
            broadcast_settings_changed();
            if had_cursors {
                cursors::reload();
            }
        };
        if let Err(e) = self.system.restore(announce) {
            log::error!("could not restore system settings: {e:#}");
        }
    }
}

impl Drop for Controller {
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
