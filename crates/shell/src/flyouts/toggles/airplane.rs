//! Airplane mode through the radio management service's `IRadioManager` (RMapi), the object
//! Windows' own airplane switch uses: "system radio state" enabled is airplane mode off.
//!
//! **Undocumented**: `CLSID_RadioManagementAPI` and `IRadioManager` are not in the SDK; only
//! the slots up to `SetSystemRadioState` are declared, in their vtable order. Windows persists
//! the result (`HKLM\SYSTEM\CurrentControlSet\Control\RadioManagement\SystemRadioState`).
#![allow(non_snake_case)]

use std::ffi::c_void;

use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
use windows::core::{GUID, HRESULT};

#[windows::core::interface("db3afbfb-08e6-46c6-aa70-bf9a34c30ab7")]
unsafe trait IRadioManager: windows::core::IUnknown {
    unsafe fn IsRMSupported(&self, state: *mut u32) -> HRESULT;
    unsafe fn GetUIRadioInstances(&self, list: *mut *mut c_void) -> HRESULT;
    unsafe fn GetSystemRadioState(
        &self,
        enabled: *mut i32,
        policy: *mut i32,
        reason: *mut i32,
    ) -> HRESULT;
    unsafe fn SetSystemRadioState(&self, enabled: i32) -> HRESULT;
}

const RADIO_MANAGEMENT_API: GUID = GUID::from_u128(0x581333f6_28db_41be_bc7a_ff201f12f3f6);

fn manager() -> windows::core::Result<IRadioManager> {
    // SAFETY: plain COM activation; the quick lane runs COM.
    unsafe { CoCreateInstance(&RADIO_MANAGEMENT_API, None, CLSCTX_ALL) }
}

/// Whether airplane mode is on, or `None` when the service doesn't answer.
pub fn read() -> Option<bool> {
    let (mut enabled, mut policy, mut reason) = (0, 0, 0);
    // SAFETY: three writable out-parameters alive for the call.
    unsafe {
        manager()
            .ok()?
            .GetSystemRadioState(&mut enabled, &mut policy, &mut reason)
            .ok()
            .ok()?;
    }
    Some(enabled == 0)
}

/// Turns airplane mode on (every radio off) or off, and reads it back.
pub fn set(on: bool) -> Result<(), String> {
    let manager = manager().map_err(|e| format!("Airplane mode: {e}"))?;
    // SAFETY: a plain call on a live object.
    unsafe { manager.SetSystemRadioState(i32::from(!on)) }
        .ok()
        .map_err(|e| format!("Airplane mode: {e}"))?;
    super::confirm("airplane mode", on, read())
}
