//! Owned primary-monitor fullscreen fixture. No render loop; one safety-exit timer.
#![windows_subsystem = "windows"]
use anyhow::{Result, ensure};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetThreadDpiAwarenessContext,
};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_TIMER || (msg == WM_KEYDOWN && wp.0 == 0x1b) {
        // SAFETY: destroy only this fixture's window, on its message-pumping thread.
        let _ = unsafe { DestroyWindow(hwnd) };
        return LRESULT(0);
    }
    if msg == WM_DESTROY {
        // SAFETY: ends only this process's message loop.
        unsafe { PostQuitMessage(0) };
        return LRESULT(0);
    }
    // SAFETY: forwards the unchanged procedure arguments.
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

fn main() -> Result<()> {
    ensure!(
        std::path::PathBuf::from(std::env::var("LOCALAPPDATA")?)
            .join(r"Temp\glaze-tools\desktop.lock")
            .is_dir(),
        "take the desktop lock first"
    );
    let seconds = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "180".into())
        .parse::<u64>()?;
    // SAFETY: owns this class/window, pumps its messages, and changes no foreign window.
    unsafe {
        SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let mut monitor = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        ensure!(
            GetMonitorInfoW(
                MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY),
                &mut monitor
            )
            .as_bool(),
            "primary monitor unavailable"
        );
        let instance = GetModuleHandleW(None)?;
        let class = WNDCLASSW {
            lpfnWndProc: Some(proc),
            hInstance: instance.into(),
            lpszClassName: w!("glaze.index.game.fixture"),
            hbrBackground: HBRUSH(GetStockObject(BLACK_BRUSH).0),
            ..Default::default()
        };
        RegisterClassW(&class);
        let r = monitor.rcMonitor;
        let window = CreateWindowExW(
            WS_EX_TOPMOST,
            class.lpszClassName,
            w!("Glaze index fullscreen measurement"),
            WS_POPUP,
            r.left,
            r.top,
            r.right - r.left,
            r.bottom - r.top,
            None,
            None,
            Some(instance.into()),
            None,
        )?;
        let _ = ShowWindow(window, SW_SHOWNORMAL);
        let _ = SetForegroundWindow(window);
        ensure!(
            SetTimer(
                Some(window),
                1,
                seconds.saturating_mul(1000).clamp(1, u64::from(u32::MAX)) as u32,
                None
            ) != 0,
            "fixture exit timer unavailable"
        );
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    Ok(())
}
