use windows::Win32::Foundation::{LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::MONITORINFOF_PRIMARY;
use windows::core::BOOL;

/// A screen-space rectangle in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Rect {
    pub fn contains(&self, other: &Rect) -> bool {
        other.x >= self.x
            && other.y >= self.y
            && other.x + other.width <= self.x + self.width
            && other.y + other.height <= self.y + self.height
    }

    pub fn contains_point(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.width && y < self.y + self.height
    }

    pub fn center(&self) -> (i32, i32) {
        (self.x + self.width / 2, self.y + self.height / 2)
    }
}

impl From<RECT> for Rect {
    fn from(r: RECT) -> Self {
        Self {
            x: r.left,
            y: r.top,
            width: r.right - r.left,
            height: r.bottom - r.top,
        }
    }
}

/// All monitors, in virtual-screen coordinates.
pub fn monitors() -> Vec<Rect> {
    let mut out: Vec<Rect> = Vec::new();
    // SAFETY: the callback only runs during this call, while `out` is alive.
    unsafe {
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(push_monitor),
            LPARAM(&raw mut out as isize),
        );
    }
    out
}

unsafe extern "system" fn push_monitor(_: HMONITOR, _: HDC, rect: *mut RECT, out: LPARAM) -> BOOL {
    // SAFETY: `out` is the Vec passed by `monitors`; `rect` is valid for the call.
    unsafe { (*(out.0 as *mut Vec<Rect>)).push(Rect::from(*rect)) };
    true.into()
}

/// A monitor as the widget host sees it: where it is, where the taskbar
/// isn't, and how it scales. Rects are physical pixels in virtual-screen
/// coordinates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Monitor {
    /// The device name, e.g. `\\.\DISPLAY1` (`MONITORINFOEXW.szDevice`).
    pub device: String,
    pub rect: Rect,
    /// The work area: `rect` minus the taskbar and docked app bars.
    pub work: Rect,
    /// Effective DPI (96 at 100% scale).
    pub dpi: u32,
    pub primary: bool,
}

/// Every monitor with its device name, work area and DPI.
pub fn monitor_infos() -> Vec<Monitor> {
    let mut out: Vec<Monitor> = Vec::new();
    // SAFETY: the callback only runs during this call, while `out` is alive.
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(push_info), LPARAM(&raw mut out as isize));
    }
    out
}

unsafe extern "system" fn push_info(monitor: HMONITOR, _: HDC, _: *mut RECT, out: LPARAM) -> BOOL {
    let mut info = MONITORINFOEXW {
        monitorInfo: MONITORINFO {
            cbSize: size_of::<MONITORINFOEXW>() as u32,
            ..Default::default()
        },
        ..Default::default()
    };
    let (mut dpi, mut dpi_y) = (96, 96);
    // SAFETY: `info` is a MONITORINFOEXW with cbSize set, as GetMonitorInfoW
    // requires for the extended struct; `out` is the Vec passed by `monitor_infos`.
    unsafe {
        if !GetMonitorInfoW(monitor, (&raw mut info).cast()).as_bool() {
            return true.into();
        }
        let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut dpi_y);
        let device = &info.szDevice;
        let len = device.iter().position(|&c| c == 0).unwrap_or(device.len());
        (*(out.0 as *mut Vec<Monitor>)).push(Monitor {
            device: String::from_utf16_lossy(&device[..len]),
            rect: info.monitorInfo.rcMonitor.into(),
            work: info.monitorInfo.rcWork.into(),
            dpi: dpi.max(1),
            primary: info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
        });
    }
    true.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn containment() {
        let monitor = Rect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        };
        let fullscreen = Rect {
            x: -8,
            y: -8,
            width: 1936,
            height: 1096,
        };
        let window = Rect {
            x: 100,
            y: 100,
            width: 800,
            height: 600,
        };
        assert!(fullscreen.contains(&monitor));
        assert!(!window.contains(&monitor));
        assert!(monitor.contains_point(1919, 1079));
        assert!(!monitor.contains_point(1920, 0));
    }
}
