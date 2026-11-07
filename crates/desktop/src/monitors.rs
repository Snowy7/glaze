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

/// Each active display's GDI name (`\\.\DISPLAY1`) with its monitor's device
/// path (`\\?\DISPLAY#GSM5B7F#...`). Windows hands out GDI numbers again after
/// driver resets and some reboots; the device path stays with the monitor and
/// the port it's plugged into. Empty when the display config can't be read.
pub fn display_paths() -> Vec<(String, String)> {
    use windows::Win32::Devices::Display::{
        DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME, DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
        DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_PATH_INFO, DISPLAYCONFIG_SOURCE_DEVICE_NAME,
        DISPLAYCONFIG_TARGET_DEVICE_NAME, DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes,
        QDC_ONLY_ACTIVE_PATHS, QueryDisplayConfig,
    };
    let text = |w: &[u16]| String::from_utf16_lossy(&w[..w.iter().position(|&c| c == 0).unwrap_or(w.len())]);
    let (mut paths, mut modes) = (0u32, 0u32);
    // SAFETY: the buffers are sized from GetDisplayConfigBufferSizes and outlive the calls;
    // each device-info request carries its own struct size in the header, as the API requires.
    unsafe {
        if GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut paths, &mut modes).is_err() {
            return Vec::new();
        }
        let mut path_info = vec![DISPLAYCONFIG_PATH_INFO::default(); paths as usize];
        let mut mode_info = vec![DISPLAYCONFIG_MODE_INFO::default(); modes as usize];
        if QueryDisplayConfig(
            QDC_ONLY_ACTIVE_PATHS,
            &mut paths,
            path_info.as_mut_ptr(),
            &mut modes,
            mode_info.as_mut_ptr(),
            None,
        )
        .is_err()
        {
            return Vec::new();
        }
        path_info.truncate(paths as usize);
        let mut out = Vec::new();
        for path in &path_info {
            let mut source = DISPLAYCONFIG_SOURCE_DEVICE_NAME::default();
            source.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME;
            source.header.size = size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32;
            source.header.adapterId = path.sourceInfo.adapterId;
            source.header.id = path.sourceInfo.id;
            let mut target = DISPLAYCONFIG_TARGET_DEVICE_NAME::default();
            target.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME;
            target.header.size = size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>() as u32;
            target.header.adapterId = path.targetInfo.adapterId;
            target.header.id = path.targetInfo.id;
            if DisplayConfigGetDeviceInfo(&mut source.header) != 0
                || DisplayConfigGetDeviceInfo(&mut target.header) != 0
            {
                continue;
            }
            let (name, device) = (text(&source.viewGdiDeviceName), text(&target.monitorDevicePath));
            if !name.is_empty() && !device.is_empty() && !out.iter().any(|(n, _)| *n == name) {
                out.push((name, device));
            }
        }
        out
    }
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
