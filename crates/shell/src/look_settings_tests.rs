use super::*;
use mypaper_bars::BarsConfig;
use mypaper_ui::{Op, Ui, icons::IconSet};
use serde_json::json;

fn settings(value: serde_json::Value) -> Look {
    serde_json::from_value(value).unwrap()
}

#[test]
fn automatic_menu_and_widget_glass_share_the_neutral_tint() {
    for value in [json!({"material":{"tint":"auto"}}), json!({"material":{"tint":"#c88d3c"}})] {
        let look = settings(value);
        let system = crate::flyouts::look::fixed_system();
        let tint = |family| appearance(&look, family, &Appearance::default(), false, &system).tint;
        assert_eq!(tint(Family::Menus), tint(Family::Widgets));
        assert_eq!(tint(Family::Menus), Some(match look.material.tint.unwrap() {
            mypaper_theme::Paint::Auto => mypaper_widgets::look::DEFAULT_TINT,
            mypaper_theme::Paint::Color(color) => color,
        }));
    }
}

fn resolve(user: &Look, bars: &BarsConfig, key: &str, text: &Text) -> crate::bars::look::Tokens {
    let system = crate::flyouts::look::fixed_system();
    let mut appearance = Appearance {
        radius: Some(5.0),
        ..Default::default()
    };
    let mut look = if key == "top" {
        bars.top.look.clone()
    } else {
        bars.dock.look.clone()
    };
    crate::bars::look::appearance_layer(bars, key, &mut look, &appearance);
    bar(
        user,
        Family::Bars,
        bars,
        key,
        &mut look,
        &mut appearance,
        &system,
        text,
    );
    crate::bars::look::Tokens::resolve_settings(&look, &appearance, &system, user, text)
}

#[test]
fn selected_tints_give_the_macos_bar_live_glass_and_readable_ink() {
    let ui = Ui::new().unwrap();
    let bars = BarsConfig::parse(r#"{"preset":"macos"}"#).unwrap();
    for color in [
        "#8b9eb7", "#c88d3c", "#7fae8e", "#c96b73", "#9388c9", "#e6e6e6", "#2476c8",
    ] {
        let user = settings(json!({"material":{"tint":color}}));
        for key in ["top", "dock"] {
            let mut tokens = resolve(&user, &bars, key, &ui.text);
            assert!(
                !tokens.bare && tokens.live_blur && !tokens.wallpaper_only,
                "{key}: {color}"
            );
            let material = tokens.material.unwrap();
            let expected = crate::bars::look::hex(color).unwrap();
            assert_eq!(material.tint, expected.with_alpha(material.tint.a));
            tokens.adapt_ink(&bars.top.look, None);
            let under = crate::glass::tone::backdrop(None, tokens.theme.dark());
            let face = crate::glass::tone::face(&material, under, 40.0);
            let ink = mypaper_ui::theme::over(tokens.theme.color.label, face);
            assert!(
                mypaper_ui::theme::contrast(ink, face) >= 4.498,
                "{key}: {color}"
            );
        }
    }
    let reset = resolve(&Look::default(), &bars, "top", &ui.text);
    assert!(reset.bare && reset.material.is_none() && !reset.live_blur);
}

#[test]
fn chosen_bodies_replace_design_transparency_but_explicit_bare_bars_win() {
    let ui = Ui::new().unwrap();
    let user = settings(json!({"material":{"tint":"#8b9eb7"}}));
    for own in [
        json!({"transparent":true}),
        json!({"background":{"type":"none"}}),
    ] {
        let bars =
            BarsConfig::parse(&json!({"preset":"macos","top":{"look":own}}).to_string()).unwrap();
        assert!(resolve(&user, &bars, "top", &ui.text).bare);
    }
    for (body, wallpaper) in [
        (json!({"type":"solid","color":"#123456"}), false),
        (json!({"type":"wallpaper"}), true),
    ] {
        let bars = BarsConfig::parse(
            &json!({"preset":"macos","top":{"look":{"background":body}}}).to_string(),
        )
        .unwrap();
        let tokens = resolve(&user, &bars, "top", &ui.text);
        assert!(!tokens.bare && !tokens.live_blur);
        assert_eq!(tokens.wallpaper_only, wallpaper);
        assert_eq!(tokens.material.is_some(), wallpaper);
    }
    let bars = BarsConfig::parse(
        r#"{"preset":"macos","overrides":{"secondary":{"top":{"look":{"transparent":true}}}}}"#,
    )
    .unwrap();
    assert!(!resolve(&user, &bars, "top", &ui.text).bare);
    assert!(
        resolve(
            &user,
            &bars.for_monitor("DISPLAY2", false).unwrap(),
            "top",
            &ui.text
        )
        .bare
    );
    for style in ["solid", "mono"] {
        let tokens = resolve(&settings(json!({"style":style})), &bars, "top", &ui.text);
        assert!(
            !tokens.bare && tokens.material.is_none(),
            "{style}: a chosen flat body"
        );
    }
}

#[test]
fn corners_override_design_and_theme_but_explicit_bar_and_monitor_radii_win() {
    let ui = Ui::new().unwrap();
    let bars = BarsConfig::parse(r#"{"preset":"macos","dock":{"look":{"radius":7}},"overrides":{"secondary":{"top":{"look":{"radius":3}}}}}"#).unwrap();
    let user = settings(json!({"shape":{"corners":0}}));
    assert_eq!(resolve(&user, &bars, "top", &ui.text).radius, 0.0);
    assert_eq!(resolve(&user, &bars, "dock", &ui.text).radius, 7.0);
    let second = bars.for_monitor("DISPLAY2", false).unwrap();
    assert_eq!(resolve(&user, &second, "top", &ui.text).radius, 3.0);
    let reset = resolve(&Look::default(), &bars, "top", &ui.text);
    assert_eq!(
        reset.radius, 5.0,
        "removing the override restores the theme"
    );
}

#[test]
fn strip_bar_drawing_and_published_shapes_share_the_explicit_radius() {
    let mut ui = Ui::new().unwrap();
    let mut bars =
        BarsConfig::parse(r#"{"top":{"layout":"strip","left":["start"],"center":[],"right":[]}}"#)
            .unwrap();
    bars.resolve(Vec::new);
    let model = crate::preview::bars::sample_model(Default::default(), "Look");
    for radius in [0.0, 24.0] {
        let user = settings(json!({"shape":{"corners":radius}, "text":{"size":1.5}}));
        let tokens = resolve(&user, &bars, "top", &ui.text);
        let frame = crate::preview::bars::top(
            1000,
            &bars,
            &model,
            &tokens,
            tokens.scale,
            &mut ui,
            &mut Default::default(),
            None,
        );
        assert!(!frame.shapes.is_empty());
        for (rect, published) in &frame.shapes {
            assert_eq!(*published, radius * tokens.scale);
            assert!(frame.list.ops.iter().any(|op| matches!(op, Op::Fill {rect:r,radius:drawn,color} if r==rect && drawn==published && *color==mypaper_ui::Rgba::HIT)));
        }
    }
}

#[test]
fn edge_switch_reaches_glass_surface_styles_and_specific_border_wins() {
    let ui = Ui::new().unwrap();
    for edge in [false, true] {
        let user = settings(json!({"shape":{"edge":edge}}));
        let tokens = resolve(&user, &BarsConfig::parse("{}").unwrap(), "top", &ui.text);
        assert!(tokens.material.is_some());
        assert_eq!(tokens.styles.edges[0].width, if edge { 1.0 } else { 0.0 });
        let bars =
            BarsConfig::parse(r#"{"top":{"look":{"border":{"rest":{"width":2}}}}}"#).unwrap();
        assert_eq!(
            resolve(&user, &bars, "top", &ui.text).styles.edges[0].width,
            2.0
        );
    }
}

#[test]
fn font_family_is_installed_or_falls_back_and_explicit_bar_family_wins() {
    assert_eq!(
        font("Missing face", |f| f == mypaper_ui::VARIABLE),
        mypaper_ui::VARIABLE
    );
    assert_eq!(font("", |_| false), "Segoe UI");
    let ui = Ui::new().unwrap();
    let user = settings(json!({"text":{"font":"Consolas"}}));
    let bars = BarsConfig::parse("{}").unwrap();
    assert_eq!(
        resolve(&user, &bars, "top", &ui.text).font.family.as_ref(),
        "Consolas"
    );
    let bars = BarsConfig::parse(r#"{"top":{"look":{"font":{"family":"Arial"}}}}"#).unwrap();
    assert_eq!(
        resolve(&user, &bars, "top", &ui.text).font.family.as_ref(),
        "Arial"
    );
    let user = settings(json!({"text":{"font":"__mypaper_missing_face__"}}));
    let tokens = resolve(&user, &BarsConfig::parse("{}").unwrap(), "top", &ui.text);
    assert!(ui.text.has_family(&tokens.font.family));
    let panel = crate::flyouts::look::Look::resolve_settings(
        &BarsConfig::parse(r#"{"top":{"look":{"font":{"family":"Segoe UI"}}}}"#)
            .unwrap()
            .top
            .look,
        &Appearance::default(),
        &crate::flyouts::look::fixed_system(),
        &Default::default(),
        &user,
        &ui.text,
    );
    assert_eq!(
        panel.font.as_ref(),
        "Segoe UI",
        "a literal installed family survives popup resolution"
    );
}

#[test]
fn text_size_is_a_content_scale_and_projection_does_not_apply_it_twice() {
    let ui = Ui::new().unwrap();
    for size in [0.5, 1.5, 2.0] {
        let user = settings(json!({"text":{"size":size}}));
        let bars = BarsConfig::parse(
            &json!({"appearance":{"scale":size}, "top":{"look":{"font":{"size":18}}}}).to_string(),
        )
        .unwrap();
        let tokens = resolve(&user, &bars, "top", &ui.text);
        assert_eq!(tokens.scale, size);
        assert_eq!(tokens.font.scaled(tokens.scale).size, 18.0 * size);
    }
}

#[test]
fn text_shadow_only_changes_text_directly_on_the_wallpaper() {
    let system = crate::flyouts::look::fixed_system();
    let base = Appearance {
        shadow: Some(true),
        ..Default::default()
    };
    let user = settings(json!({"text":{"shadow":false}}));
    assert_eq!(
        appearance(&user, Family::Widgets, &base, true, &system).shadow,
        Some(false)
    );
    assert_eq!(
        appearance(&user, Family::Widgets, &base, false, &system).shadow,
        Some(true)
    );
}

#[test]
fn text_tone_only_changes_bare_labels_and_auto_restores_the_wallpaper() {
    let system = crate::flyouts::look::fixed_system();
    let base = Appearance {
        text: Some(TextTone::Light),
        ..Default::default()
    };
    let user = settings(json!({"text":{"tone":"dark"}}));
    assert_eq!(
        appearance(&user, Family::Widgets, &base, true, &system).text,
        Some(TextTone::Dark)
    );
    assert_eq!(
        appearance(&user, Family::Widgets, &base, false, &system).text,
        Some(TextTone::Light)
    );
    let user = settings(json!({"text":{"tone":"auto"}}));
    assert_eq!(
        appearance(&user, Family::Widgets, &base, true, &system).text,
        Some(TextTone::Light)
    );
}

#[test]
fn icons_select_lucide_or_fluent_independently_of_native_style() {
    let mut ui = Ui::new().unwrap();
    for (name, vector) in [("lucide", true), ("fluent", false)] {
        let user = settings(json!({"text":{"icons":name}}));
        let tokens = resolve(
            &user,
            &BarsConfig::parse(r#"{"look":{"style":"fluent"}}"#).unwrap(),
            "top",
            &ui.text,
        );
        assert_eq!(tokens.theme.icons == IconSet::Vector, vector);
        let mut panel = crate::flyouts::look::Look::default();
        panel.apply_settings(&user, &ui.text);
        assert_eq!(
            panel.menu_tokens().icon_set == Some(IconSet::Vector),
            vector
        );
        let items = vec![
            mypaper_ui::Entry::action("Settings", ())
                .with_icon(mypaper_ui::MenuIcon::Glyph('\u{E713}')),
        ];
        let tokens = panel.menu_tokens();
        let layout = mypaper_ui::menu::layout(&items, &tokens, 1.0, &mut ui.text);
        let list = mypaper_ui::menu::view(&items, &layout, None, &tokens);
        assert_eq!(
            list.ops.iter().any(|op| matches!(
                op,
                Op::Icon {
                    icon: mypaper_ui::icons::Icon::Settings,
                    ..
                }
            )),
            vector
        );
    }
}

#[test]
fn bars_family_style_beats_the_global_style_and_explicit_bar_style_beats_both() {
    let ui = Ui::new().unwrap();
    let user = settings(json!({"style":"liquid", "bars":{"style":"solid"}}));
    let bars = BarsConfig::parse("{}").unwrap();
    assert_eq!(
        resolve(&user, &bars, "top", &ui.text).theme.style,
        mypaper_ui::Style::Solid
    );
    let solid = resolve(&user, &bars, "top", &ui.text);
    assert_eq!(solid.fill.unwrap_or(solid.theme.color.surface).a, 1.0);
    let tinted = BarsConfig::parse(r#"{"top":{"look":{"tint":0.4}}}"#).unwrap();
    assert_eq!(resolve(&user, &tinted, "top", &ui.text).fill.unwrap().a, 0.4);
    let bars = BarsConfig::parse(r#"{"top":{"look":{"style":"fluent"}}}"#).unwrap();
    assert_eq!(
        resolve(&user, &bars, "top", &ui.text).theme.style,
        mypaper_ui::Style::Fluent
    );
}

#[test]
fn menus_family_style_is_independent_of_bars_and_resets_to_the_global_style() {
    let ui = Ui::new().unwrap();
    let bars = BarsConfig::parse(r#"{"top":{"look":{"style":"fluent"}}}"#).unwrap();
    let system = crate::flyouts::look::fixed_system();
    for (user, expected) in [
        (
            settings(json!({"style":"liquid", "bars":{"style":"solid"}, "menus":{"style":"mono"}})),
            mypaper_ui::Style::Mono,
        ),
        (
            settings(json!({"style":"liquid", "bars":{"style":"solid"}})),
            mypaper_ui::Style::Liquid,
        ),
        (
            settings(json!({"style":"liquid", "menus":{"style":"solid"}})),
            mypaper_ui::Style::Solid,
        ),
    ] {
        let mut own = bars.top.look.clone();
        let mut a = Appearance::default();
        bar(
            &user,
            Family::Menus,
            &bars,
            "top",
            &mut own,
            &mut a,
            &system,
            &ui.text,
        );
        let panel = crate::flyouts::look::Look::resolve(&own, &a, &system);
        assert_eq!(panel.style, expected);
        if expected == mypaper_ui::Style::Solid { assert_eq!(panel.fill.a, 1.0); }
    }
}

#[test]
fn menus_share_explicit_radius_and_edge_with_their_glass_shape() {
    let mut ui = Ui::new().unwrap();
    let user = settings(json!({"shape":{"corners":0,"edge":true}, "menus":{"style":"solid"}}));
    let bars = BarsConfig::parse("{}").unwrap();
    let system = crate::flyouts::look::fixed_system();
    let mut bar_look = bars.top.look.clone();
    let mut a = Appearance::default();
    bar(
        &user,
        Family::Menus,
        &bars,
        "top",
        &mut bar_look,
        &mut a,
        &system,
        &ui.text,
    );
    let mut panel = crate::flyouts::look::Look::resolve(&bar_look, &a, &system);
    panel.apply_settings(&user, &ui.text);
    assert_eq!(panel.radius, 0.0);
    assert_eq!(panel.menu_tokens().radius, 0.0);
    assert!(panel.menu_tokens().border);
    assert!(panel.glass.is_none());
    let items = vec![mypaper_ui::Entry::action("Settings", ())];
    let tokens = panel.menu_tokens();
    let layout = mypaper_ui::menu::layout(&items, &tokens, 1.0, &mut ui.text);
    let list = mypaper_ui::menu::view(&items, &layout, None, &tokens);
    assert!(
        list.ops
            .iter()
            .any(|op| matches!(op, Op::Stroke { radius, .. } if *radius == 0.0))
    );
}
