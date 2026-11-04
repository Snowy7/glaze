//! The network flyout's Wi-Fi (docs/bars.md, "Flyouts" → "Network"), on the `wifi` lane, through
//! the documented WLAN API: the networks in range (`WlanGetAvailableNetworkList`, one entry per
//! SSID, the strongest), a fresh scan (`WlanScan`; the WLAN notification says when it's done),
//! connecting with a saved profile or a new one (`WlanSetProfile` with a profile written here, then
//! `WlanConnect`), and disconnecting.
//!
//! Since Windows 11 24H2, desktop apps only see Wi-Fi networks with precise location allowed for
//! them: the list comes back as `ERROR_ACCESS_DENIED`, which the flyout turns into a button to
//! the location privacy settings.

use std::ffi::c_void;

use serde::Serialize;
use windows::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_SUCCESS, HANDLE};
use windows::Win32::NetworkManagement::WiFi::{
    DOT11_AUTH_ALGO_80211_OPEN, DOT11_AUTH_ALGO_80211_SHARED_KEY, DOT11_AUTH_ALGO_OWE,
    DOT11_AUTH_ALGO_RSNA, DOT11_AUTH_ALGO_RSNA_PSK, DOT11_AUTH_ALGO_WPA, DOT11_AUTH_ALGO_WPA_PSK,
    DOT11_AUTH_ALGO_WPA3, DOT11_AUTH_ALGO_WPA3_ENT, DOT11_AUTH_ALGO_WPA3_SAE, DOT11_AUTH_ALGORITHM,
    L2_NOTIFICATION_DATA, WLAN_AVAILABLE_NETWORK_CONNECTED, WLAN_AVAILABLE_NETWORK_HAS_PROFILE,
    WLAN_AVAILABLE_NETWORK_LIST, WLAN_CONNECTION_PARAMETERS, WLAN_INTERFACE_INFO_LIST,
    WLAN_NOTIFICATION_SOURCE_ACM, WLAN_NOTIFICATION_SOURCE_NONE, WlanCloseHandle, WlanConnect,
    WlanDisconnect, WlanEnumInterfaces, WlanFreeMemory, WlanGetAvailableNetworkList,
    WlanOpenHandle, WlanRegisterNotification, WlanScan, WlanSetProfile,
    dot11_BSS_type_infrastructure, wlan_connection_mode_profile,
    wlan_notification_acm_connection_complete, wlan_notification_acm_disconnected,
    wlan_notification_acm_scan_complete,
};
use windows::core::{GUID, HSTRING, PCWSTR};

use crate::flyouts::{Done, Watched};
use crate::wake::{Msg, Waker};

/// How a network is secured, as far as the flyout cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Security {
    Open,
    /// WPA2-Personal (a passphrase).
    Wpa2,
    /// WPA3-Personal (SAE).
    Wpa3,
    /// WPA-Personal (the original).
    Wpa,
    /// 802.1X: a sign-in Settings handles.
    Enterprise,
    /// WEP, Enhanced Open and the rest: Settings handles them.
    Other,
}

impl Security {
    pub fn of(auth: DOT11_AUTH_ALGORITHM) -> Self {
        match auth {
            DOT11_AUTH_ALGO_80211_OPEN => Self::Open,
            DOT11_AUTH_ALGO_RSNA_PSK => Self::Wpa2,
            DOT11_AUTH_ALGO_WPA3_SAE => Self::Wpa3,
            DOT11_AUTH_ALGO_WPA_PSK => Self::Wpa,
            DOT11_AUTH_ALGO_RSNA
            | DOT11_AUTH_ALGO_WPA
            | DOT11_AUTH_ALGO_WPA3
            | DOT11_AUTH_ALGO_WPA3_ENT => Self::Enterprise,
            DOT11_AUTH_ALGO_80211_SHARED_KEY | DOT11_AUTH_ALGO_OWE => Self::Other,
            _ => Self::Other,
        }
    }

    /// The profile's `<authentication>` and `<encryption>`, for the kinds the flyout can join.
    fn profile(self) -> Option<(&'static str, &'static str)> {
        match self {
            Self::Open => Some(("open", "none")),
            Self::Wpa2 => Some(("WPA2PSK", "AES")),
            Self::Wpa3 => Some(("WPA3SAE", "AES")),
            Self::Wpa => Some(("WPAPSK", "AES")),
            Self::Enterprise | Self::Other => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Network {
    pub ssid: String,
    /// 0–100.
    pub signal: u32,
    pub security: Security,
    pub connected: bool,
    /// A saved profile exists (connecting needs no password).
    pub known: bool,
    #[serde(skip)]
    profile: String,
}

impl Network {
    /// A network as the flyout lists it, without a saved profile's name (previews and tests;
    /// the real ones come from [`WifiLane::read`]).
    pub fn new(ssid: &str, signal: u32, security: Security, connected: bool, known: bool) -> Self {
        Self {
            ssid: ssid.to_owned(),
            signal,
            security,
            connected,
            known,
            profile: String::new(),
        }
    }
}

/// What the flyout shows, or why it can't.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WifiState {
    pub networks: Vec<Network>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<WifiError>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum WifiError {
    /// No Wi-Fi adapter (or it's disabled).
    NoAdapter,
    /// Location access is off for desktop apps (Windows 11 24H2 and later).
    Location,
    Unavailable,
}

/// One entry per SSID: the connected one, else the one with a profile, else the strongest;
/// hidden networks (no SSID) left out; strongest first, connected on top.
pub fn dedupe(mut networks: Vec<Network>) -> Vec<Network> {
    networks.retain(|n| !n.ssid.is_empty());
    networks.sort_by(|a, b| {
        (b.connected, b.known, b.signal)
            .cmp(&(a.connected, a.known, a.signal))
            .then_with(|| a.ssid.cmp(&b.ssid))
    });
    let mut seen = std::collections::HashSet::new();
    networks.retain(|n| seen.insert(n.ssid.clone()));
    networks.sort_by_key(|n| std::cmp::Reverse((n.connected, n.signal)));
    networks
}

/// Whether `password` can secure a personal network: a passphrase of 8–63 printable ASCII
/// characters, or a 64-digit hex key.
pub fn valid_password(password: &str) -> bool {
    let passphrase =
        (8..=63).contains(&password.len()) && password.bytes().all(|b| (0x20..=0x7E).contains(&b));
    let hex_key = password.len() == 64 && password.bytes().all(|b| b.is_ascii_hexdigit());
    passphrase || hex_key
}

fn xml_escape(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '&' => "&amp;".to_owned(),
            '<' => "&lt;".to_owned(),
            '>' => "&gt;".to_owned(),
            '"' => "&quot;".to_owned(),
            '\'' => "&apos;".to_owned(),
            c => c.to_string(),
        })
        .collect()
}

/// A WLAN profile for a new network (the schema `netsh wlan export profile` writes), named after
/// it. The SSID goes in as hex too, so any bytes survive.
pub fn profile_xml(
    ssid: &str,
    security: Security,
    password: Option<&str>,
) -> Result<String, String> {
    let (authentication, encryption) = security
        .profile()
        .ok_or("this network needs Windows' Wi-Fi settings to join")?;
    let name = xml_escape(ssid);
    let hex: String = ssid.bytes().map(|b| format!("{b:02X}")).collect();
    let key = match (security, password) {
        (Security::Open, _) => String::new(),
        (_, Some(password)) if valid_password(password) => {
            let kind = if password.len() == 64 {
                "networkKey"
            } else {
                "passPhrase"
            };
            format!(
                "<sharedKey><keyType>{kind}</keyType><protected>false</protected><keyMaterial>{}</keyMaterial></sharedKey>",
                xml_escape(password)
            )
        }
        (_, Some(_)) => return Err("the password must be 8 to 63 characters".into()),
        (_, None) => return Err("this network needs a password".into()),
    };
    Ok(format!(
        r#"<?xml version="1.0"?>
<WLANProfile xmlns="http://www.microsoft.com/networking/WLAN/profile/v1">
<name>{name}</name>
<SSIDConfig><SSID><hex>{hex}</hex><name>{name}</name></SSID></SSIDConfig>
<connectionType>ESS</connectionType>
<connectionMode>auto</connectionMode>
<MSM><security><authEncryption><authentication>{authentication}</authentication><encryption>{encryption}</encryption><useOneX>false</useOneX></authEncryption>{key}</security></MSM>
</WLANProfile>"#
    ))
}

/// The lane's state: an open WLAN client, with scan and connection notifications.
pub struct WifiLane {
    handle: Option<HANDLE>,
    /// The notification callback's context, freed after unregistering.
    waker: *mut Waker,
}

// SAFETY: the handle and the context pointer are only used from the lane's thread; the callback
// only reads the Waker, which is Send.
unsafe impl Send for WifiLane {}

impl WifiLane {
    pub fn new(waker: Waker) -> Self {
        let waker = Box::into_raw(Box::new(waker));
        let mut version = 0;
        let mut handle = HANDLE::default();
        // SAFETY: out-pointers valid for the call; the context outlives the registration.
        let handle = unsafe {
            (WlanOpenHandle(2, None, &mut version, &mut handle) == ERROR_SUCCESS.0).then(|| {
                let _ = WlanRegisterNotification(
                    handle,
                    WLAN_NOTIFICATION_SOURCE_ACM,
                    true,
                    Some(on_wlan),
                    Some(waker.cast_const().cast()),
                    None,
                    None,
                );
                handle
            })
        };
        Self { handle, waker }
    }

    /// The first Wi-Fi interface.
    fn interface(&self) -> Option<GUID> {
        let handle = self.handle?;
        let mut list: *mut WLAN_INTERFACE_INFO_LIST = std::ptr::null_mut();
        // SAFETY: the list is allocated by the API and freed after reading.
        unsafe {
            if WlanEnumInterfaces(handle, None, &mut list) != ERROR_SUCCESS.0 || list.is_null() {
                return None;
            }
            let count = (*list).dwNumberOfItems as usize;
            let first = (count > 0).then(|| (*list).InterfaceInfo[0].InterfaceGuid);
            WlanFreeMemory(list.cast());
            first
        }
    }

    pub fn read(&self) -> WifiState {
        let (Some(handle), Some(interface)) = (self.handle, self.interface()) else {
            return WifiState {
                networks: Vec::new(),
                error: Some(WifiError::NoAdapter),
            };
        };
        let mut list: *mut WLAN_AVAILABLE_NETWORK_LIST = std::ptr::null_mut();
        // SAFETY: the list is allocated by the API and freed after reading.
        unsafe {
            let status = WlanGetAvailableNetworkList(handle, &interface, 0, None, &mut list);
            if status == ERROR_ACCESS_DENIED.0 {
                return WifiState {
                    networks: Vec::new(),
                    error: Some(WifiError::Location),
                };
            }
            if status != ERROR_SUCCESS.0 || list.is_null() {
                return WifiState {
                    networks: Vec::new(),
                    error: Some(WifiError::Unavailable),
                };
            }
            let entries = std::slice::from_raw_parts(
                (*list).Network.as_ptr(),
                (*list).dwNumberOfItems as usize,
            );
            let networks = entries
                .iter()
                .map(|n| {
                    let len = (n.dot11Ssid.uSSIDLength as usize).min(32);
                    let profile_len = n.strProfileName.iter().position(|&c| c == 0).unwrap_or(0);
                    Network {
                        ssid: String::from_utf8_lossy(&n.dot11Ssid.ucSSID[..len]).into_owned(),
                        signal: n.wlanSignalQuality.min(100),
                        security: if n.bSecurityEnabled.as_bool() {
                            Security::of(n.dot11DefaultAuthAlgorithm)
                        } else {
                            Security::Open
                        },
                        connected: n.dwFlags & WLAN_AVAILABLE_NETWORK_CONNECTED != 0,
                        known: n.dwFlags & WLAN_AVAILABLE_NETWORK_HAS_PROFILE != 0,
                        profile: String::from_utf16_lossy(&n.strProfileName[..profile_len]),
                    }
                })
                .collect();
            WlanFreeMemory(list.cast());
            WifiState {
                networks: dedupe(networks),
                error: None,
            }
        }
    }

    /// Asks the adapter to scan; the notification reports when it's done.
    pub fn scan(&self) {
        if let (Some(handle), Some(interface)) = (self.handle, self.interface()) {
            // SAFETY: our handle and an interface it listed.
            unsafe { WlanScan(handle, &interface, None, None, None) };
        }
    }

    pub fn connect(&self, ssid: &str, password: Option<&str>) -> Result<(), String> {
        let (Some(handle), Some(interface)) = (self.handle, self.interface()) else {
            return Err("no Wi-Fi adapter".into());
        };
        let state = self.read();
        let network = state
            .networks
            .iter()
            .find(|n| n.ssid == ssid)
            .ok_or("that network isn't in range")?;
        let profile = if network.known && password.is_none() {
            network.profile.clone()
        } else {
            let xml = profile_xml(ssid, network.security, password)?;
            let mut reason = 0;
            // SAFETY: valid strings for the call; `reason` outlives it.
            let status = unsafe {
                WlanSetProfile(
                    handle,
                    &interface,
                    0,
                    &HSTRING::from(xml),
                    PCWSTR::null(),
                    true,
                    None,
                    &mut reason,
                )
            };
            if status != ERROR_SUCCESS.0 {
                return Err(format!(
                    "saving the profile failed (error {status}, reason {reason})"
                ));
            }
            ssid.to_owned()
        };
        let name = HSTRING::from(profile);
        let parameters = WLAN_CONNECTION_PARAMETERS {
            wlanConnectionMode: wlan_connection_mode_profile,
            strProfile: PCWSTR(name.as_ptr()),
            pDot11Ssid: std::ptr::null_mut(),
            pDesiredBssidList: std::ptr::null_mut(),
            dot11BssType: dot11_BSS_type_infrastructure,
            dwFlags: 0,
        };
        // SAFETY: the parameters and the profile name outlive the call.
        match unsafe { WlanConnect(handle, &interface, &parameters, None) } {
            0 => Ok(()),
            status => Err(format!("connecting failed (error {status})")),
        }
    }

    pub fn disconnect(&self) -> Result<(), String> {
        let (Some(handle), Some(interface)) = (self.handle, self.interface()) else {
            return Err("no Wi-Fi adapter".into());
        };
        // SAFETY: our handle and an interface it listed.
        match unsafe { WlanDisconnect(handle, &interface, None) } {
            0 => Ok(()),
            status => Err(format!("disconnecting failed (error {status})")),
        }
    }
}

impl Drop for WifiLane {
    fn drop(&mut self) {
        // SAFETY: unregisters and closes our handle; afterwards no callback runs, so the context
        // can be freed.
        unsafe {
            if let Some(handle) = self.handle {
                let _ = WlanRegisterNotification(
                    handle,
                    WLAN_NOTIFICATION_SOURCE_NONE,
                    true,
                    None,
                    None,
                    None,
                    None,
                );
                WlanCloseHandle(handle, None);
            }
            drop(Box::from_raw(self.waker));
        }
    }
}

unsafe extern "system" fn on_wlan(data: *mut L2_NOTIFICATION_DATA, context: *mut c_void) {
    // SAFETY: the API passes valid notification data; the context is our Waker.
    let (code, waker) = unsafe {
        (
            (*data).NotificationCode,
            &*context.cast_const().cast::<Waker>(),
        )
    };
    let interesting = [
        wlan_notification_acm_scan_complete.0,
        wlan_notification_acm_connection_complete.0,
        wlan_notification_acm_disconnected.0,
    ];
    if interesting.contains(&(code as i32)) {
        waker.send(Msg::Flyout(Done::Changed(Watched::Wifi)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn network(ssid: &str, signal: u32, connected: bool, known: bool) -> Network {
        Network {
            ssid: ssid.into(),
            signal,
            security: Security::Wpa2,
            connected,
            known,
            profile: String::new(),
        }
    }

    #[test]
    fn one_entry_per_network_connected_first() {
        let networks = dedupe(vec![
            network("Cafe", 40, false, false),
            network("Home", 60, false, true),
            network("Home", 90, false, false),
            network("", 99, false, false),
            network("Office", 70, true, true),
        ]);
        let got: Vec<(&str, u32, bool)> = networks
            .iter()
            .map(|n| (n.ssid.as_str(), n.signal, n.known))
            .collect();
        assert_eq!(
            got,
            [
                ("Office", 70, true),
                ("Home", 60, true),
                ("Cafe", 40, false)
            ],
            "the saved Home wins over a stronger unsaved one; hidden networks are left out"
        );
    }

    #[test]
    fn profiles_for_personal_networks() {
        let xml = profile_xml("Tom & Jerry's", Security::Wpa2, Some("p<ss>word1")).unwrap();
        assert!(xml.contains("<name>Tom &amp; Jerry&apos;s</name>"), "{xml}");
        assert!(xml.contains("<hex>546F6D2026204A657272792773</hex>"));
        assert!(
            xml.contains("<authentication>WPA2PSK</authentication><encryption>AES</encryption>")
        );
        assert!(xml.contains("<keyType>passPhrase</keyType>"));
        assert!(xml.contains("<keyMaterial>p&lt;ss&gt;word1</keyMaterial>"));

        let open = profile_xml("Guest", Security::Open, None).unwrap();
        assert!(
            open.contains("<authentication>open</authentication><encryption>none</encryption>")
        );
        assert!(!open.contains("sharedKey"));

        let wpa3 = profile_xml("Lab", Security::Wpa3, Some(&"a".repeat(64))).unwrap();
        assert!(wpa3.contains("WPA3SAE") && wpa3.contains("<keyType>networkKey</keyType>"));

        assert!(profile_xml("Corp", Security::Enterprise, Some("whatever1")).is_err());
        assert!(profile_xml("Home", Security::Wpa2, None).is_err());
        assert!(profile_xml("Home", Security::Wpa2, Some("short")).is_err());
    }

    #[test]
    fn passwords_follow_wpa_rules() {
        assert!(valid_password("12345678"));
        assert!(valid_password(&"x".repeat(63)));
        assert!(
            !valid_password(&"x".repeat(64)),
            "64 characters must be hex"
        );
        assert!(valid_password(&"aF".repeat(32)));
        assert!(!valid_password("1234567"));
        assert!(!valid_password("pässwort123"), "ASCII only");
    }

    #[test]
    fn security_comes_from_the_auth_algorithm() {
        assert_eq!(Security::of(DOT11_AUTH_ALGO_RSNA_PSK), Security::Wpa2);
        assert_eq!(Security::of(DOT11_AUTH_ALGO_WPA3_SAE), Security::Wpa3);
        assert_eq!(Security::of(DOT11_AUTH_ALGO_RSNA), Security::Enterprise);
        assert_eq!(
            Security::of(DOT11_AUTH_ALGO_80211_SHARED_KEY),
            Security::Other
        );
    }
}
