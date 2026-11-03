//! mypaper-core: the supervisor. Starts the wallpaper engine, restarts it
//! when it crashes or the desktop changes, and owns the panic hotkey.
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

mod restart;
mod tray;

use std::os::windows::io::AsRawHandle;
use std::process::{Child, Command, ExitCode};
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

use crate::restart::{Decision, Restarts};
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

    let spawn = || -> Result<Child> {
        let mut command = Command::new(&wallpaper_exe);
        if theme_override.is_some() {
            command.arg(&theme);
        }
        command.spawn().context("starting the wallpaper")
    };
    let mut child = Some(spawn()?);
    let mut restarts = Restarts::default();
    let mut restart_at: Option<Instant> = None;

    loop {
        let handle = child.as_ref().map(|c| HANDLE(c.as_raw_handle()));
        let timeout = restart_at.map_or(INFINITE, |t| {
            t.saturating_duration_since(Instant::now())
                .as_millis()
                .min(u128::from(INFINITE - 1)) as u32
        });
        let handles = handle.as_ref().map(std::slice::from_ref);
        // SAFETY: the child handle stays valid while `child` is alive.
        let woke = unsafe { MsgWaitForMultipleObjects(handles, false, timeout, QS_ALLINPUT) };

        if handle.is_some() && woke == WAIT_OBJECT_0 {
            let mut exited = child.take().context("child vanished")?;
            let code = exited.wait()?.code();
            match restarts.on_exit(code, Instant::now()) {
                Decision::Stop => {
                    log::info!("wallpaper exited cleanly");
                    break;
                }
                Decision::RestartAfter(delay) => {
                    log::warn!("wallpaper exited ({code:?}); restarting in {delay:?}");
                    restart_at = Some(Instant::now() + delay);
                }
                Decision::Broken => {
                    log::error!("wallpaper cannot start with this configuration (see error above)");
                    break;
                }
                Decision::GiveUp => {
                    log::error!("wallpaper keeps crashing; giving up (desktop restored)");
                    break;
                }
            }
        }
        if restart_at.is_some_and(|t| Instant::now() >= t) {
            restart_at = None;
            child = Some(spawn()?);
        }
        match pump() {
            Some(Signal::Panic) => {
                log::warn!("panic hotkey: stopping everything");
                break;
            }
            Some(Signal::Quit) => break,
            Some(Signal::Restart) => {
                log::info!("restarting the wallpaper");
                if let Some(mut old) = child.take() {
                    let _ = old.kill();
                    let _ = old.wait();
                }
                // A killed wallpaper cannot clean up after itself.
                if let Ok(host) = DesktopHost::find() {
                    host.restore();
                }
                restart_at = Some(Instant::now() + MANUAL_RESTART_DELAY);
            }
            None => {}
        }
    }

    if let Some(mut child) = child {
        let _ = child.kill();
        let _ = child.wait();
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

/// Drains the message queue (running the tray's window procedure) and
/// returns the most urgent request in it.
fn pump() -> Option<Signal> {
    let mut msg = MSG::default();
    let mut signal = None;
    // SAFETY: standard message pump on the thread that owns the tray window.
    unsafe {
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            let requested = match msg.message {
                WM_HOTKEY if msg.wParam.0 == PANIC_HOTKEY as usize => Signal::Panic,
                WM_QUIT => Signal::Quit,
                WM_RESTART_WALLPAPER => Signal::Restart,
                _ => {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                    continue;
                }
            };
            signal = signal.max(Some(requested));
        }
    }
    signal
}
