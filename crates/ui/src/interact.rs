//! How a surface's hit items respond to the pointer and keys: hover, press, keyboard focus and
//! activation, in one place, so every surface (bar, flyouts, widgets) clicks the same way.
//!
//! A surface keeps one [`Interaction`] and feeds it its events with the display list on
//! screen. It answers [`Response::Redraw`] only when a state the view draws changed (Rule 2),
//! and [`Response::Activate`] when a control fires:
//! - **press** shows at once: the next frame (drawn in the same loop pass as the
//!   `WM_LBUTTONDOWN`) has the pressed wash;
//! - **activation** happens on release over the item that was pressed (moving off it before
//!   releasing cancels, as everywhere in Windows), or on Enter / Space for the focused item;
//! - **focus rings** show only after the keyboard moved focus (`focus_visible`), never on a
//!   click.

use crate::focus;
use crate::geom::{Point, Rgba};
use crate::scene::{DisplayList, HitId};
use crate::surface::{Button, Event};

/// What a control looks like now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Rest,
    Hover,
    Pressed,
    Disabled,
}

impl State {
    /// The wash over a control's own fill: subtle on hover, clearly deeper while pressed.
    /// `ink` is the surface's text colour, so it works over dark and light surfaces.
    pub fn wash(self, ink: Rgba) -> Rgba {
        match self {
            Self::Hover => ink.with_alpha(0.10),
            Self::Pressed => ink.with_alpha(0.20),
            Self::Rest | Self::Disabled => Rgba::TRANSPARENT,
        }
    }

    /// The scale of a control's content: pressed content sinks a little (3%).
    pub fn press_scale(self) -> f32 {
        if self == Self::Pressed { 0.97 } else { 1.0 }
    }
}

/// What the surface should do after an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Response {
    /// Nothing the view draws changed.
    Unchanged,
    /// Hover, press or focus changed: build and present the list again.
    Redraw,
    /// The control fired (and the state may have changed: redraw too).
    Activate(HitId),
}

/// Pointer and keyboard state over one surface's hit items.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Interaction {
    pub hover: Option<HitId>,
    /// Pressed and not yet released (shown pressed only while the pointer is over it).
    pub pressed: Option<HitId>,
    pub focus: Option<HitId>,
    /// The keyboard moved the focus: draw its ring.
    pub focus_visible: bool,
}

impl Interaction {
    /// How control `id` looks.
    pub fn state(&self, id: HitId, enabled: bool) -> State {
        if !enabled {
            State::Disabled
        } else if self.pressed == Some(id) && self.hover == Some(id) {
            State::Pressed
        } else if self.hover == Some(id) && self.pressed.is_none() {
            State::Hover
        } else {
            State::Rest
        }
    }

    /// Whether `id` draws a focus ring.
    pub fn ring(&self, id: HitId) -> bool {
        self.focus_visible && self.focus == Some(id)
    }

    /// Forgets everything (the surface was hidden or rebuilt from scratch).
    pub fn reset(&mut self) -> Response {
        let changed = *self != Self::default();
        *self = Self::default();
        if changed {
            Response::Redraw
        } else {
            Response::Unchanged
        }
    }

    /// Feeds one event; `list` is the display list on screen (its hits).
    pub fn handle(&mut self, list: &DisplayList, event: &Event) -> Response {
        let before = self.clone();
        let at = |p: Point| list.hit_at(p).filter(|h| h.enabled).map(|h| h.id);
        let mut fired = None;
        match *event {
            Event::Move(p) => self.hover = at(p.at),
            Event::Leave => {
                self.hover = None;
                // Without capture no release will come: a press dragged off the window ends.
                self.pressed = None;
            }
            Event::Press(p, Button::Left) | Event::DoubleClick(p, Button::Left) => {
                self.hover = at(p.at);
                self.pressed = self.hover;
                self.focus = self.hover.or(self.focus);
                self.focus_visible = false;
            }
            Event::Release(p, Button::Left) => {
                let over = at(p.at);
                if self.pressed.is_some() && self.pressed == over {
                    fired = over;
                }
                self.pressed = None;
                self.hover = over;
            }
            Event::CaptureLost | Event::Cancelled | Event::Deactivated => self.pressed = None,
            Event::Key(key) => match key {
                // Tab / Shift+Tab are told apart by the caller (see `step`); arrows step too.
                0x25 | 0x26 => self.step(list, -1),
                0x27 | 0x28 => self.step(list, 1),
                0x0D | 0x20 => {
                    fired = self
                        .focus
                        .filter(|id| list.hits.iter().any(|h| h.id == *id && h.enabled));
                }
                0x1B if self.focus_visible => {
                    self.focus = None;
                    self.focus_visible = false;
                }
                _ => {}
            },
            Event::Invoke(id) => fired = Some(id),
            Event::Focus(id) => {
                self.focus = Some(id);
                self.focus_visible = true;
            }
            _ => {}
        }
        match fired {
            Some(id) => Response::Activate(id),
            None if *self != before => Response::Redraw,
            None => Response::Unchanged,
        }
    }

    /// Moves keyboard focus `delta` items through the enabled hits, wrapping (Tab is +1,
    /// Shift+Tab -1; the caller knows the modifier).
    pub fn step(&mut self, list: &DisplayList, delta: i32) {
        self.focus = focus::step(&list.hits, self.focus, delta);
        self.focus_visible = self.focus.is_some();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::Rect;
    use crate::scene::Hit;
    use crate::surface::Pointer;

    fn list() -> DisplayList {
        let hit = |id, x, enabled| Hit {
            id,
            rect: Rect::new(x, 0.0, 10.0, 10.0),
            role: accesskit::Role::Button,
            label: "".into(),
            enabled,
            checked: None,
        };
        DisplayList {
            hits: vec![hit(1, 0.0, true), hit(2, 20.0, true), hit(3, 40.0, false)],
            ..Default::default()
        }
    }

    fn at(x: f32) -> Pointer {
        Pointer {
            at: Point::new(x, 5.0),
            screen: (x as i32, 5),
        }
    }

    #[test]
    fn press_shows_at_once_and_release_over_the_same_item_fires() {
        let (list, mut i) = (list(), Interaction::default());
        assert_eq!(i.handle(&list, &Event::Move(at(5.0))), Response::Redraw);
        assert_eq!(i.state(1, true), State::Hover);
        assert_eq!(i.handle(&list, &Event::Move(at(6.0))), Response::Unchanged);
        assert_eq!(
            i.handle(&list, &Event::Press(at(5.0), Button::Left)),
            Response::Redraw
        );
        assert_eq!(i.state(1, true), State::Pressed);
        assert!(!i.ring(1), "a click never shows the focus ring");
        assert_eq!(
            i.handle(&list, &Event::Release(at(5.0), Button::Left)),
            Response::Activate(1)
        );
        assert_eq!(i.state(1, true), State::Hover);
    }

    #[test]
    fn releasing_elsewhere_cancels_and_disabled_items_never_fire() {
        let (list, mut i) = (list(), Interaction::default());
        i.handle(&list, &Event::Press(at(5.0), Button::Left));
        i.handle(&list, &Event::Move(at(25.0)));
        assert_eq!(
            i.state(1, true),
            State::Rest,
            "off the pressed item: not pressed"
        );
        assert_eq!(
            i.state(2, true),
            State::Rest,
            "nor is the one under the pointer"
        );
        assert_eq!(
            i.handle(&list, &Event::Release(at(25.0), Button::Left)),
            Response::Redraw
        );
        i.handle(&list, &Event::Press(at(45.0), Button::Left));
        assert_eq!(i.pressed, None);
        assert_eq!(
            i.handle(&list, &Event::Release(at(45.0), Button::Left)),
            Response::Unchanged
        );
    }

    #[test]
    fn keys_move_a_visible_focus_and_enter_fires_it() {
        let (list, mut i) = (list(), Interaction::default());
        assert_eq!(i.handle(&list, &Event::Key(0x28)), Response::Redraw);
        assert_eq!(i.focus, Some(1));
        assert!(i.ring(1));
        i.handle(&list, &Event::Key(0x28));
        i.handle(&list, &Event::Key(0x28));
        assert_eq!(i.focus, Some(1), "wraps over the disabled item");
        assert_eq!(i.handle(&list, &Event::Key(0x0D)), Response::Activate(1));
        assert_eq!(i.handle(&list, &Event::Leave), Response::Unchanged);
    }
}
