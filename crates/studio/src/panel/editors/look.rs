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
    let wallpaper = crate::panel::wallpaper_look::target(app);
    let subtitle = if app.nav.look_theme {
        format!(
            "The look {} brings",
            wallpaper.map_or("the wallpaper", |w| w.name.as_str())
        )
    } else {
        "Your look".into()
    };
    let reset = Rect::new(c.right() - b.px(28.0), c.y, b.px(28.0), b.px(28.0));
    crate::panel::controls::icon_button(
        b,
        reset,
        glyph::RESET,
        if app.nav.look_theme {
            "Return to this style's defaults"
        } else {
            "Return to the wallpaper's look"
        },
        !app.nav.look_theme || wallpaper.is_some_and(|wallpaper| wallpaper.editable()),
        if app.nav.look_theme {
            wallpaper.map_or(Action::None, |wallpaper| {
                crate::panel::wallpaper_look::style_action(
                    wallpaper,
                    crate::panel::wallpaper_look::selected(&crate::panel::saving::theme_value(
                        app, wallpaper,
                    ))
                    .unwrap_or_default(),
                )
            })
        } else {
            Action::LookReset
        },
    );
    title(b, c, "Look", &subtitle);
    if let Some(wallpaper) = wallpaper {
        let r = Rect::new(c.x, c.y + b.px(6.0), c.w, b.px(30.0));
        let wallpaper_label = format!("{}'s", wallpaper.name);
        b.segmented(
            key("panel-look-scope"),
            r,
            &["Yours", &wallpaper_label],
            Some(usize::from(app.nav.look_theme)),
            "Look scope",
            true,
            |i| Action::Panel(Command::Scope(i == 1)),
        );
        b.stroke(r, b.px(7.0), b.px(1.0), b.p.edge);
        c.y = r.bottom + b.px(4.0);
    }
    if app.nav.look_theme {
        if wallpaper.is_none() {
            notice(b, c, "Choose a wallpaper to edit the look it brings.");
            action_button(b, c, "Choose a wallpaper", open(Page::Wallpapers));
            return;
        }
        // Reuses the wallpaper editor's validated theme paths and built-in copy protection.
        wallpaper_scope(app, b, c);
        return;
    }
    let current: Style = app
        .get(DocId::Config, "/look/style")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    let mut shown = Some(current);
    group(b, c, "Style", |b, c| {
        crate::ui::anchor(app, "style", c.view, c.y);
        shown = style_tiles(app, b, c, Some(current), |style| {
            Action::SetStyle(Some(style))
        });
    });
    let preview = shown.filter(|style| *style != current).map(style_name);
    group(b, c, "Material", |b, c| {
        crate::panel::material::applied_controls(b, c, preview, |b, c| {
            if preview.is_none() {
                notice(b, c, &format!("Your {} settings", style_name(current)));
            }
            let sliders: Vec<_> = ["clarity", "blur", "refraction", "highlight"]
                .into_iter()
                .zip(crate::look::uses(current))
                .filter(|(_, uses)| *uses)
                .map(|(id, _)| {
                    let setting = crate::settings::Setting::get(id).unwrap();
                    crate::ui::anchor(app, id, c.view, c.y);
                    crate::panel::material::Slider {
                        label: setting.label,
                        value: crate::panel::material::look_value(app, current, id),
                        max: if id == "blur" { 32.0 } else { 1.0 },
                        edit: Edit::new(setting.doc, setting.path, setting.label),
                    }
                })
                .collect();
            crate::ui::anchor(app, "tint", c.view, c.y);
            crate::panel::material::block(
                app,
                b,
                c,
                &sliders,
                Edit::new(DocId::Config, "/look/material/tint", "Tint"),
                app.get(DocId::Config, "/look/material/tint"),
                true,
            );
        });
    });
    group(b, c, "Color", |b, c| {
        crate::panel::material::applied_controls(b, c, preview, |b, c| {
            if preview.is_none() {
                notice(b, c, &format!("Your {} colors", style_name(current)));
            }
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
                if family == "widgets" {
                    let mut nested = Col {
                        x: c.x + b.px(16.0),
                        w: c.w - b.px(16.0),
                        ..*c
                    };
                    select(
                        b,
                        &mut nested,
                        "Background",
                        app.get(DocId::Widgets, "/appearance/surface")
                            .cloned()
                            .unwrap_or(Value::Null),
                        Edit::new(DocId::Widgets, "/appearance/surface", "Widgets background"),
                        vec![
                            (Value::Null, "From Look".into()),
                            (json!("none"), "None".into()),
                            (json!("glass"), "Glass".into()),
                            (json!("solid"), "Solid".into()),
                        ],
                    );
                    c.y = nested.y;
                }
            }
        });
    }
}

/// Look and widget overrides share the same five pictured choices.
pub(super) fn style_tiles(
    app: &App,
    b: &mut B,
    c: &mut Col,
    selected: Option<Style>,
    action: impl Fn(Style) -> Action,
) -> Option<Style> {
    let choices: Vec<_> = STYLES
        .iter()
        .map(|(style, name, _)| (*name, selected == Some(*style), action(*style)))
        .collect();
    let picture_height = b.px(164.0);
    let shown = crate::panel::choosers::draw(
        app,
        b,
        c,
        key(("panel-look-styles", c.view)),
        picture_height,
        &choices,
        |b, index, rect| {
            crate::panel::specimens::material(
                app,
                b,
                rect,
                index.map_or(selected.unwrap_or_default(), |index| STYLES[index].0),
            )
        },
    );
    shown.map(|index| STYLES[index].0)
}

pub(super) fn style_name(style: Style) -> &'static str {
    STYLES
        .iter()
        .find(|(value, _, _)| *value == style)
        .unwrap()
        .1
}

fn wallpaper_scope(app: &App, b: &mut B, c: &mut Col) {
    use crate::panel::{specimens, wallpaper_look as target};
    let Some(wallpaper) = target::target(app) else {
        return;
    };
    if !wallpaper.editable() {
        notice(b, c, "Built in. Make a copy to change the look it brings.");
        if let Some(index) = app
            .library
            .items
            .iter()
            .position(|item| item.dir == wallpaper.dir)
        {
            action_button(b, c, "Make a copy to edit", Action::CopyWallpaper(index));
        }
        return;
    }
    let theme = crate::panel::saving::theme_value(app, wallpaper);
    notice(
        b,
        c,
        "Saved to this wallpaper. Your own look still wins where you set it.",
    );
    let selected = target::selected(&theme);
    let mut shown = None;
    group(b, c, "Style", |b, c| {
        let choices: Vec<_> = STYLES
            .iter()
            .map(|(style, name, _)| {
                (
                    *name,
                    selected == Some(*style),
                    target::style_action(wallpaper, *style),
                )
            })
            .collect();
        shown = crate::panel::choosers::draw(
            app,
            b,
            c,
            key(("panel-wallpaper-styles", &wallpaper.id)),
            b.px(164.0),
            &choices,
            |b, index, rect| {
                let shown = index.map(|index| target::with_style(app, &theme, STYLES[index].0));
                specimens::material_sample(
                    app,
                    b,
                    rect,
                    target::sample(app, shown.as_ref().unwrap_or(&theme)),
                    Some(wallpaper),
                );
            },
        );
    });
    let preview = shown
        .map(|index| STYLES[index].0)
        .filter(|style| Some(*style) != selected)
        .map(style_name);
    group(b, c, "Material", |b, c| {
        crate::panel::material::applied_controls(b, c, preview, |b, c| {
            if preview.is_none() {
                notice(
                    b,
                    c,
                    &format!(
                        "This wallpaper's {} settings",
                        selected.map_or("Custom", style_name)
                    ),
                );
            }
            crate::panel::material::wallpaper(app, b, c, wallpaper, &theme);
        });
    });
    let edit = |path: &str, label: &str| target::edit(wallpaper, path, label);
    group(b, c, "Color", |b, c| {
        crate::panel::material::applied_controls(b, c, preview, |b, c| {
            if preview.is_none() {
                notice(
                    b,
                    c,
                    &format!(
                        "This wallpaper's {} colors",
                        selected.map_or("Custom", style_name)
                    ),
                );
            }
            select(
                b,
                c,
                "Text",
                theme
                    .pointer("/widgets/appearance/text")
                    .cloned()
                    .unwrap_or(Value::Null),
                edit("/@both/appearance/text", "Text tone"),
                vec![
                    (Value::Null, "The style's".into()),
                    (json!("light"), "Light".into()),
                    (json!("dark"), "Dark".into()),
                ],
            );
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
            crate::panel::colors::swatches(
                app,
                b,
                c,
                &edit("/@both/appearance/accent", "Accent"),
                theme.pointer("/widgets/appearance/accent"),
                true,
                false,
                &COLORS,
            );
        });
    });
    if more(app, b, c, "Text and shape") {
        group(b, c, "Text", |b, c| {
            crate::panel::pickers::font(
                app,
                b,
                c,
                edit("/@both/appearance/font", "Font"),
                theme
                    .pointer("/widgets/appearance/font")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
            );
        });
        group(b, c, "Shape", |b, c| {
            let sample = target::sample(app, &theme);
            crate::panel::material::row(
                b,
                c,
                "Corners",
                sample.radius as f64,
                (0.0, 48.0, 1.0),
                &crate::panel::controls::unit(sample.radius as f64, "px"),
                edit("/@both/appearance/radius", "Corners"),
            );
            toggle(
                b,
                c,
                "Edge line",
                "",
                sample.edge,
                Action::Set(
                    edit("/@both/appearance/border", "Edge line"),
                    json!(!sample.edge),
                ),
            );
        });
    }
}

fn windows_looks() -> [(&'static str, Value); 4] {
    [
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
    ]
}

pub fn windows_summary(app: &App) -> &'static str {
    let layer = app
        .get(DocId::Config, "/look/windows")
        .cloned()
        .unwrap_or(json!({}));
    windows_looks()
        .into_iter()
        .find(|(_, preset)| windows_matches(&layer, preset))
        .map_or("Custom", |(name, _)| name)
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
    let looks = windows_looks();
    let selected = looks
        .iter()
        .position(|(_, preset)| windows_matches(&layer, preset));
    group(b, c, "Look", |b, c| {
        let choices: Vec<_> = looks
            .iter()
            .enumerate()
            .map(|(i, (name, _))| (*name, selected == Some(i), Action::WindowsLook(i)))
            .collect();
        let picture_height = b.px(174.0);
        crate::panel::choosers::draw(
            app,
            b,
            c,
            key("panel-windows-looks"),
            picture_height,
            &choices,
            |b, index, rect| super::super::specimens::window_choice(app, b, rect, index),
        );
    });
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
        "accent_on_taskbar",
        "accent_on_title_bars",
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
    fn header_reset_returns_to_the_wallpaper_and_preserves_windows_styling() {
        let ui = mypaper_ui::Ui::new().unwrap();
        let mut app = crate::goldens::app(true, &ui);
        app.panel = Some(crate::panel::State {
            page: Page::Look,
            ..Default::default()
        });
        let original = json!({"style":"solid","material":{"blur":8},"windows":{"apps":"dark","corners":"square"}});
        app.run(Action::Set(
            Edit::new(DocId::Config, "/look", "Your look"),
            original.clone(),
        ));
        app.nav.look_windows = true;
        let checkpoint = app.docs.undo_count();
        app.run(Action::LookReset);
        assert_eq!(
            app.get(DocId::Config, "/look/windows"),
            Some(&original["windows"])
        );
        assert!(app.get(DocId::Config, "/look/style").is_none());
        assert_eq!(app.docs.undo_count(), checkpoint + 1);
        app.run(Action::Undo);
        assert_eq!(app.get(DocId::Config, "/look"), Some(&original));
    }
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
        let mut custom = sharp.clone();
        custom["accent_on_title_bars"] = json!(true);
        assert!(!windows_matches(&custom, &sharp));
    }
}
