use mypaper_config::look::Style;
use mypaper_ui::controls::Align;
use serde_json::{Value, json};

use super::*;
use crate::panel::controls::{more, setting};

pub(super) const STYLES: [(Style, &str, &str); 5] = [
    (
        Style::Liquid,
        "Liquid",
        "Clear glass, bent light and a soft highlight",
    ),
    (Style::Oneui, "One UI", "Soft plates, generous corners"),
    (Style::Solid, "Solid", "Opaque surfaces and quiet edges"),
    (
        Style::Fluent,
        "Fluent",
        "Frosted depth, familiar to Windows",
    ),
    (Style::Mono, "Mono", "Simple surfaces with just a tint"),
];
const COLORS: [[u8; 3]; 6] = [
    [139, 158, 183],
    [200, 141, 60],
    [127, 174, 142],
    [201, 107, 115],
    [147, 136, 201],
    [230, 230, 230],
];

pub fn view(app: &App, b: &mut B, c: &mut Col) {
    let wallpaper = app.library.active().map(|i| &app.library.items[i]);
    let subtitle = if app.nav.look_theme {
        format!(
            "The look {} brings",
            wallpaper.map_or("the wallpaper", |w| w.name.as_str())
        )
    } else {
        "Your look".into()
    };
    title(b, c, "Look", &subtitle);
    let r = Rect::new(c.x, c.y + b.px(6.0), c.w, b.px(30.0));
    let wallpaper_scope = format!("{}'s", wallpaper.map_or("Wallpaper", |w| w.name.as_str()));
    b.segmented(
        key("panel-look-scope"),
        r,
        &["Yours", &wallpaper_scope],
        Some(usize::from(app.nav.look_theme)),
        "Look scope",
        wallpaper.is_some(),
        |i| Action::Panel(Command::Scope(i == 1)),
    );
    b.stroke(r, b.px(7.0), b.px(1.0), b.p.edge);
    c.y = r.bottom + b.px(4.0);
    if app.nav.look_theme {
        // Reuses the wallpaper editor's validated theme paths and built-in copy protection.
        crate::pages::look::theme_scope(app, b, c);
        return;
    }
    let current: Style = app
        .get(DocId::Config, "/look/style")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    group(b, c, "Style", |b, c| {
        crate::ui::anchor(app, "style", c.view, c.y);
        style_tiles(app, b, c, Some(current), |style| {
            Action::SetStyle(Some(style))
        });
    });
    group(b, c, "Material", |b, c| {
        for (id, uses) in ["clarity", "blur", "refraction", "highlight"]
            .into_iter()
            .zip(crate::look::uses(current))
        {
            if uses {
                let s = crate::settings::Setting::get(id).unwrap();
                setting(app, b, c, id, s.label, "");
            }
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
        crate::ui::anchor(app, "tint", c.view, c.y);
        c.y += height(b, 28.0);
        super::super::colors::swatches(
            app,
            b,
            c,
            &Edit::new(DocId::Config, "/look/material/tint", "Tint"),
            app.get(DocId::Config, "/look/material/tint"),
            true,
            true,
            &COLORS,
        );
    });
    group(b, c, "Color", |b, c| {
        setting(app, b, c, "appearance", "Appearance", "");
        text(
            b,
            Rect::new(c.x, c.y, c.w, height(b, 28.0)),
            "Accent",
            13.0,
            400,
            b.p.ink1,
            Align::Start,
        );
        crate::ui::anchor(app, "accent", c.view, c.y);
        c.y += height(b, 28.0);
        super::super::colors::swatches(
            app,
            b,
            c,
            &Edit::new(DocId::Config, "/look/accent", "Accent"),
            app.get(DocId::Config, "/look/accent"),
            true,
            false,
            &COLORS,
        );
    });
    let more_hint = if b.t.text_scale > 1.0 {
        ""
    } else {
        "Text and icons, Shape, Different for"
    };
    if more(app, b, c, more_hint) {
        group(b, c, "Text and icons", |b, c| {
            for id in [
                "font",
                "text_size",
                "text_shadow",
                "text_tone",
                "icon_style",
            ] {
                let s = crate::settings::Setting::get(id).unwrap();
                setting(app, b, c, id, s.label, "");
            }
        });
        group(b, c, "App icons", |b, c| {
            crate::pages::look::icon_pack(app, b, c)
        });
        group(b, c, "Shape", |b, c| {
            setting(app, b, c, "corners", "Corners", "");
            setting(app, b, c, "edge", "Edge line", "");
        });
        group(b, c, "Different for", |b, c| {
            for (family, name) in [
                ("bars", "Bars"),
                ("widgets", "Widgets"),
                ("menus", "Menus and panels"),
            ] {
                let path = format!("/look/{family}/style");
                let value = app
                    .get(DocId::Config, &path)
                    .cloned()
                    .unwrap_or(Value::Null);
                let mut options = vec![(Value::Null, "From Look".into())];
                options.extend(STYLES.into_iter().map(|(s, n, _)| (json!(s), n.into())));
                select(
                    b,
                    c,
                    name,
                    value,
                    Edit::new(DocId::Config, path, format!("{name} style")),
                    options,
                );
            }
        });
    }
}

/// The same five choices on Look and a widget's own look, always in one row.
pub(super) fn style_tiles(
    app: &App,
    b: &mut B,
    c: &mut Col,
    selected: Option<Style>,
    action: impl Fn(Style) -> Action,
) {
    let gap = b.px(6.0);
    let width = (c.w - gap * 4.0) / 5.0;
    let picture_height = width * 0.8;
    let label_height = height(b, 24.0);
    b.group(key(("panel-look-styles", c.view)), |b| {
        for (i, (style, name, _)) in STYLES.iter().enumerate() {
            let rect = Rect::new(
                c.x + i as f32 * (width + gap),
                c.y,
                width,
                picture_height + label_height,
            );
            let k = key(("panel-look-style", c.view, i));
            let on = selected == Some(*style);
            b.plate(k, rect.inflate(b.px(2.0)), b.px(9.0), on);
            super::super::specimens::material(
                app,
                b,
                Rect::new(rect.left, rect.top, width, picture_height),
                *style,
            );
            text(
                b,
                Rect::new(rect.left, rect.top + picture_height, width, label_height),
                name,
                11.0,
                if on { 600 } else { 400 },
                b.p.ink1,
                Align::Center,
            );
            b.hit_checked(
                k,
                rect,
                Role::RadioButton,
                name,
                Some(on),
                action(*style),
                Behavior::Click,
            );
            b.focus_ring(k, rect, b.px(9.0));
        }
    });
    c.y += picture_height + label_height + b.px(8.0);
    if let Some((_, _, sentence)) = STYLES.iter().find(|(style, _, _)| Some(*style) == selected) {
        notice(b, c, sentence);
    }
}

pub fn windows(app: &App, b: &mut B, c: &mut Col) {
    let reset = Rect::new(c.right() - b.px(28.0), c.y, b.px(28.0), b.px(28.0));
    crate::panel::controls::icon_button(
        b,
        reset,
        glyph::RESET,
        "Reset to how Windows has it",
        true,
        Action::WindowsLook(0),
    );
    title(b, c, "Windows", "Your Windows look");
    let layer = app
        .get(DocId::Config, "/look/windows")
        .cloned()
        .unwrap_or(json!({}));
    let looks = [
        (
            "As is",
            json!({"accent":null,"border":null,"caption":null,"caption_text":null,"corners":null,"backdrop":null,"apps":"as-is","cursors":null}),
        ),
        (
            "Match mypaper",
            json!({"accent":"auto","border":"auto","corners":"round","backdrop":"mica","apps":"match"}),
        ),
        (
            "Glass",
            json!({"accent":"auto","border":null,"corners":"round","backdrop":"acrylic","apps":"match"}),
        ),
        (
            "Sharp",
            json!({"accent":"auto","border":"auto","corners":"square","backdrop":"none","apps":"match"}),
        ),
    ];
    let selected = looks
        .iter()
        .position(|(_, preset)| windows_matches(&layer, preset));
    group(
        b,
        c,
        if selected.is_none() {
            "Look · Custom"
        } else {
            "Look"
        },
        |b, c| {
            let choices: Vec<_> = looks
                .iter()
                .enumerate()
                .map(|(i, (name, _))| (*name, selected == Some(i), Action::WindowsLook(i)))
                .collect();
            crate::ui::tiles(
                b,
                c,
                key("panel-windows-looks"),
                2,
                false,
                &choices,
                |b, i, pic| super::super::specimens::window(app, b, pic, i),
            );
        },
    );
    group(b, c, "", |b, c| {
        for id in ["windows_apps", "windows_corners", "windows_material"] {
            let s = crate::settings::Setting::get(id).unwrap();
            setting(app, b, c, id, s.label, "");
        }
        text(
            b,
            Rect::new(c.x, c.y, c.w, height(b, 28.0)),
            "Accent",
            13.0,
            400,
            b.p.ink1,
            Align::Start,
        );
        c.y += height(b, 28.0);
        super::super::colors::swatches(
            app,
            b,
            c,
            &Edit::new(DocId::Config, "/look/windows/accent", "Windows accent"),
            layer.get("accent"),
            true,
            false,
            &COLORS,
        );
    });
    group(b, c, "Pointer", |b, c| {
        let current = layer.get("cursors").and_then(Value::as_str);
        for (id, name) in [
            (None, "As is"),
            (Some("mypaper-dark"), "Dark"),
            (Some("mypaper-light"), "Light"),
        ] {
            let action = match id {
                Some(id) => Action::Set(
                    Edit::new(DocId::Config, "/look/windows/cursors", "Pointer"),
                    json!(id),
                ),
                None => Action::Set(
                    Edit::new(DocId::Studio, "/windows_null/cursors", "Pointer"),
                    Value::Null,
                ),
            };
            row(b, c, None, name, "", "", Some(current == id), action);
        }
        action_button(
            b,
            c,
            "Choose a pointer folder",
            Action::PickFile("cursors".into(), "/look/windows/cursors".into()),
        );
    });
    if more(app, b, c, "Window edge, title bar and title text") {
        group(b, c, "Edges and title bars", |b, c| {
            for (key, label) in [
                ("border", "Window edge"),
                ("caption", "Title bar"),
                ("caption_text", "Title text"),
            ] {
                let edit = Edit::new(DocId::Config, format!("/look/windows/{key}"), label);
                text(
                    b,
                    Rect::new(c.x, c.y, c.w, b.px(26.0)),
                    label,
                    13.0,
                    400,
                    b.p.ink1,
                    Align::Start,
                );
                c.y += b.px(28.0);
                super::super::colors::swatches(
                    app,
                    b,
                    c,
                    &edit,
                    layer.get(key),
                    false,
                    false,
                    &COLORS,
                );
            }
        });
    }
    notice(
        b,
        c,
        "Windows' own styling comes back when mypaper stops or you restore the Windows desktop.",
    );
}

/// Absent native keys mean As is; extra overrides make a preset Custom.
pub(super) fn windows_matches(layer: &Value, preset: &Value) -> bool {
    [
        "accent",
        "border",
        "caption",
        "caption_text",
        "corners",
        "backdrop",
        "apps",
        "cursors",
    ]
    .into_iter()
    .all(|key| {
        let default = if key == "apps" {
            json!("as-is")
        } else {
            Value::Null
        };
        layer.get(key).filter(|v| !v.is_null()).unwrap_or(&default)
            == preset.get(key).filter(|v| !v.is_null()).unwrap_or(&default)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_native_fields_are_as_is_and_extra_changes_are_custom() {
        let as_is = json!({"accent":null,"border":null,"caption":null,"caption_text":null,"corners":null,"backdrop":null,"apps":"as-is","cursors":null});
        assert!(windows_matches(&json!({}), &as_is));
        assert!(windows_matches(
            &json!({"apps":"as-is", "corners":null}),
            &as_is
        ));
        assert!(!windows_matches(&json!({"corners":"round"}), &as_is));
        let sharp = json!({"accent":"auto","border":"auto","corners":"square","backdrop":"none","apps":"match"});
        assert!(windows_matches(&sharp, &sharp));
        let mut custom = sharp.clone();
        custom["caption"] = json!("#123456");
        assert!(!windows_matches(&custom, &sharp));
    }
}
