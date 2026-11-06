//! The Look editor's wallpaper target, projected into today's theme appearance/glass fields.
use mypaper_config::look::Style;
use serde_json::{Value, json};

use crate::{actions::{Action, Edit}, app::App, docs::DocId, library::Wallpaper};
use super::specimens::MaterialSample;

pub fn target(app: &App) -> Option<&Wallpaper> {
    let index = if app.nav.look_theme { app.nav.wallpaper.or_else(|| app.library.active()) } else { app.library.active() }?;
    app.library.items.get(index)
}

pub fn edit(wallpaper: &Wallpaper, path: &str, label: &str) -> Edit {
    Edit::new(DocId::Studio, format!("/theme:{}{path}", crate::docs::token(&wallpaper.dir.display().to_string())), label)
}

pub fn selected(theme: &Value) -> Option<Style> {
    let appearance: mypaper_theme::Appearance = serde_json::from_value(theme.pointer("/widgets/appearance")?.clone()).ok()?;
    super::widget_look::selected(&appearance)
}

pub fn sample(app: &App, theme: &Value) -> MaterialSample {
    let style = selected(theme).unwrap_or_default();
    let mut sample = MaterialSample::defaults(app, style);
    let appearance: mypaper_theme::Appearance = theme.pointer("/widgets/appearance")
        .and_then(|value| serde_json::from_value(value.clone()).ok()).unwrap_or_default();
    let glass: mypaper_theme::Liquid = theme.pointer("/widgets/glass")
        .and_then(|value| serde_json::from_value(value.clone()).ok()).unwrap_or_default();
    sample.clarity = glass.tint.map(|value| 1.0 - value).or_else(|| appearance.opacity.map(|value| 1.0 - value)).unwrap_or(sample.clarity);
    sample.blur = glass.blur.or(appearance.blur).unwrap_or(sample.blur);
    sample.refraction = glass.refraction.unwrap_or(sample.refraction);
    sample.highlight = glass.highlight.unwrap_or(sample.highlight);
    sample.tint = appearance.tint.map_or(sample.tint, |color| mypaper_ui::Rgba::rgb8(color.0, 1.0));
    sample.radius = appearance.radius.unwrap_or(sample.radius);
    sample.edge = appearance.border.unwrap_or(sample.edge);
    if appearance.surface == Some(mypaper_theme::Surface::Solid) { sample.clarity = 0.0; }
    sample
}

/// Preserve CSS and unrelated per-family choices while changing the shared design defaults.
pub fn with_style(app: &App, theme: &Value, style: Style) -> Value {
    let sample = MaterialSample::defaults(app, style);
    let mut next = theme.clone();
    for family in ["widgets", "bars"] {
        for (field, value) in [
            ("style", json!(if style == Style::Liquid { "liquid" } else { "minimal" })),
            ("surface", json!(if style == Style::Solid { "solid" } else { "glass" })),
            ("opacity", json!(if style == Style::Solid { 1.0 } else { 1.0 - sample.clarity })),
            ("blur", json!(sample.blur)),
            ("radius", json!(sample.radius)),
            ("border", json!(sample.edge)),
        ] {
            next[family]["appearance"][field] = value;
        }
        for (field, value) in [
            ("tint", 1.0 - sample.clarity), ("blur", sample.blur),
            ("refraction", sample.refraction), ("highlight", sample.highlight),
        ] {
            next[family]["glass"][field] = json!(value);
        }
    }
    next
}

pub fn style_action(wallpaper: &Wallpaper, style: Style) -> Action {
    Action::Set(Edit::new(DocId::Studio,
        format!("/panel/wallpaper-style/{}", crate::docs::token(&wallpaper.dir.display().to_string())),
        "Wallpaper style"), json!(style))
}

pub fn intercept_set(app: &mut App, edit: &Edit, value: &Value) -> bool {
    if edit.doc != DocId::Studio { return false; }
    let Some(folder) = edit.path.strip_prefix("/panel/wallpaper-style/") else { return false };
    let folder = std::path::PathBuf::from(folder.replace("~1", "/").replace("~0", "~"));
    let Some(wallpaper) = app.library.items.iter().find(|wallpaper| wallpaper.dir == folder) else { return true };
    let Ok(style) = serde_json::from_value(value.clone()) else { return true };
    let theme = super::saving::theme_value(app, wallpaper);
    let next = with_style(app, &theme, style);
    if app.fixture { return true; }
    app.panel.as_mut().unwrap().theme_retry = Some(Action::Set(edit.clone(), value.clone()));
    // The existing writer checks ownership and validates the complete theme before one write.
    match crate::library::edit_theme(&folder, "", Some(next)) {
        Ok(()) => {
            super::saving::theme_saved(app, &folder);
            app.library = crate::library::Library::scan(crate::app::theme_name(&app.docs).as_deref());
            app.docs.saved = crate::docs::Saved::Done(std::time::Instant::now());
            super::history::record_drag(app, "Wallpaper style");
        }
        Err(error) => super::saving::theme_failed(app, &folder, &format!("{error:#}")),
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_style_fits_the_theme_format_and_preserves_sources_and_family_css() {
        let ui = mypaper_ui::Ui::new().unwrap();
        let app = crate::goldens::app(true, &ui);
        let original = json!({"name":"Own", "wallpaper":{"kind":"video","source":"clip.mp4","fit":"contain"},
            "widgets":{"css":"widget.css","appearance":{"font":"Segoe UI","tint":"#239569"}},
            "bars":{"css":"bars.css","appearance":{"text":"light","accent":"auto"}}});
        for style in Style::ALL {
            let next = with_style(&app, &original, style);
            let parsed: mypaper_theme::Theme = serde_json::from_value(next.clone()).unwrap();
            parsed.widgets.unwrap().glass.unwrap().validate().unwrap();
            assert_eq!(selected(&next), Some(style));
            assert_eq!(next["wallpaper"], original["wallpaper"]);
            assert_eq!(next["widgets"]["css"], original["widgets"]["css"]);
            assert_eq!(next["bars"]["css"], original["bars"]["css"]);
            assert_eq!(next["widgets"]["appearance"]["font"], json!("Segoe UI"));
            assert_eq!(next["bars"]["appearance"]["accent"], json!("auto"));
        }
    }
}
