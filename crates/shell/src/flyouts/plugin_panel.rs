//! A component plugin's panel as a flyout (docs/plugins.md, "Panels"): opened by a click on the
//! plugin's bar module when that module names a `panel`. The view tree is the plugin's, drawn
//! with the same renderer as its desktop faces (`widgets::views::plugin`) inside the flyout
//! chrome, so it gets the flyout's glass, motion, Escape and focus like every built-in panel.
//! While it is on screen the widgets host keeps the instance `panel:<plugin>--<panel>` open
//! (`bars::plugin_modules`), and closing the flyout closes it.
use glaze_plugin_sdk::Face;
use glaze_ui::{HitId, Rect, icons::Icon};

use super::kit::Kit;
use super::{Cx, Effect, Panel};
use crate::bars::plugin_modules;
use crate::widgets::views::plugin::{self as render, Hit, Status};

/// Design px per grid cell of the panel's `size.default`.
const CELL: f32 = 48.0;

pub struct PluginPanel {
    /// `<plugin>--<panel>`.
    id: String,
    title: String,
    icon: Icon,
    /// Design px.
    size: (f32, f32),
    face: Option<Face>,
}

/// The panel's outer size in design px for `cells` (columns, rows), kept to a flyout's range.
fn size(cells: [u8; 2]) -> (f32, f32) {
    ((cells[0] as f32 * CELL).clamp(300.0, 520.0), (cells[1] as f32 * CELL).clamp(160.0, 560.0))
}

impl PluginPanel {
    pub fn new(id: String) -> Self {
        let spec = plugin_modules::parse(&id).and_then(|(plugin, panel)| {
            let (manifest, _) = glaze_widgets::registry::find_plugin(plugin)?;
            manifest.panels.into_iter().find(|p| p.id == panel)
        });
        let (title, icon, cells) = match spec {
            Some(spec) => (spec.name, spec.icon.as_deref().map_or(Icon::Sparkles, render::icon), spec.size.default),
            None => (id.clone(), Icon::Sparkles, [6, 5]),
        };
        plugin_modules::show_panel(&id, true);
        let face = plugin_modules::panel_view(&id);
        Self { id, title, icon, size: size(cells), face }
    }
}

impl Drop for PluginPanel {
    fn drop(&mut self) {
        plugin_modules::show_panel(&self.id, false);
    }
}

impl Panel for PluginPanel {
    fn width(&self) -> f32 {
        self.size.0
    }

    fn view(&self, kit: &mut Kit) {
        let (pad, height) = (kit.pad(), kit.px(self.size.1));
        kit.frame(|f, area| {
            // The face brings its own inset; draw it over the flyout's padding, not inside it.
            let rect = Rect::new(area.left - pad, area.top - pad, area.width() + 2.0 * pad, height);
            let status = if self.face.is_some() { Status::Live } else { Status::Starting };
            render::draw(f, rect, &self.title, self.icon, self.face.as_ref(), status);
            rect.bottom - pad
        });
    }

    fn activate(&mut self, id: HitId, _cx: &mut Cx) -> Effect {
        match self.face.as_ref().and_then(|face| render::action_at(face, id)) {
            Some(Hit::Action(action) | Hit::Select { action, .. }) => {
                plugin_modules::panel_click(&self.id, action);
                Effect::Redraw
            }
            // Text entry isn't hosted in flyouts yet; the field's action fires without a value.
            Some(Hit::Field(action)) => {
                plugin_modules::panel_click(&self.id, action);
                Effect::Redraw
            }
            None => Effect::None,
        }
    }

    fn refresh(&mut self, _cx: &mut Cx) -> bool {
        let latest = plugin_modules::panel_view(&self.id);
        let changed = latest != self.face;
        self.face = latest;
        changed
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn panel_sizes_follow_their_cells_within_flyout_bounds() {
        assert_eq!(super::size([8, 5]), (384.0, 240.0));
        assert_eq!(super::size([1, 1]), (300.0, 160.0));
        assert_eq!(super::size([20, 20]), (520.0, 560.0));
    }
}
