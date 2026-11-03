//! mypaper-core: the supervisor. Starts the wallpaper engine, restarts it
//! when it crashes or the desktop changes, and owns the panic hotkey.
//!
//! ```text
//! mypaper-core [THEME]
//! ```
//!
//! **Panic hotkey: Ctrl+Alt+Shift+Backspace** stops everything and restores
//! the stock desktop, whatever state mypaper is in.

mod restart;

use std::os::windows::io::AsRawHandle;
use std::process::{Child, Command, ExitCode};
use std::time::Instant;

use anyhow::{Context, Result, bail};
use mypaper_desktop::DesktopHost;
use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, HANDLE, WAIT_OBJECT_0};
use windows::Win32::System::Threading::{CreateMutexW, INFINITE};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, RegisterHotKey, UnregisterHotKey, VK_BACK,
};
use windows::Win32::UI::WindowsAndMessaging::{
    MSG, MsgWaitForMultipleObjects, PM_REMOVE, PeekMessageW, QS_ALLINPUT, WM_HOTKEY, WM_QUIT,
};
use windows::core::w;

use crate::restart::{Decision, Restarts};

const PANIC_HOTKEY: i32 = 1;

fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
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
    // Resolve once, up front: a missing theme fails here instead of crash-looping
    // the child, and the child gets an absolute path whatever our working dir is.
    let theme = mypaper_theme::locate(std::env::args().nth(1).as_deref())?;
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

    let spawn = || -> Result<Child> {
        Command::new(&wallpaper_exe)
            .arg(&theme)
            .spawn()
            .context("starting the wallpaper")
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

enum Signal {
    Panic,
    Quit,
}

fn pump() -> Option<Signal> {
    let mut msg = MSG::default();
    // SAFETY: standard message pump; this thread has no windows.
    unsafe {
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            match msg.message {
                WM_HOTKEY if msg.wParam.0 == PANIC_HOTKEY as usize => return Some(Signal::Panic),
                WM_QUIT => return Some(Signal::Quit),
                _ => {}
            }
        }
    }
    None
}
