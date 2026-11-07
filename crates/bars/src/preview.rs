//! Live DWM thumbnails registered into a native preview popup. Call open for source sizes,
//! place with popup-local rects, and close (or drop) to unregister. No window capture or device.

use serde::Serialize;
use windows::Win32::Foundation::{HWND, RECT, SIZE};
use windows::Win32::Graphics::Dwm::{
    DWM_THUMBNAIL_PROPERTIES, DWM_TNP_OPACITY, DWM_TNP_RECTDESTINATION,
    DWM_TNP_SOURCECLIENTAREAONLY, DWM_TNP_VISIBLE, DwmQueryThumbnailSourceSize,
    DwmRegisterThumbnail, DwmUnregisterThumbnail, DwmUpdateThumbnailProperties,
};
use windows::Win32::UI::WindowsAndMessaging::IsWindow;

use crate::system;

/// A thumbnail source's size in px (the page keeps its aspect).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct SourceSize {
    pub hwnd: isize,
    pub width: i32,
    pub height: i32,
}

/// The thumbnails one bar shows.
#[derive(Default)]
pub struct Previews {
    live: Vec<(isize, isize)>,
}

impl Previews {
    /// Registers thumbnails of `hwnds` into `bar` (hidden until placed) and returns their sizes.
    /// Windows that are gone are left out.
    pub fn open(&mut self, bar: HWND, hwnds: &[isize]) -> Vec<SourceSize> {
        self.close();
        let mut sizes = Vec::new();
        for &hwnd in hwnds.iter().take(16) {
            let source = system::hwnd(hwnd);
            // SAFETY: registers a thumbnail of a live window into our own window; unregistered
            // in `close`.
            let thumbnail = unsafe {
                if !IsWindow(Some(source)).as_bool() {
                    continue;
                }
                DwmRegisterThumbnail(bar, source)
            };
            let Ok(thumbnail) = thumbnail else { continue };
            // SAFETY: a thumbnail we just registered.
            let size = unsafe { DwmQueryThumbnailSourceSize(thumbnail) };
            self.live.push((hwnd, thumbnail));
            let size = size.unwrap_or(SIZE { cx: 16, cy: 9 });
            sizes.push(SourceSize {
                hwnd,
                width: size.cx.max(1),
                height: size.cy.max(1),
            });
        }
        sizes
    }

    /// Shows each window's thumbnail at `rect` (DIPs from the window's top-left; `scale` turns
    /// them into px). Thumbnails not listed are hidden. DWM preserves source alpha even
    /// at opacity 255; the destination must paint an opaque surface under each rect.
    pub fn place(&self, windows: &[(isize, [f64; 4])], scale: f64) {
        for &(hwnd, thumbnail) in &self.live {
            let rect = windows.iter().find(|(h, _)| *h == hwnd).map(|(_, r)| *r);
            let px = |v: f64| (v * scale).round() as i32;
            let properties = DWM_THUMBNAIL_PROPERTIES {
                dwFlags: DWM_TNP_VISIBLE
                    | DWM_TNP_RECTDESTINATION
                    | DWM_TNP_OPACITY
                    | DWM_TNP_SOURCECLIENTAREAONLY,
                rcDestination: rect.map_or(RECT::default(), |[x, y, w, h]| RECT {
                    left: px(x),
                    top: px(y),
                    right: px(x + w),
                    bottom: px(y + h),
                }),
                opacity: 255,
                fVisible: rect.is_some().into(),
                fSourceClientAreaOnly: false.into(),
                ..Default::default()
            };
            // SAFETY: a thumbnail we registered; the struct is fully initialized.
            let _ = unsafe { DwmUpdateThumbnailProperties(thumbnail, &properties) };
        }
    }

    pub fn close(&mut self) {
        for (_, thumbnail) in self.live.drain(..) {
            // SAFETY: a thumbnail we registered, unregistered once.
            let _ = unsafe { DwmUnregisterThumbnail(thumbnail) };
        }
    }
}

impl Drop for Previews {
    fn drop(&mut self) {
        self.close();
    }
}
