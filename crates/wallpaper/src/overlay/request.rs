//! What the shell asks of the engine's glass overlays, and how it travels: plain Rust types, sent
//! as fixed-size `WM_COPYDATA` for lifecycle requests and a shared latest-state mailbox plus
//! posted wake for animation (`Channel`). Both target the engine's message-only window
//! (`OVERLAY_CLASS`). This file has no GPU or engine dependencies: the shell compiles it into its overlay
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
#[path = "curve.rs"]
mod curve;
pub use channel::Channel;
pub use curve::FadeCurve;
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

/// An overlay's finer glass knobs and its fade (parity C07, C10, C12, C13), sent with
/// [`Request::Style`] after each `Open` (a new overlay starts at [`Style::DEFAULT`]). The knobs
/// mean what `mypaper_glass_ipc::Style`'s do for plates.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct Style {
    /// Brightness of what's behind the glass, a multiplier, 0..=2 (1: as is).
    pub brightness: f32,
    /// Saturation of what's behind the glass, 0..=3 (1: as is, 0: grey).
    pub saturation: f32,
    /// Corner smoothing, 0..=1: 0 quarter circles, 0.6 iOS's continuous (squircle) corners.
    pub smoothing: f32,
    /// The soft light just inside the lit (top) edge, 0..=1 (0.5: the kit's); negative:
    /// follows the highlight.
    pub sheen: f32,
    /// The broad soft shade under the lit edge, 0..=1 (0.5: the kit's); negative: follows the
    /// highlight.
    pub depth: f32,
    /// How long the glass fades in when it first shows, in ms (0..=5000); 0 shows it at once,
    /// for reduced motion.
    pub fade_ms: f32,
    /// Bottom inner shadow, 0..=1; zero is off, independently of the lit rim's `depth`.
    pub inner_shadow: f32,
    /// Resolved named/Bézier/physical spring curve. Default keeps the original cubic ease-out.
    pub fade_curve: FadeCurve,
}

impl Style {
    /// What an overlay has until a `Style` request: everything as before, a 300 ms fade.
    pub const DEFAULT: Self = Self {
        brightness: 1.0,
        saturation: 1.0,
        smoothing: 0.0,
        sheen: -1.0,
        depth: -1.0,
        fade_ms: 300.0,
        inner_shadow: 0.0,
        fade_curve: FadeCurve::DEFAULT,
    };

    fn valid(&self) -> bool {
        let within = |v: f32, max: f32| (0.0..=max).contains(&v);
        let optional = |v: f32| v < 0.0 || within(v, 1.0);
        within(self.brightness, 2.0)
            && within(self.saturation, 3.0)
            && within(self.smoothing, 1.0)
            && optional(self.sheen)
            && optional(self.depth)
            && within(self.fade_ms, 5000.0)
            && within(self.inner_shadow, 1.0)
            && self.fade_curve.valid()
    }
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
    /// Sets the overlay's finer knobs and fade. An engine older than it refuses it (and only
    /// it): the rest of the wire is unchanged.
    Style { client: HWND, style: Style },
}

impl Request {
    pub fn client(&self) -> HWND {
        match *self {
            Self::Open { client, .. }
            | Self::Place { client, .. }
            | Self::Material { client, .. }
            | Self::Close { client }
            | Self::Style { client, .. } => client,
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

/// The request on the wire: fixed size, no padding. Only a `Style` request sends the `style`
/// tail; every other request is the shorter first form, as before it existed.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct Wire {
    kind: u32,
    layer: u32,
    client: u64,
    rect: [i32; 4],
    radius: f32,
    material: Material,
    style: Style,
}

/// The wire without its `style` tail.
const SHORT: usize = std::mem::offset_of!(Wire, style);
/// The first style version, retained for older engines and shells.
const STYLE_V1: usize = SHORT + std::mem::offset_of!(Style, inner_shadow);

const OPEN: u32 = 1;
const PLACE: u32 = 2;
const MATERIAL: u32 = 3;
const CLOSE: u32 = 4;
const STYLE: u32 = 5;

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
            Request::Style { style, .. } => {
                wire.kind = STYLE;
                wire.style = style;
            }
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
            STYLE if self.style.valid() => Request::Style {
                client,
                style: self.style,
            },
            _ => return None,
        })
    }

    /// The bytes `send` puts on the wire for it.
    fn bytes(&self) -> &[u8] {
        let all = bytemuck::bytes_of(self);
        if self.kind == STYLE {
            all
        } else {
            &all[..SHORT]
        }
    }
}

/// The engine side: the request inside a `WM_COPYDATA`'s `lParam`, if it is one.
///
/// # Safety
/// `lparam` must be the `lParam` of a `WM_COPYDATA` being handled (a valid `COPYDATASTRUCT`).
pub unsafe fn decode(lparam: LPARAM) -> Option<Request> {
    // SAFETY: the caller's contract.
    let data = unsafe { &*(lparam.0 as *const COPYDATASTRUCT) };
    let len = data.cbData as usize;
    if data.dwData != MAGIC
        || !(len == SHORT || len == STYLE_V1 || len == size_of::<Wire>())
        || data.lpData.is_null()
    {
        return None;
    }
    // A short wire has no style tail: it reads as zeros, which only a `Style` request (which
    // must be long) would look at.
    let mut wire = Wire::zeroed();
    wire.style = Style::DEFAULT;
    // SAFETY: `len` (at most the wire's size) bytes at `lpData`, alive for the message.
    unsafe {
        std::ptr::copy_nonoverlapping(data.lpData.cast::<u8>(), (&raw mut wire).cast::<u8>(), len)
    };
    if wire.kind == STYLE && len == SHORT {
        return None;
    }
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
    let mut wire = Wire::from(request);
    let Some(reply) = send_wire(engine, wire.bytes()) else {
        return Reply::Rejected;
    };
    if reply == Reply::Rejected && wire.kind == STYLE {
        // Older engines reject the extended size before reading it. Retry the original style
        // so they still receive brightness, saturation, corners and the supported fade range.
        wire.style.fade_ms = wire.style.fade_ms.min(2000.0);
        return send_wire(engine, &bytemuck::bytes_of(&wire)[..STYLE_V1])
            .unwrap_or(Reply::Rejected);
    }
    reply
}

fn send_wire(engine: HWND, bytes: &[u8]) -> Option<Reply> {
    let data = COPYDATASTRUCT {
        dwData: MAGIC,
        cbData: bytes.len() as u32,
        lpData: bytes.as_ptr().cast_mut().cast(),
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
        return None;
    }
    Some(Reply::from_lresult(result))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(request: Request) -> Option<Request> {
        let wire = Wire::from(&request);
        let bytes = wire.bytes().to_vec();
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
            Request::Style {
                client,
                style: Style {
                    saturation: 1.8,
                    smoothing: 0.6,
                    sheen: 0.4,
                    fade_ms: 0.0,
                    inner_shadow: 0.8,
                    fade_curve: FadeCurve::bezier([0.42, 0.0, 0.58, 1.0]),
                    ..Style::DEFAULT
                },
            },
        ] {
            assert_eq!(roundtrip(request), Some(request));
        }
    }

    #[test]
    fn only_a_style_request_is_longer_than_the_wire_before_it() {
        // An engine older than `Style` takes every other request: its wire was `SHORT` bytes.
        assert_eq!(SHORT, 56);
        let client = HWND(0x1234 as *mut _);
        assert_eq!(Wire::from(&Request::Close { client }).bytes().len(), SHORT);
        let style = Request::Style {
            client,
            style: Style::DEFAULT,
        };
        assert_eq!(Wire::from(&style).bytes().len(), size_of::<Wire>());
        // A style cut short, or out of range, is refused.
        let mut short = Wire::from(&style);
        let data = COPYDATASTRUCT {
            dwData: MAGIC,
            cbData: SHORT as u32,
            lpData: (&raw mut short).cast(),
        };
        // SAFETY: a valid COPYDATASTRUCT over a live wire.
        assert_eq!(unsafe { decode(LPARAM(&raw const data as isize)) }, None);
        let wild = Request::Style {
            client,
            style: Style {
                brightness: 9.0,
                ..Style::DEFAULT
            },
        };
        assert_eq!(roundtrip(wild), None);
    }

    #[test]
    fn legacy_styles_keep_their_knobs_and_new_fields_default() {
        assert_eq!(STYLE_V1, 80);
        let request = Request::Style {
            client: HWND(0x1234 as *mut _),
            style: Style {
                smoothing: 0.6,
                saturation: 2.0,
                fade_ms: 1200.0,
                ..Style::DEFAULT
            },
        };
        let wire = Wire::from(&request);
        let data = COPYDATASTRUCT {
            dwData: MAGIC,
            cbData: STYLE_V1 as u32,
            lpData: bytemuck::bytes_of(&wire).as_ptr().cast_mut().cast(),
        };
        // SAFETY: a valid COPYDATASTRUCT over the old prefix of a live wire.
        assert_eq!(
            unsafe { decode(LPARAM(&raw const data as isize)) },
            Some(request)
        );
        for (duration, curve, valid) in [
            (5000.0, FadeCurve::bezier([0.0, 0.0, 1.0, 1.0]), true),
            (0.0, FadeCurve::DEFAULT, true),
            (5001.0, FadeCurve::DEFAULT, false),
            (300.0, FadeCurve::bezier([f32::NAN, 0.0, 1.0, 1.0]), false),
            (300.0, FadeCurve::bezier([1.1, 0.0, 1.0, 1.0]), false),
        ] {
            let request = Request::Style {
                client: request.client(),
                style: Style {
                    fade_ms: duration,
                    fade_curve: curve,
                    ..Style::DEFAULT
                },
            };
            assert_eq!(roundtrip(request).is_some(), valid);
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
