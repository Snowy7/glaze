//! Look's five styles projected into the appearance fields today's widget host reads.
//! No new appearance fields or legacy style names are written into widgets.json.
use mypaper_config::look::Style;
use mypaper_theme::{Appearance, Surface, WidgetStyle};
use mypaper_widgets::layout::Instance;
use serde_json::{Value, json};

use crate::{app::App, docs::DocId};

pub fn inherited_style(app: &App) -> Style {
    app.get(DocId::Config, "/look/widgets/style")
        .or_else(|| app.get(DocId::Config, "/look/style"))
        .and_then(|value| serde_json::from_value(value.clone()).ok())
        .unwrap_or_default()
}

pub fn selected(own: &Appearance) -> Option<Style> {
    if own.style == Some(WidgetStyle::Liquid) {
        return Some(Style::Liquid);
    }
    if own.surface == Some(Surface::Solid) && own.opacity == Some(1.0) {
        return Some(Style::Solid);
    }
    match own.radius {
        Some(28.0) => Some(Style::Oneui),
        Some(8.0) => Some(Style::Fluent),
        Some(16.0) if own.blur == Some(0.0) => Some(Style::Mono),
        _ => None,
    }
}

pub fn appearance(app: &App, instance: &Instance, style: Style) -> Value {
    let theme = super::specimens::style_theme(app, style);
    let material = mypaper_ui::theme::material(theme.style, theme.color.surface);
    let mut own = mypaper_widgets::look::json_of(&instance.appearance);
    own.extend([
        (
            "style".into(),
            json!(if style == Style::Liquid {
                "liquid"
            } else {
                "minimal"
            }),
        ),
        (
            "surface".into(),
            json!(if style == Style::Solid {
                "solid"
            } else {
                "glass"
            }),
        ),
        ("radius".into(), json!(theme.radius.surface)),
        (
            "opacity".into(),
            json!(material.map_or(
                if style == Style::Solid {
                    1.0
                } else {
                    theme.color.surface.a
                },
                |m| 1.0 - m.clarity
            )),
        ),
        ("blur".into(), json!(material.map_or(0.0, |m| m.blur))),
        ("border".into(), json!(style == Style::Fluent)),
    ]);
    Value::Object(own)
}

/// The slider speaks Clarity; the existing file stores its inverse, opacity.
pub fn slider_edit(
    edit: &crate::actions::Edit,
    value: &Value,
) -> Option<(crate::actions::Edit, Value)> {
    let path = edit.path.strip_prefix("/_panel/clarity")?;
    let clarity = value.as_f64()?.clamp(0.0, 1.0);
    Some((
        crate::actions::Edit::new(edit.doc, path, edit.label.clone()),
        json!(((1.0 - clarity) * 1000.0).round() / 1000.0),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clarity_is_inverted_only_at_the_storage_boundary() {
        let edit = crate::actions::Edit::new(
            DocId::Widgets,
            "/_panel/clarity/instances/0/appearance/opacity",
            "Clarity",
        );
        let (mapped, value) = slider_edit(&edit, &json!(0.73)).unwrap();
        assert_eq!(mapped.path.as_ref(), "/instances/0/appearance/opacity");
        assert_eq!(value, json!(0.27));
        assert!(slider_edit(&mapped, &value).is_none());
    }

    #[test]
    fn every_style_round_trips_through_the_current_widget_format() {
        let ui = mypaper_ui::Ui::new().unwrap();
        let app = crate::goldens::app(true, &ui);
        let layout = crate::pages::widgets::layout(&app);
        for style in Style::ALL {
            let raw = appearance(&app, &layout.instances[0], style);
            let parsed: Appearance = serde_json::from_value(raw).unwrap();
            parsed.validate().unwrap();
            assert_eq!(selected(&parsed), Some(style));
        }
    }

    #[test]
    fn consecutive_widget_style_picks_share_one_undo_step_and_history_row() {
        use crate::{
            actions::{Action, Edit},
            panel::State,
        };
        let ui = mypaper_ui::Ui::new().unwrap();
        let mut app = crate::goldens::app(true, &ui);
        app.panel = Some(State::default());
        crate::panel::history::observe(&app);
        let original = app.docs.doc(DocId::Widgets).raw.clone();
        for style in Style::ALL {
            let layout = crate::pages::widgets::layout(&app);
            let own = appearance(&app, &layout.instances[0], style);
            app.run(Action::Set(
                Edit::new(DocId::Widgets, "/instances/0/appearance", "Widget style"),
                own,
            ));
        }
        assert_eq!(app.docs.undo_count(), 1);
        assert_eq!(
            app.panel.as_ref().unwrap().history.borrow().entries.len(),
            2
        );
        app.run(Action::Undo);
        assert_eq!(app.docs.doc(DocId::Widgets).raw, original);
    }
}
