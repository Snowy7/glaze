//! A "game" on the primary monitor only, for `engine.during_games`: one borderless topmost window
//! of our own covering the primary monitor, made foreground (a guarded click on our own window if
//! Windows refuses, cursor put back), so core's game signal turns on while the other screens stay
//! uncovered. Exits after the given seconds. Take the desktop lock before running it.
//! `cargo run -p mypaper-wallpaper --example game_primary --release -- 60`

use std::time::{Duration, Instant};

use anyhow::{Result, ensure};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, mouse_event,
};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    // SAFETY: the unchanged arguments, for our own window.
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

fn pump(milliseconds: u32) {
    // SAFETY: this thread's own queue and windows.
    unsafe {
        MsgWaitForMultipleObjects(None, false, milliseconds, QS_ALLINPUT);
        let mut msg = MSG::default();
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// Foreground for our own window: asked for, else one click on a point verified (twice) to be
/// ours, with the cursor put back.
unsafe fn activate(window: HWND, center: POINT) -> Result<()> {
    // SAFETY: activation and input only ever target this process's own window.
    unsafe {
        let _ = SetForegroundWindow(window);
        if GetForegroundWindow() == window {
            return Ok(());
        }
        let owned = || {
            let mut pid = 0;
            GetWindowThreadProcessId(WindowFromPoint(center), Some(&mut pid));
            pid == std::process::id()
        };
        ensure!(owned(), "activation point is not ours");
        let mut previous = POINT::default();
        GetCursorPos(&mut previous)?;
        SetCursorPos(center.x, center.y)?;
        let clicked = (|| -> Result<()> {
            ensure!(owned(), "activation target changed");
            mouse_event(MOUSEEVENTF_LEFTDOWN, 0, 0, 0, 0);
            mouse_event(MOUSEEVENTF_LEFTUP, 0, 0, 0, 0);
            pump(100);
            ensure!(GetForegroundWindow() == window, "could not take the foreground");
            Ok(())
        })();
        SetCursorPos(previous.x, previous.y)?;
        clicked
    }
}

fn main() -> Result<()> {
    ensure!(
        std::path::Path::new(r"C:\Users\you\AppData\Local\Temp\mypaper-tools\desktop.lock")
            .is_dir(),
        "take the desktop lock first"
    );
    mypaper_desktop::enable_dpi_awareness();
    let seconds: u64 = std::env::args().nth(1).unwrap_or_else(|| "60".into()).parse()?;
    let primary = mypaper_desktop::monitor_infos()
        .into_iter()
        .find(|m| m.rect.contains_point(0, 0))
        .map(|m| m.rect)
        .ok_or_else(|| anyhow::anyhow!("no primary monitor"))?;
    // SAFETY: our own class and window, pumped on this thread and destroyed with the process.
    let window = unsafe {
        let instance = GetModuleHandleW(None)?;
        let class = WNDCLASSW {
            lpfnWndProc: Some(proc),
            hInstance: instance.into(),
            lpszClassName: w!("mypaper.engine.game-primary"),
            hbrBackground: windows::Win32::Graphics::Gdi::HBRUSH(
                (windows::Win32::Graphics::Gdi::COLOR_WINDOW.0 + 1) as _,
            ),
            ..Default::default()
        };
        RegisterClassW(&class);
        let window = CreateWindowExW(
            WS_EX_TOPMOST,
            class.lpszClassName,
            w!("mypaper game fixture (primary monitor only)"),
            WS_POPUP,
            primary.x,
            primary.y,
            primary.width,
            primary.height,
            None,
            None,
            Some(instance.into()),
            None,
        )?;
        let _ = ShowWindow(window, SW_SHOWNORMAL);
        pump(50);
        let center = POINT {
            x: primary.x + primary.width / 2,
            y: primary.y + primary.height / 2,
        };
        activate(window, center)?;
        window
    };
    println!("game fixture pid={} on {primary:?}", std::process::id());
    let until = Instant::now() + Duration::from_secs(seconds);
    let mut said = false;
    while Instant::now() < until {
        pump(250);
        if !said && let Some((signal, _)) = mypaper_tweaks::game_activity::read() {
            if signal.activity.pause_background {
                println!("game signal on: {:?}", signal.activity);
                said = true;
            }
        }
    }
    // SAFETY: our own window.
    unsafe { DestroyWindow(window)? };
    Ok(())
}
