//! One no-activate Undo button. Repeated picks repaint the same HWND; hover pauses its fade.
use anyhow::Result;
use mypaper_bars::config::BarsConfig;
use mypaper_desktop::Monitor;
use mypaper_ui::{
    Button, DisplayList, Event, Hit, Kind, Op, Rect, Rgba, Role, Surface, SurfaceId, TextStyle, Ui,
};
use std::time::{Duration, Instant};

const HOLD: Duration = Duration::from_millis(2650);
const FADE: Duration = Duration::from_millis(350);

pub(super) struct Notice {
    pub display: String,
    text: String,
    pub step: u64,
    lifetime: Lifetime,
    pressed: bool,
    bounds: Rect,
    surface: Option<Surface>,
}

impl Notice {
    pub fn new(display: String, text: String, step: u64) -> Self {
        Self {
            display,
            text,
            step,
            lifetime: Lifetime::new(Instant::now()),
            pressed: false,
            bounds: Rect::default(),
            surface: None,
        }
    }

    pub fn update(&mut self, display: String, text: String, step: u64) {
        self.display = display;
        self.text = text;
        self.step = step;
        self.lifetime.elapsed = Duration::ZERO;
        self.lifetime.last = Instant::now();
        self.pressed = false;
    }

    /// `Some` consumes only this button's input; true requests a semantic Studio undo.
    pub fn event(&mut self, surface: SurfaceId, event: &Event) -> Option<bool> {
        if self.surface.as_ref()?.id() != surface {
            return None;
        }
        self.lifetime.advance(Instant::now());
        let mut undo = false;
        match event {
            Event::Move(_) => self.lifetime.hovered = true,
            Event::Leave => {
                self.lifetime.hovered = false;
                self.pressed = false;
            }
            Event::Press(p, Button::Left) => self.pressed = self.bounds.contains(p.at),
            Event::Release(p, Button::Left) => {
                undo = self.pressed && self.bounds.contains(p.at);
                self.pressed = false;
            }
            Event::Invoke(1) => undo = true,
            Event::Cancelled | Event::CaptureLost => self.pressed = false,
            _ => {}
        }
        Some(undo && self.step != 0)
    }

    /// False after the final fade frame; dropping the notice closes only its owned HWND.
    pub fn tick(&mut self, ui: &mut Ui, monitors: &[Monitor], bars: &BarsConfig) -> Result<bool> {
        self.lifetime.advance(Instant::now());
        let alpha = opacity(self.lifetime.elapsed);
        if alpha == 0.0 {
            return Ok(false);
        }
        let Some(monitor) = monitors.iter().find(|m| m.device == self.display) else {
            return Ok(false);
        };
        if self.surface.is_none() {
            let surface = Surface::new(
                Kind::Popup { activate: false },
                "Studio undo step",
                Role::Button,
            )?;
            surface.set_no_activate(true);
            self.surface = Some(surface);
        }
        let scale = monitor.dpi as f32 / 96.0;
        let work = super::chrome::work_area(monitor, bars);
        let style = TextStyle::new("Segoe UI", 12.0 * scale);
        let measured = ui.text.measure(&self.text, &style);
        let width = (measured.0 + 28.0 * scale).ceil().min(work.width());
        let height = (32.0 * scale).ceil().min(work.height());
        self.bounds = Rect::new(0.0, 0.0, width, height);
        let origin = (
            (work.left + (work.width() - width) / 2.0).round() as i32,
            (work.bottom - height - 12.0 * scale).max(work.top).round() as i32,
        );
        let surface = self.surface.as_mut().unwrap();
        surface.present(
            ui,
            origin,
            (width as u32, height as u32),
            DisplayList {
                ops: vec![
                    Op::Fill {
                        rect: Rect::new(0.0, 0.0, width, height),
                        radius: 8.0 * scale,
                        color: Rgba::rgb8([25, 25, 25], 0.96 * alpha),
                    },
                    Op::Stroke {
                        rect: Rect::new(scale / 2.0, scale / 2.0, width - scale, height - scale),
                        radius: 8.0 * scale,
                        width: scale,
                        color: Rgba::rgb8([230, 230, 230], 0.4 * alpha),
                    },
                    Op::Text {
                        rect: Rect::new(
                            14.0 * scale,
                            (height - measured.1) / 2.0,
                            (width - 28.0 * scale).max(0.0),
                            measured.1,
                        ),
                        text: self.text.clone().into(),
                        style,
                        color: Rgba::rgb8([245, 245, 245], alpha),
                    },
                ],
                hits: vec![Hit {
                    id: 1,
                    rect: self.bounds,
                    role: Role::Button,
                    label: self.text.clone().into(),
                    enabled: self.step != 0,
                    checked: None,
                }],
                ..Default::default()
            },
        )?;
        surface.show();
        Ok(true)
    }
}

struct Lifetime {
    elapsed: Duration,
    last: Instant,
    hovered: bool,
}

impl Lifetime {
    fn new(now: Instant) -> Self {
        Self {
            elapsed: Duration::ZERO,
            last: now,
            hovered: false,
        }
    }
    fn advance(&mut self, now: Instant) {
        if !self.hovered {
            self.elapsed += now.saturating_duration_since(self.last);
        }
        self.last = now;
    }
}

fn opacity(elapsed: Duration) -> f32 {
    1.0 - (elapsed.saturating_sub(HOLD).as_secs_f32() / FADE.as_secs_f32()).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stays_calm_then_fades_out_at_three_seconds() {
        assert_eq!(opacity(Duration::from_secs(2)), 1.0);
        assert!((opacity(HOLD + FADE / 2) - 0.5).abs() < 0.001);
        assert_eq!(opacity(Duration::from_secs(3)), 0.0);
    }
    #[test]
    fn hover_pauses_even_a_partially_faded_chip_then_resumes() {
        let start = Instant::now();
        let mut life = Lifetime::new(start);
        life.advance(start + HOLD + FADE / 2);
        life.hovered = true;
        life.advance(start + Duration::from_secs(30));
        assert!((opacity(life.elapsed) - 0.5).abs() < 0.001);
        life.hovered = false;
        life.advance(start + Duration::from_secs(30) + FADE / 2);
        assert_eq!(opacity(life.elapsed), 0.0);
    }
}
