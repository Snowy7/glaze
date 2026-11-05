//! Hook-up for SDK plugin faces (docs/plugins.md): placing a `Widget::Plugin` instance opens its
//! face in the plugin host, the host's cached view lands in the model's `data` for the pure view
//! (`views::plugin`), and clicks go back to the plugin as actions.
use std::time::Instant;

use mypaper_widgets::plugins::{FaceStatus, Plugins};
use serde_json::json;

use super::Widgets;
use super::model::{Model, Widget, plugin_face};
use super::views::plugin::{Hit, action_at};

/// Use the same design-pixel thresholds as the built-in views, independent of grid cells.
fn size_class(width: f32, height: f32, scale: f32) -> mypaper_plugin_sdk::SizeClass {
    use mypaper_plugin_sdk::SizeClass;
    match mypaper_ui::layout::SizeClass::of(width / scale, height / scale) {
        mypaper_ui::layout::SizeClass::Compact => SizeClass::Small,
        mypaper_ui::layout::SizeClass::Regular => SizeClass::Medium,
        mypaper_ui::layout::SizeClass::Large => SizeClass::Large,
    }
}

/// Seeds a new face; synchronization opens it once its rendered dimensions are available.
pub(super) fn open_plugin_face(_plugins: &mut Plugins, m: &mut Model) {
    if m.kind != Widget::Plugin {
        return;
    }
    let Some(face) = plugin_face(&m.placed) else { return };
    m.data = json!({ "title": face.title, "icon": face.icon, "status": "starting" });
    // The entry's monitor, pixel rectangle and resolved scale exist after placement. Open it
    // in sync_plugin_faces, which also updates the variant after a resize or scale change.
}

impl Widgets {

    /// Copies changed views and statuses from the plugin host into their models.
    pub(super) fn sync_plugin_faces(&mut self) {
        for entry in self.entries.values_mut() {
            let m = &mut entry.model;
            if m.kind != Widget::Plugin { continue; }
            let Some(face) = plugin_face(&m.placed) else { continue };
            let size = size_class(entry.rect.width(), entry.rect.height(), entry.dpi_scale * entry.tokens.scale);
            let size_key = match size {
                mypaper_plugin_sdk::SizeClass::Small => "small",
                mypaper_plugin_sdk::SizeClass::Medium => "medium",
                mypaper_plugin_sdk::SizeClass::Large => "large",
            };
            if m.data["opened_size"].as_str() == Some(size_key)
                && m.data["opened_settings"].as_object() == Some(&m.placed.settings)
                && m.data["opened_widget"].as_str() == Some(&m.placed.widget)
            { continue; }
            let settings = serde_json::Value::Object(m.placed.settings.clone());
            m.data["opened_size"] = size_key.into();
            m.data["opened_widget"] = m.placed.widget.clone().into();
            m.data["opened_settings"] = settings.clone();
            if let Err(e) = self.plugins.open_face(&m.placed.instance, &face.plugin, &face.face, size, settings, Instant::now()) {
                log::warn!("{}: {}", m.placed.instance, e.message);
                m.data["status"] = "failed".into();
                m.data["why"] = e.message.into();
                m.dirty = true;
            }
        }
        for instance in self.plugins.take_changed() {
            let Some(entry) = self.entries.get_mut(&instance) else { continue };
            let m = &mut entry.model;
            let status = match self.plugins.face_status(&instance) {
                Some(FaceStatus::Live) => "live",
                Some(FaceStatus::Restarting) => "restarting",
                Some(FaceStatus::Failed(why)) => {
                    m.data["why"] = why.into();
                    "failed"
                }
                Some(FaceStatus::Starting) | None => "starting",
            };
            m.data["status"] = status.into();
            if let Some(face) = self.plugins.face_view(&instance) {
                m.data["face"] = serde_json::to_value(face).unwrap_or_default();
            }
            m.dirty = true;
        }
    }

    pub(super) fn plugin_action(&mut self, ui: &mut mypaper_ui::Ui, id: &str, hit: mypaper_ui::HitId) {
        match self.plugins.face_view(id).and_then(|face| action_at(face, hit)) {
            Some(Hit::Action(action)) => self.plugins.face_action(id, &action, None),
            // Text goes through the shell's RichEdit field; `submit_plugin_field` sends it.
            Some(Hit::Field(action)) => {
                if let Some(entry) = self.entries.get_mut(id) {
                    entry.model.data["field_action"] = action.into();
                }
                self.open_field(id, None);
            }
            Some(Hit::Select { action, options, selected }) => {
                use mypaper_ui::menu::{Entry, Menu, screen_at};
                let Some(entry) = self.entries.get(id) else { return };
                let at = (entry.rect.left as i32, entry.rect.bottom as i32);
                let items = options.into_iter().map(|option| Entry::radio(&option.label, (id.to_owned(), action.clone(), option.value.clone()), selected.as_ref() == Some(&option.value))).collect();
                if self.plugin_menu.is_none() {
                    self.plugin_menu = Menu::new("Plugin selection").ok();
                }
                if let Some(menu) = &mut self.plugin_menu {
                    if let Err(error) = menu.open(ui, items, at, screen_at(at.0, at.1), &entry.tokens) {
                        log::warn!("Plugin selection: {error:#}");
                    }
                }
            }
            None => {}
        }
    }

    /// The field opened by a plugin `field` was submitted: its text goes back as the action's value.
    pub(super) fn submit_plugin_field(&mut self, id: &str, text: String) {
        let Some(entry) = self.entries.get_mut(id) else { return };
        let action = entry.model.data["field_action"].as_str().map(str::to_owned);
        if let Some(field) = &entry.model.field {
            let _ = field.set_text("");
        }
        entry.model.dirty = true;
        if let Some(action) = action {
            self.plugins.face_action(id, &action, Some(text));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::size_class;
    use mypaper_plugin_sdk::SizeClass;

    #[test]
    fn face_variants_follow_rendered_size_and_scale() {
        for scale in [1.0, 1.5, 2.0] {
            for (width, height, expected) in [
                (180.0, 132.0, SizeClass::Small),
                (180.0, 180.0, SizeClass::Medium),
                (360.0, 220.0, SizeClass::Medium),
                (540.0, 330.0, SizeClass::Large),
                (540.0, 149.0, SizeClass::Small),
                (150.0, 300.0, SizeClass::Medium),
                (300.0, 300.0, SizeClass::Large),
            ] {
                assert_eq!(size_class(width * scale, height * scale, scale), expected);
            }
        }
    }
}
