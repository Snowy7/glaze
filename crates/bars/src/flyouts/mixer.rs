//! The audio mixer (docs/bars.md, "Flyouts" → "Mixer"), on the `mixer` lane (MTA, as Core Audio's
//! session notifications want): the output devices, and every app playing on the default one
//! with its own volume and mute (`IAudioSessionManager2`, `ISimpleAudioVolume`). Sessions coming,
//! going or changing volume (`IAudioSessionNotification`, `IAudioSessionEvents`) and the default
//! device changing (`IMMNotificationClient`) refresh the flyout while it's open.
//!
//! Switching the default output uses `IPolicyConfig`, an **undocumented** COM interface (what the
//! Sound settings page uses), declared here by hand; only with `audio.switchOutput`.

use std::path::PathBuf;

use serde::Serialize;
use windows::Win32::Foundation::{CloseHandle, PROPERTYKEY};
use windows::Win32::Graphics::Imaging::{CLSID_WICImagingFactory, IWICImagingFactory};
use windows::Win32::Media::Audio::{
    AudioSessionDisconnectReason, AudioSessionState, AudioSessionStateExpired, DEVICE_STATE,
    DEVICE_STATE_ACTIVE, EDataFlow, ERole, IAudioSessionControl, IAudioSessionControl2,
    IAudioSessionEvents, IAudioSessionEvents_Impl, IAudioSessionManager2,
    IAudioSessionNotification, IAudioSessionNotification_Impl, IMMDevice, IMMDeviceEnumerator,
    IMMNotificationClient, IMMNotificationClient_Impl, ISimpleAudioVolume, MMDeviceEnumerator,
    eConsole, eRender,
};
use windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc;
use windows::Win32::System::Com::{
    CLSCTX_ALL, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
    CoTaskMemFree, STGM_READ,
};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::Shell::SHLoadIndirectString;
use windows::core::{BOOL, GUID, Interface, PCWSTR, PWSTR, implement};

use crate::flyouts::{Done, Watched};
use crate::wake::{Msg, Waker};

/// `PKEY_Device_FriendlyName`.
const FRIENDLY_NAME: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0xa45c254e_df1c_4efd_8020_67d146a850e0),
    pid: 14,
};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub default: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Session {
    /// The session's instance identifier.
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// 0–1.
    pub level: f32,
    pub muted: bool,
    /// Windows' own sounds.
    pub system: bool,
    /// Playing right now.
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MixerState {
    pub devices: Vec<Device>,
    pub sessions: Vec<Session>,
    /// Whether the default output can be switched from here (`audio.switchOutput`).
    pub switchable: bool,
}

/// Orders the sessions as the flyout lists them: system sounds first, then playing apps, then
/// the rest, each by name.
pub fn order(sessions: &mut [Session]) {
    sessions.sort_by_cached_key(|s| (!s.system, !s.active, s.name.to_lowercase()));
}

/// Keeps the first of two sessions with the same instance id (Windows can briefly list one
/// twice). Each app's sessions keep their own rows: they have their own volumes.
pub fn dedupe(sessions: &mut Vec<Session>) {
    let mut seen = std::collections::HashSet::new();
    sessions.retain(|s| seen.insert(s.id.clone()));
}

#[implement(IAudioSessionNotification)]
struct Created(Waker);

impl IAudioSessionNotification_Impl for Created_Impl {
    fn OnSessionCreated(
        &self,
        _: windows::core::Ref<IAudioSessionControl>,
    ) -> windows::core::Result<()> {
        self.0.send(Msg::Flyout(Done::Changed(Watched::Mixer)));
        Ok(())
    }
}

#[implement(IAudioSessionEvents)]
struct SessionChanged(Waker);

impl SessionChanged_Impl {
    fn changed(&self) -> windows::core::Result<()> {
        self.0.send(Msg::Flyout(Done::Changed(Watched::Mixer)));
        Ok(())
    }
}

impl IAudioSessionEvents_Impl for SessionChanged_Impl {
    fn OnDisplayNameChanged(&self, _: &PCWSTR, _: *const GUID) -> windows::core::Result<()> {
        self.changed()
    }
    fn OnIconPathChanged(&self, _: &PCWSTR, _: *const GUID) -> windows::core::Result<()> {
        Ok(())
    }
    fn OnSimpleVolumeChanged(&self, _: f32, _: BOOL, _: *const GUID) -> windows::core::Result<()> {
        self.changed()
    }
    fn OnChannelVolumeChanged(
        &self,
        _: u32,
        _: *const f32,
        _: u32,
        _: *const GUID,
    ) -> windows::core::Result<()> {
        Ok(())
    }
    fn OnGroupingParamChanged(&self, _: *const GUID, _: *const GUID) -> windows::core::Result<()> {
        Ok(())
    }
    fn OnStateChanged(&self, _: AudioSessionState) -> windows::core::Result<()> {
        self.changed()
    }
    fn OnSessionDisconnected(&self, _: AudioSessionDisconnectReason) -> windows::core::Result<()> {
        self.changed()
    }
}

#[implement(IMMNotificationClient)]
struct DevicesChanged(Waker);

impl IMMNotificationClient_Impl for DevicesChanged_Impl {
    fn OnDeviceStateChanged(&self, _: &PCWSTR, _: DEVICE_STATE) -> windows::core::Result<()> {
        self.0.send(Msg::Flyout(Done::Changed(Watched::Mixer)));
        Ok(())
    }
    fn OnDeviceAdded(&self, _: &PCWSTR) -> windows::core::Result<()> {
        Ok(())
    }
    fn OnDeviceRemoved(&self, _: &PCWSTR) -> windows::core::Result<()> {
        Ok(())
    }
    fn OnDefaultDeviceChanged(
        &self,
        flow: EDataFlow,
        role: ERole,
        _: &PCWSTR,
    ) -> windows::core::Result<()> {
        if flow == eRender && role == eConsole {
            self.0.send(Msg::Flyout(Done::Changed(Watched::Mixer)));
        }
        Ok(())
    }
    fn OnPropertyValueChanged(&self, _: &PCWSTR, _: &PROPERTYKEY) -> windows::core::Result<()> {
        Ok(())
    }
}

/// The undocumented `IPolicyConfig` (Windows 7 and later), with its Win32 method names. Only
/// `SetDefaultEndpoint` is called; the other slots are declared so it lands at the right vtable
/// offset.
#[allow(non_snake_case)]
mod policy {
    use std::ffi::c_void;

    use windows::Win32::Media::Audio::ERole;
    use windows::core::{HRESULT, PCWSTR};

    #[windows::core::interface("f8679f50-850a-41cf-9c72-430f290290c8")]
    pub unsafe trait IPolicyConfig: windows::core::IUnknown {
        unsafe fn GetMixFormat(&self, device: PCWSTR, format: *mut *mut c_void) -> HRESULT;
        unsafe fn GetDeviceFormat(
            &self,
            device: PCWSTR,
            default: i32,
            format: *mut *mut c_void,
        ) -> HRESULT;
        unsafe fn ResetDeviceFormat(&self, device: PCWSTR) -> HRESULT;
        unsafe fn SetDeviceFormat(
            &self,
            device: PCWSTR,
            endpoint: *mut c_void,
            mix: *mut c_void,
        ) -> HRESULT;
        unsafe fn GetProcessingPeriod(
            &self,
            device: PCWSTR,
            default: i32,
            a: *mut i64,
            b: *mut i64,
        ) -> HRESULT;
        unsafe fn SetProcessingPeriod(&self, device: PCWSTR, period: *mut i64) -> HRESULT;
        unsafe fn GetShareMode(&self, device: PCWSTR, mode: *mut c_void) -> HRESULT;
        unsafe fn SetShareMode(&self, device: PCWSTR, mode: *mut c_void) -> HRESULT;
        unsafe fn GetPropertyValue(
            &self,
            device: PCWSTR,
            store: i32,
            key: *const c_void,
            value: *mut c_void,
        ) -> HRESULT;
        unsafe fn SetPropertyValue(
            &self,
            device: PCWSTR,
            store: i32,
            key: *const c_void,
            value: *mut c_void,
        ) -> HRESULT;
        unsafe fn SetDefaultEndpoint(&self, device: PCWSTR, role: ERole) -> HRESULT;
        unsafe fn SetEndpointVisibility(&self, device: PCWSTR, visible: i32) -> HRESULT;
    }

    /// `CLSID_PolicyConfigClient`.
    const POLICY_CONFIG: windows::core::GUID =
        windows::core::GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);

    /// Makes `id` the default output for every role (what Sound settings does).
    pub fn set_default(id: &str) -> windows::core::Result<()> {
        use windows::Win32::Media::Audio::{eCommunications, eConsole, eMultimedia};
        use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
        // SAFETY: an undocumented but long-stable COM object; the id outlives the calls.
        unsafe {
            let policy: IPolicyConfig = CoCreateInstance(&POLICY_CONFIG, None, CLSCTX_ALL)?;
            let id = windows::core::HSTRING::from(id);
            for role in [eConsole, eMultimedia, eCommunications] {
                policy.SetDefaultEndpoint(PCWSTR(id.as_ptr()), role).ok()?;
            }
        }
        Ok(())
    }
}

/// The lane's state.
pub struct MixerLane {
    enumerator: Option<IMMDeviceEnumerator>,
    devices: Option<IMMNotificationClient>,
    /// The default device's session manager and our notification on it.
    manager: Option<(String, IAudioSessionManager2, IAudioSessionNotification)>,
    /// Event sinks on the listed sessions.
    sinks: Vec<(IAudioSessionControl, IAudioSessionEvents)>,
    waker: Waker,
    wic: Option<IWICImagingFactory>,
    icons: PathBuf,
}

impl MixerLane {
    pub fn new(waker: Waker, icons: PathBuf) -> Self {
        // SAFETY: COM for this thread (MTA), kept for its life; plain COM creation.
        let (enumerator, wic) = unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            (
                CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL)
                    .ok(),
                CoCreateInstance::<_, IWICImagingFactory>(
                    &CLSID_WICImagingFactory,
                    None,
                    CLSCTX_INPROC_SERVER,
                )
                .ok(),
            )
        };
        let devices: Option<IMMNotificationClient> = enumerator.as_ref().map(|e| {
            let client: IMMNotificationClient = DevicesChanged(waker.clone()).into();
            // SAFETY: unregistered on drop.
            let _ = unsafe { e.RegisterEndpointNotificationCallback(&client) };
            client
        });
        Self {
            enumerator,
            devices,
            manager: None,
            sinks: Vec::new(),
            waker,
            wic,
            icons,
        }
    }

    fn devices(&self) -> Vec<Device> {
        let Some(enumerator) = &self.enumerator else {
            return Vec::new();
        };
        // SAFETY: COM calls on live objects; strings are freed after copying.
        unsafe {
            let default = enumerator
                .GetDefaultAudioEndpoint(eRender, eConsole)
                .ok()
                .and_then(|d| device_id(&d));
            let Ok(collection) = enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) else {
                return Vec::new();
            };
            (0..collection.GetCount().unwrap_or(0))
                .filter_map(|i| {
                    let device = collection.Item(i).ok()?;
                    let id = device_id(&device)?;
                    let name = device
                        .OpenPropertyStore(STGM_READ)
                        .and_then(|store| store.GetValue(&FRIENDLY_NAME))
                        .ok()
                        .and_then(|value| {
                            let text: PWSTR = PropVariantToStringAlloc(&value).ok()?;
                            let name = text.to_string().ok();
                            CoTaskMemFree(Some(text.0.cast()));
                            name
                        })
                        .unwrap_or_else(|| "Speakers".into());
                    Some(Device {
                        default: default.as_deref() == Some(id.as_str()),
                        id,
                        name,
                    })
                })
                .collect()
        }
    }

    /// The default device's session manager, bound (with our notification) when it changed.
    fn manager(&mut self) -> Option<IAudioSessionManager2> {
        let enumerator = self.enumerator.as_ref()?;
        // SAFETY: COM calls on live objects; the notification is unregistered when replaced.
        unsafe {
            let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
            let id = device_id(&device)?;
            if let Some((bound, manager, _)) = &self.manager
                && *bound == id
            {
                return Some(manager.clone());
            }
            if let Some((_, manager, notification)) = self.manager.take() {
                let _ = manager.UnregisterSessionNotification(&notification);
            }
            let manager: IAudioSessionManager2 = device.Activate(CLSCTX_ALL, None).ok()?;
            // Notifications only start after the first enumeration (Core Audio docs).
            let _ = manager.GetSessionEnumerator();
            let notification: IAudioSessionNotification = Created(self.waker.clone()).into();
            let _ = manager.RegisterSessionNotification(&notification);
            self.manager = Some((id, manager.clone(), notification));
            Some(manager)
        }
    }

    pub fn read(&mut self, switchable: bool) -> MixerState {
        let devices = self.devices();
        self.drop_sinks();
        let mut sessions = Vec::new();
        if let Some(manager) = self.manager() {
            // SAFETY: COM calls on live session objects.
            unsafe {
                if let Ok(list) = manager.GetSessionEnumerator() {
                    for i in 0..list.GetCount().unwrap_or(0) {
                        let Ok(control) = list.GetSession(i) else {
                            continue;
                        };
                        if let Some(session) = self.session(&control) {
                            let sink: IAudioSessionEvents =
                                SessionChanged(self.waker.clone()).into();
                            if control.RegisterAudioSessionNotification(&sink).is_ok() {
                                self.sinks.push((control, sink));
                            }
                            sessions.push(session);
                        }
                    }
                }
            }
        }
        dedupe(&mut sessions);
        order(&mut sessions);
        MixerState {
            devices,
            sessions,
            switchable,
        }
    }

    fn session(&self, control: &IAudioSessionControl) -> Option<Session> {
        // SAFETY: COM calls on a live session; strings are freed after copying.
        unsafe {
            let state = control.GetState().ok()?;
            if state == AudioSessionStateExpired {
                return None;
            }
            let control2: IAudioSessionControl2 = control.cast().ok()?;
            let volume: ISimpleAudioVolume = control.cast().ok()?;
            let id = take_string(control2.GetSessionInstanceIdentifier().ok()?)?;
            let system = control2.IsSystemSoundsSession() == windows::Win32::Foundation::S_OK;
            let pid = control2.GetProcessId().unwrap_or(0);
            let exe = (!system).then(|| process_exe(pid)).flatten();
            let display = control
                .GetDisplayName()
                .ok()
                .and_then(take_string)
                .and_then(|name| indirect(&name));
            let name = if system {
                "System sounds".to_owned()
            } else {
                display
                    .or_else(|| exe.as_deref().and_then(crate::icons::version_description))
                    .or_else(|| exe.as_deref().map(crate::apps::file_stem))
                    .unwrap_or_else(|| format!("Process {pid}"))
            };
            let icon = exe.as_deref().and_then(|exe| {
                let wic = self.wic.as_ref()?;
                let item = crate::icons::shell_item(exe)?;
                crate::icons::icon(&item, &exe.to_lowercase(), wic, &self.icons)
            });
            Some(Session {
                id,
                name,
                icon,
                level: volume.GetMasterVolume().unwrap_or(0.0),
                muted: volume.GetMute().is_ok_and(|m| m.as_bool()),
                system,
                active: state == windows::Win32::Media::Audio::AudioSessionStateActive,
            })
        }
    }

    pub fn set_session(
        &mut self,
        id: &str,
        level: Option<f32>,
        mute: Option<bool>,
    ) -> Result<(), String> {
        let manager = self.manager().ok_or("no output device")?;
        // SAFETY: COM calls on live session objects.
        unsafe {
            let list = manager.GetSessionEnumerator().map_err(|e| e.to_string())?;
            for i in 0..list.GetCount().unwrap_or(0) {
                let Ok(control) = list.GetSession(i) else {
                    continue;
                };
                let Ok(control2) = control.cast::<IAudioSessionControl2>() else {
                    continue;
                };
                let matches = control2
                    .GetSessionInstanceIdentifier()
                    .ok()
                    .and_then(take_string)
                    .is_some_and(|s| s == id);
                if !matches {
                    continue;
                }
                let volume: ISimpleAudioVolume = control.cast().map_err(|e| e.to_string())?;
                if let Some(level) = level {
                    volume
                        .SetMasterVolume(level.clamp(0.0, 1.0), std::ptr::null())
                        .map_err(|e| e.to_string())?;
                }
                if let Some(mute) = mute {
                    volume
                        .SetMute(mute, std::ptr::null())
                        .map_err(|e| e.to_string())?;
                }
                return Ok(());
            }
        }
        Err("that app isn't playing any more".into())
    }

    /// Makes `id` the default output for every role (what Sound settings does).
    pub fn set_output(&self, id: &str) -> Result<(), String> {
        policy::set_default(id).map_err(|e| e.to_string())
    }

    fn drop_sinks(&mut self) {
        for (control, sink) in self.sinks.drain(..) {
            // SAFETY: unregisters the sink we registered on this session.
            let _ = unsafe { control.UnregisterAudioSessionNotification(&sink) };
        }
    }
}

impl Drop for MixerLane {
    fn drop(&mut self) {
        self.drop_sinks();
        // SAFETY: unregisters what we registered.
        unsafe {
            if let Some((_, manager, notification)) = self.manager.take() {
                let _ = manager.UnregisterSessionNotification(&notification);
            }
            if let (Some(enumerator), Some(devices)) = (&self.enumerator, &self.devices) {
                let _ = enumerator.UnregisterEndpointNotificationCallback(devices);
            }
        }
    }
}

fn device_id(device: &IMMDevice) -> Option<String> {
    // SAFETY: the returned string is ours to free.
    unsafe { device.GetId().ok().and_then(take_string) }
}

/// A COM-allocated string, copied and freed.
fn take_string(text: PWSTR) -> Option<String> {
    if text.is_null() {
        return None;
    }
    // SAFETY: a valid NUL-terminated string from COM, freed once.
    unsafe {
        let value = text.to_string().ok();
        CoTaskMemFree(Some(text.0.cast()));
        value.filter(|v| !v.is_empty())
    }
}

/// `@%SystemRoot%\System32\AudioSrv.Dll,-202` → its text; plain names pass through.
fn indirect(name: &str) -> Option<String> {
    if !name.starts_with('@') {
        return Some(name.to_owned());
    }
    let mut buffer = [0u16; 512];
    // SAFETY: valid strings; the buffer outlives the call.
    unsafe {
        SHLoadIndirectString(&windows::core::HSTRING::from(name), &mut buffer, None).ok()?;
    }
    let len = buffer.iter().position(|&c| c == 0).unwrap_or(0);
    (len > 0).then(|| String::from_utf16_lossy(&buffer[..len]))
}

fn process_exe(pid: u32) -> Option<String> {
    // SAFETY: a query handle we close; the buffer outlives the call.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buffer = [0u16; 1024];
        let mut len = buffer.len() as u32;
        let ok = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut len,
        );
        let _ = CloseHandle(process);
        ok.ok()?;
        Some(String::from_utf16_lossy(&buffer[..len as usize]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(id: &str, name: &str, system: bool, active: bool) -> Session {
        Session {
            id: id.into(),
            name: name.into(),
            icon: None,
            level: 1.0,
            muted: false,
            system,
            active,
        }
    }

    #[test]
    fn system_sounds_then_playing_apps_then_the_rest() {
        let mut sessions = vec![
            session("1", "zoom", false, false),
            session("2", "Spotify", false, true),
            session("3", "System sounds", true, false),
            session("4", "Arc", false, true),
            session("2", "Spotify", false, true),
        ];
        dedupe(&mut sessions);
        order(&mut sessions);
        let names: Vec<&str> = sessions.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["System sounds", "Arc", "Spotify", "zoom"]);
    }

    #[test]
    fn indirect_names_pass_plain_text_through() {
        assert_eq!(indirect("Firefox").as_deref(), Some("Firefox"));
    }
}
