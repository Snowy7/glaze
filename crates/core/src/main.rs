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

mod autostart;
mod bars;
mod child;
mod restart;
mod styling;
mod tray;

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use mypaper_config::Config;
use mypaper_desktop::{DesktopHost, startup};
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
use crate::styling::{Styling, WM_STYLE_CONFIG, WM_STYLE_PALETTE};
use crate::tray::{Tray, WM_RESTART_WALLPAPER};

/// Pause before relaunching after "Restart wallpaper".
const MANUAL_RESTART_DELAY: Duration = Duration::from_millis(250);

const PANIC_HOTKEY: i32 = 1;

fn main() -> ExitCode {
    // `--autostart on|off|status` configures "Start with Windows" and exits (see autostart.rs).
    if let Some(code) = autostart::cli(std::env::args().skip(1)) {
        return code;
    }
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

/// Startup order (docs/performance.md, "Startup"): the children start first, so they work
/// while core sets up its own tray and styling. Before the spawn, core only checks it's the one
/// instance, takes the panic hotkey, waits for Explorer if it isn't up yet, and hides the native
/// taskbar when the bars will (so it never shows once core runs).
fn run() -> Result<()> {
    startup::ready("core", "main_ms");
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
    let exe = std::env::current_exe()?;
    let wallpaper_exe = exe.with_file_name("mypaper-wallpaper.exe");
    if !wallpaper_exe.exists() {
        bail!(
            "{} not found; build the workspace first (cargo build)",
            wallpaper_exe.display()
        );
    }
    let bars_exe = exe.with_file_name("mypaper-bars.exe");
    bars::wait_for_shell();

    // The native taskbar: hidden right away when the bars hide it. Otherwise a run that crashed
    // may have left it hidden: put it back before the bars start (they apply their mode again).
    let hidden_early = bars_exe.exists() && bars::hide_taskbar_early();
    if bars::stop_leftover() && hidden_early {
        bars::hide_taskbar_early(); // the leftover showed it again as it exited
    }
    if !hidden_early {
        bars::replay_journal();
    }

    // The wallpaper is essential: when it stops for good, so does mypaper. It reads the config
    // itself and exits with EXIT_CONFIG on a bad one, which stops core too. Only an explicit
    // theme override is forwarded, as an absolute path.
    let theme_override = std::env::args().nth(1);
    let wallpaper_args = match &theme_override {
        Some(name) => vec![mypaper_theme::locate(Some(name)).context("theme to start with")?],
        None => Vec::new(),
    };
    let mut wallpaper = Supervised::new("wallpaper", wallpaper_exe, wallpaper_args);
    wallpaper.start()?;
    // The widget host and the shell bars (docs/bars.md) are optional: if one is missing or gives
    // up, mypaper keeps running without it. When the bars stop for good, the taskbar journal is
    // replayed so the native taskbar is never left hidden.
    let mut widgets = start_optional("widget host", exe.with_file_name("mypaper-widgets.exe"));
    let mut shell_bars = start_optional("shell bars", bars_exe);
    startup::ready("core", "spawn_ms");
    log::info!("running; panic hotkey: Ctrl+Alt+Shift+Backspace");

    // Failing to show a tray icon must not stop the wallpaper.
    let _tray = Tray::new()
        .inspect_err(|e| log::warn!("no tray icon: {e:#}"))
        .ok();

    // Native Windows styling of the active theme. Created after the single-instance check: it
    // undoes a crashed run's leftovers, which a second instance must never do to a live one.
    // Dropping it (any exit path) restores everything.
    let config = Config::load()
        .inspect_err(|e| log::warn!("config.json: {e:#}"))
        .unwrap_or_default();
    let theme = theme_override.as_deref().unwrap_or(&config.theme);
    log::info!("theme: {theme}");
    let mut styling = Styling::new(&config.theme)
        .inspect_err(|e| log::warn!("no window styling: {e:#}"))
        .ok();
    if let Some(styling) = &mut styling {
        styling.set_theme(theme);
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

/// Starts an optional child when its executable is there. `None` when it's missing or didn't
/// start.
fn start_optional(name: &'static str, exe: PathBuf) -> Option<Supervised> {
    if !exe.exists() {
        return None;
    }
    let mut child = Supervised::new(name, exe, Vec::new());
    match child.start() {
        Ok(()) => Some(child),
        Err(e) => {
            log::warn!("{e:#}; continuing without the {name}");
            None
        }
    }
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
