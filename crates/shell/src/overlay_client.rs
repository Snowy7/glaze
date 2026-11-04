//! The shell's end of the engine's glass overlays (docs/engine-notes.md, "Glass overlays"): the
//! thread's [`Backdrop`], which sends each popup's `Open`, `Place`, `Material`, `Close` and
//! `Warm` to the engine's request window as one `WM_COPYDATA`.
//!
//! The wire is the engine's own `request.rs`, compiled here as is, so both ends always agree.
//! Every popup is `Layer::Popup`. When no engine answers, [`Backdrop::available`] is false and
//! the views draw a fill of their own.

#[allow(
    dead_code,
    reason = "the engine-side half of the wire (decode, replies) is unused here"
)]
#[path = "../../wallpaper/src/overlay/request.rs"]
mod request;

use std::cell::Cell;
use std::rc::Rc;

use mypaper_desktop::Rect;
use mypaper_ui::glass::{Backdrop, GlassMaterial, Shape};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    IsWindow, SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE,
};

use self::request::{Layer, Material, Reply, Request};

/// Sends this thread's glass requests to the engine from now on.
pub fn install() {
    mypaper_ui::glass::install(Rc::new(Client::default()));
}

#[derive(Default)]
struct Client {
    /// The engine's request window, while it answers.
    engine: Cell<Option<HWND>>,
}

impl Client {
    fn engine(&self) -> Option<HWND> {
        // SAFETY: a read-only query; a stale handle reads as gone.
        let alive = |hwnd: HWND| unsafe { IsWindow(Some(hwnd)) }.as_bool();
        if let Some(hwnd) = self.engine.get().filter(|&h| alive(h)) {
            return Some(hwnd);
        }
        let found = request::find_engine();
        self.engine.set(found);
        found
    }

    /// Sends `request`; whether the engine took it.
    fn send(&self, request: &Request) -> bool {
        let Some(engine) = self.engine() else {
            return false;
        };
        match request::send(engine, request) {
            Reply::Taken => true,
            Reply::TakenExcludeYourself => {
                // This Windows has no per-session exclusion list: keep our popup out of its
                // own glass the old way (it then also leaves screenshots).
                // SAFETY: our own window.
                let _ =
                    unsafe { SetWindowDisplayAffinity(request.client(), WDA_EXCLUDEFROMCAPTURE) };
                true
            }
            Reply::Rejected => {
                log::debug!("glass overlay request refused: {request:?}");
                self.engine.set(None);
                false
            }
        }
    }
}

impl Backdrop for Client {
    fn available(&self) -> bool {
        self.engine().is_some()
    }

    fn open(&self, client: HWND, shape: Shape, radius: f32, material: &GlassMaterial) -> bool {
        self.send(&Request::Open {
            client,
            layer: Layer::Popup,
            rect: rect(shape),
            radius,
            material: wire(material),
        })
    }

    fn place(&self, client: HWND, shape: Shape, radius: f32) {
        self.send(&Request::Place {
            client,
            rect: rect(shape),
            radius,
        });
    }

    fn material(&self, client: HWND, material: &GlassMaterial) {
        self.send(&Request::Material {
            client,
            material: wire(material),
        });
    }

    fn close(&self, client: HWND) {
        self.send(&Request::Close { client });
    }

    fn warm(&self, client: HWND, shape: Shape) {
        self.send(&Request::Warm {
            client,
            rect: rect(shape),
        });
    }
}

fn rect(shape: Shape) -> Rect {
    Rect {
        x: shape.x,
        y: shape.y,
        width: shape.width,
        height: shape.height,
    }
}

/// The material as the engine takes it: sRGB bytes for the tint.
fn wire(material: &GlassMaterial) -> Material {
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    let t = material.tint;
    Material {
        clarity: material.clarity.clamp(0.0, 1.0),
        blur: material.blur.max(0.0),
        refraction: material.refraction.clamp(0.0, 1.0),
        highlight: material.highlight.clamp(0.0, 1.0),
        tint: [byte(t.r), byte(t.g), byte(t.b), 0],
    }
}
