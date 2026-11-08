//! Live WinEvent/IPC lifecycle probe. Requires the shared desktop lock. Owns a
//! short-lived borderless test window; never sends keyboard/mouse input.
//! `--idle SECONDS` only pumps messages, for measuring the enabled watcher.

use anyhow::{Result, ensure};
use glaze_tweaks::game_activity::{self, GameWatcher};
use std::os::windows::io::AsRawHandle;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromPoint,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetForegroundWindow, MSG,
    MsgWaitForMultipleObjects, PM_REMOVE, PeekMessageW, QS_ALLINPUT, RegisterClassW, SW_SHOWNORMAL,
    SWP_NOZORDER, SetForegroundWindow, SetWindowPos, ShowWindow, TranslateMessage, WINDOW_EX_STYLE,
    WNDCLASSW, WS_POPUP,
};
use windows::core::w;

fn pump_until(deadline: Instant) {
    while Instant::now() < deadline {
        // SAFETY: this thread owns its queue; no handles, bounded test deadline.
        unsafe {
            MsgWaitForMultipleObjects(
                None,
                false,
                deadline
                    .saturating_duration_since(Instant::now())
                    .as_millis()
                    .min(10000) as u32,
                QS_ALLINPUT,
            );
            let mut message = MSG::default();
            while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: forwards unchanged window-procedure arguments.
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

fn test_window() -> Result<()> {
    // SAFETY: all buffers are valid; only our own test window is shown/moved/destroyed.
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let previous = GetForegroundWindow();
        let instance = GetModuleHandleW(None)?;
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance.into(),
            lpszClassName: w!("glaze-game-probe"),
            ..Default::default()
        };
        RegisterClassW(&class);
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
            "monitor info"
        );
        let r = monitor.rcMonitor;
        let window = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class.lpszClassName,
            w!("Glaze fullscreen signal probe"),
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
        pump_until(Instant::now() + Duration::from_secs(2));
        SetWindowPos(
            window,
            None,
            r.left + 100,
            r.top + 100,
            640,
            400,
            SWP_NOZORDER,
        )?;
        pump_until(Instant::now() + Duration::from_secs(2));
        DestroyWindow(window)?;
        let _ = SetForegroundWindow(previous);
    }
    Ok(())
}

fn main() -> Result<()> {
    if std::env::args().nth(1).as_deref() == Some("--read") {
        println!(
            "{}",
            serde_json::to_string(&game_activity::read().map(|(snapshot, _)| snapshot))?
        );
        return Ok(());
    }
    if std::env::args().nth(1).as_deref() == Some("--window") {
        return test_window();
    }
    ensure!(
        std::path::Path::new(r"C:\Users\you\AppData\Local\Temp\glaze-tools\desktop.lock")
            .exists(),
        "caller must hold desktop lock"
    );
    // SAFETY: this process owns its DPI context; no windows have been created yet.
    let _ = unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    let watcher = GameWatcher::start()?;
    ensure!(
        game_activity::read().is_some(),
        "publisher read/lifetime check"
    );
    let initial = game_activity::read().unwrap().0;
    println!("START {}", serde_json::to_string(&initial)?);
    if std::env::args().nth(1).as_deref() == Some("--idle") {
        let seconds = std::env::args()
            .nth(2)
            .unwrap_or_else(|| "30".into())
            .parse()?;
        pump_until(Instant::now() + Duration::from_secs(seconds));
    } else {
        let mut child = Command::new(std::env::current_exe()?)
            .arg("--window")
            .stdout(Stdio::null())
            .spawn()?;
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut entered = false;
        let mut left = false;
        let mut last = initial.activity;
        while Instant::now() < deadline {
            // SAFETY: process handle is borrowed from child; this thread pumps its own queue.
            unsafe {
                MsgWaitForMultipleObjects(
                    Some(&[HANDLE(child.as_raw_handle())]),
                    false,
                    deadline
                        .saturating_duration_since(Instant::now())
                        .as_millis()
                        .min(15000) as u32,
                    QS_ALLINPUT,
                );
                let mut message = MSG::default();
                while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }
            if let Some((snapshot, _)) = game_activity::read() {
                if snapshot.activity != last {
                    println!("EVENT {}", serde_json::to_string(&snapshot)?);
                    last = snapshot.activity;
                }
                if snapshot.activity.foreground_pid == child.id() {
                    entered |= snapshot.activity.pause_background;
                    left |= entered && !snapshot.activity.pause_background;
                }
            }
            if child.try_wait()?.is_some() {
                break;
            }
        }
        if child.try_wait()?.is_none() {
            child.kill()?;
            child.wait()?;
        }
        if entered {
            ensure!(
                left,
                "fullscreen was observed but resize did not clear the signal"
            );
        } else {
            println!(
                "UNVERIFIED fullscreen transition: Windows did not grant the child foreground focus; no input was injected"
            );
        }
    }
    drop(watcher);
    ensure!(
        !game_activity::path()?.exists(),
        "publication must disappear on disable/drop"
    );
    // Start/drop again checks hook/window/lease cleanup in the same message-pumping thread.
    drop(GameWatcher::start()?);
    ensure!(
        !game_activity::path()?.exists(),
        "second watcher publication must disappear"
    );
    println!("PASS event-driven publisher lifecycle and cleanup");
    Ok(())
}
