//! Integrations (stub).
use mypaper_ui::Rect;
use crate::app::App;
use crate::ui::B;
pub struct Integration { pub name: &'static str, pub adds: &'static str, pub glyph: char }
pub static LIST: &[Integration] = &[];
pub fn title(_app: &App) -> (String, String) { ("Integrations".into(), String::new()) }
pub fn bar(_app: &App, _b: &mut B, _bar: Rect, right: f32) -> f32 { right }
pub fn view(_app: &App, _b: &mut B, _body: Rect) {}
