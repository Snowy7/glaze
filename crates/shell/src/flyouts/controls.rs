//! Quick-setting controls with authoritative readback. Unsupported Windows settings are not
//! represented as switches. Reads run on the quick lane and never query slow display DDC/CI.
use mypaper_bars::flyouts::quick::{PowerMode, RadioInfo, RadioKind};
use windows::Devices::Radios::{Radio, RadioAccessStatus, RadioKind as Kind, RadioState};
use windows::Networking::Connectivity::NetworkInformation;
use windows::Networking::NetworkOperators::{
    NetworkOperatorTetheringManager, TetheringCapability, TetheringOperationStatus,
    TetheringOperationalState,
};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows::core::{GUID, s, w};

#[derive(Clone, Debug, PartialEq)]
pub struct State {
    pub radios: Vec<RadioInfo>,
    pub power: Option<PowerMode>,
    pub hotspot: Option<bool>,
}

fn radios() -> windows::core::Result<Vec<Radio>> {
    Ok(Radio::GetRadiosAsync()?.join()?.into_iter().collect())
}

fn kind(radio: &Radio) -> Option<RadioKind> {
    match radio.Kind().ok()? {
        Kind::WiFi => Some(RadioKind::Wifi),
        Kind::Bluetooth => Some(RadioKind::Bluetooth),
        Kind::MobileBroadband => Some(RadioKind::Cellular),
        _ => None,
    }
}

pub fn read() -> State {
    State {
        radios: radios()
            .unwrap_or_default()
            .iter()
            .filter_map(|radio| {
                Some(RadioInfo {
                    kind: kind(radio)?,
                    name: radio.Name().ok()?.to_string(),
                    on: radio.State().ok()? == RadioState::On,
                })
            })
            .collect(),
        power: power_mode(),
        hotspot: hotspot().ok().and_then(|manager| {
            match manager.TetheringOperationalState().ok()? {
                TetheringOperationalState::On => Some(true),
                TetheringOperationalState::Off => Some(false),
                _ => None,
            }
        }),
    }
}

/// Permission is requested by the UI thread, then awaited on the lane before writing.
pub fn set_radio(
    kind_wanted: RadioKind,
    on: bool,
    access: RadioAccessStatus,
) -> Result<(), String> {
    if access != RadioAccessStatus::Allowed {
        return Err(format!("Radio access: {access:?}"));
    }
    let radios: Vec<_> = radios()
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|radio| kind(radio) == Some(kind_wanted))
        .collect();
    if radios.is_empty() {
        return Err(format!("No {kind_wanted:?} radio"));
    }
    for radio in radios {
        let target = if on { RadioState::On } else { RadioState::Off };
        let result = radio
            .SetStateAsync(target)
            .and_then(|op| op.join())
            .map_err(|e| e.to_string())?;
        if result != RadioAccessStatus::Allowed {
            return Err(format!("Radio: {result:?}"));
        }
        if radio.State().map_err(|e| e.to_string())? != target {
            return Err(
                "Windows could not change the radio; check its hardware switch or policy".into(),
            );
        }
    }
    Ok(())
}

fn on_ac() -> Option<bool> {
    let mut status = SYSTEM_POWER_STATUS::default();
    // SAFETY: a writable out-parameter alive throughout the call.
    unsafe { GetSystemPowerStatus(&mut status) }.ok()?;
    match status.ACLineStatus {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}

type ReadMode = unsafe extern "system" fn(*mut GUID) -> u32;
type SetMode = unsafe extern "system" fn(*const GUID) -> u32;

/// Resolve Windows 11's documented exports at runtime; older systems hide the control.
fn power_api(ac: bool) -> Option<(ReadMode, SetMode)> {
    // SAFETY: powrprof is already imported by the provider. Names and ABI signatures below
    // are the documented exports; a missing export leaves the tile unavailable.
    unsafe {
        let module = GetModuleHandleW(w!("powrprof.dll")).ok()?;
        let read = GetProcAddress(
            module,
            if ac {
                s!("PowerGetUserConfiguredACPowerMode")
            } else {
                s!("PowerGetUserConfiguredDCPowerMode")
            },
        )?;
        let write = GetProcAddress(
            module,
            if ac {
                s!("PowerSetUserConfiguredACPowerMode")
            } else {
                s!("PowerSetUserConfiguredDCPowerMode")
            },
        )?;
        Some((
            std::mem::transmute::<unsafe extern "system" fn() -> isize, ReadMode>(read),
            std::mem::transmute::<unsafe extern "system" fn() -> isize, SetMode>(write),
        ))
    }
}

fn power_mode() -> Option<PowerMode> {
    let mut mode = GUID::default();
    let (read, _) = power_api(on_ac()?)?;
    // SAFETY: both documented functions write one GUID into a valid out-parameter.
    let status = unsafe { read(&mut mode) };
    (status == 0)
        .then(|| PowerMode::from_overlay(mode))
        .flatten()
}

pub fn set_power_mode(mode: PowerMode) -> Result<(), String> {
    let ac = on_ac().ok_or("Windows could not read the power source")?;
    let (_, write) =
        power_api(ac).ok_or("Power mode control is unavailable on this Windows version")?;
    let guid = mode.overlay();
    // SAFETY: the selected mode is one of the three documented GUIDs, alive for the call.
    let status = unsafe { write(&guid) };
    if status == 0 {
        Ok(())
    } else {
        Err(format!("Power mode: Windows error {status}"))
    }
}

fn hotspot() -> windows::core::Result<NetworkOperatorTetheringManager> {
    let profile = NetworkInformation::GetInternetConnectionProfile()?;
    let capability =
        NetworkOperatorTetheringManager::GetTetheringCapabilityFromConnectionProfile(&profile)?;
    if capability != TetheringCapability::Enabled {
        return Err(windows::core::Error::from_hresult(windows::core::HRESULT(
            0x80004001u32 as i32,
        )));
    }
    NetworkOperatorTetheringManager::CreateFromConnectionProfile(&profile)
}

pub fn set_hotspot(on: bool) -> Result<(), String> {
    let change = || -> windows::core::Result<()> {
        let manager = hotspot()?;
        let result = if on {
            manager.StartTetheringAsync()?.join()?
        } else {
            manager.StopTetheringAsync()?.join()?
        };
        let status = result.Status()?;
        if status != TetheringOperationStatus::Success {
            return Err(windows::core::Error::new(
                windows::core::HRESULT(0x80004005u32 as i32),
                format!(
                    "Mobile hotspot: {status:?} {}",
                    result.AdditionalErrorMessage()?
                ),
            ));
        }
        Ok(())
    };
    change().map_err(|e| e.to_string())
}
