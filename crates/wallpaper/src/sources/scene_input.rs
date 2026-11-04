//! Mouse input for `interactive` scene wallpapers: dragging with the left button
//! over the bare desktop orbits the camera (see `mypaper_desktop::raw_input`: the
//! mouse is read through Raw Input, nothing is hooked or injected, and the
//! desktop behaves exactly as before). The parallax lean toward the cursor
//! needs no input events at all: the engine already reads the cursor each frame.

use std::cell::Cell;
use std::rc::Rc;

use anyhow::Result;
use mypaper_desktop::desktop_hit::{Under, under};
use mypaper_desktop::raw_input::{Button, Gate, Kind, RawMouse};
use windows::Win32::Foundation::POINT;

/// Camera turn per dragged pixel, in radians (a screen width is about a turn and a half).
const RADIANS_PER_PIXEL: f32 = 0.006;
/// How far a vertical drag may lean the camera up or down (in the camera's -1..1 lean units).
const PITCH_LIMIT: f32 = 0.8;
/// Lean units per dragged pixel.
const LEAN_PER_PIXEL: f32 = 0.004;
/// Moves reach the camera once per frame at 60 fps.
const INTERVAL_MS: u32 = 16;

/// Where drags have turned the camera, relative to its own orbit.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Drag {
    /// Extra orbit angle, radians.
    pub yaw: f32,
    /// Extra up/down lean, camera lean units.
    pub pitch: f32,
}

impl Drag {
    /// The pointer moved `(dx, dy)` pixels while dragging: grabbing the scene and
    /// pulling right turns it right (the camera goes left); pulling down shows it from above.
    pub fn dragged(self, dx: i32, dy: i32) -> Self {
        Self {
            yaw: (self.yaw - dx as f32 * RADIANS_PER_PIXEL) % std::f32::consts::TAU,
            pitch: (self.pitch + dy as f32 * LEAN_PER_PIXEL).clamp(-PITCH_LIMIT, PITCH_LIMIT),
        }
    }
}

/// Listens to the mouse and keeps the camera offset up to date.
pub struct SceneInput {
    drag: Rc<Cell<Drag>>,
    raw: RawMouse,
}

impl SceneInput {
    pub fn start() -> Result<Self> {
        let drag = Rc::new(Cell::new(Drag::default()));
        let shared = drag.clone();
        let mut gate = Gate::default();
        // Where the left-button drag was last seen.
        let mut anchor: Option<(i32, i32)> = None;
        let mut raw = RawMouse::new(INTERVAL_MS, move |events| {
            for &event in events {
                let point = POINT {
                    x: event.at.0,
                    y: event.at.1,
                };
                let forwarded = gate.filter(
                    event,
                    || under(point) == Under::Desktop,
                    || mypaper_desktop::desktop_view::on_icon(event.at),
                );
                let Some(event) = forwarded else { continue };
                match (event.kind, anchor) {
                    (Kind::Down(Button::Left), _) => anchor = Some(event.at),
                    (Kind::Move, Some(from)) => {
                        shared.set(
                            shared
                                .get()
                                .dragged(event.at.0 - from.0, event.at.1 - from.1),
                        );
                        anchor = Some(event.at);
                    }
                    (Kind::Up(Button::Left), _) => anchor = None,
                    _ => {}
                }
            }
        })?;
        raw.set_active(true);
        Ok(Self { drag, raw })
    }

    /// The camera offset from drags so far.
    pub fn drag(&self) -> Drag {
        self.drag.get()
    }

    /// Listen only while some screen is visible (not while covered or held).
    pub fn set_live(&mut self, live: bool) {
        self.raw.set_active(live);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dragging_right_turns_the_camera_left_and_dragging_down_leans_it_up() {
        let drag = Drag::default().dragged(100, 50);
        assert!(drag.yaw < 0.0, "grab and pull right: the camera goes left");
        assert!(drag.pitch > 0.0, "grab and pull down: look from above");
        assert!((drag.yaw + 100.0 * RADIANS_PER_PIXEL).abs() < 1e-6);
    }

    #[test]
    fn drags_add_up_and_the_lean_is_limited_but_the_orbit_is_not() {
        let mut drag = Drag::default();
        for _ in 0..10 {
            drag = drag.dragged(-30, 0);
        }
        assert!((drag.yaw - 300.0 * RADIANS_PER_PIXEL).abs() < 1e-5);

        let steep = Drag::default().dragged(0, 100_000);
        assert_eq!(steep.pitch, PITCH_LIMIT);
        assert_eq!(Drag::default().dragged(0, -100_000).pitch, -PITCH_LIMIT);
        // Orbiting for ever stays a bounded angle.
        let spun = Drag::default().dragged(-1_000_000, 0);
        assert!(spun.yaw.abs() < std::f32::consts::TAU);
    }
}
