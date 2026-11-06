//! Desktop extras (docs/desktop-extras.md) that stay in core: the opt-in Win11 "classic context
//! menu" switch, following `config.json` live, and the desktop view helpers the tray uses.
//!
//! mypaper's own right-click menu lives in the shell (`mypaper-shell`, which core supervises),
//! so core loads no Direct2D and installs no mouse hook.

use std::sync::mpsc;
use std::time::Duration;

use mypaper_config::Config;
use mypaper_desktop::desktop_view::{DesktopView, IconSize};
use mypaper_tweaks::classic_menu::{Change, ClassicMenu};
use windows::Win32::UI::WindowsAndMessaging::{
    MB_ICONINFORMATION, MB_OK, MB_SETFOREGROUND, MB_TOPMOST, MessageBoxW,
};
use windows::core::{HSTRING, w};

/// How long the tray waits for Explorer to describe the desktop before showing its
/// menu without that part (a hung Explorer must not hang the supervisor).
const VIEW_TIMEOUT: Duration = Duration::from_millis(400);

pub struct DesktopExtras {
    classic: Option<ClassicMenu>,
}

impl DesktopExtras {
    /// Applies the classic-menu switch of `config`.
    pub fn new(config: &Config) -> Self {
        let classic = ClassicMenu::new()
            .inspect_err(|e| log::warn!("classic menu switch unavailable: {e:#}"))
            .ok();
        let mut extras = Self { classic };
        extras.apply_classic(config.classic_context_menu);
        apply_folder_routing(config.experimental.files_default);
        extras
    }

    /// `config.json` changed: bring the switch in line.
    pub fn follow_config(&mut self) {
        match Config::load() {
            Ok(config) => {
                self.apply_classic(config.classic_context_menu);
                apply_folder_routing(config.experimental.files_default);
            }
            Err(e) => log::warn!("desktop extras unchanged: {e:#}"),
        }
    }

    /// Turns the classic context menu on or off to match the config. Explorer reads the
    /// setting when it starts, so a real change gets a notice (never an automatic restart).
    fn apply_classic(&mut self, wanted: bool) {
        let Some(classic) = &mut self.classic else {
            return;
        };
        match classic.set(wanted) {
            Ok(Change::Applied) => {
                log::info!("classic context menu {}", if wanted { "on" } else { "off" });
                notify_restart_explorer(wanted);
            }
            Ok(Change::Unchanged) => {}
            Err(e) => log::warn!("classic context menu not changed: {e:#}"),
        }
    }
}

/// Persistent, journaled routing needs no resident process or worker. Config changes and
/// startup reconcile it; disabling also recovers a journal left by an interrupted run.
fn apply_folder_routing(enabled: bool) {
    let result = (|| -> anyhow::Result<()> {
        let executable = std::env::current_exe()?.with_file_name("mypaper-files.exe");
        mypaper_tweaks::folder_routing::FolderRouting::new()?.set(enabled, &executable)?;
        Ok(())
    })();
    if let Err(error) = result { log::warn!("folder routing unchanged: {error:#}"); }
}

/// Tells the user the classic menu change needs an Explorer restart. Shown on its own thread
/// so core's supervisor loop is not held up by the dialog.
fn notify_restart_explorer(classic: bool) {
    let state = if classic { "on" } else { "off" };
    let text = format!(
        "The classic Windows context menu is now {state}.\n\n\
         Explorer reads this setting when it starts: restart Explorer (Task Manager, \
         Windows Explorer, Restart) or sign out and back in to see it.\n\n\
         mypaper never restarts Explorer for you."
    );
    let spawned = std::thread::Builder::new()
        .name("classic-menu-notice".into())
        .spawn(move || {
            let flags = MB_OK | MB_ICONINFORMATION | MB_SETFOREGROUND | MB_TOPMOST;
            // SAFETY: both strings outlive the call, which blocks this thread until dismissed.
            unsafe { MessageBoxW(None, &HSTRING::from(text), w!("mypaper"), flags) };
        });
    if let Err(e) = spawned {
        log::warn!("could not show the Explorer restart notice: {e}");
    }
}

/// The desktop's icon size and whether its icons are shown, for the tray's check marks.
/// `None` when Explorer does not answer in time.
pub fn desktop_state() -> Option<(IconSize, bool)> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("desktop-state".into())
        .spawn(move || {
            let state = DesktopView::open()
                .and_then(|view| Ok((view.icon_size()?, view.icons_visible()?)))
                .inspect_err(|e| log::debug!("desktop state: {e:#}"))
                .ok();
            let _ = tx.send(state);
        })
        .ok()?;
    rx.recv_timeout(VIEW_TIMEOUT).ok().flatten()
}

/// Sets the desktop's icon size (on a thread of its own: Explorer answers over COM).
pub fn set_icon_size(size: IconSize) {
    on_desktop_view("icon size", move |view| view.set_icon_size(size));
}

/// Shows or hides the desktop icons.
pub fn set_icons_visible(visible: bool) {
    on_desktop_view("desktop icons", move |view| view.set_icons_visible(visible));
}

fn on_desktop_view(
    what: &'static str,
    action: impl FnOnce(&DesktopView) -> anyhow::Result<()> + Send + 'static,
) {
    let spawned = std::thread::Builder::new()
        .name("desktop-view".into())
        .spawn(move || {
            if let Err(e) = DesktopView::open().and_then(|view| action(&view)) {
                log::warn!("could not change the {what}: {e:#}");
            }
        });
    if let Err(e) = spawned {
        log::warn!("could not change the {what}: {e}");
    }
}
