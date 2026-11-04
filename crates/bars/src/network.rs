//! The network module: Wi-Fi (its SSID and signal) through the WLAN API, else a wired connection
//! from the interface table, else nothing. Changes arrive from `NotifyIpInterfaceChange` and WLAN
//! notifications (connects, disconnects, signal quality), only while a network module is on
//! screen.

use std::ffi::c_void;

use serde::Serialize;
use windows::Win32::Foundation::{ERROR_SUCCESS, HANDLE};
use windows::Win32::NetworkManagement::IpHelper::{
    CancelMibChangeNotify2, FreeMibTable, GetIfTable2, IF_TYPE_ETHERNET_CSMACD, IF_TYPE_IEEE80211,
    MIB_IF_TABLE2, MIB_IPINTERFACE_ROW, MIB_NOTIFICATION_TYPE, NotifyIpInterfaceChange,
};
use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
use windows::Win32::NetworkManagement::WiFi::{
    L2_NOTIFICATION_DATA, WLAN_CONNECTION_ATTRIBUTES, WLAN_INTERFACE_INFO_LIST,
    WLAN_NOTIFICATION_SOURCE_ACM, WLAN_NOTIFICATION_SOURCE_MSM, WLAN_NOTIFICATION_SOURCE_NONE,
    WlanCloseHandle, WlanEnumInterfaces, WlanFreeMemory, WlanOpenHandle, WlanQueryInterface,
    WlanRegisterNotification, wlan_interface_state_connected, wlan_intf_opcode_current_connection,
};
use windows::Win32::Networking::WinSock::AF_UNSPEC;

use crate::wake::{Msg, Waker};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Wifi,
    Ethernet,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NetworkInfo {
    pub kind: Kind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Wi-Fi signal quality, 0–100.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signal: Option<u32>,
}

/// Which physical links are up, from the interface table.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Links {
    wifi: bool,
    wired: bool,
}

/// The connection to show: Wi-Fi wins (with its details when the WLAN API has them), then wired.
fn pick(links: Links, wifi: Option<(String, u32)>) -> NetworkInfo {
    match (wifi, links) {
        (Some((name, signal)), _) => NetworkInfo {
            kind: Kind::Wifi,
            name: (!name.is_empty()).then_some(name),
            signal: Some(signal),
        },
        (None, Links { wired: true, .. }) => NetworkInfo {
            kind: Kind::Ethernet,
            name: None,
            signal: None,
        },
        (None, Links { wifi: true, .. }) => NetworkInfo {
            kind: Kind::Wifi,
            name: None,
            signal: None,
        },
        _ => NetworkInfo {
            kind: Kind::None,
            name: None,
            signal: None,
        },
    }
}

/// Reads the network now. `wlan` is an open WLAN client handle, if there is one.
pub fn read(wlan: Option<HANDLE>) -> NetworkInfo {
    pick(links(), wlan.and_then(connected_wifi))
}

fn links() -> Links {
    let mut table: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
    // SAFETY: GetIfTable2 allocates the table, freed below; rows are read while it's alive.
    unsafe {
        if GetIfTable2(&mut table) != ERROR_SUCCESS || table.is_null() {
            return Links::default();
        }
        let rows =
            std::slice::from_raw_parts((*table).Table.as_ptr(), (*table).NumEntries as usize);
        let mut links = Links::default();
        for row in rows {
            let flags = row.InterfaceAndOperStatusFlags._bitfield;
            let hardware = flags & 1 != 0;
            let filter = flags & 2 != 0;
            if !hardware || filter || row.OperStatus != IfOperStatusUp {
                continue;
            }
            match row.Type {
                IF_TYPE_IEEE80211 => links.wifi = true,
                IF_TYPE_ETHERNET_CSMACD => links.wired = true,
                _ => {}
            }
        }
        FreeMibTable(table.cast());
        links
    }
}

/// The connected Wi-Fi network's SSID and signal quality.
fn connected_wifi(wlan: HANDLE) -> Option<(String, u32)> {
    // SAFETY: lists and attributes are allocated by the WLAN API and freed with WlanFreeMemory
    // after reading.
    unsafe {
        let mut list: *mut WLAN_INTERFACE_INFO_LIST = std::ptr::null_mut();
        if WlanEnumInterfaces(wlan, None, &mut list) != ERROR_SUCCESS.0 || list.is_null() {
            return None;
        }
        let interfaces = std::slice::from_raw_parts(
            (*list).InterfaceInfo.as_ptr(),
            (*list).dwNumberOfItems as usize,
        );
        let found = interfaces
            .iter()
            .filter(|i| i.isState == wlan_interface_state_connected)
            .find_map(|interface| {
                let mut size = 0u32;
                let mut data: *mut c_void = std::ptr::null_mut();
                let status = WlanQueryInterface(
                    wlan,
                    &interface.InterfaceGuid,
                    wlan_intf_opcode_current_connection,
                    None,
                    &mut size,
                    &mut data,
                    None,
                );
                if status != ERROR_SUCCESS.0 || data.is_null() {
                    return None;
                }
                let attributes = &*data.cast::<WLAN_CONNECTION_ATTRIBUTES>();
                let association = &attributes.wlanAssociationAttributes;
                let ssid = &association.dot11Ssid;
                let len = (ssid.uSSIDLength as usize).min(ssid.ucSSID.len());
                let name = String::from_utf8_lossy(&ssid.ucSSID[..len]).into_owned();
                let signal = association.wlanSignalQuality.min(100);
                WlanFreeMemory(data);
                Some((name, signal))
            });
        WlanFreeMemory(list.cast());
        found
    }
}

/// Change notifications, while a network module is on screen; stopped on drop.
pub struct NetworkWatch {
    ip: HANDLE,
    wlan: Option<HANDLE>,
    /// Handed to both callbacks as their context; freed after both are cancelled.
    waker: *mut Waker,
}

impl NetworkWatch {
    pub fn start(waker: Waker) -> Self {
        let context = Box::into_raw(Box::new(waker));
        let mut ip = HANDLE::default();
        // SAFETY: the context outlives the registration (cancelled on drop, before freeing).
        let status = unsafe {
            NotifyIpInterfaceChange(
                AF_UNSPEC,
                Some(on_ip_change),
                Some(context.cast_const().cast()),
                false,
                &mut ip,
            )
        };
        if status != ERROR_SUCCESS {
            log::info!("no network change notifications ({status:?})");
        }
        let wlan = open_wlan().inspect(|&wlan| {
            // SAFETY: our open client handle and a context that outlives the registration.
            let status = unsafe {
                WlanRegisterNotification(
                    wlan,
                    WLAN_NOTIFICATION_SOURCE_ACM | WLAN_NOTIFICATION_SOURCE_MSM,
                    false,
                    Some(on_wlan),
                    Some(context.cast_const().cast()),
                    None,
                    None,
                )
            };
            if status != ERROR_SUCCESS.0 {
                log::info!("no Wi-Fi notifications ({status})");
            }
        });
        Self {
            ip,
            wlan,
            waker: context,
        }
    }

    pub fn wlan(&self) -> Option<HANDLE> {
        self.wlan
    }
}

impl Drop for NetworkWatch {
    fn drop(&mut self) {
        // SAFETY: cancels our registrations; after both return no callback runs, so the context
        // can be freed.
        unsafe {
            if !self.ip.is_invalid() {
                let _ = CancelMibChangeNotify2(self.ip);
            }
            if let Some(wlan) = self.wlan {
                let _ = WlanRegisterNotification(
                    wlan,
                    WLAN_NOTIFICATION_SOURCE_NONE,
                    false,
                    None,
                    None,
                    None,
                    None,
                );
                WlanCloseHandle(wlan, None);
            }
            drop(Box::from_raw(self.waker));
        }
    }
}

fn open_wlan() -> Option<HANDLE> {
    let mut version = 0u32;
    let mut handle = HANDLE::default();
    // SAFETY: out-pointers valid for the call; the handle is closed by NetworkWatch's drop.
    let status = unsafe { WlanOpenHandle(2, None, &mut version, &mut handle) };
    (status == ERROR_SUCCESS.0).then_some(handle)
}

unsafe extern "system" fn on_ip_change(
    context: *const c_void,
    _: *const MIB_IPINTERFACE_ROW,
    _: MIB_NOTIFICATION_TYPE,
) {
    // SAFETY: the context is the Waker NetworkWatch keeps alive while registered.
    let waker = unsafe { &*context.cast::<Waker>() };
    waker.send(Msg::Network);
}

unsafe extern "system" fn on_wlan(_: *mut L2_NOTIFICATION_DATA, context: *mut c_void) {
    // SAFETY: as above.
    let waker = unsafe { &*context.cast_const().cast::<Waker>() };
    waker.send(Msg::Network);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wifi_wins_then_wired_then_nothing() {
        let both = Links {
            wifi: true,
            wired: true,
        };
        assert_eq!(
            pick(both, Some(("Home".into(), 80))),
            NetworkInfo {
                kind: Kind::Wifi,
                name: Some("Home".into()),
                signal: Some(80)
            }
        );
        assert_eq!(pick(both, None).kind, Kind::Ethernet);
        let wifi_only = Links {
            wifi: true,
            wired: false,
        };
        assert_eq!(pick(wifi_only, None).kind, Kind::Wifi);
        assert_eq!(pick(Links::default(), None).kind, Kind::None);
        let json = serde_json::to_value(pick(Links::default(), None)).unwrap();
        assert_eq!(json, serde_json::json!({ "kind": "none" }));
    }
}
