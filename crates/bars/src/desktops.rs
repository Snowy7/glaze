//! Virtual desktops: how many and which is current, from the values Explorer keeps under
//! `HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\VirtualDesktops`
//! (`VirtualDesktopIDs`, `CurrentVirtualDesktop`: 16-byte GUIDs). The key is watched with
//! `RegNotifyChangeKeyValue`, whose event the main loop waits on; nothing polls.
//!
//! With a single desktop Windows doesn't write the values at all: that reads as 1 desktop.

use serde::Serialize;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_NOTIFY, KEY_READ, REG_NOTIFY_CHANGE_LAST_SET,
    REG_NOTIFY_CHANGE_NAME, RRF_RT_REG_BINARY, RegCloseKey, RegGetValueW, RegNotifyChangeKeyValue,
    RegOpenKeyExW,
};
use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
use windows::Win32::System::Threading::{CreateEventW, GetCurrentProcessId, ResetEvent};
use windows::core::{HSTRING, PCWSTR, w};

const KEY: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Explorer\VirtualDesktops");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Workspaces {
    pub count: usize,
    /// 0-based.
    pub current: usize,
}

/// Counts desktops and finds the current one from the raw registry values.
pub fn workspaces(ids: &[u8], current: &[u8]) -> Workspaces {
    let (guids, _) = ids.as_chunks::<16>();
    Workspaces {
        count: guids.len().max(1),
        current: guids
            .iter()
            .position(|g| g.as_slice() == current)
            .unwrap_or(0),
    }
}

/// The watched key.
pub struct Desktops {
    key: HKEY,
    event: HANDLE,
}

impl Desktops {
    pub fn watch() -> Option<Self> {
        // SAFETY: plain registry and event calls; both handles are closed on drop.
        unsafe {
            let mut key = HKEY::default();
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                KEY,
                None,
                KEY_READ | KEY_NOTIFY,
                &mut key,
            )
            .ok()
            .inspect_err(|e| log::info!("not watching virtual desktops: {e}"))
            .ok()?;
            Self::from_key(key)
        }
    }

    /// Takes ownership of an open notification key.
    fn from_key(key: HKEY) -> Option<Self> {
        // SAFETY: the owned handles are closed together on drop.
        unsafe {
            let Ok(event) = CreateEventW(None, true, false, PCWSTR::null()) else {
                let _ = RegCloseKey(key);
                return None;
            };
            let desktops = Self { key, event };
            desktops.arm();
            Some(desktops)
        }
    }

    /// Signaled when the values change; call [`changed`](Self::changed) then.
    pub fn event(&self) -> HANDLE {
        self.event
    }

    fn arm(&self) {
        // SAFETY: our open key and event; asynchronous, so it returns at once.
        let _ = unsafe {
            RegNotifyChangeKeyValue(
                self.key,
                true,
                REG_NOTIFY_CHANGE_LAST_SET | REG_NOTIFY_CHANGE_NAME,
                Some(self.event),
                true,
            )
        };
    }

    /// The event fired: watch again and read.
    pub fn changed(&self) -> Workspaces {
        // The outer message wait must not consume the notification before the service reads it.
        // Reset before rearming, so a change during the read remains signalled for the next pass.
        // SAFETY: the event belongs to this service and remains open through the read.
        let _ = unsafe { ResetEvent(self.event) };
        self.arm();
        self.read()
    }

    pub fn read(&self) -> Workspaces {
        let ids = binary(self.key, PCWSTR::null(), w!("VirtualDesktopIDs")).unwrap_or_default();
        let current = binary(self.key, PCWSTR::null(), w!("CurrentVirtualDesktop"))
            .or_else(session_current)
            .unwrap_or_default();
        workspaces(&ids, &current)
    }
}

impl Drop for Desktops {
    fn drop(&mut self) {
        // SAFETY: handles we opened.
        unsafe {
            let _ = RegCloseKey(self.key);
            let _ = CloseHandle(self.event);
        }
    }
}

/// Windows 10 keeps the current desktop per session instead.
fn session_current() -> Option<Vec<u8>> {
    let mut session = 0;
    // SAFETY: no preconditions; `session` outlives the call.
    unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut session) }.ok()?;
    let path = HSTRING::from(format!(
        r"Software\Microsoft\Windows\CurrentVersion\Explorer\SessionInfo\{session}\VirtualDesktops"
    ));
    binary(
        HKEY_CURRENT_USER,
        PCWSTR(path.as_ptr()),
        w!("CurrentVirtualDesktop"),
    )
}

fn binary(key: HKEY, subkey: PCWSTR, name: PCWSTR) -> Option<Vec<u8>> {
    // SAFETY: two-call pattern; the buffer is as large as the size the first call reported.
    unsafe {
        let mut bytes = 0u32;
        RegGetValueW(
            key,
            subkey,
            name,
            RRF_RT_REG_BINARY,
            None,
            None,
            Some(&mut bytes),
        )
        .ok()
        .ok()?;
        let mut data = vec![0u8; bytes as usize];
        RegGetValueW(
            key,
            subkey,
            name,
            RRF_RT_REG_BINARY,
            None,
            Some(data.as_mut_ptr().cast()),
            Some(&mut bytes),
        )
        .ok()
        .ok()?;
        data.truncate(bytes as usize);
        Some(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notification_survives_the_outer_wait_and_is_consumed_by_changed() {
        use windows::Win32::{
            Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT},
            System::{
                Registry::{KEY_ALL_ACCESS, REG_OPTION_VOLATILE, RegCreateKeyExW, RegDeleteKeyW},
                Threading::{SetEvent, WaitForSingleObject},
            },
        };
        let path = HSTRING::from(format!(
            r"Software\mypaper\tests\desktops-{}",
            std::process::id()
        ));
        struct Cleanup(HSTRING);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                // SAFETY: only the isolated key made by this test, after its handle is closed.
                let _ = unsafe { RegDeleteKeyW(HKEY_CURRENT_USER, &self.0) };
            }
        }
        let _cleanup = Cleanup(path.clone());
        // SAFETY: an isolated key and event, owned by Desktops and closed before cleanup.
        unsafe {
            let mut key = HKEY::default();
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                &path,
                None,
                None,
                REG_OPTION_VOLATILE,
                KEY_ALL_ACCESS,
                None,
                &mut key,
                None,
            )
            .ok()
            .unwrap();
            let desktops = Desktops::from_key(key).unwrap();
            let event = desktops.event();
            SetEvent(event).unwrap();
            assert_eq!(
                WaitForSingleObject(event, 0),
                WAIT_OBJECT_0,
                "outer message wait"
            );
            assert_eq!(
                WaitForSingleObject(event, 0),
                WAIT_OBJECT_0,
                "service still sees it"
            );
            assert_eq!(
                desktops.changed(),
                Workspaces {
                    count: 1,
                    current: 0
                }
            );
            assert_eq!(
                WaitForSingleObject(event, 0),
                WAIT_TIMEOUT,
                "changed consumes it"
            );
        }
    }

    #[test]
    fn counts_guids_and_finds_the_current_one() {
        let a = [1u8; 16];
        let b = [2u8; 16];
        let c = [3u8; 16];
        let ids: Vec<u8> = [a, b, c].concat();
        assert_eq!(
            workspaces(&ids, &b),
            Workspaces {
                count: 3,
                current: 1
            }
        );
        assert_eq!(
            workspaces(&[], &[]),
            Workspaces {
                count: 1,
                current: 0
            },
            "one desktop: no values"
        );
        assert_eq!(workspaces(&ids, &[9u8; 16]).current, 0);
    }
}
