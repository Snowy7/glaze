//! Spike (docs/bars.md, "Live blur"): does DWM's host backdrop brush give a Win32 popup real blur
//! of what's behind it, while the window is never activated, clipped to anti-aliased rounded
//! rects, with a saturation effect on top? Shows a never-activated topmost window on the
//! secondary monitor for `seconds` (default 6) with three glass rects, then exits.
//!
//! ```text
//! cargo run -p mypaper-bars --example glass-spike [-- seconds]
//! ```

use std::time::{Duration, Instant};

use windows_numerics::{Vector2, Vector3};
use windows::Foundation::{IPropertyValue, PropertyValue};
use windows::Graphics::Effects::{IGraphicsEffect, IGraphicsEffect_Impl, IGraphicsEffectSource, IGraphicsEffectSource_Impl};
use windows::UI::Color;
use windows::UI::Composition::{CompositionEffectSourceParameter, Compositor};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Dwm::{DWMWA_USE_HOSTBACKDROPBRUSH, DwmSetWindowAttribute};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::WinRT::Composition::ICompositorDesktopInterop;
use windows::Win32::System::WinRT::Graphics::Direct2D::{
    GRAPHICS_EFFECT_PROPERTY_MAPPING, IGraphicsEffectD2D1Interop, IGraphicsEffectD2D1Interop_Impl,
};
use windows::Win32::System::WinRT::{
    CreateDispatcherQueueController, DQTAT_COM_NONE, DQTYPE_THREAD_CURRENT, DispatcherQueueOptions,
};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{BOOL, GUID, HSTRING, Interface, PCWSTR, implement, w};

/// D2D1's color matrix effect (CLSID_D2D1ColorMatrix).
const COLOR_MATRIX: GUID = GUID::from_u128(0x921f03d6_641c_47df_852d_b4bb6153ae11);

#[implement(IGraphicsEffect, IGraphicsEffectSource, IGraphicsEffectD2D1Interop)]
struct ColorMatrix {
    matrix: [f32; 20],
    source: IGraphicsEffectSource,
}

impl IGraphicsEffect_Impl for ColorMatrix_Impl {
    fn Name(&self) -> windows::core::Result<HSTRING> {
        Ok(HSTRING::from("matrix"))
    }
    fn SetName(&self, _: &HSTRING) -> windows::core::Result<()> {
        Ok(())
    }
}
impl IGraphicsEffectSource_Impl for ColorMatrix_Impl {}
impl IGraphicsEffectD2D1Interop_Impl for ColorMatrix_Impl {
    fn GetEffectId(&self) -> windows::core::Result<GUID> {
        Ok(COLOR_MATRIX)
    }
    fn GetNamedPropertyMapping(
        &self,
        _: &PCWSTR,
        _: *mut u32,
        _: *mut GRAPHICS_EFFECT_PROPERTY_MAPPING,
    ) -> windows::core::Result<()> {
        Err(windows::Win32::Foundation::E_INVALIDARG.into())
    }
    fn GetPropertyCount(&self) -> windows::core::Result<u32> {
        Ok(3)
    }
    fn GetProperty(&self, index: u32) -> windows::core::Result<IPropertyValue> {
        match index {
            0 => PropertyValue::CreateSingleArray(&self.matrix)?.cast(),
            1 => PropertyValue::CreateUInt32(1)?.cast(), // premultiplied
            _ => PropertyValue::CreateBoolean(false)?.cast(),
        }
    }
    fn GetSource(&self, _: u32) -> windows::core::Result<IGraphicsEffectSource> {
        Ok(self.source.clone())
    }
    fn GetSourceCount(&self) -> windows::core::Result<u32> {
        Ok(1)
    }
}

/// CSS `saturate(s)` then `brightness(b)` as a D2D 5x4 matrix (row vector times matrix).
fn saturate(s: f32, b: f32) -> [f32; 20] {
    let (r, g, bl) = (0.213, 0.715, 0.072);
    let rows = [
        [r + (1.0 - r) * s, r - r * s, r - r * s, 0.0],
        [g - g * s, g + (1.0 - g) * s, g - g * s, 0.0],
        [bl - bl * s, bl - bl * s, bl + (1.0 - bl) * s, 0.0],
        [0.0, 0.0, 0.0, 1.0],
        [0.0, 0.0, 0.0, 0.0],
    ];
    let mut out = [0.0; 20];
    for (i, row) in rows.iter().enumerate() {
        for (j, v) in row.iter().enumerate() {
            out[i * 4 + j] = if i < 3 && j < 3 { v * b } else { *v };
        }
    }
    out
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_ERASEBKGND => LRESULT(1),
        _ => unsafe { DefWindowProcW(hwnd, msg, w, l) },
    }
}

fn main() -> windows::core::Result<()> {
    let seconds: u64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(6);
    unsafe {
        windows::Win32::UI::HiDpi::SetProcessDpiAwarenessContext(
            windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        )?;
        let _queue = CreateDispatcherQueueController(DispatcherQueueOptions {
            dwSize: size_of::<DispatcherQueueOptions>() as u32,
            threadType: DQTYPE_THREAD_CURRENT,
            apartmentType: DQTAT_COM_NONE,
        })?;
        let instance = GetModuleHandleW(None)?.into();
        let class = w!("mypaper.glass.spike");
        RegisterClassExW(&WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(proc),
            hInstance: instance,
            lpszClassName: class,
            ..Default::default()
        });
        // On the secondary monitor (left of the primary on this machine), near its top.
        let (x, y, width, height) = (-2560, 9 + 20, 1500, 420);
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_NOREDIRECTIONBITMAP,
            class,
            w!("glass spike"),
            WS_POPUP,
            x,
            y,
            width,
            height,
            None,
            None,
            None,
            None,
        )?;
        let on = BOOL(1);
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_HOSTBACKDROPBRUSH,
            &on as *const _ as _,
            size_of::<BOOL>() as u32,
        )?;

        let compositor = Compositor::new()?;
        let interop: ICompositorDesktopInterop = compositor.cast()?;
        let target = interop.CreateDesktopWindowTarget(hwnd, false)?;
        let root = compositor.CreateContainerVisual()?;
        root.SetRelativeSizeAdjustment(Vector2 { X: 1.0, Y: 1.0 })?;
        target.SetRoot(&root)?;

        let glass = |index: f32, brush: &windows::UI::Composition::CompositionBrush, radius: f32| {
            let sprite = compositor.CreateSpriteVisual()?;
            sprite.SetBrush(brush)?;
            let size = Vector2 { X: 440.0, Y: 300.0 };
            sprite.SetOffset(Vector3 { X: 30.0 + index * 480.0, Y: 60.0, Z: 0.0 })?;
            sprite.SetSize(size)?;
            let shape = compositor.CreateRoundedRectangleGeometry()?;
            shape.SetSize(size)?;
            shape.SetCornerRadius(Vector2 { X: radius, Y: radius })?;
            sprite.SetClip(&compositor.CreateGeometricClipWithGeometry(&shape)?)?;
            root.Children()?.InsertAtTop(&sprite)?;
            windows::core::Result::Ok(sprite)
        };
        let plain = compositor.CreateHostBackdropBrush()?;
        let _a = glass(0.0, &plain.cast()?, 24.0)?;

        let source: IGraphicsEffectSource = CompositionEffectSourceParameter::Create(&HSTRING::from("backdrop"))?.cast()?;
        let effect: IGraphicsEffect = ColorMatrix {
            matrix: saturate(1.9, 1.05),
            source,
        }
        .into();
        let factory = compositor.CreateEffectFactory(&effect)?;
        let saturated = factory.CreateBrush()?;
        saturated.SetSourceParameter(&HSTRING::from("backdrop"), &compositor.CreateHostBackdropBrush()?)?;
        let _b = glass(1.0, &saturated.cast()?, 24.0)?;

        // A tinted one: the saturated glass with a light tint over it (what the page would draw).
        let _c = glass(2.0, &saturated.cast()?, 40.0)?;
        let tint = compositor.CreateSpriteVisual()?;
        tint.SetBrush(&compositor.CreateColorBrushWithColor(Color { A: 90, R: 245, G: 245, B: 250 })?)?;
        tint.SetOffset(Vector3 { X: 30.0 + 2.0 * 480.0, Y: 60.0, Z: 0.0 })?;
        tint.SetSize(Vector2 { X: 440.0, Y: 300.0 })?;
        let shape = compositor.CreateRoundedRectangleGeometry()?;
        shape.SetSize(Vector2 { X: 440.0, Y: 300.0 })?;
        shape.SetCornerRadius(Vector2 { X: 40.0, Y: 40.0 })?;
        tint.SetClip(&compositor.CreateGeometricClipWithGeometry(&shape)?)?;
        root.Children()?.InsertAtTop(&tint)?;

        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        let end = Instant::now() + Duration::from_secs(seconds);
        let mut msg = MSG::default();
        while Instant::now() < end {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            std::thread::sleep(Duration::from_millis(15));
        }
        let _ = DestroyWindow(hwnd);
    }
    Ok(())
}
