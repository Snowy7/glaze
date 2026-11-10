//! Popup glass: DWM's native host backdrop over real application windows, with the engine's
//! captured overlays as fallback. Both live under the software-rendered layered surface.
//!
//! The wire is the engine's own `request.rs`, compiled here as is, so both ends always agree.
//! Engine fallback popups use `Layer::Popup`. When neither provider is available, views draw
//! their opaque fallback fill.

#[allow(
    dead_code,
    reason = "the engine-side half of the wire (decode, replies) is unused here"
)]
#[path = "../../wallpaper/src/overlay/request.rs"]
mod request;

#[path = "popup_stack.rs"]
mod stack;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use glaze_desktop::Rect;
use glaze_ui::glass::{Backdrop, GlassMaterial, GlassStyle, Shape};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    IsWindow, SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE,
};

use self::request::{Channel, Layer, Material, Reply, Request};

/// Installs native popup frost with an engine fallback for this UI thread.
pub fn install() {
    glaze_ui::glass::install(Rc::new(PopupBackdrop::default()));
}

/// Flat popup frost samples DWM's actual backdrop, including application windows. Keep the
/// engine available for systems without host-backdrop support. Widgets keep their plates.
struct PopupBackdrop {
    system: Rc<glaze_ui::platform::windows::system_backdrop::SystemBackdrop>,
    engine: Client,
    native: RefCell<Vec<HWND>>,
}

impl Default for PopupBackdrop {
    fn default() -> Self {
        Self {
            system: glaze_ui::platform::windows::glass::system(),
            engine: Client::default(),
            native: RefCell::default(),
        }
    }
}

impl Backdrop for PopupBackdrop {
    fn synchronous(&self) -> bool { self.system.available() }
    fn visibility(&self, client: HWND, shown: bool) -> bool {
        self.native.borrow().contains(&client) && self.system.visibility(client, shown)
    }
    fn opacity(&self, client: HWND, alpha: u8) {
        if self.native.borrow().contains(&client) { self.system.opacity(client, alpha); }
    }
    fn available(&self) -> bool {
        self.system.available() || self.engine.available()
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
        if self.system.open(client, shape, radius, material) {
            self.native.borrow_mut().push(client);
            stack::open(client);
            true
        } else {
            self.engine
                .open_styled(client, shape, radius, material, style)
        }
    }
    fn place(&self, client: HWND, shape: Shape, radius: f32) {
        if self.native.borrow().contains(&client) {
            self.system.place(client, shape, radius);
        } else {
            self.engine.place(client, shape, radius);
        }
    }
    fn material(&self, client: HWND, material: &GlassMaterial) {
        if self.native.borrow().contains(&client) {
            self.system.material(client, material);
        } else {
            self.engine.material(client, material);
        }
    }
    fn style(&self, client: HWND, style: &GlassStyle) {
        if !self.native.borrow().contains(&client) {
            self.engine.style(client, style);
        }
    }
    fn close(&self, client: HWND) {
        if self.native.borrow().contains(&client) {
            self.system.close(client);
            self.native.borrow_mut().retain(|h| *h != client);
            stack::close(client);
        } else {
            self.engine.close(client);
        }
    }
}

/// Starts WGC before a possible click. The bounded engine calls run on a worker so input
/// delivery never waits for capture setup. Closing immediately keeps only its 3 s warmth.
pub fn warm(client: HWND, shape: Shape) {
    if glaze_ui::platform::windows::glass::system().available() {
        return;
    }
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
        stack::open(client);
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
        stack::close(client);
        self.send(&Request::Close { client });
        self.plates.borrow_mut().retain(|p| p.client != client);
    }
}

/// Translate the already-resolved kit curve once per style request; names and motion policy
/// have been resolved with the surface's configuration, never in an animation frame.
fn fade_curve(style: &GlassStyle) -> request::FadeCurve {
    use glaze_ui::transition::Easing;
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
        tint: [byte(t.r), byte(t.g), byte(t.b), byte(t.a)],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_keeps_the_resolved_stained_tint_strength() {
        let material = GlassMaterial {
            clarity: 0.78,
            blur: 12.0,
            refraction: 0.6,
            highlight: 0.55,
            tint: glaze_ui::Rgba::rgb8([200, 141, 60], 0.22),
        };
        assert_eq!(wire(&material).tint, [200, 141, 60, 56]);
    }

    #[test]
    fn wire_curves_follow_the_resolved_content_transition() {
        use crate::bars::{look::glass_fade, motion};
        use glaze_bars::style::{Curve, Motion, MotionStyle};
        use std::time::{Duration, Instant};
        let curves = glaze_bars::style::NAMED_CURVES
            .iter()
            .map(|name| Curve::Named((*name).into()))
            .chain([
                Curve::Bezier([0.13, -0.4, 0.8, 1.7]),
                Curve::Spring(glaze_bars::style::Spring {
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
