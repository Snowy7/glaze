//! Editing an existing `theme.json` without disturbing it. Edits go through
//! `serde_json::Value` (with `preserve_order`), so fields Studio doesn't know
//! about, and the order of the ones it does, survive a round trip. Pure text
//! in, text out; the caller validates and writes.

use anyhow::{Context, Result, bail};
use mypaper_theme as theme;
use mypaper_theme::{Backdrop, Color, Corners, Paint};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::look::Look;

/// The `windows` section as the editor sees it. `None` means "don't change": the key is left out
/// of `theme.json`. Colors are `"#rrggbb"`, plus `"auto"` for accent and border; `cursors` is a
/// cursor set name or a folder. Reading goes through the engine's own [`theme::WindowsStyle`] and
/// saving is checked by the theme loader, so this struct only carries the values.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowsStyle {
    pub accent: Option<String>,
    pub border: Option<String>,
    pub caption: Option<String>,
    pub caption_text: Option<String>,
    pub corners: Option<String>,
    pub backdrop: Option<String>,
    pub dark_mode: Option<bool>,
    pub cursors: Option<String>,
}

impl From<&theme::WindowsStyle> for WindowsStyle {
    fn from(style: &theme::WindowsStyle) -> Self {
        // Every field is named, so a new field in the theme crate stops this from compiling.
        let theme::WindowsStyle { accent, border, caption, caption_text, corners, backdrop, dark_mode, cursors } = style;
        let paint = |paint: &Paint| match paint {
            Paint::Auto => "auto".to_owned(),
            Paint::Color(color) => color.hex(),
        };
        Self {
            accent: accent.as_ref().map(paint),
            border: border.as_ref().map(paint),
            caption: caption.map(Color::hex),
            caption_text: caption_text.map(Color::hex),
            corners: corners.map(|c| {
                match c {
                    Corners::Default => "default",
                    Corners::Round => "round",
                    Corners::Small => "small",
                    Corners::Square => "square",
                }
                .to_owned()
            }),
            backdrop: backdrop.map(|b| {
                match b {
                    Backdrop::Auto => "auto",
                    Backdrop::None => "none",
                    Backdrop::Mica => "mica",
                    Backdrop::Acrylic => "acrylic",
                    Backdrop::Tabbed => "tabbed",
                }
                .to_owned()
            }),
            dark_mode: *dark_mode,
            cursors: cursors.clone(),
        }
    }
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
        Some(section) => {
            let style: theme::WindowsStyle = serde_json::from_value(section).context("reading the windows section")?;
            Ok(WindowsStyle::from(&style))
        }
        None => Ok(WindowsStyle::default()),
    }
}

/// The manifest with its `windows` section set to `style`: each field it sets is written, each it
/// leaves unset is removed, and any other key already in the section stays. An empty section is
/// removed. Every other field of the manifest is untouched.
pub fn with_style(text: &str, style: &WindowsStyle) -> Result<String> {
    let mut manifest = parse_object(text)?;
    let Value::Object(fields) = serde_json::to_value(style)? else {
        unreachable!("a struct serializes to an object")
    };
    let mut section = match manifest.get("windows") {
        Some(Value::Object(section)) => section.clone(),
        Some(_) => bail!("the windows section must be an object"),
        None => Map::new(),
    };
    for (key, value) in fields {
        // Unset fields serialize as null; the theme format wants them absent.
        if value.is_null() {
            section.shift_remove(&key);
        } else {
            section.insert(key, value);
        }
    }
    // Replacing in place keeps the section where it was among the manifest's keys.
    if section.is_empty() {
        manifest.shift_remove("windows");
    } else {
        manifest.insert("windows".into(), Value::Object(section));
    }
    render(manifest)
}

fn takes_interactive(wallpaper: &Value) -> bool {
    matches!(wallpaper.get("kind").and_then(Value::as_str), Some("web" | "scene"))
}

/// Whether the wallpaper forwards the mouse (`wallpaper.interactive`, docs/desktop-extras.md), or
/// `None` for a wallpaper kind that has no such option (only `web` and `scene` do).
pub fn read_interactive(text: &str) -> Result<Option<bool>> {
    let manifest = parse_object(text)?;
    let wallpaper = manifest.get("wallpaper").context("theme.json has no wallpaper")?;
    Ok(takes_interactive(wallpaper).then(|| wallpaper.get("interactive").and_then(Value::as_bool).unwrap_or(false)))
}

/// The manifest with `wallpaper.interactive` on, or (it is the default) left out when off.
pub fn with_interactive(text: &str, on: bool) -> Result<String> {
    let mut manifest = parse_object(text)?;
    let Some(wallpaper) = manifest.get_mut("wallpaper") else { bail!("theme.json has no wallpaper") };
    if !takes_interactive(wallpaper) {
        bail!("only web and 3D scene wallpapers can be interactive");
    }
    let Value::Object(wallpaper) = wallpaper else { bail!("wallpaper must be an object") };
    if on {
        wallpaper.insert("interactive".into(), Value::Bool(true));
    } else {
        wallpaper.shift_remove("interactive");
    }
    render(manifest)
}

/// Which family of mypaper UI a theme skin restyles: the manifest's `widgets` or `bars` section.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SkinKind {
    Widgets,
    Bars,
}

impl SkinKind {
    pub fn key(self) -> &'static str {
        match self {
            Self::Widgets => "widgets",
            Self::Bars => "bars",
        }
    }
}

/// A `widgets` / `bars` section as the editor sees it. Empty means "no skin":
/// the section is left out of `theme.json`.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SkinEdit {
    pub appearance: Look,
    /// A style sheet, relative to the theme folder.
    pub css: Option<String>,
}

impl SkinEdit {
    fn is_empty(&self) -> bool {
        self.appearance.is_empty() && self.css.is_none()
    }
}

/// The manifest's `widgets` or `bars` section (empty if it has none).
pub fn read_skin(text: &str, kind: SkinKind) -> Result<SkinEdit> {
    match parse_object(text)?.remove(kind.key()) {
        Some(section) => serde_json::from_value(section).with_context(|| format!("reading the {} section", kind.key())),
        None => Ok(SkinEdit::default()),
    }
}

/// The manifest with its `widgets` or `bars` section replaced by `skin`. An
/// empty skin removes the section. Every other field is untouched.
pub fn with_skin(text: &str, kind: SkinKind, skin: &SkinEdit) -> Result<String> {
    let mut manifest = parse_object(text)?;
    if skin.is_empty() {
        manifest.shift_remove(kind.key());
    } else {
        let mut section = Map::new();
        if !skin.appearance.is_empty() {
            section.insert("appearance".into(), Value::Object(skin.appearance.to_map()?));
        }
        if let Some(css) = &skin.css {
            section.insert("css".into(), Value::String(css.clone()));
        }
        manifest.insert(kind.key().into(), Value::Object(section));
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

    use mypaper_theme::{Backdrop, Color, Corners, LoadedTheme, Paint, Wallpaper};

    use super::*;

    const MANIFEST: &str = r#"{
  "name": "Aurora",
  "wallpaper": { "source": "aurora.wgsl", "kind": "shader", "fps": 30 },
  "widgets": { "css": "extra.css" }
}"#;

    const WEB: &str = r#"{
  "name": "Pointer",
  "wallpaper": { "kind": "web", "url": "index.html" },
  "windows": { "accent": "auto", "cursors": "mypaper-dark" }
}"#;

    fn field(text: &str, key: &str) -> Value {
        serde_json::from_str::<Value>(text).unwrap()[key].clone()
    }

    fn loaded(text: &str) -> LoadedTheme {
        LoadedTheme::parse(PathBuf::from("themes/a"), text).unwrap()
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
        let windows = loaded(&edited).theme.windows.unwrap();
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
        let windows = loaded(&second).theme.windows.unwrap();
        assert_eq!(windows.backdrop, Some(Backdrop::Mica));
        assert_eq!(windows.border, Some(Paint::Color(Color([255, 255, 255]))));

        let cleared = with_style(&second, &WindowsStyle::default()).unwrap();
        assert!(field(&cleared, "windows").is_null());
        assert_eq!(field(&cleared, "wallpaper"), field(MANIFEST, "wallpaper"));
        assert!(read_style(&cleared).unwrap() == WindowsStyle::default());
    }

    #[test]
    fn cursors_and_interactive_round_trip_without_disturbing_the_theme() {
        // A theme that already has both is read as the engine reads it.
        let interactive = with_interactive(WEB, true).unwrap();
        let style = read_style(&interactive).unwrap();
        assert_eq!(style.cursors.as_deref(), Some("mypaper-dark"));
        assert_eq!(style.accent.as_deref(), Some("auto"));
        assert_eq!(read_interactive(&interactive).unwrap(), Some(true));
        let theme::Wallpaper::Web(web) = loaded(&interactive).theme.wallpaper else { panic!("a web wallpaper") };
        assert!(web.interactive);
        assert_eq!(loaded(&interactive).theme.windows.unwrap().cursors.as_deref(), Some("mypaper-dark"));

        // Writing the same values back changes nothing.
        assert_eq!(with_style(&interactive, &style).unwrap(), interactive);
        assert_eq!(with_interactive(&interactive, true).unwrap(), interactive);

        // Changing one leaves the other, and the other fields, as they were.
        let folder = with_style(&interactive, &WindowsStyle { cursors: Some(r"cursors\mine".into()), ..style.clone() }).unwrap();
        assert_eq!(read_interactive(&folder).unwrap(), Some(true));
        assert_eq!(loaded(&folder).theme.windows.unwrap().cursors.as_deref(), Some(r"cursors\mine"));
        assert_eq!(field(&folder, "wallpaper"), field(&interactive, "wallpaper"));

        // Off is the default, so the key goes away; the folder of cursors goes with "don't change".
        let off = with_interactive(&folder, false).unwrap();
        assert!(field(&off, "wallpaper").get("interactive").is_none());
        assert_eq!(read_interactive(&off).unwrap(), Some(false));
        let unset = with_style(&off, &WindowsStyle { cursors: None, ..style }).unwrap();
        assert_eq!(read_style(&unset).unwrap().cursors, None);
        assert_eq!(read_style(&unset).unwrap().accent.as_deref(), Some("auto"));
        assert_eq!(field(&unset, "wallpaper"), field(WEB, "wallpaper"));

        // Scenes take it too; the other kinds don't, and a cursor field nobody named stays.
        let scene = r#"{ "name": "S", "wallpaper": { "kind": "scene", "source": "a.glb" } }"#;
        assert_eq!(read_interactive(scene).unwrap(), Some(false));
        let on = with_interactive(scene, true).unwrap();
        let Wallpaper::Scene(scene) = loaded(&on).theme.wallpaper else { panic!("a scene") };
        assert!(scene.interactive);
        assert_eq!(read_interactive(MANIFEST).unwrap(), None);
        assert!(with_interactive(MANIFEST, true).is_err());
    }

    #[test]
    fn what_the_engine_would_refuse_is_refused_on_read_and_after_edit() {
        // The theme crate rejects an empty cursor name and unknown keys; Studio reads them as errors.
        assert!(read_style(r#"{ "windows": { "cursors": 3 } }"#).is_err());
        assert!(read_style(r#"{ "windows": { "glow": true } }"#).is_err());
        let empty = with_style(WEB, &WindowsStyle { cursors: Some(String::new()), ..Default::default() }).unwrap();
        assert!(LoadedTheme::parse(PathBuf::from("themes/a"), &empty).is_err());
    }

    #[test]
    fn bad_values_are_left_for_the_theme_crate_to_reject() {
        let text = with_style(MANIFEST, &WindowsStyle { accent: Some("teal".into()), ..Default::default() }).unwrap();
        assert!(LoadedTheme::parse(PathBuf::from("themes/a"), &text).is_err());
    }

    #[test]
    fn skins_replace_their_own_section_and_keep_everything_else() {
        let skin = SkinEdit {
            appearance: serde_json::from_str(r##"{ "surface": "glass", "tint": "#101820", "accent": "auto", "opacity": 0.4 }"##).unwrap(),
            css: Some("bars.css".into()),
        };
        let edited = with_skin(MANIFEST, SkinKind::Bars, &skin).unwrap();
        for key in ["wallpaper", "widgets"] {
            assert_eq!(field(&edited, key), field(MANIFEST, key));
        }
        assert!(edited.find("\"widgets\"").unwrap() < edited.find("\"bars\"").unwrap());
        assert_eq!(read_skin(&edited, SkinKind::Bars).unwrap(), skin);
        assert!(field(&edited, "bars")["appearance"].get("blur").is_none(), "unset fields stay out");

        // The css-only `widgets` section in MANIFEST reads back as a skin with no appearance.
        let widgets = read_skin(MANIFEST, SkinKind::Widgets).unwrap();
        assert!(widgets.appearance.is_empty() && widgets.css.as_deref() == Some("extra.css"));

        let theme = loaded(&edited).theme;
        let bars = theme.bars.unwrap();
        assert_eq!(bars.appearance.unwrap().accent, Some(Paint::Auto));
        assert_eq!(bars.css, Some(PathBuf::from("bars.css")));

        let cleared = with_skin(&edited, SkinKind::Bars, &SkinEdit::default()).unwrap();
        assert!(field(&cleared, "bars").is_null());
        assert_eq!(field(&cleared, "widgets"), field(MANIFEST, "widgets"));
    }

    #[test]
    fn a_widget_style_in_a_skin_is_written_and_read_back() {
        let skin = SkinEdit { appearance: serde_json::from_str(r#"{ "style": "neon", "surface": "solid" }"#).unwrap(), css: None };
        let edited = with_skin(MANIFEST, SkinKind::Widgets, &skin).unwrap();
        assert_eq!(field(&edited, "widgets")["appearance"]["style"], "neon");
        assert_eq!(read_skin(&edited, SkinKind::Widgets).unwrap(), skin);
        assert_eq!(loaded(&edited).theme.widgets.unwrap().appearance.unwrap().style, Some(theme::WidgetStyle::Neon));
    }

    #[test]
    fn skin_values_are_left_for_the_theme_crate_to_refuse() {
        let loud = SkinEdit { appearance: serde_json::from_str(r#"{ "opacity": 4 }"#).unwrap(), css: None };
        let text = with_skin(MANIFEST, SkinKind::Widgets, &loud).unwrap();
        assert!(LoadedTheme::parse(PathBuf::from("themes/a"), &text).is_err());
        let escaping = SkinEdit { css: Some(r"..\evil.css".into()), ..Default::default() };
        let text = with_skin(MANIFEST, SkinKind::Widgets, &escaping).unwrap();
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
