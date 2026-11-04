//! The tray host's wire formats and rules (docs/bars.md, "Tray host"), as pure functions:
//!
//! - what `Shell_NotifyIcon` sends to the window of class `Shell_TrayWnd` (`WM_COPYDATA`,
//!   `dwData` 1: a `SHELLTRAYDATA` holding a `NOTIFYICONDATA` with 32-bit handles, so 32- and
//!   64-bit apps send the same bytes);
//! - how the icons follow `NIM_ADD` / `NIM_MODIFY` / `NIM_DELETE` / `NIM_SETVERSION`;
//! - how a click goes back to an icon's owner, per `NOTIFYICON_VERSION`;
//! - how the icons line up with the shell's own records (`HKCU\Control Panel\NotifyIconSettings`:
//!   pinned or not, and the user's order).
//!
//! Layouts follow ManagedShell's interop (`NativeMethods.Shell32.cs`) and ReactOS's `systray.cpp`
//! (the sender's side); see docs/research/customization-landscape.md, 4.1.

use serde::Serialize;

/// `COPYDATASTRUCT.dwData` of an app-bar message (`SHAppBarMessage`).
pub const COPYDATA_APPBAR: usize = 0;
/// `COPYDATASTRUCT.dwData` of a notification-icon message (`Shell_NotifyIcon`).
pub const COPYDATA_NOTIFY: usize = 1;
/// `COPYDATASTRUCT.dwData` of an icon-rect query (`Shell_NotifyIconGetRect`).
pub const COPYDATA_ICON_RECT: usize = 3;

/// `SHELLTRAYDATA.dwSignature`.
pub const NOTIFY_SIGNATURE: u32 = 0x3475_3423;

pub const NIM_ADD: u32 = 0;
pub const NIM_MODIFY: u32 = 1;
pub const NIM_DELETE: u32 = 2;
pub const NIM_SETFOCUS: u32 = 3;
pub const NIM_SETVERSION: u32 = 4;

pub const NIF_MESSAGE: u32 = 0x01;
pub const NIF_ICON: u32 = 0x02;
pub const NIF_TIP: u32 = 0x04;
pub const NIF_STATE: u32 = 0x08;
pub const NIF_GUID: u32 = 0x20;

pub const NIS_HIDDEN: u32 = 0x1;

// Offsets inside the NOTIFYICONDATA.
const NID_HWND: usize = 4;
const NID_UID: usize = 8;
const NID_FLAGS: usize = 12;
const NID_CALLBACK: usize = 16;
const NID_ICON: usize = 20;
const NID_TIP: usize = 24;
const NID_STATE: usize = 280;
const NID_STATE_MASK: usize = 284;
const NID_VERSION: usize = 800;
const NID_GUID: usize = 936;

/// The `hIcon` field's offset in a whole `SHELLTRAYDATA` (the host swaps in its own copy).
pub const ICON_OFFSET: usize = 8 + NID_ICON;

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    bytes
        .get(at..at + 4)
        .map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// A NUL-terminated UTF-16 field of `chars` characters (missing bytes read as empty).
fn wide_at(bytes: &[u8], at: usize, chars: usize) -> String {
    let field = bytes
        .get(at..(at + 2 * chars).min(bytes.len()))
        .unwrap_or(&[]);
    let units: Vec<u16> = field
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&pair| u16::from_le_bytes(pair))
        .take_while(|&c| c != 0)
        .collect();
    String::from_utf16_lossy(&units)
}

/// One `Shell_NotifyIcon` call, as received.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notify {
    /// `NIM_*`.
    pub message: u32,
    /// The owner window (a 32-bit handle, zero-extended).
    pub hwnd: u32,
    pub uid: u32,
    /// `NIF_*`: which of the fields below the call sets.
    pub flags: u32,
    pub callback: u32,
    /// A 32-bit `HICON`.
    pub icon: u32,
    pub tip: String,
    pub state: u32,
    pub state_mask: u32,
    /// `uVersion` (a union with the balloon's `uTimeout`): meaningful for `NIM_SETVERSION`.
    pub version: u32,
    /// `guidItem`, when `NIF_GUID` is set.
    pub guid: Option<u128>,
}

/// Parses a `SHELLTRAYDATA`. Older or shorter senders are accepted: fields past the end of the
/// data read as zero (the oldest `NOTIFYICONDATA` stops after a 64-character tip).
pub fn parse_notify(bytes: &[u8]) -> Option<Notify> {
    if bytes.len() < 8 + NID_TIP || u32_at(bytes, 0) != NOTIFY_SIGNATURE {
        return None;
    }
    let nid = &bytes[8..];
    let flags = u32_at(nid, NID_FLAGS);
    let guid = (flags & NIF_GUID != 0 && nid.len() >= NID_GUID + 16).then(|| {
        let raw: [u8; 16] = nid[NID_GUID..NID_GUID + 16].try_into().unwrap_or([0; 16]);
        guid_from_le(raw)
    });
    Some(Notify {
        message: u32_at(bytes, 4),
        hwnd: u32_at(nid, NID_HWND),
        uid: u32_at(nid, NID_UID),
        flags,
        callback: u32_at(nid, NID_CALLBACK),
        icon: u32_at(nid, NID_ICON),
        tip: wide_at(nid, NID_TIP, 128),
        state: u32_at(nid, NID_STATE),
        state_mask: u32_at(nid, NID_STATE_MASK),
        version: u32_at(nid, NID_VERSION),
        guid: guid.filter(|&g| g != 0),
    })
}

/// A `GUID` as Windows stores it (the first three fields little-endian) → `u128` in the
/// canonical order `{AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE}`.
pub fn guid_from_le(raw: [u8; 16]) -> u128 {
    let d1 = u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]);
    let d2 = u16::from_le_bytes([raw[4], raw[5]]);
    let d3 = u16::from_le_bytes([raw[6], raw[7]]);
    let d4 = u64::from_be_bytes(raw[8..16].try_into().unwrap_or([0; 8]));
    (u128::from(d1) << 96) | (u128::from(d2) << 80) | (u128::from(d3) << 64) | u128::from(d4)
}

/// `{C08445CE-867D-F78A-5E78-0E2D468C15BD}` (the registry's form) → `u128`.
pub fn parse_guid(text: &str) -> Option<u128> {
    let hex: String = text
        .trim()
        .trim_start_matches('{')
        .trim_end_matches('}')
        .chars()
        .filter(|&c| c != '-')
        .collect();
    (hex.len() == 32)
        .then(|| u128::from_str_radix(&hex, 16).ok())
        .flatten()
}

/// One icon in the notification area.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Icon {
    pub hwnd: u32,
    pub uid: u32,
    pub guid: Option<u128>,
    pub callback: u32,
    /// `NOTIFYICON_VERSION`: 0 (the original), 3 or 4.
    pub version: u32,
    pub tip: String,
    pub state: u32,
    /// The image, as a `data:` URL.
    pub image: Option<String>,
    /// The owner's process and its exe (lowercase), learned when it was added.
    pub pid: u32,
    pub exe: String,
}

impl Icon {
    /// A stable id for the page: the GUID when it has one, else the owner window and uID.
    pub fn id(&self) -> String {
        match self.guid {
            Some(guid) => format!("g{guid:032x}"),
            None => format!("w{:x}.{}", self.hwnd, self.uid),
        }
    }

    pub fn hidden(&self) -> bool {
        self.state & NIS_HIDDEN != 0
    }

    fn matches(&self, notify: &Notify) -> bool {
        match notify.guid {
            Some(guid) => self.guid == Some(guid),
            None => notify.hwnd != 0 && self.hwnd == notify.hwnd && self.uid == notify.uid,
        }
    }
}

/// What a message did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Applied {
    /// What `Shell_NotifyIcon` returns to the app.
    pub accepted: bool,
    /// Whether the visible list changed.
    pub changed: bool,
}

/// What's known about a new icon's owner (its process), looked up by the host.
pub struct Owner {
    pub pid: u32,
    pub exe: String,
}

/// The notification-area icons, in the order they were added.
#[derive(Debug, Default)]
pub struct Icons {
    pub list: Vec<Icon>,
}

impl Icons {
    /// Applies one message. `image` is the icon's new picture when the message sets one
    /// (`NIF_ICON`; `Some(None)` when it couldn't be read). `owner` describes a new icon's process.
    pub fn apply(
        &mut self,
        notify: &Notify,
        image: Option<Option<String>>,
        owner: impl FnOnce() -> Owner,
    ) -> Applied {
        let at = self.list.iter().position(|i| i.matches(notify));
        match notify.message {
            NIM_ADD | NIM_MODIFY => {
                if notify.hwnd == 0 && at.is_none() {
                    // Nobody to send clicks to.
                    return Applied {
                        accepted: false,
                        changed: false,
                    };
                }
                let existed = at.is_some();
                let index = at.unwrap_or_else(|| {
                    let owner = owner();
                    self.list.push(Icon {
                        hwnd: notify.hwnd,
                        uid: notify.uid,
                        guid: notify.guid,
                        callback: 0,
                        version: 0,
                        tip: String::new(),
                        state: 0,
                        image: None,
                        pid: owner.pid,
                        exe: owner.exe,
                    });
                    self.list.len() - 1
                });
                let icon = &mut self.list[index];
                let before = icon.clone();
                if notify.hwnd != 0 {
                    icon.hwnd = notify.hwnd;
                    icon.uid = notify.uid;
                }
                if notify.flags & NIF_MESSAGE != 0 {
                    icon.callback = notify.callback;
                }
                if notify.flags & NIF_TIP != 0 {
                    icon.tip.clone_from(&notify.tip);
                }
                if notify.flags & NIF_STATE != 0 {
                    icon.state =
                        (icon.state & !notify.state_mask) | (notify.state & notify.state_mask);
                }
                if notify.flags & NIF_GUID != 0 && notify.guid.is_some() {
                    icon.guid = notify.guid;
                }
                if let Some(image) = image {
                    icon.image = image;
                }
                let changed = !existed || *icon != before;
                // A modify of an icon we didn't have is an error, so the app adds it properly
                // (with every field); what it sent is kept meanwhile.
                Applied {
                    accepted: existed || notify.message == NIM_ADD,
                    changed,
                }
            }
            NIM_DELETE => match at {
                Some(index) => {
                    self.list.remove(index);
                    Applied {
                        accepted: true,
                        changed: true,
                    }
                }
                None => Applied {
                    accepted: false,
                    changed: false,
                },
            },
            NIM_SETVERSION => {
                if notify.version > 4 {
                    return Applied {
                        accepted: false,
                        changed: false,
                    };
                }
                if let Some(index) = at {
                    self.list[index].version = notify.version;
                }
                Applied {
                    accepted: true,
                    changed: false,
                }
            }
            NIM_SETFOCUS => Applied {
                accepted: at.is_some(),
                changed: false,
            },
            _ => Applied {
                accepted: false,
                changed: false,
            },
        }
    }

    /// Sets the picture of the icon `notify` names, if it's (still) there. Returns whether it
    /// changed.
    pub fn set_image(&mut self, notify: &Notify, image: Option<String>) -> bool {
        match self.list.iter_mut().find(|i| i.matches(notify)) {
            Some(icon) if icon.image != image => {
                icon.image = image;
                true
            }
            _ => false,
        }
    }

    pub fn find(&self, id: &str) -> Option<&Icon> {
        self.list.iter().find(|i| i.id() == id)
    }

    /// Drops icons whose owner is gone (`alive` says whether an owner window still belongs to its
    /// process). Explorer drops them silently too. Returns whether any went.
    pub fn prune(&mut self, alive: impl Fn(&Icon) -> bool) -> bool {
        let before = self.list.len();
        self.list.retain(|icon| alive(icon));
        self.list.len() != before
    }
}

// --- Clicks -----------------------------------------------------------------------------------

const WM_MOUSEMOVE: u32 = 0x0200;
const WM_LBUTTONDOWN: u32 = 0x0201;
const WM_LBUTTONUP: u32 = 0x0202;
const WM_LBUTTONDBLCLK: u32 = 0x0203;
const WM_RBUTTONDOWN: u32 = 0x0204;
const WM_RBUTTONUP: u32 = 0x0205;
const WM_MBUTTONDOWN: u32 = 0x0207;
const WM_MBUTTONUP: u32 = 0x0208;
const WM_CONTEXTMENU: u32 = 0x007B;
/// `NIN_SELECT` (`WM_USER`).
const NIN_SELECT: u32 = 0x0400;

/// A click on an icon in the bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Click {
    Left,
    Right,
    Middle,
    Double,
}

/// The callback messages a click sends, as `(wParam, lParam)` pairs, in order, for an icon of
/// `version` with `uid`, the pointer at `cursor` (screen pixels).
///
/// Version 4 packs the pointer into `wParam` (x low, y high) and the message and uID into
/// `lParam`; earlier versions send the uID as `wParam` and the message as `lParam`. Versions 3
/// and up also get `NIN_SELECT` after a left click and `WM_CONTEXTMENU` after a right click
/// (Explorer sends those for version 3 too).
pub fn click_messages(
    click: Click,
    version: u32,
    uid: u32,
    cursor: (i32, i32),
) -> Vec<(usize, isize)> {
    let modern = version >= 3;
    let messages: &[u32] = match (click, modern) {
        (Click::Left, true) => &[WM_MOUSEMOVE, WM_LBUTTONDOWN, WM_LBUTTONUP, NIN_SELECT],
        (Click::Left, false) => &[WM_MOUSEMOVE, WM_LBUTTONDOWN, WM_LBUTTONUP],
        (Click::Right, true) => &[WM_MOUSEMOVE, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_CONTEXTMENU],
        (Click::Right, false) => &[WM_MOUSEMOVE, WM_RBUTTONDOWN, WM_RBUTTONUP],
        (Click::Middle, _) => &[WM_MOUSEMOVE, WM_MBUTTONDOWN, WM_MBUTTONUP],
        (Click::Double, _) => &[WM_LBUTTONDBLCLK, WM_LBUTTONUP],
    };
    messages
        .iter()
        .map(|&message| pack(version, uid, message, cursor))
        .collect()
}

/// One callback message's `(wParam, lParam)`.
pub fn pack(version: u32, uid: u32, message: u32, (x, y): (i32, i32)) -> (usize, isize) {
    let low_high = |low: u32, high: u32| (low & 0xFFFF) | ((high & 0xFFFF) << 16);
    if version >= 4 {
        // MAKEWPARAM(x, y), MAKELPARAM(message, uID). Coordinates are signed 16-bit (monitors
        // left of the primary are negative).
        let wparam = low_high(x as u16 as u32, y as u16 as u32) as usize;
        let lparam = low_high(message, uid) as isize;
        (wparam, lparam)
    } else {
        (uid as usize, message as isize)
    }
}

// --- The shell's records -----------------------------------------------------------------------

/// One `HKCU\Control Panel\NotifyIconSettings\<id>` entry.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Setting {
    /// The subkey's name, also what `UIOrderList` lists.
    pub key: u64,
    /// `ExecutablePath`, resolved and lowercase.
    pub exe: String,
    pub uid: Option<u32>,
    pub guid: Option<u128>,
    /// `IsPromoted`: the user pinned it to the taskbar (new icons go to the overflow).
    pub promoted: bool,
}

/// `UIOrderList`: the subkeys' ids as little-endian u64s, in the user's order.
pub fn parse_order(bytes: &[u8]) -> Vec<u64> {
    bytes
        .as_chunks::<8>()
        .0
        .iter()
        .map(|&id| u64::from_le_bytes(id))
        .collect()
}

/// The setting that describes `icon`: same exe, and the same GUID (or uID, for icons without
/// one).
pub fn setting_of<'a>(icon: &Icon, settings: &'a [Setting]) -> Option<&'a Setting> {
    settings.iter().find(|s| {
        s.exe == icon.exe
            && match icon.guid {
                Some(guid) => s.guid == Some(guid),
                None => s.uid == Some(icon.uid),
            }
    })
}

/// What the page gets for one icon (also the shape the UI Automation fallback sends).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TrayItem {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// Shown in the bar; the rest are in the overflow.
    pub pinned: bool,
}

/// The visible icons for the page: hidden ones left out, in the user's order (`UIOrderList`;
/// icons it doesn't know keep their arrival order after the known ones), pinned as the user
/// promoted them. `name` names an icon whose tooltip is empty (its exe's description).
pub fn items(
    icons: &[Icon],
    settings: &[Setting],
    order: &[u64],
    mut name: impl FnMut(&Icon) -> String,
) -> Vec<TrayItem> {
    let mut ranked: Vec<(usize, TrayItem)> = icons
        .iter()
        .filter(|icon| !icon.hidden())
        .map(|icon| {
            let setting = setting_of(icon, settings);
            let rank = setting
                .and_then(|s| order.iter().position(|&k| k == s.key))
                .unwrap_or(usize::MAX);
            let tip = icon.tip.trim();
            let item = TrayItem {
                id: icon.id(),
                name: if tip.is_empty() {
                    name(icon)
                } else {
                    tip.to_owned()
                },
                icon: icon.image.clone(),
                pinned: setting.is_some_and(|s| s.promoted),
            };
            (rank, item)
        })
        .collect();
    ranked.sort_by_key(|(rank, _)| *rank); // stable: unknown icons keep their arrival order
    ranked.into_iter().map(|(_, item)| item).collect()
}

// --- App-bar messages (dwData 0) ----------------------------------------------------------------

/// `APPBARMSGDATAV3`: an `APPBARDATA` with 64-bit fields (40 bytes), then the message, the
/// shared-memory handle and the sender's process id.
pub const APPBAR_SIZE: usize = 64;
pub const APPBAR_HWND: usize = 4;
pub const APPBAR_EDGE: usize = 12;
pub const APPBAR_RECT: usize = 16;
pub const APPBAR_MESSAGE: usize = 40;
pub const APPBAR_SHARED: usize = 48;
pub const APPBAR_PID: usize = 56;
/// The `APPBARDATA` itself, as it sits in the shared memory.
pub const APPBARDATA_SIZE: usize = 40;

pub const ABM_QUERYPOS: u32 = 2;
pub const ABM_SETPOS: u32 = 3;
pub const ABM_GETTASKBARPOS: u32 = 5;

/// The fields of an app-bar message the host looks at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppBarMessage {
    pub message: u32,
    pub hwnd: u32,
    pub shared: u64,
    pub pid: u32,
}

pub fn parse_appbar(bytes: &[u8]) -> Option<AppBarMessage> {
    if bytes.len() < APPBAR_SIZE || u32_at(bytes, 0) as usize != APPBARDATA_SIZE {
        return None;
    }
    let shared = u64::from_le_bytes(bytes[APPBAR_SHARED..APPBAR_SHARED + 8].try_into().ok()?);
    Some(AppBarMessage {
        message: u32_at(bytes, APPBAR_MESSAGE),
        hwnd: u32_at(bytes, APPBAR_HWND),
        shared,
        pid: u32_at(bytes, APPBAR_PID),
    })
}

/// Points an app-bar message at another shared block, owned by another process.
pub fn retarget_appbar(bytes: &mut [u8], shared: u64, pid: u32) {
    bytes[APPBAR_SHARED..APPBAR_SHARED + 8].copy_from_slice(&shared.to_le_bytes());
    bytes[APPBAR_PID..APPBAR_PID + 4].copy_from_slice(&pid.to_le_bytes());
}

/// Fills an `APPBARDATA` (in shared memory) with the taskbar's edge and rect, for
/// `ABM_GETTASKBARPOS`.
pub fn write_taskbar_pos(data: &mut [u8], edge: u32, rect: [i32; 4]) {
    data[APPBAR_EDGE..APPBAR_EDGE + 4].copy_from_slice(&edge.to_le_bytes());
    for (i, v) in rect.iter().enumerate() {
        let at = APPBAR_RECT + 4 * i;
        data[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }
}

/// `Shell_NotifyIconGetRect`'s query (dwData 3): which icon, and whether it asks for the
/// position (1) or the size (2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RectQuery {
    pub message: u32,
    pub hwnd: u32,
    pub uid: u32,
    pub guid: Option<u128>,
}

/// `WINNOTIFYICONIDENTIFIER`: magic, message, size, padding, then the icon's 32-bit window, its
/// uID and its GUID (40 bytes).
pub fn parse_rect_query(bytes: &[u8]) -> Option<RectQuery> {
    if bytes.len() < 24 {
        return None;
    }
    let guid = bytes
        .get(24..40)
        .and_then(|b| <[u8; 16]>::try_from(b).ok())
        .map(guid_from_le)
        .filter(|&g| g != 0);
    Some(RectQuery {
        message: u32_at(bytes, 4),
        hwnd: u32_at(bytes, 16),
        uid: u32_at(bytes, 20),
        guid,
    })
}

/// The reply to a rect query: `MAKELPARAM` of the position (message 1) or the size (2).
pub fn rect_reply(message: u32, [x, y, width, height]: [i32; 4]) -> isize {
    let pack = |low: i32, high: i32| ((i32::from(high as i16) << 16) | (low & 0xFFFF)) as isize;
    match message {
        1 => pack(x, y),
        2 => pack(width, height),
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `NOTIFYICONDATA` as sent: 32-bit handles, `szTip[128]`, `szInfo[256]`, `szInfoTitle[64]`.
    const NID_SIZE: usize = 956;
    /// `SHELLTRAYDATA`: signature, message, then the `NOTIFYICONDATA`.
    const SHELLTRAYDATA_SIZE: usize = 8 + NID_SIZE;

    /// A `SHELLTRAYDATA` as `Shell_NotifyIcon` sends it, cut to `len` bytes.
    fn shelltraydata(message: u32, fields: &[(usize, u32)], tip: &str, len: usize) -> Vec<u8> {
        let mut bytes = vec![0u8; SHELLTRAYDATA_SIZE];
        bytes[0..4].copy_from_slice(&NOTIFY_SIGNATURE.to_le_bytes());
        bytes[4..8].copy_from_slice(&message.to_le_bytes());
        bytes[8..12].copy_from_slice(&(NID_SIZE as u32).to_le_bytes());
        for &(at, value) in fields {
            bytes[8 + at..12 + at].copy_from_slice(&value.to_le_bytes());
        }
        for (i, unit) in tip.encode_utf16().enumerate() {
            bytes[8 + NID_TIP + 2 * i..10 + NID_TIP + 2 * i].copy_from_slice(&unit.to_le_bytes());
        }
        bytes.truncate(len);
        bytes
    }

    const GUID_TEXT: &str = "{C08445CE-867D-F78A-5E78-0E2D468C15BD}";

    fn guid_bytes() -> [u8; 16] {
        // How Windows lays that GUID out in memory.
        [
            0xCE, 0x45, 0x84, 0xC0, 0x7D, 0x86, 0x8A, 0xF7, 0x5E, 0x78, 0x0E, 0x2D, 0x46, 0x8C,
            0x15, 0xBD,
        ]
    }

    #[test]
    fn parses_every_notifyicondata_version() {
        let fields = [
            (NID_HWND, 0x0001_0ABC),
            (NID_UID, 7),
            (NID_FLAGS, NIF_MESSAGE | NIF_ICON | NIF_TIP),
            (NID_CALLBACK, 0x8001),
            (NID_ICON, 0x0002_0DEF),
        ];
        // The full struct (Vista and later), the 5.0/6.0 sizes, and the original one (a
        // 64-character tip and nothing after it).
        for len in [SHELLTRAYDATA_SIZE, 8 + 952, 8 + 936, 8 + 24 + 128] {
            let notify = parse_notify(&shelltraydata(NIM_ADD, &fields, "Spotify", len)).unwrap();
            assert_eq!(notify.message, NIM_ADD, "{len}");
            assert_eq!(notify.hwnd, 0x0001_0ABC);
            assert_eq!(notify.uid, 7);
            assert_eq!(notify.callback, 0x8001);
            assert_eq!(notify.icon, 0x0002_0DEF);
            assert_eq!(notify.tip, "Spotify");
            assert_eq!(notify.guid, None);
        }

        let mut full = shelltraydata(
            NIM_SETVERSION,
            &[(NID_HWND, 5), (NID_FLAGS, NIF_GUID), (NID_VERSION, 4)],
            "",
            SHELLTRAYDATA_SIZE,
        );
        full[8 + NID_GUID..8 + NID_GUID + 16].copy_from_slice(&guid_bytes());
        let notify = parse_notify(&full).unwrap();
        assert_eq!(notify.version, 4);
        assert_eq!(notify.guid, parse_guid(GUID_TEXT));
        // Without NIF_GUID the field is ignored.
        full[8 + NID_FLAGS..12 + NID_FLAGS].copy_from_slice(&0u32.to_le_bytes());
        assert_eq!(parse_notify(&full).unwrap().guid, None);
    }

    #[test]
    fn refuses_what_isnt_a_tray_message() {
        let mut bytes = shelltraydata(NIM_ADD, &[], "", SHELLTRAYDATA_SIZE);
        bytes[0] ^= 1;
        assert_eq!(parse_notify(&bytes), None, "wrong signature");
        assert_eq!(parse_notify(&[0; 12]), None, "too short");
    }

    #[test]
    fn guids_read_the_way_windows_writes_them() {
        assert_eq!(guid_from_le(guid_bytes()), parse_guid(GUID_TEXT).unwrap());
        assert_eq!(
            parse_guid("c08445ce-867d-f78a-5e78-0e2d468c15bd"),
            parse_guid(GUID_TEXT)
        );
        assert_eq!(parse_guid("{nope}"), None);
    }

    fn notify(message: u32, hwnd: u32, uid: u32, flags: u32) -> Notify {
        Notify {
            message,
            hwnd,
            uid,
            flags,
            callback: 0x8000,
            icon: 0,
            tip: "Tip".into(),
            state: 0,
            state_mask: 0,
            version: 0,
            guid: None,
        }
    }

    fn owner() -> Owner {
        Owner {
            pid: 42,
            exe: r"c:\app\app.exe".into(),
        }
    }

    #[test]
    fn icons_follow_add_modify_delete() {
        let mut icons = Icons::default();
        let add = notify(NIM_ADD, 10, 1, NIF_MESSAGE | NIF_TIP | NIF_ICON);
        let applied = icons.apply(&add, Some(Some("data:a".into())), owner);
        assert_eq!(
            applied,
            Applied {
                accepted: true,
                changed: true
            }
        );
        assert_eq!(icons.list[0].image.as_deref(), Some("data:a"));
        assert_eq!(icons.list[0].pid, 42);

        // A modify that changes nothing visible.
        let same = notify(NIM_MODIFY, 10, 1, NIF_TIP);
        assert!(!icons.apply(&same, None, owner).changed);

        // Hidden through the state mask, then shown again.
        let mut hide = notify(NIM_MODIFY, 10, 1, NIF_STATE);
        hide.state = NIS_HIDDEN;
        hide.state_mask = NIS_HIDDEN;
        assert!(icons.apply(&hide, None, owner).changed);
        assert!(icons.list[0].hidden());
        hide.state = 0;
        icons.apply(&hide, None, owner);
        assert!(!icons.list[0].hidden());

        // A modify for an unknown icon is refused (so the app adds it again) but kept.
        let stray = notify(NIM_MODIFY, 11, 1, NIF_TIP);
        assert!(!icons.apply(&stray, None, owner).accepted);
        assert_eq!(icons.list.len(), 2);

        // Versions.
        let mut version = notify(NIM_SETVERSION, 10, 1, 0);
        version.version = 4;
        assert!(icons.apply(&version, None, owner).accepted);
        assert_eq!(icons.list[0].version, 4);
        version.version = 9;
        assert!(!icons.apply(&version, None, owner).accepted);

        let delete = notify(NIM_DELETE, 10, 1, 0);
        assert!(icons.apply(&delete, None, owner).changed);
        assert!(!icons.apply(&delete, None, owner).accepted, "already gone");
        assert_eq!(icons.list.len(), 1);
    }

    #[test]
    fn guid_icons_are_matched_by_guid_even_without_a_window() {
        let mut icons = Icons::default();
        let mut add = notify(NIM_ADD, 10, 0, NIF_MESSAGE | NIF_GUID);
        add.guid = Some(7);
        icons.apply(&add, None, owner);
        let mut modify = notify(NIM_MODIFY, 0, 0, NIF_TIP | NIF_GUID);
        modify.guid = Some(7);
        modify.tip = "New".into();
        assert!(icons.apply(&modify, None, owner).accepted);
        assert_eq!(icons.list[0].tip, "New");
        assert_eq!(icons.list[0].hwnd, 10, "a GUID-only update keeps the owner");
        assert_eq!(icons.list[0].id(), format!("g{:032x}", 7));

        // Without a window and unknown: nobody to click.
        let mut orphan = notify(NIM_ADD, 0, 0, NIF_GUID);
        orphan.guid = Some(8);
        assert!(!icons.apply(&orphan, None, owner).accepted);
    }

    #[test]
    fn clicks_pack_per_version() {
        // Version 0 and 3: wParam is the uID, lParam the message.
        assert_eq!(
            click_messages(Click::Left, 0, 7, (100, 5)),
            vec![(7, 0x200), (7, 0x201), (7, 0x202)]
        );
        assert_eq!(
            click_messages(Click::Right, 3, 7, (100, 5)),
            vec![(7, 0x200), (7, 0x204), (7, 0x205), (7, 0x7B)]
        );
        // Version 4: wParam is the pointer, lParam the message and the uID.
        let at = (1920 + 40, 12);
        let point = (1960 | (12 << 16)) as usize;
        assert_eq!(
            click_messages(Click::Left, 4, 7, at),
            vec![
                (point, 0x200 | (7 << 16)),
                (point, 0x201 | (7 << 16)),
                (point, 0x202 | (7 << 16)),
                (point, 0x400 | (7 << 16)),
            ]
        );
        assert_eq!(
            click_messages(Click::Double, 4, 2, at)[0],
            (point, 0x203 | (2 << 16))
        );
        // A monitor left of the primary: x is negative, packed as signed 16-bit.
        assert_eq!(pack(4, 1, 0x7B, (-10, 3)).0, (0xFFF6 | (3 << 16)) as usize);
    }

    fn icon(exe: &str, uid: u32, guid: Option<u128>, tip: &str) -> Icon {
        Icon {
            hwnd: 1,
            uid,
            guid,
            callback: 0,
            version: 4,
            tip: tip.into(),
            state: 0,
            image: None,
            pid: 1,
            exe: exe.into(),
        }
    }

    #[test]
    fn items_take_the_users_pins_and_order() {
        let icons = [
            icon(r"c:\a.exe", 1, None, "A"),
            icon(r"c:\b.exe", 0, Some(9), ""),
            icon(r"c:\c.exe", 3, None, "C"),
            Icon {
                state: NIS_HIDDEN,
                ..icon(r"c:\d.exe", 1, None, "Hidden")
            },
        ];
        let settings = [
            Setting {
                key: 100,
                exe: r"c:\a.exe".into(),
                uid: Some(1),
                guid: None,
                promoted: false,
            },
            Setting {
                key: 200,
                exe: r"c:\b.exe".into(),
                uid: None,
                guid: Some(9),
                promoted: true,
            },
            // Same exe, another uID: not A's.
            Setting {
                key: 300,
                exe: r"c:\a.exe".into(),
                uid: Some(2),
                guid: None,
                promoted: true,
            },
        ];
        let order = parse_order(&[200u64, 300, 100].map(u64::to_le_bytes).concat());
        assert_eq!(order, [200, 300, 100]);
        let got = items(&icons, &settings, &order, |i| format!("({})", i.exe));
        let summary: Vec<(&str, bool)> = got.iter().map(|i| (i.name.as_str(), i.pinned)).collect();
        assert_eq!(
            summary,
            [(r"(c:\b.exe)", true), ("A", false), ("C", false)],
            "B first and pinned (by GUID), A by uID, C unknown at the end, the hidden one left out"
        );
    }

    #[test]
    fn appbar_messages_are_read_and_retargeted() {
        let mut bytes = vec![0u8; APPBAR_SIZE];
        bytes[0..4].copy_from_slice(&(APPBARDATA_SIZE as u32).to_le_bytes());
        bytes[APPBAR_HWND..APPBAR_HWND + 4].copy_from_slice(&0x1234u32.to_le_bytes());
        bytes[APPBAR_MESSAGE..APPBAR_MESSAGE + 4].copy_from_slice(&ABM_SETPOS.to_le_bytes());
        bytes[APPBAR_SHARED..APPBAR_SHARED + 8].copy_from_slice(&0xABCDu64.to_le_bytes());
        bytes[APPBAR_PID..APPBAR_PID + 4].copy_from_slice(&77u32.to_le_bytes());
        let message = parse_appbar(&bytes).unwrap();
        assert_eq!(
            message,
            AppBarMessage {
                message: ABM_SETPOS,
                hwnd: 0x1234,
                shared: 0xABCD,
                pid: 77
            }
        );
        retarget_appbar(&mut bytes, 0x9999, 4);
        let moved = parse_appbar(&bytes).unwrap();
        assert_eq!(
            (moved.shared, moved.pid, moved.message),
            (0x9999, 4, ABM_SETPOS)
        );
        assert_eq!(parse_appbar(&bytes[..40]), None);

        let mut data = vec![0u8; APPBARDATA_SIZE];
        write_taskbar_pos(&mut data, 1, [0, 0, 2560, 32]);
        assert_eq!(u32_at(&data, APPBAR_EDGE), 1);
        assert_eq!(u32_at(&data, APPBAR_RECT + 8), 2560);
    }

    #[test]
    fn rect_queries_and_replies() {
        let mut bytes = vec![0u8; 40];
        bytes[4..8].copy_from_slice(&2u32.to_le_bytes());
        bytes[16..20].copy_from_slice(&0x77u32.to_le_bytes());
        bytes[20..24].copy_from_slice(&5u32.to_le_bytes());
        let query = parse_rect_query(&bytes).unwrap();
        assert_eq!(
            (query.message, query.hwnd, query.uid, query.guid),
            (2, 0x77, 5, None)
        );
        bytes[24..40].copy_from_slice(&guid_bytes());
        assert_eq!(
            parse_rect_query(&bytes).unwrap().guid,
            parse_guid(GUID_TEXT)
        );
        assert_eq!(
            rect_reply(1, [-20, 4, 24, 24]),
            ((4 << 16) | 0xFFEC) as isize
        );
        assert_eq!(rect_reply(2, [-20, 4, 24, 24]), ((24 << 16) | 24) as isize);
        assert_eq!(rect_reply(7, [0, 0, 1, 1]), 0);
    }
}
