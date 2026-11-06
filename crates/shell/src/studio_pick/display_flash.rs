//! A click-through, nonactivating display outline, present only during segment hover.
use anyhow::Result;
use mypaper_desktop::Monitor;
use mypaper_ui::{DisplayList, Kind, Op, Rect, Rgba, Role, Surface, Ui};

#[derive(Default)]
pub(super) struct Flash {
    display: Option<String>,
    surface: Option<Surface>,
}

impl Flash {
    pub fn set(&mut self, display: Option<String>) {
        if self.display != display {
            self.surface = None;
            self.display = display;
        }
    }
    pub fn tick(&mut self, ui: &mut Ui, monitors: &[Monitor]) -> Result<()> {
        let Some(display) = &self.display else {
            return Ok(());
        };
        let Some(monitor) = monitors.iter().find(|m| &m.device == display) else {
            self.surface = None;
            return Ok(());
        };
        if self.surface.is_some() {
            return Ok(());
        }
        let mut surface = Surface::new(
            Kind::Popup { activate: false },
            "Studio display frame",
            Role::Pane,
        )?;
        surface.set_no_activate(true);
        surface.set_click_through(true);
        let r = monitor.rect;
        let s = monitor.dpi as f32 / 96.0;
        surface.present(
            ui,
            (r.x, r.y),
            (r.width as u32, r.height as u32),
            DisplayList {
                ops: outline(r.width as f32, r.height as f32, s),
                ..Default::default()
            },
        )?;
        surface.show();
        self.surface = Some(surface);
        Ok(())
    }
}

fn outline(width: f32, height: f32, scale: f32) -> Vec<Op> {
    let rect = Rect::new(
        2.0 * scale,
        2.0 * scale,
        width - 4.0 * scale,
        height - 4.0 * scale,
    );
    vec![
        Op::Stroke {
            rect,
            radius: 0.0,
            width: 4.0 * scale,
            color: Rgba::rgb8([18; 3], 0.9),
        },
        Op::Stroke {
            rect,
            radius: 0.0,
            width: 1.5 * scale,
            color: Rgba::rgb8([250; 3], 1.0),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frame_is_only_a_thin_outline_inside_the_display_at_both_dpis() {
        for scale in [1.0, 1.5] {
            let ops = outline(1920., 1080., scale);
            assert_eq!(ops.len(), 2);
            assert!(
                ops.iter()
                    .all(|op| matches!(op, Op::Stroke { rect, width, .. }
                if rect.left - width / 2. >= 0. && rect.right + width / 2. <= 1920.
                    && rect.top - width / 2. >= 0. && rect.bottom + width / 2. <= 1080.))
            );
        }
    }
}
