//! Win32 plumbing shared by mypaper processes. Everything is event-driven,
//! with no polling threads, so an idle mypaper costs ~0% CPU.

mod cover;
mod host;
mod monitors;

pub use cover::{WindowEvents, covered_monitors};
pub use host::DesktopHost;
pub use monitors::{Rect, monitors};

use std::sync::atomic::{AtomicU32, Ordering};

/// Process exit code meaning "the desktop changed under me (Explorer
/// restarted, monitors changed); start me again". The supervisor relaunches.
pub const EXIT_RESTART: u8 = 3;

/// Process exit code meaning "my configuration is broken (bad arguments,
/// missing or invalid theme)". Restarting cannot help, so the supervisor stops.
pub const EXIT_CONFIG: u8 = 2;

use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::System::Console::SetConsoleCtrlHandler;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::WindowsAndMessaging::{PostThreadMessageW, WM_QUIT};
use windows::core::BOOL;

/// Physical-pixel coordinates everywhere. Call first thing in `main`.
pub fn enable_dpi_awareness() {
    let context = DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2;
    // SAFETY: no preconditions; fails harmlessly if already set.
    if let Err(e) = unsafe { SetProcessDpiAwarenessContext(context) } {
        log::debug!("DPI awareness already set: {e}");
    }
}

static QUIT_THREAD: AtomicU32 = AtomicU32::new(0);

/// Turns Ctrl+C / console close into `WM_QUIT` on the calling thread, so the
/// message loop exits and normal cleanup (restoring the desktop) runs.
pub fn quit_on_ctrl_c() {
    // SAFETY: plain Win32 calls; the handler only touches an atomic.
    unsafe {
        QUIT_THREAD.store(GetCurrentThreadId(), Ordering::Relaxed);
        if let Err(e) = SetConsoleCtrlHandler(Some(on_ctrl), true) {
            log::warn!("could not install Ctrl+C handler: {e}");
        }
    }
}

unsafe extern "system" fn on_ctrl(_event: u32) -> BOOL {
    let thread = QUIT_THREAD.load(Ordering::Relaxed);
    // SAFETY: posting to a thread id is always safe; failure means it is gone.
    let _ = unsafe { PostThreadMessageW(thread, WM_QUIT, WPARAM(0), LPARAM(0)) };
    true.into()
}
