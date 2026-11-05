//! Bars (stub).
use mypaper_ui::{Point, Rect};
use crate::actions::{Action, Bar};
use crate::app::{App, Drag};
use crate::ui::B;
pub fn title(_app: &App) -> (String, String) { ("Bars".into(), String::new()) }
pub fn view(_app: &App, _b: &mut B, _body: Rect) {}
pub fn run(_app: &mut App, _action: Action) {}
pub fn add_sheet(_app: &App, _b: &mut B, _bar: Bar) {}
pub fn key(_app: &mut App, _vk: u16) -> bool { false }
pub fn drag_to(_app: &mut App, _drag: Drag, _p: Point) {}
pub fn drop_drag(_app: &mut App, _drag: Drag) {}
pub fn module_name(module: &mypaper_bars::ModuleRef) -> String { format!("{module:?}") }
pub fn design_name(_app: &App, _bar: Bar) -> String { String::new() }
