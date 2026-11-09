//! Opt-in tests against a separate, owned, no-activate process. The test-only PID
//! filter denies every other process, including Windows apps on the user's desktop.
use super::*;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::WPARAM;
use windows::Win32::UI::WindowsAndMessaging::*;

fn pump() {
    let until = Instant::now() + Duration::from_millis(1300);
    while Instant::now() < until {
        let mut message = MSG::default();
        // SAFETY: this thread's messages only; dispatch is needed for out-of-context hooks.
        unsafe {
            while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
#[ignore = "requires GLAZE_CORNER_PROBE_PID from scripts/corner-probe-host.cs"]
fn foreign_window_corner_lifecycle() {
    let pid: u32 = std::env::var("GLAZE_CORNER_PROBE_PID").unwrap().parse().unwrap();
    assert_ne!(pid, std::process::id());
    TEST_PROCESS.set(Some(pid));
    let mut handles = Vec::new();
    for_each_window(|hwnd| {
        let mut actual = 0;
        // SAFETY: a read-only PID query.
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut actual)) };
        if actual == pid && unsafe { IsWindowVisible(hwnd) }.as_bool() { handles.push(hwnd); }
    });
    assert_eq!(handles.len(), 1, "launch a fresh probe process");
    let hwnd = handles[0];
    let original = read(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE).unwrap();
    assert_eq!(original, 0);
    for mode in [Corners::Default, Corners::Round, Corners::Small, Corners::Square] {
        let raw = WindowsStyle { corners: Some(mode), ..Default::default() };
        let styler = Styler::start(&raw, None).unwrap();
        let expected = corner_preference(mode).0 as u32;
        assert_eq!(read(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE), Some(expected), "existing {mode:?}");
        // SAFETY: only the owned foreign test HWND receives these custom messages.
        unsafe { PostMessageW(Some(hwnd), 0x8001, WPARAM(if expected == 3 { 2 } else { 3 }), LPARAM(0)) }.unwrap();
        pump();
        assert_eq!(read(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE), Some(expected), "app reset {mode:?}");
        unsafe { PostMessageW(Some(hwnd), 0x8002, WPARAM(0), LPARAM(0)) }.unwrap();
        pump();
        let mut new = Vec::new();
        for_each_window(|candidate| {
            let mut actual = 0;
            unsafe { GetWindowThreadProcessId(candidate, Some(&mut actual)) };
            if actual == pid && candidate != hwnd && unsafe { IsWindowVisible(candidate) }.as_bool() { new.push(candidate); }
        });
        assert_eq!(new.len(), 1);
        assert_eq!(read(new[0], DWMWA_WINDOW_CORNER_PREFERENCE), Some(expected), "new {mode:?}");
        styler.update(&WindowsStyle::default(), None);
        assert_eq!(read(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE), Some(original), "release {mode:?}");
        styler.update(&raw, None);
        drop(styler);
        assert_eq!(read(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE), Some(original), "drop {mode:?}");
        unsafe { PostMessageW(Some(new[0]), WM_CLOSE, WPARAM(0), LPARAM(0)) }.unwrap();
        pump();
        println!("{mode:?}: existing/new/reset/release/drop passed");
    }
    let styler = Styler::start(&WindowsStyle { corners: Some(Corners::Round), ..Default::default() }, None).unwrap();
    // A tool surface releases its managed corners; the app regains its normal frame afterwards.
    let ex = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) };
    unsafe { SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex | WS_EX_TOOLWINDOW.0 as isize) };
    pump();
    assert_eq!(read(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE), Some(original));
    unsafe { SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex) };
    pump();
    assert_eq!(read(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE), Some(2));
    let mut before = RECT::default();
    unsafe { GetWindowRect(hwnd, &mut before) }.unwrap();
    let style = unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) };
    let mut monitor = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
    unsafe { GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST), &mut monitor) }.ok().unwrap();
    let r = monitor.rcMonitor;
    // Simulate a borderless full-monitor app, without foregrounding or changing any other HWND.
    unsafe {
        SetWindowLongPtrW(hwnd, GWL_STYLE, (WS_POPUP.0 | WS_VISIBLE.0) as isize);
        SetWindowPos(hwnd, None, r.left, r.top, r.right-r.left, r.bottom-r.top,
            SWP_NOACTIVATE | SWP_NOZORDER | SWP_FRAMECHANGED).unwrap();
    }
    pump();
    assert_eq!(read(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE), Some(original));
    unsafe {
        SetWindowLongPtrW(hwnd, GWL_STYLE, style);
        SetWindowPos(hwnd, None, before.left, before.top, before.right-before.left, before.bottom-before.top,
            SWP_NOACTIVATE | SWP_NOZORDER | SWP_FRAMECHANGED).unwrap();
    }
    pump();
    assert_eq!(read(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE), Some(2));
    drop(styler);
    unsafe { PostMessageW(Some(hwnd), 0x8001, WPARAM(1), LPARAM(0)) }.unwrap();
    pump();
    reset_all(None);
    assert_eq!(read(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE), Some(0));
    println!("tool/full-monitor exclusions and crash-recovery default reset passed");
    TEST_PROCESS.set(None);
}

#[test]
#[ignore = "requires GLAZE_ELEVATED_CORNER_PROBE_PID from an elevated owned host"]
fn elevated_window_is_untouched() {
    let pid: u32 = std::env::var("GLAZE_ELEVATED_CORNER_PROBE_PID").unwrap().parse().unwrap();
    TEST_PROCESS.set(Some(pid));
    let mut count = 0;
    for_each_window(|hwnd| {
        let mut actual = 0;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut actual)) };
        if actual == pid && unsafe { IsWindowVisible(hwnd) }.as_bool() {
            count += 1;
            assert_eq!(allowed_process(hwnd, &windows_dir().unwrap()), None);
            assert_eq!(read(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE), Some(0));
        }
    });
    assert_eq!(count, 1);
    TEST_PROCESS.set(None);
}
