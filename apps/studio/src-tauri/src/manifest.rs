//! Editing an existing `theme.json` without disturbing it. Edits go through
//! `serde_json::Value` (with `preserve_order`), so fields Studio doesn't know
//! about, and the order of the ones it does, survive a round trip. Pure text
//! in, text out; the caller validates and writes.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// The `windows` section as the editor sees it. `None` means "don't change":
/// the key is left out of `theme.json`. Colors are `"#rrggbb"`, plus `"auto"`
/// for accent and border; the theme crate validates every value on load.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WindowsStyle {
    pub accent: Option<String>,
    pub border: Option<String>,
    pub caption: Option<String>,
    pub caption_text: Option<String>,
    pub corners: Option<String>,
    pub backdrop: Option<String>,
    pub dark_mode: Option<bool>,
}

fn parse_object(text: &str) -> Result<Map<String, Value>> {
    match serde_json::from_str(text).context("theme.json is not valid JSON")? {
        Value::Object(map) => Ok(map),
        _ => bail!("theme.json must hold a JSON object"),
    }
}

fn render(map: Map<String, Value>) -> Result<String> {
    Ok(serde_json::to_string_pretty(&Value::Object(map))? + "\n")
}

/// The manifest's `windows` section (all "don't change" if it has none).
pub fn read_style(text: &str) -> Result<WindowsStyle> {
    match parse_object(text)?.remove("windows") {
        Some(section) => serde_json::from_value(section).context("reading the windows section"),
        None => Ok(WindowsStyle::default()),
    }
}

/// The manifest with its `windows` section replaced by `style`. An all-unset
/// style removes the section. Every other field is untouched.
pub fn with_style(text: &str, style: &WindowsStyle) -> Result<String> {
    let mut manifest = parse_object(text)?;
    let Value::Object(section) = serde_json::to_value(style)? else {
        unreachable!("a struct serializes to an object")
    };
    // Unset fields serialize as null; the theme format wants them absent.
    let section: Map<String, Value> = section.into_iter().filter(|(_, v)| !v.is_null()).collect();
    if section.is_empty() {
        manifest.shift_remove("windows");
    } else {
        manifest.insert("windows".into(), Value::Object(section));
    }
    render(manifest)
}

/// The manifest with a new display name.
pub fn with_name(text: &str, name: &str) -> Result<String> {
    let mut manifest = parse_object(text)?;
    manifest.insert("name".into(), Value::String(name.into()));
    render(manifest)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use mypaper_theme::{Backdrop, Color, Corners, LoadedTheme, Paint};

    use super::*;

    const MANIFEST: &str = r#"{
  "name": "Aurora",
  "wallpaper": { "source": "aurora.wgsl", "kind": "shader", "fps": 30 },
  "widgets": { "css": "extra.css" }
}"#;

    fn field(text: &str, key: &str) -> Value {
        serde_json::from_str::<Value>(text).unwrap()[key].clone()
    }

    #[test]
    fn styling_keeps_every_other_field_and_their_order() {
        let style = WindowsStyle {
            accent: Some("auto".into()),
            corners: Some("round".into()),
            dark_mode: Some(true),
            ..Default::default()
        };
        let edited = with_style(MANIFEST, &style).unwrap();

        // Untouched fields are identical, including the inner order of `wallpaper`.
        assert_eq!(field(&edited, "wallpaper"), field(MANIFEST, "wallpaper"));
        assert_eq!(field(&edited, "widgets"), field(MANIFEST, "widgets"));
        let wallpaper_at = edited.find("\"source\"").unwrap();
        assert!(wallpaper_at < edited.find("\"kind\"").unwrap(), "key order changed:\n{edited}");
        assert!(edited.find("\"widgets\"").unwrap() < edited.find("\"windows\"").unwrap());

        // Only the set fields are written, and the theme crate accepts them.
        let windows = field(&edited, "windows");
        assert_eq!(windows.as_object().unwrap().len(), 3);
        let loaded = LoadedTheme::parse(PathBuf::from("themes/a"), &edited).unwrap();
        let windows = loaded.theme.windows.unwrap();
        assert_eq!(windows.accent, Some(Paint::Auto));
        assert_eq!(windows.corners, Some(Corners::Round));
        assert_eq!(windows.dark_mode, Some(true));
        assert_eq!(windows.border, None);
        assert_eq!(read_style(&edited).unwrap(), style);
    }

    #[test]
    fn editing_again_replaces_the_section_in_place_and_unset_removes_it() {
        let first = with_style(MANIFEST, &WindowsStyle { caption: Some("#112233".into()), ..Default::default() }).unwrap();
        let second = with_style(
            &first,
            &WindowsStyle { backdrop: Some("mica".into()), border: Some("#ffffff".into()), ..Default::default() },
        )
        .unwrap();
        let windows = field(&second, "windows");
        assert!(windows.get("caption").is_none(), "old fields must not linger");
        let loaded = LoadedTheme::parse(PathBuf::from("themes/a"), &second).unwrap();
        let windows = loaded.theme.windows.unwrap();
        assert_eq!(windows.backdrop, Some(Backdrop::Mica));
        assert_eq!(windows.border, Some(Paint::Color(Color([255, 255, 255]))));

        let cleared = with_style(&second, &WindowsStyle::default()).unwrap();
        assert!(field(&cleared, "windows").is_null());
        assert_eq!(field(&cleared, "wallpaper"), field(MANIFEST, "wallpaper"));
        assert!(read_style(&cleared).unwrap() == WindowsStyle::default());
    }

    #[test]
    fn bad_values_are_left_for_the_theme_crate_to_reject() {
        let text = with_style(MANIFEST, &WindowsStyle { accent: Some("teal".into()), ..Default::default() }).unwrap();
        assert!(LoadedTheme::parse(PathBuf::from("themes/a"), &text).is_err());
    }

    #[test]
    fn renaming_changes_only_the_name() {
        let renamed = with_name(MANIFEST, "Aurora (copy)").unwrap();
        assert_eq!(field(&renamed, "name"), "Aurora (copy)");
        assert_eq!(field(&renamed, "wallpaper"), field(MANIFEST, "wallpaper"));
        assert!(renamed.find("\"name\"").unwrap() < renamed.find("\"wallpaper\"").unwrap());
    }

    #[test]
    fn non_objects_are_refused() {
        assert!(with_style("[]", &WindowsStyle::default()).is_err());
        assert!(with_name("not json", "x").is_err());
    }
}
