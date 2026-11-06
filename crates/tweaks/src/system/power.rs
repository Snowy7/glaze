//! Documented Windows 11 user-configured power modes, never a custom power plan.
//! Dynamic lookup keeps older Windows versions loadable when the option is absent.

use anyhow::{Context, Result, bail};
use mypaper_config::PowerMode;
use windows::Win32::Foundation::{FreeLibrary, WIN32_ERROR};
use windows::Win32::System::LibraryLoader::{
    GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
};
use windows::core::{GUID, PCSTR, w};

use crate::undo::{Value, Write};

/// A virtual Store namespace, not a registry path. The journal names the public API.
pub(super) const KEY: &str = "@mypaper/power-mode";

fn guid(mode: PowerMode) -> GUID {
    GUID::from_u128(match mode {
        PowerMode::Efficiency => 0x961cc777_2547_4f9d_8174_7d86181b8a7a,
        PowerMode::Balanced => 0,
        PowerMode::Performance => 0xded574b5_45a0_4f42_8737_46345c09c238,
    })
}

pub(super) fn writes(mode: PowerMode) -> [Write; 2] {
    ["ac", "dc"].map(|name| Write {
        key: KEY,
        name,
        value: encode(guid(mode)),
    })
}

fn encode(guid: GUID) -> Value {
    Value::Binary(guid.to_u128().to_le_bytes().to_vec())
}

fn api(name: &str, set: bool) -> Result<&'static [u8]> {
    match (name, set) {
        ("ac", false) => Ok(b"PowerGetUserConfiguredACPowerMode\0"),
        ("dc", false) => Ok(b"PowerGetUserConfiguredDCPowerMode\0"),
        ("ac", true) => Ok(b"PowerSetUserConfiguredACPowerMode\0"),
        ("dc", true) => Ok(b"PowerSetUserConfiguredDCPowerMode\0"),
        _ => bail!("unknown power-mode source {name}"),
    }
}

fn call(name: &[u8], guid: &mut GUID, set: bool) -> Result<()> {
    // SAFETY: fixed system DLL, with the search explicitly limited to System32.
    let library =
        unsafe { LoadLibraryExW(w!("powrprof.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32)? };
    // SAFETY: the DLL remains loaded until after invoking its exported function.
    let symbol = unsafe { GetProcAddress(library, PCSTR(name.as_ptr())) };
    let result = match symbol {
        Some(symbol) => {
            // SAFETY: names above designate documented DWORD(GUID*) getters or
            // DWORD(const GUID*) setters with the system ABI; the GUID is valid.
            let status = unsafe {
                if set {
                    let function: unsafe extern "system" fn(*const GUID) -> u32 =
                        std::mem::transmute(symbol);
                    function(guid)
                } else {
                    let function: unsafe extern "system" fn(*mut GUID) -> u32 =
                        std::mem::transmute(symbol);
                    function(guid)
                }
            };
            WIN32_ERROR(status)
                .ok()
                .context("Windows user-configured power mode")
        }
        None => Err(anyhow::anyhow!(
            "Windows does not expose {}",
            String::from_utf8_lossy(&name[..name.len() - 1])
        )),
    };
    // SAFETY: one release for the LoadLibraryExW above, after the function returns.
    let _ = unsafe { FreeLibrary(library) };
    result
}

pub(super) fn read(name: &str) -> Result<Value> {
    let mut guid = GUID::zeroed();
    call(api(name, false)?, &mut guid, false)?;
    Ok(encode(guid))
}

pub(super) fn write(name: &str, value: &Value) -> Result<()> {
    let Value::Binary(bytes) = value else {
        bail!("power mode journal must contain a GUID")
    };
    let bytes: [u8; 16] = bytes
        .as_slice()
        .try_into()
        .context("power mode GUID length")?;
    let mut guid = GUID::from_u128(u128::from_le_bytes(bytes));
    call(api(name, true)?, &mut guid, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn power_modes_use_the_documented_guids_and_separate_supply_originals() {
        for mode in [
            PowerMode::Efficiency,
            PowerMode::Balanced,
            PowerMode::Performance,
        ] {
            let writes = writes(mode);
            assert_eq!(writes[0].name, "ac");
            assert_eq!(writes[1].name, "dc");
            assert_eq!(writes[0].value, encode(guid(mode)));
            assert_eq!(writes[0].value, writes[1].value);
        }
        assert!(write("ac", &Value::Binary(vec![])).is_err());
        assert!(api("other", true).is_err());
    }
}
