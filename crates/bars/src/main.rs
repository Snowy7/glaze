//! mypaper-bars: the top bar, the dock and the native taskbar's mode (docs/bars.md).
//!
//! ```text
//! mypaper-bars
//! ```
//!
//! Reads `%APPDATA%\mypaper\bars.json` and follows it live. One instance per session
//! (`Local\mypaper-bars`). Core starts and stops it; posting `WM_CLOSE` to its message-only window
//! (class `mypaper.bars.host`) makes it restore the taskbar and exit.

// No console window in release builds; logs go to a file (see init_logging).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod appbar;
mod apps;
mod audio;
mod bar;
mod bridge;
mod desktops;
mod hooks;
mod icons;
mod look;
mod media;
mod network;
mod system;
mod tray;
mod wake;
mod web;

use std::process::ExitCode;

use mypaper_desktop::EXIT_CONFIG;
use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
use windows::Win32::System::Threading::CreateMutexW;
use windows::core::w;

fn main() -> ExitCode {
    mypaper_desktop::init_logging();
    if std::env::args()
        .nth(1)
        .is_some_and(|a| a == "-h" || a == "--help")
    {
        println!("usage: mypaper-bars   (see docs/bars.md)");
        return ExitCode::SUCCESS;
    }
    mypaper_desktop::enable_dpi_awareness();
    mypaper_desktop::quit_on_ctrl_c();
    // SAFETY: a named mutex held for the lifetime of the process.
    let _instance = unsafe { CreateMutexW(None, true, w!(r"Local\mypaper-bars")) };
    // SAFETY: reads the calling thread's last error.
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        log::error!("mypaper-bars is already running");
        return ExitCode::from(EXIT_CONFIG);
    }
    // No white flash while a WebView starts: transparent from its first frame.
    // SAFETY: set before any other thread exists, so nothing reads the environment concurrently.
    unsafe { std::env::set_var("WEBVIEW2_DEFAULT_BACKGROUND_COLOR", "0") };

    match app::App::new() {
        Ok(mut app) => {
            let code = app.run();
            drop(app); // restores the taskbar and removes the app bars
            ExitCode::from(code)
        }
        Err(e) => {
            log::error!("{e:#}");
            ExitCode::FAILURE
        }
    }
}
