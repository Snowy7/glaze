//! An overlay's window: a layered, transparent (never hit), never activated tool window with no
//! redirection bitmap, z-ordered directly under its client window and showing a premultiplied
//! DirectComposition surface under a rounded clip in the client surface's exact shape.
//!
//! Both layers are top-level windows; a `Popup` overlay is topmost, a `Desktop` one isn't (the
//! shell keeps its widget windows just above the desktop, so the overlay under one covers the
//! wallpaper and icons and stays under apps). A child of the desktop host would sit there too, but
//! the capture's exclusion list ignores child windows: a child overlay captured itself and its
//! widget (N0.2, docs/engine-notes.md "Glass overlays").
//!
//! The window spans the overlay's frame (the surface grown by the glass's reach) so the rim can
//! bend in what lies just outside; the clip keeps the glass inside.
//!
//! The content is a DirectComposition surface, not a swapchain: on this machine the monitors hang
//! off the dGPU while the engine renders on the iGPU, and the first composition swapchain on the
//! iGPU costs ~32 MB of cross-adapter machinery (+15 MB for each larger one), a surface ~3 MB.

use std::sync::Once;
use std::time::Duration;

use anyhow::{Context, Result};
use mypaper_desktop::Rect;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::DirectComposition::{
    IDCompositionAnimation, IDCompositionDesktopDevice, IDCompositionRectangleClip,
    IDCompositionSurface, IDCompositionTarget, IDCompositionVisual2, IDCompositionVisual3,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, HTTRANSPARENT, LWA_ALPHA, MA_NOACTIVATE,
    RegisterClassExW, SWP_NOACTIVATE, SWP_NOCOPYBITS, SWP_NOSIZE, SWP_SHOWWINDOW,
    SetLayeredWindowAttributes, SetWindowPos, WINDOW_EX_STYLE, WM_MOUSEACTIVATE, WM_NCHITTEST,
    WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::core::{PCWSTR, w};
use windows_core::Interface;

use super::capture::{CaptureDevice, Crop};
use super::request::{FadeCurve, Layer};

const CLASS: PCWSTR = w!("mypaper.wallpaper.glass");
/// The glass's fade-in, matching the shell's open animation.
pub const FADE: Duration = Duration::from_millis(300);

pub struct OverlayWindow {
    // Drop order: the DirectComposition tree, then the window.
    surface: IDCompositionSurface,
    size: (u32, u32),
    clip: IDCompositionRectangleClip,
    visual: IDCompositionVisual2,
    _target: IDCompositionTarget,
    hwnd: Owned,
}

/// A window destroyed on drop.
struct Owned(HWND);

impl Drop for Owned {
    fn drop(&mut self) {
        // SAFETY: our own window, destroyed on the thread that made it.
        let _ = unsafe { DestroyWindow(self.0) };
    }
}

impl OverlayWindow {
    /// Creates the window over `frame`, invisible until [`fade_in`](Self::fade_in).
    pub fn new(dcomp: &IDCompositionDesktopDevice, layer: Layer, frame: Rect) -> Result<Self> {
        register();
        let topmost = match layer {
            Layer::Popup => WS_EX_TOPMOST,
            Layer::Desktop => WINDOW_EX_STYLE::default(),
        };
        // SAFETY: class registered above.
        let hwnd = unsafe {
            CreateWindowExW(
                topmost
                    | WS_EX_TOOLWINDOW
                    | WS_EX_LAYERED
                    | WS_EX_TRANSPARENT
                    | WS_EX_NOREDIRECTIONBITMAP
                    | WS_EX_NOACTIVATE,
                CLASS,
                w!("mypaper glass"),
                WS_POPUP,
                frame.x,
                frame.y,
                frame.width,
                frame.height,
                None,
                None,
                Some(GetModuleHandleW(None)?.into()),
                None,
            )
            .context("CreateWindowExW (glass overlay)")?
        };
        let hwnd = Owned(hwnd);
        // A layered window shows nothing until its attributes are set; with no redirection bitmap
        // this only lets the DirectComposition content show. (Layered and transparent is what
        // makes clicks pass through.)
        // SAFETY: our own window.
        unsafe { SetLayeredWindowAttributes(hwnd.0, Default::default(), 255, LWA_ALPHA)? };

        let size = pixels(frame);
        // SAFETY: DirectComposition calls on a live device and our own window.
        let (target, visual, clip, surface) = unsafe {
            let target = dcomp.CreateTargetForHwnd(hwnd.0, true)?;
            let visual = dcomp.CreateVisual()?;
            let clip = dcomp.CreateRectangleClip()?;
            let surface = surface(dcomp, size)?;
            visual.SetContent(&surface)?;
            visual.SetClip(&clip)?;
            visual.cast::<IDCompositionVisual3>()?.SetOpacity2(0.0)?;
            target.SetRoot(&visual)?;
            (target, visual, clip, surface)
        };
        Ok(Self {
            surface,
            size,
            clip,
            visual,
            _target: target,
            hwnd,
        })
    }

    pub fn hwnd(&self) -> HWND {
        self.hwnd.0
    }

    /// Moves the window over `frame`, directly under `client`, and clips the glass to `plate`
    /// (frame-local) with `radius` corners. A new size needs a new surface (drawn before the
    /// next commit). Takes effect at the next commit.
    pub fn place(
        &mut self,
        dcomp: &IDCompositionDesktopDevice,
        client: HWND,
        frame: Rect,
        plate: Rect,
        radius: f32,
        smoothing: f32,
    ) -> Result<()> {
        let size = pixels(frame);
        if size != self.size {
            // SAFETY: DirectComposition calls on live objects.
            unsafe {
                self.surface = surface(dcomp, size)?;
                self.visual.SetContent(&self.surface)?;
            }
            self.size = size;
        }
        move_under(self.hwnd.0, client, frame, false)?;
        self.set_clip(plate, radius, smoothing)
    }

    /// Changes only the clip. The caller commits it together with the matching shader frame
    /// so restyling a visible overlay never exposes the previous contour for a frame.
    pub fn set_clip(&self, plate: Rect, radius: f32, smoothing: f32) -> Result<()> {
        let (x0, y0) = (plate.x as f32, plate.y as f32);
        let (x1, y1) = (x0 + plate.width as f32, y0 + plate.height as f32);
        // Continuous corners are clipped by the glass shader's own coverage. A circular
        // compositor clip cuts into that contour even when its nominal radius agrees.
        let r = if smoothing > 0.0 {
            0.0
        } else {
            radius.max(0.0)
        };
        // SAFETY: DirectComposition calls on a live clip.
        unsafe {
            self.clip.SetLeft2(x0)?;
            self.clip.SetTop2(y0)?;
            self.clip.SetRight2(x1)?;
            self.clip.SetBottom2(y1)?;
            for set in [
                IDCompositionRectangleClip::SetTopLeftRadiusX2,
                IDCompositionRectangleClip::SetTopLeftRadiusY2,
                IDCompositionRectangleClip::SetTopRightRadiusX2,
                IDCompositionRectangleClip::SetTopRightRadiusY2,
                IDCompositionRectangleClip::SetBottomLeftRadiusX2,
                IDCompositionRectangleClip::SetBottomLeftRadiusY2,
                IDCompositionRectangleClip::SetBottomRightRadiusX2,
                IDCompositionRectangleClip::SetBottomRightRadiusY2,
            ] {
                set(&self.clip, r)?;
            }
        }
        Ok(())
    }

    /// Shows `glass` (drawn by wgpu, which the capture device has waited for). Takes effect at
    /// the next commit.
    pub fn show(&self, capture: &CaptureDevice, glass: &Crop) -> Result<()> {
        capture.present(glass, &self.surface)
    }

    /// Fades the window in over `fade` (the shell's, [`FADE`] by default), along the resolved
    /// curve in DirectComposition (no frames of ours); zero shows it at once. Takes
    /// effect at the next commit.
    pub fn fade_in(
        &self,
        dcomp: &IDCompositionDesktopDevice,
        fade: Duration,
        curve: FadeCurve,
    ) -> Result<()> {
        if fade.is_zero() {
            return self.settle();
        }
        let t = fade.as_secs_f32();
        // SAFETY: DirectComposition calls on live objects.
        unsafe {
            let fade: IDCompositionAnimation = dcomp.CreateAnimation()?;
            if curve == FadeCurve::DEFAULT {
                // Preserve the original curve exactly for old/default clients.
                fade.AddCubic(0.0, 0.0, 3.0 / t, -3.0 / (t * t), 1.0 / (t * t * t))?;
            } else {
                for [x0, y0, x1, y1] in curve.segments() {
                    fade.AddCubic(f64::from(x0) * f64::from(t), y0, (y1 - y0) / ((x1 - x0) * t), 0.0, 0.0)?;
                }
            }
            fade.End(f64::from(t), 1.0)?;
            self.visual
                .cast::<IDCompositionVisual3>()?
                .SetOpacity(&fade)?;
        }
        Ok(())
    }

    /// Replaces the finished fade with a plain opacity of 1. Takes effect at the next commit.
    pub fn settle(&self) -> Result<()> {
        // SAFETY: a DirectComposition call on a live visual.
        unsafe {
            self.visual
                .cast::<IDCompositionVisual3>()?
                .SetOpacity2(1.0)?
        };
        Ok(())
    }
}

/// Puts the overlay window `hwnd` over `frame`, directly under `client`; with `keep_size`, only
/// moves it. Also called from the request window's procedure: that is how a `Place` moves the
/// glass in the same frame as the client, before the engine's loop runs.
pub fn move_under(hwnd: HWND, client: HWND, frame: Rect, keep_size: bool) -> Result<()> {
    let mut flags = SWP_NOACTIVATE | SWP_NOCOPYBITS | SWP_SHOWWINDOW;
    if keep_size {
        flags |= SWP_NOSIZE;
    }
    // SAFETY: our own window, put directly under the client (`hWndInsertAfter`).
    unsafe {
        SetWindowPos(
            hwnd,
            Some(client),
            frame.x,
            frame.y,
            frame.width,
            frame.height,
            flags,
        )?;
    }
    Ok(())
}

/// A premultiplied BGRA surface of `size`.
///
/// # Safety
/// `dcomp` must be a live device with a rendering device.
unsafe fn surface(
    dcomp: &IDCompositionDesktopDevice,
    (w, h): (u32, u32),
) -> Result<IDCompositionSurface> {
    // SAFETY: the caller's contract.
    unsafe {
        Ok(dcomp.CreateSurface(
            w,
            h,
            DXGI_FORMAT_B8G8R8A8_UNORM,
            DXGI_ALPHA_MODE_PREMULTIPLIED,
        )?)
    }
}

fn pixels(frame: Rect) -> (u32, u32) {
    (frame.width.max(1) as u32, frame.height.max(1) as u32)
}

fn register() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        // SAFETY: plain Win32 registration with a 'static window procedure.
        unsafe {
            let class = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(wndproc),
                hInstance: GetModuleHandleW(None).unwrap_or_default().into(),
                lpszClassName: CLASS,
                ..Default::default()
            };
            RegisterClassExW(&class);
        }
    });
}

/// Never hit, never activated.
unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        // SAFETY: the system's own message, forwarded unchanged.
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}
