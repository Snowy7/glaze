//! Opt-in per-pixel-alpha rendering for an owned app window. Existing app surfaces retain
//! their opaque rendering path. This helper keeps the native resize/snap frame and the kit's
//! client-coordinate hits, but presents software pixels through UpdateLayeredWindow.
use anyhow::{Result, ensure};
use windows::Win32::{Foundation::*, Graphics::Gdi::*, UI::{Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass}, WindowsAndMessaging::*}};
use crate::{DisplayList, Op, PixelRect, Rect, Rgba, Surface, Ui, render::Canvas};

struct Bitmap { dc: HDC, bitmap: HBITMAP, previous: HGDIOBJ, bits: *mut u8, width: u32 }
impl Bitmap {
    fn new(width: u32, height: u32) -> Result<Self> {
        let info = BITMAPINFO { bmiHeader: BITMAPINFOHEADER { biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width as i32, biHeight: -(height as i32), biPlanes: 1, biBitCount: 32,
            biCompression: BI_RGB.0, ..Default::default() }, ..Default::default() };
        // SAFETY: owns this DC and its selected DIB; the old object is restored before deletion.
        unsafe {
            let dc = CreateCompatibleDC(None);
            let mut bits = std::ptr::null_mut();
            let bitmap = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0)
                .inspect_err(|_| { let _ = DeleteDC(dc); })?;
            let previous = SelectObject(dc, bitmap.into());
            Ok(Self { dc, bitmap, previous, bits: bits.cast(), width })
        }
    }
}
impl Drop for Bitmap { fn drop(&mut self) { unsafe { SelectObject(self.dc, self.previous); let _ = DeleteObject(self.bitmap.into()); let _ = DeleteDC(self.dc); } } }

/// A software alpha canvas attached to one app surface. Disable it before returning to
/// `Surface::present`; dropping it releases both bitmaps and the HWND subclass.
pub struct LayeredApp {
    hwnd: HWND,
    enabled: bool,
    pixels: Option<(Canvas, Bitmap)>,
    size: (u32, u32),
    offset: (i32, i32),
    origin: (i32, i32),
}
impl LayeredApp {
    pub fn new(surface: &Surface) -> Result<Self> {
        let hwnd = surface.hwnd();
        // SAFETY: a static subclass of our own HWND, removed on drop.
        ensure!(unsafe { SetWindowSubclass(hwnd, Some(paint), 0x171, 0) }.as_bool(), "attaching alpha app canvas");
        Ok(Self { hwnd, enabled: false, pixels: None, size: (0, 0), offset: (0, 0), origin: (0, 0) })
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        if self.enabled == enabled { return; }
        // SAFETY: changes only this owned app window's extended style. No global composition state.
        unsafe {
            let style = GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE);
            SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, if enabled { style | WS_EX_LAYERED.0 as isize } else { style & !(WS_EX_LAYERED.0 as isize) });
            let _ = SetWindowPos(self.hwnd, None, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED);
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
        self.enabled = enabled;
        self.pixels = None;
    }

    pub fn present(&mut self, surface: &mut Surface, ui: &mut Ui, list: DisplayList) -> Result<bool> {
        ensure!(self.enabled && self.hwnd == surface.hwnd(), "alpha canvas is disabled or belongs to another window");
        let mut window = RECT::default();
        let mut client = POINT::default();
        // SAFETY: read-only geometry queries, with writable locals.
        unsafe { GetWindowRect(self.hwnd, &mut window)?; ClientToScreen(self.hwnd, &mut client).ok()?; }
        let size = ((window.right - window.left).max(1) as u32, (window.bottom - window.top).max(1) as u32);
        let offset = (client.x - window.left, client.y - window.top);
        let origin = (window.left, window.top);
        let resized = self.pixels.is_none() || size != self.size || offset != self.offset;
        let damage = if resized { Some(PixelRect::full(size.0, size.1)) } else {
            list.damage(surface.shown()).map(|area| area.offset(offset.0 as f32, offset.1 as f32)).and_then(|area| area.pixels(size.0, size.1))
        };
        if resized {
            self.pixels = None;
            self.pixels = Some((Canvas::new(ui, size.0, size.1)?, Bitmap::new(size.0, size.1)?));
        }
        if let Some(area) = damage {
            let (canvas, bitmap) = self.pixels.as_mut().expect("allocated above");
            let mut translated = list.translated(offset.0 as f32, offset.1 as f32);
            // Keep the invisible native resize frame hit-testable too.
            translated.ops.insert(0, Op::Fill { rect: Rect::new(0.0, 0.0, size.0 as f32, size.1 as f32), radius: 0.0, color: Rgba::HIT });
            canvas.draw(&mut ui.text, &translated, area)?;
            canvas.read(area, |row, bytes| {
                let at = (area.y + row) as usize * bitmap.width as usize * 4 + area.x as usize * 4;
                // SAFETY: `area` is clipped to this live DIB's allocation above.
                unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), bitmap.bits.add(at), bytes.len()); }
            })?;
        }
        if damage.is_some() || origin != self.origin {
            let at = POINT { x: origin.0, y: origin.1 };
            let size = SIZE { cx: size.0 as i32, cy: size.1 as i32 };
            let zero = POINT::default();
            let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA as u8, ..Default::default() };
            let info = UPDATELAYEREDWINDOWINFO { cbSize: size_of::<UPDATELAYEREDWINDOWINFO>() as u32,
                pptDst: &at, psize: &size, hdcSrc: self.pixels.as_ref().expect("allocated above").1.dc, pptSrc: &zero,
                pblend: &blend, dwFlags: ULW_ALPHA, ..Default::default() };
            // SAFETY: every pointer refers to a live local or owned DIB until the call returns.
            unsafe { UpdateLayeredWindowIndirect(self.hwnd, &info).ok()?; }
        }
        self.size = size; self.offset = offset; self.origin = origin;
        surface.record_layered_app_size(surface.client_size());
        surface.set_shown(list);
        Ok(damage.is_some())
    }
}
impl Drop for LayeredApp { fn drop(&mut self) { unsafe { let _ = RemoveWindowSubclass(self.hwnd, Some(paint), 0x171); } } }

unsafe extern "system" fn paint(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM, _: usize, _: usize) -> LRESULT {
    if msg == WM_PAINT && unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } & WS_EX_LAYERED.0 as isize != 0 {
        // UpdateLayeredWindow owns the shown bitmap; do not let opaque GDI paint cover it.
        let _ = unsafe { ValidateRect(Some(hwnd), None) };
        return LRESULT(0);
    }
    unsafe { DefSubclassProc(hwnd, msg, w, l) }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn alpha_canvas_preserves_native_client_geometry_and_can_return_to_opaque() {
        let mut ui = Ui::new().unwrap();
        let mut surface = Surface::new(crate::Kind::App { min: (100, 100) }, "Alpha canvas test", crate::Role::Window).unwrap();
        let original = surface.client_size();
        let list = DisplayList { ops: vec![Op::Fill { rect: Rect::new(0.0, 0.0, original.0 as f32, original.1 as f32), radius: 8.0, color: Rgba::new(0.1, 0.2, 0.3, 0.5) }], ..Default::default() };
        let mut alpha = LayeredApp::new(&surface).unwrap();
        alpha.set_enabled(true);
        assert!(alpha.present(&mut surface, &mut ui, list.clone()).unwrap());
        assert_eq!(surface.client_size(), original);
        assert!(!alpha.present(&mut surface, &mut ui, list.clone()).unwrap());
        alpha.set_enabled(false);
        surface.release();
        surface.present(&mut ui, (0, 0), original, list).unwrap();
        assert_eq!(surface.client_size(), original);
    }
}
