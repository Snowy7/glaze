//! The shell's end of the engine's glass overlays (docs/engine-notes.md, "Glass overlays"): the
//! thread's [`Backdrop`], which sends each popup's `Open`, `Place`, `Material` and `Close` to
//! the engine's request window. Open/close use `WM_COPYDATA`; changing geometry and material
//! overwrite a shared snapshot and post a coalesced wake, without waiting for the engine.
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

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use mypaper_desktop::Rect;
use mypaper_ui::glass::{Backdrop, GlassMaterial, GlassStyle, Shape};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    IsWindow, SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE,
};

use self::request::{Channel, Layer, Material, Reply, Request};

/// Sends this thread's glass requests to the engine from now on.
pub fn install() {
    mypaper_ui::glass::install(Rc::new(Client::default()));
}

/// Starts WGC before a possible click. The bounded engine calls run on a worker so input
/// delivery never waits for capture setup. Closing immediately keeps only its 3 s warmth.
pub fn warm(client: HWND, shape: Shape) {
    let client = client.0 as isize;
    let _ = std::thread::Builder::new()
        .name("glass-hover".into())
        .spawn(move || {
            let Some(engine) = request::find_engine() else {
                return;
            };
            let client = HWND(client as _);
            let start = std::time::Instant::now();
            let _ = request::send(
                engine,
                &Request::Open {
                    client,
                    layer: Layer::Popup,
                    rect: rect(shape),
                    radius: 0.0,
                    material: Material::CLEAR,
                },
            );
            let _ = request::send(
                engine,
                &Request::Style {
                    client,
                    style: request::Style::DEFAULT,
                },
            );
            let _ = request::send(engine, &Request::Close { client });
            log::debug!(
                "hover capture setup: {:.1} ms",
                start.elapsed().as_secs_f64() * 1000.0
            );
        });
}

#[derive(Default)]
struct Client {
    /// The engine's request window, while it answers.
    engine: Cell<Option<HWND>>,
    plates: RefCell<Vec<Plate>>,
}

struct Plate {
    client: HWND,
    shape: Shape,
    radius: f32,
    material: Material,
    channel: Channel,
}

impl Plate {
    fn request(&self) -> Request {
        Request::Open {
            client: self.client,
            layer: Layer::Popup,
            rect: rect(self.shape),
            radius: self.radius,
            material: self.material,
        }
    }
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
        self.open_styled(client, shape, radius, material, &GlassStyle::default())
    }

    fn open_styled(
        &self,
        client: HWND,
        shape: Shape,
        radius: f32,
        material: &GlassMaterial,
        style: &GlassStyle,
    ) -> bool {
        let Ok(channel) = Channel::open(client, true) else {
            return false;
        };
        let mut plate = Plate {
            client,
            shape,
            radius,
            material: wire(material),
            channel,
        };
        let request = plate.request();
        plate.channel.write(&request);
        if !self.send(&request) {
            return false;
        }
        self.style(client, style);
        let mut plates = self.plates.borrow_mut();
        plates.retain(|p| p.client != client);
        plates.push(plate);
        true
    }

    fn style(&self, client: HWND, style: &GlassStyle) {
        self.send(&Request::Style {
            client,
            style: request::Style {
                brightness: style.brightness,
                saturation: style.saturation,
                smoothing: style.smoothing,
                sheen: style.sheen.unwrap_or(-1.0),
                depth: style.depth.unwrap_or(-1.0),
                fade_ms: style.fade_ms,
                inner_shadow: style.inner_shadow,
                fade_curve: fade_curve(style),
            },
        });
    }

    fn place(&self, client: HWND, shape: Shape, radius: f32) {
        let mut plates = self.plates.borrow_mut();
        let Some(plate) = plates.iter_mut().find(|p| p.client == client) else {
            return;
        };
        if plate.shape == shape && plate.radius == radius {
            return;
        }
        let Some(engine) = self.engine() else {
            return;
        };
        let previous = (plate.shape, plate.radius);
        plate.shape = shape;
        plate.radius = radius;
        let request = plate.request();
        if !plate.channel.publish(engine, &request) {
            (plate.shape, plate.radius) = previous;
        }
    }

    fn material(&self, client: HWND, material: &GlassMaterial) {
        let material = wire(material);
        let mut plates = self.plates.borrow_mut();
        let Some(plate) = plates.iter_mut().find(|p| p.client == client) else {
            return;
        };
        if plate.material == material {
            return;
        }
        let Some(engine) = self.engine() else {
            return;
        };
        let previous = plate.material;
        plate.material = material;
        let request = plate.request();
        if !plate.channel.publish(engine, &request) {
            plate.material = previous;
        }
    }

    fn close(&self, client: HWND) {
        self.send(&Request::Close { client });
        self.plates.borrow_mut().retain(|p| p.client != client);
    }
}

/// Translate the already-resolved kit curve once per style request; names and motion policy
/// have been resolved with the surface's configuration, never in an animation frame.
fn fade_curve(style: &GlassStyle) -> request::FadeCurve {
    use mypaper_ui::transition::Easing;
    match style.fade_curve {
        None => request::FadeCurve::DEFAULT,
        Some(Easing::Bezier(points)) => request::FadeCurve::bezier(points),
        Some(Easing::Spring(s)) => {
            request::FadeCurve::spring(s.mass, s.stiffness, s.damping, style.fade_ms / 1000.0)
        }
        Some(Easing::TimedSpring { spring: s, settle }) => {
            request::FadeCurve::spring(s.mass, s.stiffness, s.damping, settle)
        }
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
        blur: material.blur.clamp(0.0, 256.0),
        refraction: material.refraction.clamp(0.0, 1.0),
        highlight: material.highlight.clamp(0.0, 1.0),
        tint: [byte(t.r), byte(t.g), byte(t.b), 0],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_curves_follow_the_resolved_content_transition() {
        use crate::bars::{look::glass_fade, motion};
        use mypaper_bars::style::{Curve, Motion, MotionStyle};
        use std::time::{Duration, Instant};
        let curves = mypaper_bars::style::NAMED_CURVES
            .iter()
            .map(|name| Curve::Named((*name).into()))
            .chain([
                Curve::Bezier([0.13, -0.4, 0.8, 1.7]),
                Curve::Spring(mypaper_bars::style::Spring {
                    mass: 1.0,
                    stiffness: 80.0,
                    damping: 25.0,
                }),
            ]);
        for curve in curves {
            let root = Motion {
                duration: Some(5000.0),
                curve: Some(curve.clone()),
                ..Default::default()
            };
            let timing = motion::resolve(&root, false, motion::Leaf::Open, true, false);
            let mut style = GlassStyle::default();
            glass_fade(&mut style, timing, MotionStyle::Fade);
            let wire = fade_curve(&style);
            assert!(wire.valid(), "{curve:?}");
            let now = Instant::now();
            let content = timing.start(0.0, 1.0, now);
            for i in 0..=100 {
                let x = i as f32 / 100.0;
                let at = Duration::from_secs_f32(x * timing.duration.as_secs_f32());
                assert!(
                    (wire.at(x) - content.value(now + at).clamp(0.0, 1.0)).abs() < 0.001,
                    "{curve:?} at {x}"
                );
            }
        }
        let timing = motion::resolve(&Motion::default(), false, motion::Leaf::Open, false, false);
        let mut style = GlassStyle::default();
        glass_fade(&mut style, timing, MotionStyle::Fade);
        assert_eq!(style.fade_ms, 0.0);
        assert_eq!(style.fade_curve, None);
    }
}
