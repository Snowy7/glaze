//! Owned borderless fullscreen fixture, guarded own-window activation and no render loop.
//! Run only under the desktop lock. Covers all monitors, logs the real core signal, and exits
//! after the requested duration. Never moves/minimizes another application's windows.
use anyhow::{Context, Result, ensure};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, mouse_event,
};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    // SAFETY: forward the unchanged arguments for this owned window.
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

fn pump(milliseconds: u32) {
    // SAFETY: this fixture owns the thread's windows and queue; no foreign input is sent.
    unsafe {
        MsgWaitForMultipleObjects(None, false, milliseconds, QS_ALLINPUT);
        let mut msg = MSG::default();
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// Windows may deny a background launcher foreground permission. Only click a point
/// verified twice to belong to this fixture, then restore the original cursor position.
unsafe fn focus_owned(window: HWND) -> Result<()> {
    // SAFETY: focus and input target only this fixture's own visible window.
    unsafe {
        let _ = SetForegroundWindow(window);
        if GetForegroundWindow() == window {
            return Ok(());
        }
        let mut rect = RECT::default();
        GetWindowRect(window, &mut rect)?;
        let point = POINT {
            x: rect.left + (rect.right - rect.left) / 2,
            y: rect.top + (rect.bottom - rect.top) / 2,
        };
        let owned = || {
            let mut pid = 0;
            GetWindowThreadProcessId(WindowFromPoint(point), Some(&mut pid));
            pid == std::process::id()
        };
        ensure!(owned(), "activation point is not fixture-owned");
        let mut previous = POINT::default();
        GetCursorPos(&mut previous)?;
        SetCursorPos(point.x, point.y)?;
        let result = (|| -> Result<()> {
            ensure!(owned(), "activation target changed");
            mouse_event(MOUSEEVENTF_LEFTDOWN, 0, 0, 0, 0);
            mouse_event(MOUSEEVENTF_LEFTUP, 0, 0, 0, 0);
            pump(100);
            ensure!(
                GetForegroundWindow() == window,
                "fixture could not take focus"
            );
            Ok(())
        })();
        SetCursorPos(previous.x, previous.y)?;
        result
    }
}

fn phase(
    name: &str,
    paused: bool,
    publisher: u32,
    results: &mut Vec<serde_json::Value>,
) -> Result<()> {
    let start = Instant::now();
    loop {
        pump(50);
        let current = glaze_tweaks::game_activity::read().map(|(snapshot, _)| snapshot);
        ensure!(
            current.is_some_and(|s| s.publisher_pid == publisher),
            "publisher restarted during {name}: {current:?}"
        );
        let current = current.expect("checked above");
        let owned_foreground = !matches!(name, "minimize" | "exit");
        if current.activity.pause_background == paused
            && (!owned_foreground || current.activity.foreground_pid == std::process::id())
        {
            println!(
                "{name}: {:?} after {:.1}ms",
                current.activity,
                start.elapsed().as_secs_f64() * 1000.0
            );
            results.push(serde_json::json!({ "phase": name, "expected_pause": paused, "snapshot": current, "elapsed_ms": start.elapsed().as_secs_f64() * 1000.0 }));
            // Let consumers actually apply every hold/resume before the next transition;
            // otherwise the fixture could merely coalesce all phases into its final state.
            let settle = Instant::now() + Duration::from_millis(1250);
            while Instant::now() < settle {
                pump(50);
            }
            return Ok(());
        }
        ensure!(
            start.elapsed() < Duration::from_secs(5),
            "{name} never became paused={paused}: {current:?}"
        );
    }
}

/// Real focus/minimize/destruction events, all in this fixture. Core stays alive throughout.
unsafe fn lifecycle(
    instance: windows::Win32::Foundation::HMODULE,
    class: windows::core::PCWSTR,
    mut full: Vec<HWND>,
    output: PathBuf,
) -> Result<()> {
    let publisher = glaze_tweaks::game_activity::read()
        .context("core is not publishing activity")?
        .0
        .publisher_pid;
    let r = glaze_desktop::monitor_infos()[0].rect;
    let mut results = Vec::new();
    // SAFETY: creates/moves/minimizes/destroys only owned fixture windows.
    unsafe {
        let normal = CreateWindowExW(
            WS_EX_TOPMOST,
            class,
            w!("Glaze normal-window resume fixture"),
            WS_OVERLAPPEDWINDOW,
            r.x + 100,
            r.y + 100,
            640,
            400,
            None,
            None,
            Some(instance.into()),
            None,
        )?;
        let run = (|| -> Result<()> {
            let focus = |window| focus_owned(window);
            let _ = ShowWindow(normal, SW_SHOWNORMAL);
            focus(normal)?;
            phase("normal", false, publisher, &mut results)?;
            focus(full[0])?;
            phase(
                "fullscreen-before-focus-away",
                true,
                publisher,
                &mut results,
            )?;
            focus(normal)?;
            phase("focus-away", false, publisher, &mut results)?;
            focus(full[0])?;
            phase("fullscreen-before-minimize", true, publisher, &mut results)?;
            for &window in &full {
                let _ = ShowWindow(window, SW_MINIMIZE);
            }
            phase("minimize", false, publisher, &mut results)?;
            for &window in &full {
                let _ = ShowWindow(window, SW_RESTORE);
            }
            focus(full[0])?;
            phase("fullscreen-before-exit", true, publisher, &mut results)?;
            for window in full.drain(..) {
                DestroyWindow(window)?;
            }
            phase("exit", false, publisher, &mut results)?;
            for _ in 0..20 {
                pump(50);
            }
            std::fs::write(output, serde_json::to_vec_pretty(&results)?)?;
            Ok(())
        })();
        for window in full {
            let _ = DestroyWindow(window);
        }
        let _ = DestroyWindow(normal);
        run
    }
}

fn main() -> Result<()> {
    ensure!(
        std::path::Path::new(r"C:\Users\you\AppData\Local\Temp\glaze-tools\desktop.lock")
            .is_dir(),
        "take the desktop lock before starting the fixture"
    );
    glaze_desktop::enable_dpi_awareness();
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
            lpszClassName: w!("glaze.game-quiet.fixture"),
            hbrBackground: windows::Win32::Graphics::Gdi::HBRUSH(
                (windows::Win32::Graphics::Gdi::COLOR_WINDOW.0 + 1) as _,
            ),
            ..Default::default()
        };
        RegisterClassW(&class);
        for monitor in glaze_desktop::monitor_infos() {
            let r = monitor.rect;
            let window = CreateWindowExW(
                WS_EX_TOPMOST,
                class.lpszClassName,
                w!("Glaze fullscreen quiescence fixture"),
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
        if let Some(output) = std::env::args().nth(2) {
            return lifecycle(
                instance,
                class.lpszClassName,
                windows,
                PathBuf::from(output),
            );
        }
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
            let activity = glaze_tweaks::game_activity::read().map(|(s, _)| s.activity);
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
