//! Wallpapers (stub).
use mypaper_ui::Rect;
use crate::actions::Action;
use crate::app::App;
use crate::ui::B;
pub fn title(_app: &App) -> (String, String) { ("Wallpapers".into(), String::new()) }
pub fn bar(_app: &App, _b: &mut B, _bar: Rect, right: f32) -> f32 { right }
pub fn view(_app: &App, _b: &mut B, _body: Rect) {}
pub fn run(_app: &mut App, _action: Action) {}
pub fn add_sheet(_app: &App, _b: &mut B) {}
