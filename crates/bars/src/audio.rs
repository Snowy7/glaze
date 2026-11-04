//! The output volume through Core Audio: `IAudioEndpointVolume` on the default render device,
//! with change callbacks for the level and for the default device. Exists only while a volume
//! module is on screen.

use serde::Serialize;
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::Media::Audio::Endpoints::{
    IAudioEndpointVolume, IAudioEndpointVolumeCallback, IAudioEndpointVolumeCallback_Impl,
};
use windows::Win32::Media::Audio::{
    AUDIO_VOLUME_NOTIFICATION_DATA, DEVICE_STATE, EDataFlow, ERole, IMMDeviceEnumerator,
    IMMNotificationClient, IMMNotificationClient_Impl, MMDeviceEnumerator, eConsole, eRender,
};
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance, CoTaskMemFree};
use windows::core::{GUID, PCWSTR, implement};

use crate::wake::{Msg, Waker};

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Volume {
    /// 0–1.
    pub level: f32,
    pub muted: bool,
}

#[implement(IAudioEndpointVolumeCallback)]
struct LevelChanged(Waker);

impl IAudioEndpointVolumeCallback_Impl for LevelChanged_Impl {
    fn OnNotify(&self, _: *mut AUDIO_VOLUME_NOTIFICATION_DATA) -> windows::core::Result<()> {
        self.0.send(Msg::Volume);
        Ok(())
    }
}

#[implement(IMMNotificationClient)]
struct DeviceChanged(Waker);

impl IMMNotificationClient_Impl for DeviceChanged_Impl {
    fn OnDeviceStateChanged(&self, _: &PCWSTR, _: DEVICE_STATE) -> windows::core::Result<()> {
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
            self.0.send(Msg::Volume);
        }
        Ok(())
    }
    fn OnPropertyValueChanged(&self, _: &PCWSTR, _: &PROPERTYKEY) -> windows::core::Result<()> {
        Ok(())
    }
}

/// The default output device's volume, followed across device changes.
pub struct Audio {
    enumerator: IMMDeviceEnumerator,
    devices: IMMNotificationClient,
    /// The bound device: its id, its volume, and our callback on it.
    endpoint: Option<(String, IAudioEndpointVolume, IAudioEndpointVolumeCallback)>,
    waker: Waker,
}

impl Audio {
    /// Needs COM on the calling thread.
    pub fn new(waker: Waker) -> Option<Self> {
        // SAFETY: COM creation and registration; unregistered on drop.
        unsafe {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                    .inspect_err(|e| log::warn!("no volume (Core Audio): {e}"))
                    .ok()?;
            let devices: IMMNotificationClient = DeviceChanged(waker.clone()).into();
            let _ = enumerator.RegisterEndpointNotificationCallback(&devices);
            let mut audio = Self {
                enumerator,
                devices,
                endpoint: None,
                waker,
            };
            audio.follow_default();
            Some(audio)
        }
    }

    /// Binds to the current default device if it changed.
    pub fn follow_default(&mut self) {
        // SAFETY: COM calls on objects we own; the id string is freed after copying.
        unsafe {
            let Ok(device) = self.enumerator.GetDefaultAudioEndpoint(eRender, eConsole) else {
                self.unbind();
                return;
            };
            let id = device
                .GetId()
                .ok()
                .and_then(|raw| {
                    let id = raw.to_string().ok();
                    CoTaskMemFree(Some(raw.0.cast()));
                    id
                })
                .unwrap_or_default();
            if self
                .endpoint
                .as_ref()
                .is_some_and(|(bound, ..)| *bound == id)
            {
                return;
            }
            self.unbind();
            let Ok(volume) = device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None) else {
                return;
            };
            let callback: IAudioEndpointVolumeCallback = LevelChanged(self.waker.clone()).into();
            let _ = volume.RegisterControlChangeNotify(&callback);
            self.endpoint = Some((id, volume, callback));
        }
    }

    fn unbind(&mut self) {
        if let Some((_, volume, callback)) = self.endpoint.take() {
            // SAFETY: unregisters the callback we registered on this endpoint.
            let _ = unsafe { volume.UnregisterControlChangeNotify(&callback) };
        }
    }

    pub fn read(&self) -> Option<Volume> {
        let (_, volume, _) = self.endpoint.as_ref()?;
        // SAFETY: COM calls on a live endpoint.
        unsafe {
            Some(Volume {
                level: volume.GetMasterVolumeLevelScalar().ok()?,
                muted: volume.GetMute().ok()?.as_bool(),
            })
        }
    }

    /// Sets the level (`set`, or `delta` from the current one, clamped to 0–1) and the mute.
    pub fn change(
        &self,
        set: Option<f32>,
        mute: Option<bool>,
        delta: Option<f32>,
    ) -> Option<Volume> {
        let (_, volume, _) = self.endpoint.as_ref()?;
        let current = self.read()?;
        let level = set
            .or(delta.map(|d| current.level + d))
            .map(|l| l.clamp(0.0, 1.0));
        // SAFETY: COM calls on a live endpoint; a null event context.
        unsafe {
            if let Some(level) = level {
                volume
                    .SetMasterVolumeLevelScalar(level, std::ptr::null::<GUID>())
                    .ok()?;
                // Turning the volume up unmutes, as the system flyout does.
                if mute.is_none() && level > current.level && current.muted {
                    let _ = volume.SetMute(false, std::ptr::null::<GUID>());
                }
            }
            if let Some(mute) = mute {
                volume.SetMute(mute, std::ptr::null::<GUID>()).ok()?;
            }
        }
        self.read()
    }
}

impl Drop for Audio {
    fn drop(&mut self) {
        self.unbind();
        // SAFETY: unregisters the client we registered.
        let _ = unsafe {
            self.enumerator
                .UnregisterEndpointNotificationCallback(&self.devices)
        };
    }
}
