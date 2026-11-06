use mypaper_config::look::Style;
use mypaper_ui::controls::Align;
use serde_json::{Value, json};

use super::*;
use crate::panel::controls::{more, setting};

const STYLES: [(Style, &str, &str); 5] = [
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
    button(
        b,
        r,
        if app.nav.look_theme {
            "Switch to your look"
        } else {
            "The look the wallpaper brings"
        },
        None,
        false,
        wallpaper.is_some(),
        Action::Panel(Command::Scope(!app.nav.look_theme)),
    );
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
        for (style, name, caption) in STYLES {
            row(
                b,
                c,
                Some(glyph::LOOK),
                name,
                "",
                caption,
                Some(style == current),
                Action::SetStyle(Some(style)),
            );
        }
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
        crate::ui::swatches(
            app,
            b,
            c,
            &Edit::new(DocId::Config, "/look/material/tint", "Tint"),
            app.get(DocId::Config, "/look/material/tint"),
            Some(super::super::tokens::hex(0x8b9eb7)),
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
        crate::ui::swatches(
            app,
            b,
            c,
            &Edit::new(DocId::Config, "/look/accent", "Accent"),
            app.get(DocId::Config, "/look/accent"),
            Some(super::super::tokens::hex(0x8b9eb7)),
            &COLORS,
        );
    });
    if more(app, b, c, "Text and icons, Shape, Different for") {
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

pub fn windows(app: &App, b: &mut B, c: &mut Col) {
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
    group(b, c, "Look", |b, c| {
        for (i, (name, value)) in looks.iter().enumerate() {
            let on = value
                .as_object()
                .unwrap()
                .iter()
                .all(|(k, v)| layer.get(k) == Some(v));
            row(
                b,
                c,
                Some(glyph::APP),
                name,
                "",
                "",
                Some(on),
                Action::WindowsLook(i),
            );
        }
    });
    group(b, c, "", |b, c| {
        for id in ["windows_apps", "windows_corners", "windows_material"] {
            let s = crate::settings::Setting::get(id).unwrap();
            setting(app, b, c, id, s.label, "");
        }
        select(
            b,
            c,
            "Accent",
            layer.get("accent").cloned().unwrap_or(Value::Null),
            Edit::new(DocId::Studio, "/panel/windows/accent", "Windows accent"),
            vec![
                (json!("auto"), "Same as mypaper".into()),
                (json!("#8b9eb7"), "Blue gray".into()),
                (Value::Null, "As is".into()),
            ],
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
                crate::ui::swatches(app, b, c, &edit, layer.get(key), None, &COLORS);
                action_button(
                    b,
                    c,
                    &format!("{label}: As is"),
                    Action::Set(
                        Edit::new(DocId::Studio, format!("/windows_null/{key}"), label),
                        Value::Null,
                    ),
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
