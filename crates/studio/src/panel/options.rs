//! Panel option metadata stays beside its editor; runtime widget manifests are owned by
//! another crate. `when` declares the faces/settings to which an option applies.
use mypaper_widgets::manifest::Setting;
use serde::Deserialize;
use serde_json::{Map, Value};
use std::{collections::BTreeMap, sync::OnceLock};

#[derive(Deserialize)]
struct Rule {
    widget: String,
    key: String,
    when: BTreeMap<String, Vec<Value>>,
}

pub fn visible(widget: &str, setting: &Setting, values: &Map<String, Value>) -> bool {
    static RULES: OnceLock<Vec<Rule>> = OnceLock::new();
    RULES
        .get_or_init(|| {
            serde_json::from_str(include_str!("option-faces.json"))
                .expect("panel option declarations")
        })
        .iter()
        .filter(|rule| rule.widget == widget && rule.key == setting.key())
        .all(|rule| {
            rule.when
                .iter()
                .all(|(key, allowed)| values.get(key).is_some_and(|value| allowed.contains(value)))
        })
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
}
