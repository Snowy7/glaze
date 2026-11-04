//! mypaper-widgets: interactive desktop widgets on the desktop layer
//! (docs/widgets.md).
//!
//! ```text
//! mypaper-widgets [--edit on|off|toggle] [--overlay on|off|toggle]
//! ```
//!
//! Without arguments it hosts the widgets from `%APPDATA%\mypaper\widgets.json`
//! (one instance per session). `--edit` and `--overlay` are tiny clients: they ask the
//! running host to turn edit mode, or the overlay layer, on, off or over, and exit.

// No console window in release builds; logs go to a file (see init_logging).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod appicon;
mod audio;
mod bridge;
mod display;
mod files;
mod layout;
mod look;
mod manifest;
mod media;
mod net;
mod overlay;
mod pick;
mod plates;
mod plugins;
mod registry;
mod rpc;
mod stats;
mod storage;
mod surface;
mod theme;
mod userinfo;
mod wake;

use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use mypaper_desktop::EXIT_CONFIG;
use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, LPARAM, WPARAM};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowExW, HWND_MESSAGE, PostMessageW, RegisterWindowMessageW,
};
use windows::core::{PCWSTR, w};

const USAGE: &str = "usage: mypaper-widgets [--edit on|off|toggle] [--overlay on|off|toggle]";

fn main() -> ExitCode {
    mypaper_desktop::init_logging();
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        [] => {}
        [flag @ ("--edit" | "--overlay"), mode] => {
            let message = if *flag == "--edit" {
                app::EDIT_MESSAGE
            } else {
                overlay::OVERLAY_MESSAGE
            };
            return match post_mode(flag, message, mode) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    log::error!("{e:#}");
                    ExitCode::FAILURE
                }
            };
        }
        ["-h" | "--help"] => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        _ => {
            log::error!("{USAGE}");
            return ExitCode::from(EXIT_CONFIG);
        }
    }

    mypaper_desktop::enable_dpi_awareness();
    mypaper_desktop::quit_on_ctrl_c();
    // SAFETY: named mutex held for the lifetime of the process.
    let _instance = unsafe { CreateMutexW(None, true, w!(r"Local\mypaper-widgets")) };
    // SAFETY: reads the calling thread's last error.
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        log::error!("mypaper-widgets is already running");
        return ExitCode::from(EXIT_CONFIG);
    }
    // No white flash while a WebView starts: transparent from its first frame.
    // SAFETY: set before any other thread exists, so nothing reads the
    // environment concurrently.
    unsafe { std::env::set_var("WEBVIEW2_DEFAULT_BACKGROUND_COLOR", "0") };

    match app::App::new() {
        Ok(mut app) => {
            let code = app.run();
            drop(app);
            ExitCode::from(code)
        }
        Err(e) => {
            log::error!("{e:#}");
            ExitCode::FAILURE
        }
    }
}

/// `--edit` and `--overlay`: posts the registered window message `name` to the running host
/// (`wParam` 0 off, 1 on, 2 toggle) and exits.
fn post_mode(flag: &str, name: PCWSTR, mode: &str) -> Result<()> {
    let wparam = match mode {
        "off" => 0,
        "on" => 1,
        "toggle" => 2,
        other => bail!("{flag} takes on, off or toggle, not {other:?}\n{USAGE}"),
    };
    // SAFETY: plain lookups and a post; nothing is created or activated.
    unsafe {
        let host = FindWindowExW(Some(HWND_MESSAGE), None, app::HOST_CLASS, PCWSTR::null())
            .context("mypaper-widgets isn't running")?;
        let message = RegisterWindowMessageW(name);
        PostMessageW(Some(host), message, WPARAM(wparam), LPARAM(0))?;
    }
    Ok(())
}
