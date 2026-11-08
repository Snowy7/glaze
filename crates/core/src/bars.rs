//! Native-shell lifetime and taskbar recovery. Core never touches the journal while the shell
//! owns its journal lease. A replacement paints first and waits for core's replay to finish.

use std::time::{Duration, Instant};

use windows::Win32::Foundation::{CloseHandle, LPARAM, WPARAM};
use windows::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowExW, GetShellWindow, GetWindowThreadProcessId, HWND_MESSAGE, PostMessageW, WM_CLOSE,
};
use windows::core::{PCWSTR, w};

/// The class of the bars' message-only window.
const HOST_CLASS: PCWSTR = w!("glaze.shell.inbox");
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
pub fn stop_leftover() -> anyhow::Result<bool> {
    // SAFETY: a lookup, a post and a wait on a handle we open and close here.
    unsafe {
        let Ok(host) = FindWindowExW(Some(HWND_MESSAGE), None, HOST_CLASS, PCWSTR::null()) else {
            return Ok(false);
        };
        log::warn!("stopping the shell bars left by an earlier run");
        let mut pid = 0;
        GetWindowThreadProcessId(host, Some(&mut pid));
        let process = OpenProcess(PROCESS_SYNCHRONIZE, false, pid)?;
        let _ = PostMessageW(Some(host), WM_CLOSE, WPARAM(0), LPARAM(0));
        {
            let exited = WaitForSingleObject(process, GRACE.as_millis() as u32);
            let _ = CloseHandle(process);
            anyhow::ensure!(
                exited == windows::Win32::Foundation::WAIT_OBJECT_0,
                "the previous shell has not exited; refusing to race its taskbar journal"
            );
        }
        Ok(true)
    }
}

/// Puts the native taskbar back from the journal, if the bars left one.
pub fn replay_journal() {
    match glaze_bars::replay() {
        Ok(true) => log::warn!("restored the native taskbar from the bars' journal"),
        Ok(false) => {}
        Err(e) => log::error!("restoring the native taskbar: {e:#}"),
    }
}
