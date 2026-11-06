//! Manifest-owned visibility and presentation for widget controls.
use mypaper_widgets::manifest::Setting;
use serde_json::{Map, Value};

pub fn visible(_widget: &str, setting: &Setting, values: &Map<String, Value>) -> bool {
    let face = [
        "variant", "face", "style", "layout", "look", "shape", "display", "frame",
    ]
    .into_iter()
    .find_map(|key| values.get(key).and_then(Value::as_str));
    setting.visible_on(face, values)
}

/// Remove empty/default instructions from labels; string controls show them as placeholders.
pub fn presentation(setting: &Setting) -> Setting {
    let mut value = serde_json::to_value(setting).expect("setting schema");
    if let Some(label) = value["label"]
        .as_str()
        .and_then(|s| s.split_once(" (").map(|(label, _)| label.to_owned()))
    {
        value["label"] = Value::String(label);
    }
    serde_json::from_value(value).expect("same setting schema")
}

pub fn placeholder(widget: &str, setting: &Setting) -> String {
    let default = setting.default_value();
    if let Some(value) = default.as_str().filter(|s| !s.is_empty()) {
        return value.into();
    }
    match (widget, setting.key()) {
        ("clock", "name") => "Your Windows name",
        (_, "font") => "From the widget font",
        ("countdown", "date") => "Next 1 January · YYYY-MM-DD",
        ("countdown", "startDate") => "A year before · YYYY-MM-DD",
        ("system-monitor", "drives") => "All drives",
        ("todo", "title") => "Hide the title",
        _ => "",
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clock_and_photo_options_follow_the_selected_face() {
        let widgets = mypaper_widgets::registry::builtins();
        let clock = &widgets.get("clock").unwrap().manifest;
        let mut values = clock.resolve_settings(&Map::new());
        let zones = clock.setting("zones").unwrap();
        assert!(!visible("clock", zones, &values));
        values.insert("variant".into(), Value::String("world".into()));
        assert!(visible("clock", zones, &values));
        assert!(!visible("clock", clock.setting("name").unwrap(), &values));
        let photo = &widgets.get("photo").unwrap().manifest;
        let mut values = photo.resolve_settings(&Map::new());
        assert!(!visible(
            "photo",
            photo.setting("caption").unwrap(),
            &values
        ));
        values.insert("variant".into(), Value::String("polaroid".into()));
        assert!(visible("photo", photo.setting("caption").unwrap(), &values));
    }

    #[test]
    fn media_and_conditional_options_use_the_manifest_rules() {
        let widgets = mypaper_widgets::registry::builtins();
        let media = &widgets.get("now-playing").unwrap().manifest;
        let mut values = media.resolve_settings(&Map::new());
        assert!(!visible(
            "now-playing",
            media.setting("viz").unwrap(),
            &values
        ));
        values.insert("variant".into(), Value::String("viz".into()));
        assert!(visible(
            "now-playing",
            media.setting("viz").unwrap(),
            &values
        ));
        let launcher = &widgets.get("launcher").unwrap().manifest;
        let mut values = launcher.resolve_settings(&Map::new());
        assert!(!visible(
            "launcher",
            launcher.setting("customUrl").unwrap(),
            &values
        ));
        values.insert("engine".into(), Value::String("custom".into()));
        assert!(visible(
            "launcher",
            launcher.setting("customUrl").unwrap(),
            &values
        ));
        values.insert("search".into(), Value::Bool(false));
        assert!(!visible(
            "launcher",
            launcher.setting("customUrl").unwrap(),
            &values
        ));
    }
}
