//! mypaper's native UI layer (docs/ui.md): per-pixel-alpha layered windows drawn with software
//! Direct2D and DirectWrite on a WIC bitmap. No hardware GPU device, ever (plan Rule 1).
//!
//! A frame is `view(&model, &tokens) -> DisplayList`; [`Surface::present`] compares it with the
//! list on screen and redraws only the bounds that changed (nothing at all when it is the same).
//! Input arrives as [`Event`]s from [`next_event`], handled by the app's loop after dispatching
//! window messages. [`Menu`] is the first component built on it.

mod a11y;
pub mod calendar;
pub mod focus;
pub mod geom;
#[cfg(any(test, feature = "golden"))]
pub mod golden;
pub mod icons;
pub mod menu;
pub mod modules;
pub mod motion;
mod render;
mod scene;
mod surface;
mod text;
pub mod text_field;
mod tokens;

pub use geom::{PixelRect, Point, Rect, Rgba};
pub use menu::{Entry, Menu, Outcome, Screen, screen_at};
pub use motion::{Spring, Ticker, Tween};
pub use render::{Image, Ui, render};
pub use scene::{DisplayList, Hit, HitId, Op};
pub use surface::{Button, Event, Kind, Pointer, Surface, SurfaceId, next_event};
pub use text::{Text, TextStyle};
pub use text_field::TextField;
pub use tokens::{Colors, Material, MenuMetrics, Tokens};

/// Accessibility roles for [`Hit`] items.
pub use accesskit::Role;
