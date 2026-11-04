//! Quick settings (docs/bars.md, "Flyouts" → "Quick settings"), on the `quick` lane:
//!
//! - **Radios**: Wi-Fi and Bluetooth on/off through `Windows.Devices.Radios` (documented; the
//!   user may be asked for consent, and policy may forbid it).
//! - **Brightness**: the built-in panel through WMI (`WmiMonitorBrightness`,
//!   `WmiMonitorBrightnessMethods.WmiSetBrightness`), external monitors through DDC/CI
//!   (`GetMonitorBrightness` / `SetMonitorBrightness`). DDC/CI takes tens of milliseconds per
//!   call, which is why this is a lane.
//! - **Power mode**: the Settings slider's overlay schemes (`PowerGetEffectiveOverlayScheme`,
//!   `PowerSetActiveOverlayScheme` from powrprof.dll; not in the SDK headers).
//! - **Battery saver** (read; Settings turns it on), **do not disturb** and **focus** (read:
//!   writing them has no documented API).

use serde::{Deserialize, Serialize};
use windows::Devices::Radios::{Radio, RadioKind as WinRadioKind, RadioState};
use windows::UI::Notifications::{ToastNotificationManager, ToastNotificationMode};
use windows::UI::Shell::FocusSessionManager;
use windows::Win32::Devices::Display::{
    DestroyPhysicalMonitors, GetMonitorBrightness, GetNumberOfPhysicalMonitorsFromHMONITOR,
    GetPhysicalMonitorsFromHMONITOR, PHYSICAL_MONITOR, SetMonitorBrightness,
};
use windows::Win32::Foundation::{LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, HDC, HMONITOR};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
};
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::System::Wmi::{
    IWbemClassObject, IWbemLocator, IWbemServices, WBEM_FLAG_FORWARD_ONLY,
    WBEM_FLAG_RETURN_IMMEDIATELY, WBEM_GENERIC_FLAG_TYPE, WBEM_INFINITE, WbemLocator,
};
use windows::core::{BOOL, BSTR, GUID, w};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RadioKind {
    Wifi,
    Bluetooth,
    Cellular,
}

impl RadioKind {
    fn of(kind: WinRadioKind) -> Option<Self> {
        match kind {
            WinRadioKind::WiFi => Some(Self::Wifi),
            WinRadioKind::Bluetooth => Some(Self::Bluetooth),
            WinRadioKind::MobileBroadband => Some(Self::Cellular),
            _ => None,
        }
    }
}

/// The power mode slider's three positions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PowerMode {
    Efficiency,
    Balanced,
    Performance,
}

const EFFICIENCY: GUID = GUID::from_u128(0x961cc777_2547_4f9d_8174_7d86181b8a7a);
const BALANCED: GUID = GUID::from_u128(0);
const PERFORMANCE: GUID = GUID::from_u128(0xded574b5_45a0_4f42_8737_46345c09c238);
/// Windows 10's "better performance", which Windows 11 shows as balanced.
const BETTER_PERFORMANCE: GUID = GUID::from_u128(0x3af9b8d9_7c97_431d_ad78_34a8bfea439f);

impl PowerMode {
    pub fn from_overlay(guid: GUID) -> Option<Self> {
        match guid {
            EFFICIENCY => Some(Self::Efficiency),
            BALANCED | BETTER_PERFORMANCE => Some(Self::Balanced),
            PERFORMANCE => Some(Self::Performance),
            _ => None,
        }
    }

    pub fn overlay(self) -> GUID {
        match self {
            Self::Efficiency => EFFICIENCY,
            Self::Balanced => BALANCED,
            Self::Performance => PERFORMANCE,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RadioInfo {
    pub kind: RadioKind,
    pub name: String,
    pub on: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Dnd {
    /// Every notification shows.
    Off,
    /// Priority only.
    Priority,
    /// Alarms only.
    Alarms,
}

/// The quick settings, as the flyout shows them. Missing parts aren't available here.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Quick {
    pub radios: Vec<RadioInfo>,
    /// 0–100, the first display that has a control.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub brightness: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub power: Option<PowerMode>,
    /// Battery saver, on machines with a battery.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saver: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dnd: Option<Dnd>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub focus: Option<bool>,
}

/// The lane's state.
pub struct QuickLane {
    wmi: Option<IWbemServices>,
}

impl QuickLane {
    /// Runs on the lane: COM (MTA: WinRT radios and WMI are fine there).
    pub fn new() -> Self {
        // SAFETY: COM for this thread, kept for its life.
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        Self { wmi: connect_wmi() }
    }

    pub fn read(&self) -> Quick {
        Quick {
            radios: radios()
                .into_iter()
                .filter_map(|radio| {
                    Some(RadioInfo {
                        kind: RadioKind::of(radio.Kind().ok()?)?,
                        name: radio.Name().map(|n| n.to_string()).unwrap_or_default(),
                        on: radio.State().ok()? == RadioState::On,
                    })
                })
                .collect(),
            brightness: self.wmi_brightness().or_else(ddc_brightness),
            power: power_mode(),
            saver: battery_saver(),
            dnd: ToastNotificationManager::GetDefault()
                .and_then(|m| m.NotificationMode())
                .ok()
                .map(|mode| match mode {
                    ToastNotificationMode::PriorityOnly => Dnd::Priority,
                    ToastNotificationMode::AlarmsOnly => Dnd::Alarms,
                    _ => Dnd::Off,
                }),
            focus: FocusSessionManager::IsSupported()
                .ok()
                .filter(|&supported| supported)
                .and_then(|_| FocusSessionManager::GetDefault().ok())
                .and_then(|m| m.IsFocusActive().ok()),
        }
    }

    pub fn set_radio(&self, kind: RadioKind, on: bool) -> Result<(), String> {
        match Radio::RequestAccessAsync().and_then(|op| op.join()) {
            Ok(windows::Devices::Radios::RadioAccessStatus::Allowed) => {}
            Ok(status) => return Err(format!("radio access: {status:?}")),
            Err(e) => return Err(e.to_string()),
        }
        let state = if on { RadioState::On } else { RadioState::Off };
        let mut found = false;
        for radio in radios() {
            if radio.Kind().ok().and_then(RadioKind::of) == Some(kind) {
                found = true;
                let status = radio
                    .SetStateAsync(state)
                    .and_then(|op| op.join())
                    .map_err(|e| e.to_string())?;
                if status != windows::Devices::Radios::RadioAccessStatus::Allowed {
                    return Err(format!("radio: {status:?}"));
                }
            }
        }
        if found {
            Ok(())
        } else {
            Err(format!("no {kind:?} radio"))
        }
    }

    pub fn set_brightness(&self, level: u32) -> Result<(), String> {
        if self.wmi_set_brightness(level) || ddc_set_brightness(level) {
            Ok(())
        } else {
            Err("no display with a brightness control".into())
        }
    }

    fn wmi_brightness(&self) -> Option<u32> {
        let wmi = self.wmi.as_ref()?;
        let object = first(wmi, "SELECT CurrentBrightness FROM WmiMonitorBrightness")?;
        let mut value = VARIANT::default();
        // SAFETY: a COM call on a live object; `value` outlives it.
        unsafe { object.Get(w!("CurrentBrightness"), 0, &mut value, None, None) }.ok()?;
        variant_u32(&value)
    }

    /// `WmiMonitorBrightnessMethods.WmiSetBrightness(Timeout, Brightness)`.
    fn wmi_set_brightness(&self, level: u32) -> bool {
        let Some(wmi) = &self.wmi else { return false };
        let set = || -> Option<()> {
            let instance = first(wmi, "SELECT * FROM WmiMonitorBrightnessMethods")?;
            let mut path = VARIANT::default();
            // SAFETY: COM calls on live WMI objects; every out-pointer outlives its call.
            unsafe {
                instance.Get(w!("__PATH"), 0, &mut path, None, None).ok()?;
                let path = BSTR::from(path.to_string());
                let mut class = None;
                wmi.GetObject(
                    &BSTR::from("WmiMonitorBrightnessMethods"),
                    WBEM_GENERIC_FLAG_TYPE(0),
                    None,
                    Some(&mut class),
                    None,
                )
                .ok()?;
                let mut inputs = None;
                class?
                    .GetMethod(w!("WmiSetBrightness"), 0, &mut inputs, std::ptr::null_mut())
                    .ok()?;
                let params = inputs?.SpawnInstance(0).ok()?;
                params.Put(w!("Timeout"), 0, &VARIANT::from(0i32), 0).ok()?;
                params
                    .Put(w!("Brightness"), 0, &VARIANT::from(level as i32), 0)
                    .ok()?;
                wmi.ExecMethod(
                    &path,
                    &BSTR::from("WmiSetBrightness"),
                    WBEM_GENERIC_FLAG_TYPE(0),
                    None,
                    &params,
                    None,
                    None,
                )
                .ok()?;
            }
            Some(())
        };
        set().is_some()
    }
}

fn radios() -> Vec<Radio> {
    Radio::GetRadiosAsync()
        .and_then(|op| op.join())
        .map(|list| list.into_iter().collect())
        .unwrap_or_default()
}

fn connect_wmi() -> Option<IWbemServices> {
    // SAFETY: plain COM creation and a local connection.
    unsafe {
        let locator: IWbemLocator =
            CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER).ok()?;
        locator
            .ConnectServer(
                &BSTR::from(r"root\WMI"),
                &BSTR::new(),
                &BSTR::new(),
                &BSTR::new(),
                0,
                &BSTR::new(),
                None,
            )
            .ok()
    }
}

/// The first object a WQL query returns.
fn first(wmi: &IWbemServices, query: &str) -> Option<IWbemClassObject> {
    // SAFETY: COM calls on live WMI objects.
    unsafe {
        let results = wmi
            .ExecQuery(
                &BSTR::from("WQL"),
                &BSTR::from(query),
                WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_IMMEDIATELY,
                None,
            )
            .ok()?;
        let mut objects = [None];
        let mut returned = 0;
        results
            .Next(WBEM_INFINITE, &mut objects, &mut returned)
            .ok()
            .ok()?;
        objects[0].take()
    }
}

fn variant_u32(value: &VARIANT) -> Option<u32> {
    value.to_string().trim().parse().ok()
}

/// Every physical monitor that answers DDC/CI, with its brightness as a percent.
fn with_physical_monitors<T>(
    mut f: impl FnMut(&PHYSICAL_MONITOR, u32, u32, u32) -> Option<T>,
) -> Vec<T> {
    unsafe extern "system" fn collect(
        monitor: HMONITOR,
        _: HDC,
        _: *mut RECT,
        out: LPARAM,
    ) -> BOOL {
        // SAFETY: `out` is the Vec passed below, alive for the enumeration.
        unsafe { (*(out.0 as *mut Vec<HMONITOR>)).push(monitor) };
        true.into()
    }
    let mut monitors: Vec<HMONITOR> = Vec::new();
    let mut out = Vec::new();
    // SAFETY: the callback only runs during this call; physical monitor handles are destroyed
    // after use.
    unsafe {
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(collect),
            LPARAM(&raw mut monitors as isize),
        );
        for monitor in monitors {
            let mut count = 0;
            if GetNumberOfPhysicalMonitorsFromHMONITOR(monitor, &mut count).is_err() || count == 0 {
                continue;
            }
            let mut physical = vec![PHYSICAL_MONITOR::default(); count as usize];
            if GetPhysicalMonitorsFromHMONITOR(monitor, &mut physical).is_err() {
                continue;
            }
            for one in &physical {
                let (mut min, mut current, mut max) = (0, 0, 0);
                if GetMonitorBrightness(one.hPhysicalMonitor, &mut min, &mut current, &mut max) != 0
                    && max > min
                    && let Some(value) = f(one, min, current, max)
                {
                    out.push(value);
                }
            }
            let _ = DestroyPhysicalMonitors(&physical);
        }
    }
    out
}

fn ddc_brightness() -> Option<u32> {
    with_physical_monitors(|_, min, current, max| Some(percent(min, current, max)))
        .into_iter()
        .next()
}

fn ddc_set_brightness(level: u32) -> bool {
    !with_physical_monitors(|monitor, min, _, max| {
        let value = min + (max - min) * level.min(100) / 100;
        // SAFETY: a live physical monitor handle, during the enumeration.
        (unsafe { SetMonitorBrightness(monitor.hPhysicalMonitor, value) } != 0).then_some(())
    })
    .is_empty()
}

/// A DDC/CI value as a percent of its range.
pub fn percent(min: u32, current: u32, max: u32) -> u32 {
    if max <= min {
        return 0;
    }
    (current.clamp(min, max) - min) * 100 / (max - min)
}

fn power_mode() -> Option<PowerMode> {
    windows_core::link!("powrprof.dll" "system" fn PowerGetEffectiveOverlayScheme(effectiveoverlaypolicyguid: *mut GUID) -> u32);
    let mut guid = GUID::default();
    // SAFETY: an out-pointer valid for the call.
    let status = unsafe { PowerGetEffectiveOverlayScheme(&mut guid) };
    (status == 0)
        .then(|| PowerMode::from_overlay(guid))
        .flatten()
}

pub fn set_power_mode(mode: PowerMode) -> Result<(), String> {
    windows_core::link!("powrprof.dll" "system" fn PowerSetActiveOverlayScheme(overlayschemeguid: GUID) -> u32);
    // SAFETY: a plain call with a GUID by value.
    match unsafe { PowerSetActiveOverlayScheme(mode.overlay()) } {
        0 => Ok(()),
        status => Err(format!("PowerSetActiveOverlayScheme: error {status}")),
    }
}

fn battery_saver() -> Option<bool> {
    let mut status = SYSTEM_POWER_STATUS::default();
    // SAFETY: `status` outlives the call.
    unsafe { GetSystemPowerStatus(&mut status) }.ok()?;
    // No battery (128) or unknown (255): nothing to save.
    let battery = status.BatteryFlag & 128 == 0 && status.BatteryFlag != 255;
    battery.then_some(status.SystemStatusFlag == 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn power_modes_map_to_the_settings_slider() {
        for mode in [
            PowerMode::Efficiency,
            PowerMode::Balanced,
            PowerMode::Performance,
        ] {
            assert_eq!(PowerMode::from_overlay(mode.overlay()), Some(mode));
        }
        assert_eq!(
            PowerMode::from_overlay(BETTER_PERFORMANCE),
            Some(PowerMode::Balanced)
        );
        assert_eq!(PowerMode::from_overlay(GUID::from_u128(7)), None);
    }

    #[test]
    fn ddc_values_become_percents() {
        assert_eq!(percent(0, 50, 100), 50);
        assert_eq!(percent(10, 10, 20), 0);
        assert_eq!(percent(10, 25, 20), 100, "clamped");
        assert_eq!(percent(5, 5, 5), 0, "no range");
    }
}

impl Default for QuickLane {
    fn default() -> Self {
        Self::new()
    }
}
