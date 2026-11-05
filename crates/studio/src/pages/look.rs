//! Look (stub).
use mypaper_ui::Rect;
use crate::actions::Action;
use crate::app::App;
use crate::ui::B;
pub fn scope(_app: &App, _b: &mut B, rect: Rect) -> f32 { rect.left }
pub fn bar(_app: &App, _b: &mut B, _bar: Rect, right: f32) -> f32 { right }
pub fn view(_app: &App, _b: &mut B, _body: Rect) {}
pub fn run(_app: &mut App, _action: Action) {}
pub fn summary(_app: &App) -> String { "Liquid · Dark".into() }
