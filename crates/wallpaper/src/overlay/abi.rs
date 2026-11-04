//! The two Windows.Graphics.Capture interfaces that `windows` 0.62 and the installed SDK
//! (10.0.26100.0) lack, both added in SDK 10.0.26100.8249: the per-session window exclusion
//! list, and the configuration iteration a frame was rendered with. IIDs and vtable order are
//! from the Windows metadata (as generated on windows-rs master). Support is detected by
//! `QueryInterface`, never by build number.

#![allow(non_snake_case)]

use std::ffi::c_void;

use windows::Graphics::Capture::{Direct3D11CaptureFrame, GraphicsCaptureSession};
use windows::UI::WindowId;
use windows::Win32::Foundation::HWND;
use windows_collections::IIterable;
use windows_core::{GUID, HRESULT, IUnknown, IUnknown_Vtbl, Interface, interface};

// Both are WinRT interfaces (IInspectable-based). The `interface` macro only derives from
// IUnknown-style parents, so IInspectable's three methods are spelled out as the first slots;
// they are never called.

/// `Windows.Graphics.Capture.IDisplayGraphicsCaptureSession`: only sessions on a display item
/// implement it.
#[interface("bb91f61b-218a-587d-8580-2701a74c0525")]
unsafe trait IDisplayGraphicsCaptureSession: IUnknown {
    fn GetIids(&self, count: *mut u32, iids: *mut *mut GUID) -> HRESULT;
    fn GetRuntimeClassName(&self, name: *mut *mut c_void) -> HRESULT;
    fn GetTrustLevel(&self, level: *mut i32) -> HRESULT;
    /// `IIterable<WindowId>` in; the configuration iteration that applies it out.
    fn SetWindowExclusionList(&self, windows: *mut c_void, iteration: *mut u64) -> HRESULT;
    fn GetWindowExclusionList(&self, list: *mut *mut c_void) -> HRESULT;
}

/// `Windows.Graphics.Capture.IDirect3D11CaptureFrame3`.
#[interface("71616dc8-fea5-5741-a3d8-591acc39a9ee")]
unsafe trait IDirect3D11CaptureFrame3: IUnknown {
    fn GetIids(&self, count: *mut u32, iids: *mut *mut GUID) -> HRESULT;
    fn GetRuntimeClassName(&self, name: *mut *mut c_void) -> HRESULT;
    fn GetTrustLevel(&self, level: *mut i32) -> HRESULT;
    fn ConfigurationIteration(&self, value: *mut u64) -> HRESULT;
}

/// A display session's exclusion list, if this Windows has the API.
pub struct Exclusion(IDisplayGraphicsCaptureSession);

impl Exclusion {
    /// `None` when the session does not implement the interface (an older Windows, or a
    /// window item).
    pub fn of(session: &GraphicsCaptureSession) -> Option<Self> {
        session.cast().ok().map(Self)
    }

    /// Replaces the list; returns the configuration iteration from which frames honour it.
    pub fn set(&self, windows: &[HWND]) -> windows_core::Result<u64> {
        let ids: Vec<WindowId> = windows
            .iter()
            .map(|w| WindowId {
                Value: w.0 as usize as u64,
            })
            .collect();
        let list = IIterable::<WindowId>::from(ids);
        let mut iteration = 0;
        // SAFETY: a live COM interface; `list` outlives the call, which AddRefs what it keeps.
        unsafe { self.0.SetWindowExclusionList(list.as_raw(), &mut iteration) }.ok()?;
        Ok(iteration)
    }
}

/// The configuration iteration `frame` was rendered with; 0 when Windows doesn't report one
/// (then no exclusion list is in use either).
pub fn iteration(frame: &Direct3D11CaptureFrame) -> u64 {
    let Ok(frame) = frame.cast::<IDirect3D11CaptureFrame3>() else {
        return 0;
    };
    let mut value = 0;
    // SAFETY: a live COM interface and a valid out pointer.
    let _ = unsafe { frame.ConfigurationIteration(&mut value) };
    value
}
