//! One transient, click-through undo chip. Repeated picks repaint the same HWND.
use anyhow::Result;
use mypaper_bars::config::BarsConfig;
use mypaper_desktop::Monitor;
use mypaper_ui::{DisplayList, Kind, Op, Rect, Rgba, Role, Surface, TextStyle, Ui};
use std::time::{Duration, Instant};

const HOLD: Duration = Duration::from_millis(2650);
const FADE: Duration = Duration::from_millis(350);

pub(super) struct Notice {
    pub display: String,
    text: String,
    updated: Instant,
    surface: Option<Surface>,
}

impl Notice {
    pub fn new(display: String, text: String) -> Self {
        Self {
            display,
            text,
            updated: Instant::now(),
            surface: None,
        }
    }

    pub fn update(&mut self, display: String, text: String) {
        self.display = display;
        self.text = text;
        self.updated = Instant::now();
    }

    /// False after the final fade frame; dropping the notice closes only its owned HWND.
    pub fn tick(&mut self, ui: &mut Ui, monitors: &[Monitor], bars: &BarsConfig) -> Result<bool> {
        let alpha = opacity(self.updated.elapsed());
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
                Role::Pane,
            )?;
            surface.set_no_activate(true);
            surface.set_click_through(true);
            self.surface = Some(surface);
        }
        let scale = monitor.dpi as f32 / 96.0;
        let work = super::chrome::work_area(monitor, bars);
        let style = TextStyle::new("Segoe UI", 12.0 * scale);
        let measured = ui.text.measure(&self.text, &style);
        let width = (measured.0 + 28.0 * scale).ceil().min(work.width());
        let height = (32.0 * scale).ceil().min(work.height());
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
                ..Default::default()
            },
        )?;
        surface.show();
        Ok(true)
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
}
