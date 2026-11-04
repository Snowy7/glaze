//! Win32 plumbing shared by mypaper processes. Everything is event-driven,
//! with no polling threads, so an idle mypaper costs ~0% CPU.

mod config_watch;
mod control;
mod cover;
pub mod desktop_hit;
pub mod desktop_menu;
pub mod desktop_view;
pub mod desktop_wallpaper;
mod fullscreen;
pub mod hold;
mod host;
mod menu_model;
mod menu_ui;
mod monitors;
mod power;
pub mod raw_input;
pub mod startup;
#[cfg(feature = "webview")]
pub mod webview_env;

pub use config_watch::{ConfigWatcher, watch_config, watch_file};
pub use control::{ControlWindow, WM_FACT};
pub use cover::{WindowEvents, covered_monitors};
pub use fullscreen::{fullscreen_app_in_front, fullscreen_app_monitor};
pub use host::DesktopHost;
pub use monitors::{Monitor, Rect, monitor_infos, monitors};
pub use power::on_battery;

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

/// Debug builds log to the console. Release builds have no console (they
/// use the Windows subsystem so double-clicking opens no terminal), so they
/// append to `%LOCALAPPDATA%\mypaper\mypaper.log`. Filter with `RUST_LOG`.
pub fn init_logging() {
    let env = env_logger::Env::default().default_filter_or("info");
    let mut builder = env_logger::Builder::from_env(env);
    if !cfg!(debug_assertions)
        && let Some(file) = log_file()
    {
        builder.target(env_logger::Target::Pipe(Box::new(file)));
    }
    builder.init();
}

fn log_file() -> Option<std::fs::File> {
    const MAX_BYTES: u64 = 4 << 20;
    let dir = std::path::PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("mypaper");
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join("mypaper.log");
    let too_big = std::fs::metadata(&path).is_ok_and(|m| m.len() > MAX_BYTES);
    let mut options = std::fs::OpenOptions::new();
    options.create(true);
    if too_big {
        options.write(true).truncate(true);
    } else {
        options.append(true);
    }
    options.open(path).ok()
}

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
