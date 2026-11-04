//! The shell bars child (docs/bars.md, "Core and tray"): asking it to stop so it restores the
//! native taskbar itself, and replaying the taskbar journal when it couldn't (killed, crashed).

use std::time::Duration;

use windows::Win32::Foundation::{CloseHandle, LPARAM, WPARAM};
use windows::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowExW, GetWindowThreadProcessId, HWND_MESSAGE, PostMessageW, WM_CLOSE,
};
use windows::core::{PCWSTR, w};

/// The class of the bars' message-only window.
const HOST_CLASS: PCWSTR = w!("mypaper.bars.host");
/// How long the bars get to restore the taskbar and exit before they're killed.
pub const GRACE: Duration = Duration::from_secs(3);

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
/// starting and fight over the taskbar: ask it to exit, and wait for it.
pub fn stop_leftover() {
    // SAFETY: a lookup, a post and a wait on a handle we open and close here.
    unsafe {
        let Ok(host) = FindWindowExW(Some(HWND_MESSAGE), None, HOST_CLASS, PCWSTR::null()) else {
            return;
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
