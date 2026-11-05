//! mypaper's native UI layer (docs/ui.md): per-pixel-alpha layered windows drawn with software
//! Direct2D and DirectWrite on a WIC bitmap. No hardware GPU device, ever (plan Rule 1).
//!
//! A frame is `view(&model, &tokens) -> DisplayList`; [`Surface::present`] compares it with the
//! list on screen and redraws only the bounds that changed (nothing at all when it is the same).
//! Input arrives as [`Event`]s from [`next_event`], handled by the app's loop after dispatching
//! window messages. [`Menu`] is the first component built on it.

mod a11y;
pub mod app_window;
pub mod calendar;
pub mod chart;
pub mod compose;
pub mod controls;
pub mod focus;
pub mod geom;
pub mod glass;
#[cfg(any(test, feature = "golden"))]
pub mod golden;
mod icon_paths;
pub mod icons;
pub mod interact;
pub mod kit;
pub mod layout;
pub mod menu;
pub mod modules;
pub mod motion;
mod render;
mod scene;
pub mod scroll;
mod surface;
mod text;
pub mod theme;
pub mod text_field;
mod tokens;
mod watch;

pub use app_window::{Chrome, Cursor, set_modal_hook};
pub use geom::{PixelRect, Point, Rect, Rgba};
pub use glass::{Glass, GlassMaterial};
pub use interact::Interaction;
pub use kit::{Accessory, ButtonKind, Frame, Lead, Row as ListRow, Tile};
pub use layout::{Align, Insets, SizeClass, Stack};
pub use scroll::Scroll;
pub use menu::{Entry, Menu, MenuIcon, Outcome, Screen, screen_at};
pub use motion::{Spring, Ticker, Tween, display_frame};
pub use render::{Image, TextMode, TextQuality, Ui, render};
pub use scene::{DisplayList, Hit, HitId, Op};
pub use surface::{Button, Event, Kind, Pointer, Surface, SurfaceId, next_event};
pub use text::{Text, TextStyle, VARIABLE};
pub use theme::{Palette, Scheme, Style, TextRole, Theme};
pub use text_field::TextField;
pub use tokens::{Colors, Ink, Material, MenuMetrics, Step, Tokens, Typeface};
pub use watch::FileWatch;

/// Accessibility roles for [`Hit`] items.
pub use accesskit::Role;
