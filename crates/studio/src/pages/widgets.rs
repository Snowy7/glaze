//! Widgets (stub).
use mypaper_ui::{Point, Rect};
use crate::actions::Action;
use crate::app::{App, Drag};
use crate::ui::B;
pub fn title(_app: &App) -> (String, String) { ("Widgets".into(), String::new()) }
pub fn bar(_app: &App, _b: &mut B, _bar: Rect, right: f32) -> f32 { right }
pub fn view(_app: &App, _b: &mut B, _body: Rect) {}
pub fn run(_app: &mut App, _action: Action) {}
pub fn add_sheet(_app: &App, _b: &mut B) {}
pub fn key(_app: &mut App, _vk: u16, _shift: bool) -> bool { false }
pub fn drag_to(_app: &mut App, _drag: Drag, _p: Point) {}
pub fn drop_drag(_app: &mut App, _drag: Drag) {}
pub fn glyph_of(_id: &str) -> char { crate::theme::glyph::WIDGETS }
pub fn count_here(_app: &App) -> usize { 0 }
