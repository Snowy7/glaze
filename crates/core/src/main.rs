//! mypaper-core: the supervisor. Starts the wallpaper engine, the widget
//! host and the shell bars, restarts them when they crash or the desktop
//! changes, and owns the panic hotkey.
//!
//! ```text
//! mypaper-core [THEME]
//! ```
//!
//! The wallpaper follows `config.json` on its own (theme, pausing), so core
//! passes it nothing; an explicit `THEME` is forwarded as a run-only override.
//! Core owns the tray icon (pause, themes, studio, restart, quit).
//!
//! **Panic hotkey: Ctrl+Alt+Shift+Backspace** stops everything and restores
//! the stock desktop, whatever state mypaper is in.

// No console window in release builds; logs go to a file (see init_logging).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod bars;
mod child;
mod desktop_extras;
mod restart;
mod styling;
mod tray;

use std::process::ExitCode;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use mypaper_config::Config;
use mypaper_desktop::DesktopHost;
use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, HANDLE, WAIT_OBJECT_0};
use windows::Win32::System::Threading::{CreateMutexW, INFINITE};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, RegisterHotKey, UnregisterHotKey, VK_BACK,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, MSG, MsgWaitForMultipleObjects, PM_REMOVE, PeekMessageW, QS_ALLINPUT,
    TranslateMessage, WM_HOTKEY, WM_QUIT,
};
use windows::core::w;

use crate::child::{Outcome, Supervised};
use crate::desktop_extras::DesktopExtras;
use crate::styling::{Styling, WM_STYLE_CONFIG, WM_STYLE_PALETTE};
use crate::tray::{Tray, WM_RESTART_WALLPAPER};

/// Pause before relaunching after "Restart wallpaper".
const MANUAL_RESTART_DELAY: Duration = Duration::from_millis(250);

const PANIC_HOTKEY: i32 = 1;

fn main() -> ExitCode {
    mypaper_desktop::init_logging();
    mypaper_desktop::quit_on_ctrl_c();
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            log::error!("{e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    // Validate once, up front: a missing theme fails here with a clear error
    // instead of crash-looping the child. The child reads the config itself;
    // only an explicit override is forwarded, as an absolute path.
    let config = Config::load().context("config.json")?;
    let theme_override = std::env::args().nth(1);
    let theme = mypaper_theme::locate(Some(theme_override.as_deref().unwrap_or(&config.theme)))
        .context("theme to start with")?;
    log::info!("theme folder: {}", theme.display());
    let wallpaper_exe = std::env::current_exe()?.with_file_name("mypaper-wallpaper.exe");
    if !wallpaper_exe.exists() {
        bail!(
            "{} not found; build the workspace first (cargo build)",
            wallpaper_exe.display()
        );
    }

    // SAFETY: named mutex we hold for the lifetime of the process.
    let _instance = unsafe { CreateMutexW(None, true, w!("Local\\mypaper-core"))? };
    // SAFETY: reads the calling thread's last error.
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        bail!("mypaper is already running");
    }
    // SAFETY: thread-level hotkey; WM_HOTKEY arrives in our message queue.
    unsafe {
        RegisterHotKey(
            None,
            PANIC_HOTKEY,
            MOD_CONTROL | MOD_ALT | MOD_SHIFT | MOD_NOREPEAT,
            VK_BACK.0.into(),
        )
        .context("panic hotkey Ctrl+Alt+Shift+Backspace is taken by another app")?;
    }
    log::info!("running; panic hotkey: Ctrl+Alt+Shift+Backspace");

    // Failing to show a tray icon must not stop the wallpaper.
    let _tray = Tray::new()
        .inspect_err(|e| log::warn!("no tray icon: {e:#}"))
        .ok();

    // Native Windows styling of the active theme. Created after the
    // single-instance check: it undoes a crashed run's leftovers, which a
    // second instance must never do to a live one. Dropping it (any exit
    // path) restores everything.
    let mut styling = Styling::new(&config.theme)
        .inspect_err(|e| log::warn!("no window styling: {e:#}"))
        .ok();
    if let Some(styling) = &mut styling {
        styling.set_theme(&theme.to_string_lossy());
    }

    // The desktop right-click menu and the classic-menu switch, both opt-in via config.json
    // (docs/desktop-extras.md). Nothing is installed unless asked for.
    let mut extras = DesktopExtras::new(&config);

    // The same for the native taskbar: a run that crashed may have left it hidden. Put it back
    // before the bars start (they apply their mode again from scratch).
    bars::stop_leftover();
    bars::replay_journal();

    // The wallpaper is essential: when it stops for good, so does mypaper.
    // The widget host is optional: if it is missing or gives up, mypaper
    // keeps running without widgets.
    let wallpaper_args = if theme_override.is_some() {
        vec![theme.clone()]
    } else {
        Vec::new()
    };
    let mut wallpaper = Supervised::new("wallpaper", wallpaper_exe, wallpaper_args);
    wallpaper.start()?;
    let widgets_exe = std::env::current_exe()?.with_file_name("mypaper-widgets.exe");
    let mut widgets = widgets_exe
        .exists()
        .then(|| Supervised::new("widget host", widgets_exe, Vec::new()));
    if let Some(host) = &mut widgets
        && let Err(e) = host.start()
    {
        log::warn!("{e:#}; continuing without widgets");
        widgets = None;
    }
    // The shell bars are optional too (docs/bars.md). When they stop for good, the taskbar
    // journal is replayed so the native taskbar is never left hidden.
    let bars_exe = std::env::current_exe()?.with_file_name("mypaper-bars.exe");
    let mut shell_bars = bars_exe
        .exists()
        .then(|| Supervised::new("shell bars", bars_exe, Vec::new()));
    if let Some(child) = &mut shell_bars
        && let Err(e) = child.start()
    {
        log::warn!("{e:#}; continuing without the shell bars");
        shell_bars = None;
    }

    loop {
        let children = [Some(&wallpaper), widgets.as_ref(), shell_bars.as_ref()];
        let handles: Vec<HANDLE> = children
            .iter()
            .flatten()
            .filter_map(|c| c.handle())
            .collect();
        let owners: Vec<&'static str> = children
            .iter()
            .flatten()
            .filter(|c| c.handle().is_some())
            .map(|c| c.name)
            .collect();
        let next_restart = children
            .iter()
            .flatten()
            .filter_map(|c| c.restart_at())
            .min();
        let timeout = next_restart.map_or(INFINITE, |t| {
            t.saturating_duration_since(Instant::now())
                .as_millis()
                .min(u128::from(INFINITE - 1)) as u32
        });
        let wait_on = (!handles.is_empty()).then_some(handles.as_slice());
        // SAFETY: each handle stays valid while its child is alive (owned by `Supervised`).
        let woke = unsafe { MsgWaitForMultipleObjects(wait_on, false, timeout, QS_ALLINPUT) };

        let exited = (woke.0.wrapping_sub(WAIT_OBJECT_0.0) as usize) < handles.len();
        if exited {
            let index = woke.0.wrapping_sub(WAIT_OBJECT_0.0) as usize;
            match owners[index] {
                name if name == wallpaper.name => {
                    if wallpaper.on_exit()? == Outcome::Stopped {
                        break;
                    }
                }
                name if widgets.as_ref().is_some_and(|w| w.name == name) => {
                    if let Some(host) = &mut widgets
                        && host.on_exit()? == Outcome::Stopped
                    {
                        widgets = None;
                    }
                }
                _ => {
                    if let Some(child) = &mut shell_bars
                        && child.on_exit()? == Outcome::Stopped
                    {
                        shell_bars = None;
                        bars::replay_journal();
                    }
                }
            }
        }
        wallpaper.restart_if_due()?;
        if let Some(host) = &mut widgets
            && let Err(e) = host.restart_if_due()
        {
            log::warn!("{e:#}; continuing without widgets");
            widgets = None;
        }
        if let Some(child) = &mut shell_bars
            && let Err(e) = child.restart_if_due()
        {
            log::warn!("{e:#}; continuing without the shell bars");
            shell_bars = None;
            bars::replay_journal();
        }
        let pumped = pump();
        match pumped.signal {
            Some(Signal::Panic) => {
                log::warn!("panic hotkey: stopping everything");
                break;
            }
            Some(Signal::Quit) => break,
            Some(Signal::Restart) => {
                log::info!("restarting the wallpaper");
                wallpaper.restart_after(MANUAL_RESTART_DELAY);
                // A killed wallpaper cannot clean up after itself.
                if let Ok(host) = DesktopHost::find() {
                    host.restore();
                }
            }
            None => {}
        }
        if let Some(styling) = &mut styling {
            if pumped.config_changed {
                styling.follow_config();
            }
            if pumped.palette_changed {
                styling.palette_changed();
            }
        }
        // The menu's look follows the theme (and its palette), so both signals count.
        if pumped.config_changed || pumped.palette_changed {
            extras.follow_config();
        }
    }

    // The bars first: they put the native taskbar back as they exit. If they had to be killed,
    // or crashed earlier, the journal does it.
    if let Some(child) = &mut shell_bars {
        child.stop_gracefully(bars::ask_to_close, bars::GRACE);
    }
    bars::replay_journal();
    if let Some(host) = &mut widgets {
        host.stop();
    }
    wallpaper.stop();
    extras.stop();
    if let Some(styling) = &mut styling {
        styling.stop();
    }
    if let Ok(host) = DesktopHost::find() {
        host.restore();
    }
    // SAFETY: unregisters the hotkey this thread registered.
    let _ = unsafe { UnregisterHotKey(None, PANIC_HOTKEY) };
    log::info!("desktop restored");
    Ok(())
}

/// What the message queue asked of the supervisor, least urgent first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Signal {
    Restart,
    Quit,
    Panic,
}

/// What one drain of the message queue found.
#[derive(Debug, Default)]
struct Pumped {
    /// The most urgent request in the queue.
    signal: Option<Signal>,
    /// `config.json` changed: the theme (and its styling) may have too.
    config_changed: bool,
    /// The wallpaper wrote a new `palette.json`.
    palette_changed: bool,
}

/// Drains the message queue (running the tray's window procedure and any
/// window-event callbacks of the styling).
fn pump() -> Pumped {
    let mut msg = MSG::default();
    let mut pumped = Pumped::default();
    // SAFETY: standard message pump on the thread that owns the tray window.
    unsafe {
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            let requested = match msg.message {
                WM_HOTKEY if msg.wParam.0 == PANIC_HOTKEY as usize => Signal::Panic,
                WM_QUIT => Signal::Quit,
                WM_RESTART_WALLPAPER => Signal::Restart,
                WM_STYLE_CONFIG => {
                    pumped.config_changed = true;
                    continue;
                }
                WM_STYLE_PALETTE => {
                    pumped.palette_changed = true;
                    continue;
                }
                _ => {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                    continue;
                }
            };
            pumped.signal = pumped.signal.max(Some(requested));
        }
    }
    pumped
}
