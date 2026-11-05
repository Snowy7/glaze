//! What the shell asks of the engine's glass overlays, and how it travels: plain Rust types, sent
//! as one fixed-size `WM_COPYDATA` to the engine's message-only window (`OVERLAY_CLASS`). This
//! file has no GPU or engine dependencies: the shell compiles this same file into its overlay
//! client (`crates/shell/src/overlay_client.rs`, by `#[path]`), so both ends share one wire.
//!
//! Every overlay belongs to one client window (a shell popup or widget surface), which the shell
//! fills at alpha ≥ 1/255 over its whole shape so clicks land on it; the engine draws the glass in
//! a click-through window directly under it.

use bytemuck::{Pod, Zeroable};
use mypaper_desktop::Rect;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowExW, HWND_MESSAGE, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_COPYDATA,
};
use windows::core::{PCWSTR, w};

#[path = "channel.rs"]
mod channel;
pub use channel::Channel;
pub const WM_PLACEMENT: u32 = channel::WM_PLACEMENT;

/// The window class of the engine's overlay message window (message-only).
pub const OVERLAY_CLASS: PCWSTR = w!("mypaper.wallpaper.overlay");
/// `COPYDATASTRUCT::dwData` of an overlay request ("MPOV"), and the wire version in its high byte.
const MAGIC: usize = 0x4D50_4F56;
/// How long the shell waits for the engine to take a request.
const TIMEOUT_MS: u32 = 50;

/// Where an overlay sits in the z-order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    /// A topmost window directly under the client popup (flyouts, menus, a bar over apps).
    Popup,
    /// A plain top-level window directly under the client widget, which the shell keeps just
    /// above the desktop (Progman): over the wallpaper and icons, under every app (web and app
    /// wallpapers, D-W). Not a child of the desktop: the capture can't exclude child windows.
    Desktop,
}

/// The glass material (plan: "Material controls"). Values are 0..1 unless noted.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct Material {
    /// 1 is clear glass, 0 fully tinted: the tint colour's opacity.
    pub clarity: f32,
    /// Blur radius in px: liquid-rust's frost, in points once divided by the screen's scale.
    pub blur: f32,
    /// How far the rim bends what is behind it.
    pub refraction: f32,
    /// The specular rim's strength.
    pub highlight: f32,
    /// The tint colour, sRGB bytes (the fourth byte is unused).
    pub tint: [u8; 4],
}

impl Material {
    /// Clear Liquid Glass, the default style's material.
    pub const CLEAR: Self = Self {
        clarity: 1.0,
        blur: 0.0,
        refraction: 0.6,
        highlight: 0.55,
        tint: [0; 4],
    };
}

/// One request from the shell. `client` names the overlay: one per client window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Request {
    /// Creates the overlay under `client`; its glass fades in once the backdrop arrives.
    Open {
        client: HWND,
        layer: Layer,
        /// The client surface's shape, in virtual-screen physical px.
        rect: Rect,
        /// Corner radius in px.
        radius: f32,
        material: Material,
    },
    /// Moves or resizes the overlay (send it with the client's own move, in the same frame).
    Place {
        client: HWND,
        rect: Rect,
        radius: f32,
    },
    /// Changes the material.
    Material { client: HWND, material: Material },
    /// Destroys the overlay. The capture stays warm for a few seconds, for the next popup.
    Close { client: HWND },
}

impl Request {
    pub fn client(&self) -> HWND {
        match *self {
            Self::Open { client, .. }
            | Self::Place { client, .. }
            | Self::Material { client, .. }
            | Self::Close { client } => client,
        }
    }
}

/// What the engine answers (the `WM_COPYDATA` result).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reply {
    /// Not taken: no engine, a malformed request, or the engine is hung.
    Rejected,
    /// Taken.
    Taken,
    /// Taken, but this Windows has no per-session exclusion list: the client must set
    /// `WDA_EXCLUDEFROMCAPTURE` on its own window (`SetWindowDisplayAffinity` only works on a
    /// process's own windows), or its glass would capture itself.
    TakenExcludeYourself,
}

impl Reply {
    pub fn to_lresult(self) -> isize {
        self as isize
    }

    fn from_lresult(value: usize) -> Self {
        match value {
            1 => Self::Taken,
            2 => Self::TakenExcludeYourself,
            _ => Self::Rejected,
        }
    }
}

/// The request on the wire: fixed size, no padding.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct Wire {
    kind: u32,
    layer: u32,
    client: u64,
    rect: [i32; 4],
    radius: f32,
    material: Material,
}

const OPEN: u32 = 1;
const PLACE: u32 = 2;
const MATERIAL: u32 = 3;
const CLOSE: u32 = 4;

impl From<&Request> for Wire {
    fn from(request: &Request) -> Self {
        let mut wire = Self::zeroed();
        wire.client = request.client().0 as usize as u64;
        match *request {
            Request::Open {
                layer,
                rect,
                radius,
                material,
                ..
            } => {
                wire.kind = OPEN;
                wire.layer = match layer {
                    Layer::Popup => 0,
                    Layer::Desktop => 1,
                };
                wire.rect = [rect.x, rect.y, rect.width, rect.height];
                wire.radius = radius;
                wire.material = material;
            }
            Request::Place { rect, radius, .. } => {
                wire.kind = PLACE;
                wire.rect = [rect.x, rect.y, rect.width, rect.height];
                wire.radius = radius;
            }
            Request::Material { material, .. } => {
                wire.kind = MATERIAL;
                wire.material = material;
            }
            Request::Close { .. } => wire.kind = CLOSE,
        }
        wire
    }
}

impl Wire {
    fn decode(self) -> Option<Request> {
        let client = HWND(self.client as usize as *mut _);
        let [x, y, width, height] = self.rect;
        let rect = Rect {
            x,
            y,
            width,
            height,
        };
        let shape_ok = width > 0 && height > 0 && self.radius.is_finite() && self.radius >= 0.0;
        let material_ok = [
            self.material.clarity,
            self.material.blur,
            self.material.refraction,
            self.material.highlight,
        ]
        .iter()
        .all(|v| v.is_finite());
        Some(match self.kind {
            OPEN if shape_ok && material_ok => Request::Open {
                client,
                layer: match self.layer {
                    0 => Layer::Popup,
                    1 => Layer::Desktop,
                    _ => return None,
                },
                rect,
                radius: self.radius,
                material: self.material,
            },
            PLACE if shape_ok => Request::Place {
                client,
                rect,
                radius: self.radius,
            },
            MATERIAL if material_ok => Request::Material {
                client,
                material: self.material,
            },
            CLOSE => Request::Close { client },
            _ => return None,
        })
    }
}

/// The engine side: the request inside a `WM_COPYDATA`'s `lParam`, if it is one.
///
/// # Safety
/// `lparam` must be the `lParam` of a `WM_COPYDATA` being handled (a valid `COPYDATASTRUCT`).
pub unsafe fn decode(lparam: LPARAM) -> Option<Request> {
    // SAFETY: the caller's contract.
    let data = unsafe { &*(lparam.0 as *const COPYDATASTRUCT) };
    if data.dwData != MAGIC || data.cbData as usize != size_of::<Wire>() || data.lpData.is_null() {
        return None;
    }
    // SAFETY: `cbData` bytes at `lpData`, alive for the message; read unaligned.
    let wire = unsafe { data.lpData.cast::<Wire>().read_unaligned() };
    wire.decode()
}

/// The shell side: the engine's overlay window, if the engine runs.
pub fn find_engine() -> Option<HWND> {
    // SAFETY: a plain lookup among message-only windows.
    unsafe { FindWindowExW(Some(HWND_MESSAGE), None, OVERLAY_CLASS, PCWSTR::null()) }.ok()
}

/// The shell side: sends `request` to the engine window `engine`, waiting at most 50 ms for the
/// engine to take it (it is queued there, not drawn yet).
pub fn send(engine: HWND, request: &Request) -> Reply {
    let wire = Wire::from(request);
    let data = COPYDATASTRUCT {
        dwData: MAGIC,
        cbData: size_of::<Wire>() as u32,
        lpData: (&raw const wire).cast_mut().cast(),
    };
    let mut result = 0usize;
    // SAFETY: `data` and `wire` outlive the call; the engine copies them before it returns.
    let sent = unsafe {
        SendMessageTimeoutW(
            engine,
            WM_COPYDATA,
            WPARAM(0),
            LPARAM(&raw const data as isize),
            SMTO_ABORTIFHUNG,
            TIMEOUT_MS,
            Some(&mut result),
        )
    };
    if sent.0 == 0 {
        return Reply::Rejected;
    }
    Reply::from_lresult(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(request: Request) -> Option<Request> {
        let wire = Wire::from(&request);
        let bytes = bytemuck::bytes_of(&wire).to_vec();
        let data = COPYDATASTRUCT {
            dwData: MAGIC,
            cbData: bytes.len() as u32,
            lpData: bytes.as_ptr().cast_mut().cast(),
        };
        // SAFETY: a valid COPYDATASTRUCT over live bytes.
        unsafe { decode(LPARAM(&raw const data as isize)) }
    }

    #[test]
    fn every_request_survives_the_wire() {
        let client = HWND(0x1234 as *mut _);
        let rect = Rect {
            x: -2400,
            y: 9,
            width: 320,
            height: 480,
        };
        let material = Material {
            tint: [20, 30, 40, 0],
            ..Material::CLEAR
        };
        for request in [
            Request::Open {
                client,
                layer: Layer::Desktop,
                rect,
                radius: 18.0,
                material,
            },
            Request::Place {
                client,
                rect,
                radius: 4.0,
            },
            Request::Material { client, material },
            Request::Close { client },
        ] {
            assert_eq!(roundtrip(request), Some(request));
        }
    }

    #[test]
    fn malformed_requests_are_refused() {
        let client = HWND(0x1234 as *mut _);
        let empty = Rect {
            x: 0,
            y: 0,
            width: 0,
            height: 10,
        };
        assert_eq!(
            roundtrip(Request::Place {
                client,
                rect: empty,
                radius: 4.0
            }),
            None,
            "an empty rect"
        );
        let nan = Material {
            blur: f32::NAN,
            ..Material::CLEAR
        };
        assert_eq!(
            roundtrip(Request::Material {
                client,
                material: nan
            }),
            None
        );
        let mut wire = Wire::from(&Request::Close { client });
        wire.kind = 9;
        assert_eq!(wire.decode(), None, "an unknown kind");
    }
}
