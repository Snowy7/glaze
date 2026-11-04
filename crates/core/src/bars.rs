//! The shell bars child (docs/bars.md, "Core and tray"): hiding the native taskbar before the
//! bars start, asking them to stop so they restore it themselves, and replaying the taskbar
//! journal when they couldn't (killed, crashed).

use std::time::{Duration, Instant};

use mypaper_bars::taskbar::{Taskbar, Wanted};
use mypaper_bars::{BarsConfig, TaskbarMode};
use mypaper_desktop::startup;
use windows::Win32::Foundation::{CloseHandle, LPARAM, WPARAM};
use windows::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowExW, GetShellWindow, GetWindowThreadProcessId, HWND_MESSAGE, PostMessageW, WM_CLOSE,
};
use windows::core::{PCWSTR, w};

/// The class of the bars' message-only window.
const HOST_CLASS: PCWSTR = w!("mypaper.bars.host");
/// How long the bars get to restore the taskbar and exit before they're killed.
pub const GRACE: Duration = Duration::from_secs(3);
/// How long core waits for Explorer's desktop at logon before starting without it.
const SHELL_WAIT: Duration = Duration::from_secs(60);

/// At logon the scheduled task can start core before Explorer has made its desktop window,
/// which every child needs. Waits for it, checking every 25 ms; returns at once when Explorer
/// is up (every start but the logon one).
pub fn wait_for_shell() {
    // SAFETY: a read-only query.
    let up = || !unsafe { GetShellWindow() }.is_invalid();
    if up() {
        return;
    }
    let started = Instant::now();
    log::info!("waiting for Explorer's desktop");
    while !up() && started.elapsed() < SHELL_WAIT {
        std::thread::sleep(Duration::from_millis(25));
    }
    log::info!(
        "Explorer's desktop {} after {:.1?}",
        if up() { "is up" } else { "is still missing" },
        started.elapsed()
    );
}

/// When `bars.json` hides the native taskbar, hides it now, before any child starts, so it
/// never shows while the bars load (they apply the same mode again; the journal keeps the
/// originals either way). Returns whether `bars.json` says hide.
pub fn hide_taskbar_early() -> bool {
    let config = mypaper_bars::config::path().and_then(|path| BarsConfig::load(&path));
    let taskbar = match config {
        Ok(config) if config.taskbar.mode == TaskbarMode::Hide => config.taskbar,
        Ok(_) => return false,
        Err(e) => {
            log::warn!("bars.json: {e:#}");
            return false;
        }
    };
    let wanted = Wanted {
        mode: taskbar.mode,
        alignment: taskbar.alignment,
    };
    match Taskbar::native().and_then(|mut native| native.apply(wanted)) {
        Ok(()) => startup::ready("core", "taskbar_hidden_ms"),
        // No taskbar yet (Explorer is still starting): the bars hide it when it comes.
        Err(e) => log::warn!("hiding the taskbar early: {e:#}"),
    }
    true
}

/// Posts `WM_CLOSE` to a running bars host. Returns whether there was one.
pub fn ask_to_close() -> bool {
    // SAFETY: a lookup and a post; nothing is created or activated.
    unsafe {
        match FindWindowExW(Some(HWND_MESSAGE), None, HOST_CLASS, PCWSTR::null()) {
            Ok(host) => PostMessageW(Some(host), WM_CLOSE, WPARAM(0), LPARAM(0)).is_ok(),
            Err(_) => false,
        }
    }
}

/// A bars process left by an earlier run (it outlived a crashed core) would keep ours from
/// starting and fight over the taskbar: ask it to exit, and wait for it. Returns whether there
/// was one.
pub fn stop_leftover() -> bool {
    // SAFETY: a lookup, a post and a wait on a handle we open and close here.
    unsafe {
        let Ok(host) = FindWindowExW(Some(HWND_MESSAGE), None, HOST_CLASS, PCWSTR::null()) else {
            return false;
        };
        log::warn!("stopping the shell bars left by an earlier run");
        let mut pid = 0;
        GetWindowThreadProcessId(host, Some(&mut pid));
        let process = OpenProcess(PROCESS_SYNCHRONIZE, false, pid);
        let _ = PostMessageW(Some(host), WM_CLOSE, WPARAM(0), LPARAM(0));
        if let Ok(process) = process {
            WaitForSingleObject(process, GRACE.as_millis() as u32);
            let _ = CloseHandle(process);
        }
        true
    }
}

/// Puts the native taskbar back from the journal, if the bars left one.
pub fn replay_journal() {
    match mypaper_bars::replay() {
        Ok(true) => log::warn!("restored the native taskbar from the bars' journal"),
        Ok(false) => {}
        Err(e) => log::error!("restoring the native taskbar: {e:#}"),
    }
}
