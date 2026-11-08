//! A shared material block for Your look, wallpaper looks and widget overrides.
use super::controls::{height, text};
use crate::{
    actions::{Action, Edit},
    app::App,
    ui::{B, Col},
};
use mypaper_ui::{
    Rect,
    controls::{Align, key},
};
use serde_json::Value;

pub const SWATCHES: [[u8; 3]; 6] = [
    [52, 58, 66],
    [91, 98, 107],
    [122, 117, 109],
    [117, 128, 121],
    [142, 138, 148],
    [230, 230, 230],
];

pub struct Slider {
    pub label: &'static str,
    pub value: f64,
    pub max: f64,
    pub edit: Edit,
}

pub fn row(
    b: &mut B,
    c: &mut Col,
    label: &str,
    value: f64,
    range: (f64, f64, f64),
    readout: &str,
    edit: Edit,
) {
    let label_width = super::controls::label_width(b, c);
    let stacked = c.w < b.px(340.0) || b.t.text_scale > 1.0 || label_width > c.w * 0.45;
    let h = height(b, 34.0);
    text(
        b,
        Rect::new(
            c.x,
            c.y,
            if stacked { c.w } else { label_width },
            if stacked { height(b, 24.0) } else { h },
        ),
        label,
        13.0,
        400,
        b.p.ink1,
        Align::Start,
    );
    if stacked {
        c.y += height(b, 24.0);
    }
    let inset = if stacked {
        0.0
    } else {
        label_width + b.px(8.0)
    };
    b.slider(
        key(("panel-material", edit.doc, edit.path.as_ref())),
        Rect::new(c.x + inset, c.y, c.w - inset, h),
        value,
        range,
        readout,
        b.px(58.0),
        label,
        Action::Slider(edit),
    );
    c.y += h;
}

/// Hover previews never alter the applied style's controls or their geometry.
pub fn applied_controls(
    b: &mut B,
    c: &mut Col,
    _preview: Option<&str>,
    body: impl FnOnce(&mut B, &mut Col),
) {
    body(b, c);
}

pub fn block(
    app: &App,
    b: &mut B,
    c: &mut Col,
    sliders: &[Slider],
    tint: Edit,
    current: Option<&Value>,
    auto: bool,
) {
    for slider in sliders {
        let percent = slider.max == 1.0;
        let readout = if percent {
            super::controls::unit(slider.value * 100.0, "%")
        } else {
            super::controls::unit(slider.value, "px")
        };
        row(
            b,
            c,
            slider.label,
            slider.value,
            (0.0, slider.max, if percent { 0.01 } else { 1.0 }),
            &readout,
            slider.edit.clone(),
        );
    }
    text(
        b,
        Rect::new(c.x, c.y, c.w, height(b, 28.0)),
        "Glass colour",
        13.0,
        400,
        b.p.ink1,
        Align::Start,
    );
    c.y += height(b, 28.0);
    super::controls::notice(b, c, "Colours the surface behind text and controls.");
    super::colors::swatches(app, b, c, &tint, current, auto, true, &SWATCHES);
}

pub fn wallpaper(
    app: &App,
    b: &mut B,
    c: &mut Col,
    wallpaper: &crate::library::Wallpaper,
    theme: &Value,
) {
    let sample = super::wallpaper_look::sample(app, theme);
    let uses = super::wallpaper_look::selected(theme)
        .map(crate::look::uses)
        .unwrap_or(match sample.style {
            mypaper_config::look::Style::Liquid => [true; 4],
            mypaper_config::look::Style::Solid => [false; 4],
            _ => [true, true, false, false],
        });
    let sliders: Vec<_> = [
        ("clarity", "Clarity", sample.clarity),
        ("blur", "Blur", sample.blur),
        ("refraction", "Refraction", sample.refraction),
        ("highlight", "Highlight", sample.highlight),
    ]
    .into_iter()
    .zip(uses)
    .filter(|(_, used)| *used)
    .map(|((id, label, current), _)| {
        let edit = super::wallpaper_look::material_edit(wallpaper, id, label);
        let value = app
            .pending_value
            .as_ref()
            .filter(|(pending, _)| pending == &edit)
            .and_then(|(_, value)| value.as_f64())
            .unwrap_or(current as f64);
        Slider {
            label,
            value,
            max: if id == "blur" { 32.0 } else { 1.0 },
            edit,
        }
    })
    .collect();
    block(
        app,
        b,
        c,
        &sliders,
        super::wallpaper_look::edit(wallpaper, "/@both/appearance/tint", "Tint"),
        theme.pointer("/widgets/appearance/tint"),
        false,
    );
}

/// Unset knobs follow the wallpaper until a style is chosen, then that style's defaults.
pub fn look_value(app: &App, style: mypaper_config::look::Style, id: &str) -> f64 {
    use crate::docs::DocId;
    if let Some(value) = app
        .pending_value
        .as_ref()
        .filter(|(edit, _)| {
            edit.doc == DocId::Config && edit.path.as_ref() == format!("/look/material/{id}")
        })
        .and_then(|(_, value)| value.as_f64())
    {
        return value;
    }
    if let Some(value) = app
        .get(DocId::Config, &format!("/look/material/{id}"))
        .and_then(Value::as_f64)
    {
        return value;
    }
    if app.get(DocId::Config, "/look/style").is_none()
        && let Some(wallpaper) = app
            .library
            .active()
            .and_then(|index| app.library.items.get(index))
    {
        let theme = super::saving::theme_value(app, wallpaper);
        let field = if id == "clarity" { "tint" } else { id };
        if let Some(value) = theme
            .pointer(&format!("/widgets/glass/{field}"))
            .and_then(Value::as_f64)
        {
            return if id == "clarity" { 1.0 - value } else { value };
        }
    }
    let theme = super::specimens::style_theme(app, style);
    let defaults = mypaper_ui::theme::material(theme.style, theme.color.surface);
    (match id {
        "clarity" => defaults.map_or(1.0 - theme.color.surface.a, |m| m.clarity),
        "blur" => defaults.map_or(0.0, |m| m.blur),
        "refraction" => defaults.map_or(0.0, |m| m.refraction),
        "highlight" => defaults.map_or(0.0, |m| m.highlight),
        _ => 0.0,
    }) as f64
}

/// The Windows matching specimen uses the same tint source as the desktop's current material.
pub fn look_tint(app: &App, style: mypaper_config::look::Style) -> mypaper_ui::Rgba {
    use crate::docs::DocId;
    let theme = super::specimens::style_theme(app, style);
    let current = app
        .pending_value
        .as_ref()
        .filter(|(edit, _)| {
            edit.doc == DocId::Config && edit.path.as_ref() == "/look/material/tint"
        })
        .map(|(_, value)| value)
        .or_else(|| app.get(DocId::Config, "/look/material/tint"))
        .and_then(Value::as_str);
    if current == Some("auto") {
        return super::specimens::wallpaper_color(app).unwrap_or(theme.color.surface);
    }
    if let Some(color) = current.and_then(mypaper_config::Color::parse) {
        return mypaper_ui::Rgba::rgb8(color.0, 1.0);
    }
    if app.get(DocId::Config, "/look/style").is_none()
        && let Some(wallpaper) = app
            .library
            .active()
            .and_then(|index| app.library.items.get(index))
        && let Some(color) = super::saving::theme_value(app, wallpaper)
            .pointer("/widgets/appearance/tint")
            .and_then(Value::as_str)
            .and_then(mypaper_config::Color::parse)
    {
        return mypaper_ui::Rgba::rgb8(color.0, 1.0);
    }
    theme.color.surface
}
