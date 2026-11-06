//! Owned borderless fullscreen fixture, no input injection and no render/timer loop.
//! Run only under the desktop lock. Covers all monitors, logs the real core signal, and exits
//! after the requested duration. Never moves/minimizes another application's windows.
use anyhow::{Result, ensure};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    // SAFETY: forward the unchanged arguments for this owned window.
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

fn main() -> Result<()> {
    ensure!(
        std::path::Path::new(r"C:\Users\you\AppData\Local\Temp\mypaper-tools\desktop.lock")
            .is_dir(),
        "take the desktop lock before starting the fixture"
    );
    mypaper_desktop::enable_dpi_awareness();
    let seconds = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "100".into())
        .parse::<u64>()?;
    let mut windows = Vec::new();
    // SAFETY: owns every HWND/class and pumps their messages on this thread. No foreign
    // window is modified; activation is only requested for our own fixture.
    unsafe {
        let instance = GetModuleHandleW(None)?;
        let class = WNDCLASSW {
            lpfnWndProc: Some(proc),
            hInstance: instance.into(),
            lpszClassName: w!("mypaper.game-quiet.fixture"),
            hbrBackground: windows::Win32::Graphics::Gdi::HBRUSH(
                (windows::Win32::Graphics::Gdi::COLOR_WINDOW.0 + 1) as _,
            ),
            ..Default::default()
        };
        RegisterClassW(&class);
        for monitor in mypaper_desktop::monitor_infos() {
            let r = monitor.rect;
            let window = CreateWindowExW(
                WS_EX_TOPMOST,
                class.lpszClassName,
                w!("mypaper fullscreen quiescence fixture"),
                WS_POPUP,
                r.x,
                r.y,
                r.width,
                r.height,
                None,
                None,
                Some(instance.into()),
                None,
            )?;
            let _ = ShowWindow(window, SW_SHOWNORMAL);
            windows.push(window);
        }
        if let Some(&window) = windows.first() {
            let _ = SetForegroundWindow(window);
        }
        println!(
            "fixture pid={} monitors={}",
            std::process::id(),
            windows.len()
        );
        let until = Instant::now() + Duration::from_secs(seconds);
        let mut last = None;
        while Instant::now() < until {
            // Fixture-only bounded timeout; production consumers use watch() and Owner.
            MsgWaitForMultipleObjects(None, false, 1000, QS_ALLINPUT);
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            let activity = mypaper_tweaks::game_activity::read().map(|(s, _)| s.activity);
            if activity != last {
                println!("activity={}", serde_json::to_string(&activity)?);
                last = activity;
            }
        }
        for window in windows {
            DestroyWindow(window)?;
        }
    }
    Ok(())
}
