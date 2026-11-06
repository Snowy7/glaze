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
    [139, 158, 183],
    [200, 141, 60],
    [127, 174, 142],
    [201, 107, 115],
    [147, 136, 201],
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
    let stacked = c.w < b.px(340.0) || b.t.text_scale > 1.0;
    let h = height(b, 38.0);
    text(
        b,
        Rect::new(
            c.x,
            c.y,
            if stacked { c.w } else { b.px(84.0) },
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
    let inset = if stacked { 0.0 } else { b.px(88.0) };
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
            format!("{:.0}%", slider.value * 100.0)
        } else {
            format!("{:.0} px", slider.value)
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
        "Tint",
        13.0,
        400,
        b.p.ink1,
        Align::Start,
    );
    c.y += height(b, 28.0);
    super::colors::swatches(app, b, c, &tint, current, auto, true, &SWATCHES);
}

pub fn wallpaper(
    app: &App,
    b: &mut B,
    c: &mut Col,
    wallpaper: &crate::library::Wallpaper,
    theme: &Value,
) {
    let edit = |path: &str, label: &str| {
        Edit::new(
            crate::docs::DocId::Studio,
            format!(
                "/theme:{}{path}",
                crate::docs::token(&wallpaper.dir.display().to_string())
            ),
            label,
        )
    };
    let sliders: Vec<_> = [
        ("clarity", "Clarity", 0.94),
        ("blur", "Blur", 0.0),
        ("refraction", "Refraction", 0.6),
        ("highlight", "Highlight", 0.55),
    ]
    .into_iter()
    .map(|(id, label, fallback)| {
        let stored = if id == "clarity" { "tint" } else { id };
        let value = theme
            .pointer(&format!("/widgets/glass/{stored}"))
            .and_then(Value::as_f64)
            .map(|v| if id == "clarity" { 1.0 - v } else { v })
            .unwrap_or(fallback);
        Slider {
            label,
            value,
            max: if id == "blur" { 32.0 } else { 1.0 },
            edit: edit(&format!("/@both/glass/{id}"), label),
        }
    })
    .collect();
    block(
        app,
        b,
        c,
        &sliders,
        edit("/@both/appearance/tint", "Tint"),
        theme.pointer("/widgets/appearance/tint"),
        true,
    );
}
