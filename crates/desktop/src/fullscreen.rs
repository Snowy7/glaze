//! "Is a fullscreen app or game in front?", used to give games the whole GPU.

use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromWindow,
};
use windows::Win32::UI::Shell::{
    QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN, SHQueryUserNotificationState,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GWL_STYLE, GetForegroundWindow, GetWindowLongPtrW, IsZoomed, WS_CAPTION,
};

use crate::cover::opaque_frame;
use crate::{Rect, monitors};

/// True while an exclusive-fullscreen Direct3D game or presentation is
/// running, or the foreground window covers its entire monitor (borderless
/// fullscreen games, F11 video and browsers). Maximized windows with a title
/// bar and the desktop/shell never count. Cheap: call it when
/// [`WindowEvents`](crate::WindowEvents) fires, not on a timer.
pub fn fullscreen_app_in_front() -> bool {
    // SAFETY: no preconditions.
    match unsafe { SHQueryUserNotificationState() } {
        Ok(QUNS_RUNNING_D3D_FULL_SCREEN | QUNS_PRESENTATION_MODE) => true,
        // Borderless fullscreen reports as "busy" at best, so decide by geometry.
        _ => foreground_covers_monitor(),
    }
}

fn foreground_covers_monitor() -> bool {
    // SAFETY: read-only queries on the foreground window handle.
    let frame = unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_invalid() || maximized_with_caption(hwnd) {
            return false;
        }
        opaque_frame(hwnd, std::process::id())
    };
    frame.is_some_and(|frame| covers_whole_monitor(frame, &monitors()))
}

/// A maximized window with a title bar is never fullscreen, even when it covers its whole monitor
/// (with the taskbar hidden or auto-hiding, and no app bar there, the work area is the monitor).
/// Borderless fullscreen windows have no caption.
fn maximized_with_caption(hwnd: HWND) -> bool {
    // SAFETY: read-only queries on a window handle.
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
        IsZoomed(hwnd).as_bool() && style & WS_CAPTION.0 == WS_CAPTION.0
    }
}

/// Where [`fullscreen_app_in_front`] sees a fullscreen app: the rect of the monitor it covers.
/// Exclusive fullscreen and presentation mode count on the foreground window's monitor.
pub fn fullscreen_app_monitor() -> Option<Rect> {
    // SAFETY: read-only queries on the foreground window and its monitor.
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_invalid() {
            return None;
        }
        match SHQueryUserNotificationState() {
            Ok(QUNS_RUNNING_D3D_FULL_SCREEN | QUNS_PRESENTATION_MODE) => {
                let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTOPRIMARY);
                let mut info = MONITORINFO {
                    cbSize: size_of::<MONITORINFO>() as u32,
                    ..Default::default()
                };
                GetMonitorInfoW(monitor, &mut info)
                    .as_bool()
                    .then(|| info.rcMonitor.into())
            }
            _ if maximized_with_caption(hwnd) => None,
            _ => {
                let frame = opaque_frame(hwnd, std::process::id())?;
                let (cx, cy) = frame.center();
                monitors()
                    .into_iter()
                    .find(|m| m.contains_point(cx, cy) && frame.contains(m))
            }
        }
    }
}

/// Does `frame` cover, completely, the monitor it is mostly on?
fn covers_whole_monitor(frame: Rect, monitors: &[Rect]) -> bool {
    let (cx, cy) = frame.center();
    monitors
        .iter()
        .any(|monitor| monitor.contains_point(cx, cy) && frame.contains(monitor))
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn rect(x: i32, y: i32, width: i32, height: i32) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    const SCREENS: [Rect; 2] = [rect(0, 0, 2560, 1440), rect(2560, 0, 1920, 1080)];

    #[test]
    fn borderless_game_covers_its_monitor() {
        assert!(covers_whole_monitor(rect(0, 0, 2560, 1440), &SCREENS));
        // Second monitor, with a few pixels of invisible frame overshoot.
        assert!(covers_whole_monitor(rect(2552, -8, 1936, 1096), &SCREENS));
    }

    #[test]
    fn maximized_and_partial_windows_do_not() {
        // Maximized: stops at the taskbar.
        assert!(!covers_whole_monitor(rect(0, 0, 2560, 1392), &SCREENS));
        assert!(!covers_whole_monitor(rect(100, 100, 800, 600), &SCREENS));
        // Fills one monitor's worth of pixels but sits on the wrong one.
        assert!(!covers_whole_monitor(rect(3000, 0, 1920, 1080), &SCREENS));
    }
}
