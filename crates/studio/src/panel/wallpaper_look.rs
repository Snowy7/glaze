//! The Look editor's wallpaper target, projected into today's theme appearance/glass fields.
use mypaper_config::look::Style;
use serde_json::{Value, json};

use super::specimens::MaterialSample;
use crate::{
    actions::{Action, Edit},
    app::App,
    docs::DocId,
    library::Wallpaper,
};

pub fn target(app: &App) -> Option<&Wallpaper> {
    let index = if app.nav.look_theme {
        app.nav.wallpaper.or_else(|| app.library.active())
    } else {
        app.library.active()
    }?;
    app.library.items.get(index)
}

pub fn edit(wallpaper: &Wallpaper, path: &str, label: &str) -> Edit {
    Edit::new(
        DocId::Studio,
        format!(
            "/theme:{}{path}",
            crate::docs::token(&wallpaper.dir.display().to_string())
        ),
        label,
    )
}

pub fn selected(theme: &Value) -> Option<Style> {
    let style = |family| {
        let appearance: mypaper_theme::Appearance =
            serde_json::from_value(theme.pointer(&format!("/{family}/appearance"))?.clone())
                .ok()?;
        super::widget_look::selected(&appearance)
    };
    let widgets = style("widgets")?;
    (style("bars") == Some(widgets)).then_some(widgets)
}

pub fn sample(app: &App, theme: &Value) -> MaterialSample {
    let appearance: mypaper_theme::Appearance = theme
        .pointer("/widgets/appearance")
        .and_then(|value| serde_json::from_value(value.clone()).ok())
        .unwrap_or_default();
    let style = super::widget_look::selected(&appearance).unwrap_or(
        if appearance.style == Some(mypaper_theme::WidgetStyle::Liquid) {
            Style::Liquid
        } else {
            Style::Mono
        },
    );
    let mut sample = MaterialSample::defaults(app, style);
    let stored_glass: mypaper_theme::Liquid = theme
        .pointer("/widgets/glass")
        .and_then(|value| serde_json::from_value(value.clone()).ok())
        .unwrap_or_default();
    let effective = appearance.over(&mypaper_widgets::look::defaults());
    let glass = stored_glass.over(&mypaper_widgets::look::liquid_defaults());
    if style == Style::Liquid {
        sample.clarity = 1.0 - glass.tint.unwrap();
        sample.blur = glass.blur.unwrap();
        sample.refraction = glass.refraction.unwrap();
        sample.highlight = glass.highlight.unwrap();
    } else {
        sample.clarity = 1.0 - effective.opacity.unwrap();
        sample.blur = effective.blur.unwrap();
        sample.refraction = 0.0;
        sample.highlight = 0.0;
    }
    sample.tint = effective
        .tint
        .map_or(sample.tint, |color| mypaper_ui::Rgba::rgb8(color.0, 1.0));
    sample.radius = effective.radius.unwrap();
    sample.edge = effective.border.unwrap();
    if appearance.surface == Some(mypaper_theme::Surface::Solid) {
        sample.clarity = 0.0;
    }
    if let Some((edit, value)) = &app.pending_value
        && let Some(wallpaper) = target(app)
    {
        for id in ["clarity", "blur", "refraction", "highlight"] {
            if edit.path == material_edit(wallpaper, id, "").path
                && let Some(value) = value.as_f64()
            {
                match id {
                    "clarity" => sample.clarity = value as f32,
                    "blur" => sample.blur = value as f32,
                    "refraction" => sample.refraction = value as f32,
                    "highlight" => sample.highlight = value as f32,
                    _ => unreachable!(),
                }
            }
        }
        if edit.path == self::edit(wallpaper, "/@both/appearance/tint", "").path
            && let Some(color) = value.as_str().and_then(mypaper_config::Color::parse)
        {
            sample.tint = mypaper_ui::Rgba::rgb8(color.0, 1.0);
        }
        if edit.path == self::edit(wallpaper, "/@both/appearance/radius", "").path
            && let Some(radius) = value.as_f64()
        {
            sample.radius = radius as f32;
        }
    }
    sample
}

/// Preserve CSS and unrelated per-family choices while changing the shared design defaults.
pub fn with_style(app: &App, theme: &Value, style: Style) -> Value {
    let sample = MaterialSample::defaults(app, style);
    let mut next = theme.clone();
    for family in ["widgets", "bars"] {
        for (field, value) in [
            (
                "style",
                json!(if style == Style::Liquid {
                    "liquid"
                } else {
                    "minimal"
                }),
            ),
            (
                "surface",
                json!(if style == Style::Solid {
                    "solid"
                } else {
                    "glass"
                }),
            ),
            (
                "opacity",
                json!(if style == Style::Solid {
                    1.0
                } else {
                    1.0 - sample.clarity
                }),
            ),
            ("blur", json!(sample.blur)),
            ("radius", json!(sample.radius)),
            ("border", json!(sample.edge)),
        ] {
            next[family]["appearance"][field] = value;
        }
        for (field, value) in [
            ("tint", 1.0 - sample.clarity),
            ("blur", sample.blur),
            ("refraction", sample.refraction),
            ("highlight", sample.highlight),
        ] {
            next[family]["glass"][field] = json!(value);
        }
    }
    next
}

pub fn style_action(wallpaper: &Wallpaper, style: Style) -> Action {
    Action::Set(
        Edit::new(
            DocId::Studio,
            format!(
                "/panel/wallpaper-style/{}",
                crate::docs::token(&wallpaper.dir.display().to_string())
            ),
            "Wallpaper style",
        ),
        json!(style),
    )
}

pub fn material_edit(wallpaper: &Wallpaper, id: &str, label: &str) -> Edit {
    Edit::new(
        DocId::Studio,
        format!(
            "/panel/wallpaper-material/{}/{id}",
            crate::docs::token(&wallpaper.dir.display().to_string())
        ),
        label,
    )
}

fn with_material(theme: &Value, id: &str, value: f64) -> Option<Value> {
    let mut next = theme.clone();
    let (field, value) = match id {
        "clarity" => ("tint", 1.0 - value.clamp(0.0, 1.0)),
        "blur" => ("blur", value.clamp(0.0, 32.0)),
        "refraction" => ("refraction", value.clamp(0.0, 1.0)),
        "highlight" => ("highlight", value.clamp(0.0, 1.0)),
        _ => return None,
    };
    for family in ["widgets", "bars"] {
        next[family]["glass"][field] = json!(value);
        if matches!(id, "clarity" | "blur") {
            next[family]["appearance"][if id == "clarity" { "opacity" } else { "blur" }] =
                json!(value);
        }
    }
    Some(next)
}

pub fn intercept_set(app: &mut App, edit: &Edit, value: &Value) -> bool {
    if edit.doc != DocId::Studio {
        return false;
    }
    let (folder, knob) = if let Some(folder) = edit.path.strip_prefix("/panel/wallpaper-style/") {
        (folder, None)
    } else if let Some(rest) = edit.path.strip_prefix("/panel/wallpaper-material/") {
        let Some((folder, knob)) = rest.rsplit_once('/') else {
            return true;
        };
        (folder, Some(knob))
    } else {
        return false;
    };
    let folder = std::path::PathBuf::from(folder.replace("~1", "/").replace("~0", "~"));
    let Some(wallpaper) = app
        .library
        .items
        .iter()
        .find(|wallpaper| wallpaper.dir == folder)
    else {
        return true;
    };
    let theme = super::saving::theme_value(app, wallpaper);
    let next = if let Some(knob) = knob {
        let Some(next) = value
            .as_f64()
            .and_then(|value| with_material(&theme, knob, value))
        else {
            return true;
        };
        next
    } else {
        let Ok(style) = serde_json::from_value(value.clone()) else {
            return true;
        };
        with_style(app, &theme, style)
    };
    if app.fixture {
        return true;
    }
    app.panel.as_mut().unwrap().theme_retry = Some(Action::Set(edit.clone(), value.clone()));
    // The existing writer checks ownership and validates the complete theme before one write.
    match crate::library::edit_theme(&folder, "", Some(next)) {
        Ok(()) => {
            super::saving::theme_saved(app, &folder);
            app.library =
                crate::library::Library::scan(crate::app::theme_name(&app.docs).as_deref());
            app.docs.saved = crate::docs::Saved::Done(std::time::Instant::now());
            super::history::record_drag(app, &edit.label);
        }
        Err(error) => super::saving::theme_failed(app, &folder, &format!("{error:#}")),
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_an_inactive_wallpapers_look_keeps_it_as_the_edit_target() {
        let mut ui = mypaper_ui::Ui::new().unwrap();
        let mut app = crate::goldens::app(true, &ui);
        app.panel = Some(super::super::State::default());
        app.size = (480, 1600);
        let active = app.library.active();
        let index = app
            .library
            .items
            .iter()
            .position(|wallpaper| !wallpaper.active)
            .unwrap();
        app.library.items[index].origin = crate::library::Origin::User;
        let folder = app.library.items[index].dir.clone();
        let before = app.docs.doc(DocId::Config).raw.clone();
        app.run(Action::Panel(super::super::Command::WallpaperLook(index)));
        assert_eq!(target(&app).unwrap().dir, folder);
        assert_eq!(app.library.active(), active);
        assert_eq!(app.docs.doc(DocId::Config).raw, before);
        let frame = super::super::build(&app, &mut ui.text);
        assert_eq!(
            frame
                .targets
                .iter()
                .filter(|target| target.group.is_some()
                    && matches!(&target.action,
            Action::Set(edit, _) if edit.path.starts_with("/panel/wallpaper-style/")))
                .count(),
            5
        );
        assert!(frame.targets.iter().any(|target| matches!(&target.action,
            Action::Slider(edit) if edit.path.starts_with("/panel/wallpaper-material/"))));
        assert!(frame.targets.iter().any(|target| matches!(&target.action,
            Action::Choose(choices) if choices.edit.path.contains("/@both/appearance/text"))));
        app.run(
            frame
                .targets
                .iter()
                .find(|target| matches!(target.action, Action::More(_)))
                .unwrap()
                .action
                .clone(),
        );
        let frame = super::super::build(&app, &mut ui.text);
        assert!(frame.targets.iter().any(|target| matches!(&target.action,
            Action::Slider(edit) if edit.path.contains("/@both/appearance/radius"))));
        assert_eq!(
            app.docs.undo_count(),
            0,
            "opening or expanding the editor is read-only"
        );
    }

    #[test]
    fn clarity_and_blur_update_the_native_and_liquid_fields_together() {
        let theme = json!({"widgets":{"css":"widgets.css"},"bars":{"css":"bars.css"}});
        let clarity = with_material(&theme, "clarity", 0.75).unwrap();
        let blur = with_material(&clarity, "blur", 18.0).unwrap();
        for family in ["widgets", "bars"] {
            assert_eq!(blur[family]["glass"]["tint"], json!(0.25));
            assert_eq!(blur[family]["appearance"]["opacity"], json!(0.25));
            assert_eq!(blur[family]["glass"]["blur"], json!(18.0));
            assert_eq!(blur[family]["appearance"]["blur"], json!(18.0));
            assert_eq!(blur[family]["css"], theme[family]["css"]);
        }
    }

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
