//! Goldens and alpha/hit tests use precisely the display lists the live surfaces present.
use super::*;
use mypaper_bars::{
    apps::{Item, ItemWindow},
    style::{Family, NativeStyle, Scheme},
};
use mypaper_ui::{Image, Op, Rgba, render};

#[test]
fn studio_fields_reach_the_shared_preview_and_native_bar_views() {
    use crate::preview::bars as shared;
    let mut ui = Ui::new().unwrap();
    let mut config = BarsConfig::parse(r##"{
        "top":{"left":["app"],"center":[],"right":[]},
        "modules":{"app":{"style":{"size":18,"weight":800,"color":"#112233","border":{"width":3}}}},
        "dock":{"style":"taskbar","titles":true,"button":212,"pinned":[]}
    }"##).unwrap();
    config.resolve(Vec::new);
    let system = providers::accent::read();
    let tokens = shared::Tokens::resolve(&config.top.look, &Appearance::default(), &system);
    let mut model = shared::sample_model(model().clock, "Studio fields");
    model.app_icon = None;
    let mut images = shared::Images::default();
    let top = shared::top(1280, &config, &model, &tokens, 1.0, &mut ui, &mut images, None);
    assert!(top.list.ops.iter().any(|op| matches!(op, Op::Text { text, style, color, .. }
        if text.as_ref() == "Studio fields" && style.weight == 800 && style.size == 18.0
        && *color == Rgba::rgb8([0x11,0x22,0x33], 1.0))));
    assert!(top.list.ops.iter().any(|op| matches!(op, Op::Stroke { width, color, .. }
        if *width == 3.0 && *color == tokens.theme.color.separator)), "a width-only module edge follows the bar");
    let dock = shared::dock(&config, &model, &apps(), &tokens, 1.0, (1280, 80), &[], &[], &mut ui, &mut images, None);
    let index = dock.targets.iter().position(|target| matches!(target, shared::Target::Dock(_))).unwrap();
    let hit = dock.list.hits.iter().find(|hit| hit.id == index as u32 + 1).unwrap();
    assert_eq!(hit.rect.width(), 212.0, "explicit button length wins over the titled default");
}

#[test]
fn module_status_tooltips_keep_provider_details_and_custom_failures() {
    let mut ui = Ui::new().unwrap();
    let mut model = model();
    model.network = Some(providers::network::NetworkInfo {
        kind: providers::network::Kind::Wifi,
        name: Some("Studio".into()),
        signal: Some(42),
    });
    model.custom.outputs.insert(
        "test".into(),
        providers::custom::Output {
            text: "ignored".into(),
            tooltip: Some("Command timed out".into()),
            error: true,
            ..Default::default()
        },
    );
    let mut config = BarsConfig::parse(
        r#"{"top":{"left":["network","volume","custom:test"],"center":[],"right":[]}}"#,
    )
    .unwrap();
    config.resolve(Vec::new);
    let tokens = look::Tokens::resolve(
        &config.top.look,
        &Appearance::default(),
        &providers::accent::read(),
    );
    let frame = view::top(
        1280,
        &config,
        &model,
        &tokens,
        1.0,
        &mut ui,
        &mut Default::default(),
        None,
    );
    assert!(
        frame
            .list
            .hits
            .iter()
            .any(|h| h.label.as_ref() == "Command timed out")
    );
    assert!(
        frame
            .list
            .hits
            .iter()
            .any(|h| h.label.contains("Studio") && h.label.contains("42%"))
    );
    assert!(
        frame
            .list
            .hits
            .iter()
            .any(|h| h.label.contains("Volume 50%"))
    );
}

#[test]
fn future_deadlines_never_turn_into_busy_polling() {
    assert_eq!(services::wait_ms(Duration::ZERO), 0);
    assert_eq!(services::wait_ms(Duration::from_nanos(1)), 1);
    assert_eq!(services::wait_ms(Duration::from_micros(1000)), 1);
    assert_eq!(services::wait_ms(Duration::from_micros(1001)), 2);
    assert_eq!(services::wait_ms(Duration::MAX), u32::MAX - 1);
}

#[test]
fn island_crops_route_clicks_to_original_targets_and_keep_focus_inside_their_tree() {
    let mut ui = Ui::new().unwrap();
    let mut config = BarsConfig::parse(r#"{"top":{"layout":"islands","left":["start"],"center":["clock"],"right":["quick","power"]}}"#).unwrap();
    config.resolve(Vec::new);
    let tokens = look::Tokens::resolve(
        &config.top.look,
        &Appearance::default(),
        &providers::accent::read(),
    );
    let mut frame = view::top(
        1280,
        &config,
        &model(),
        &tokens,
        1.0,
        &mut ui,
        &mut Default::default(),
        None,
    );
    let focused = frame.list.hits.last().unwrap().id;
    frame.list.focus = Some(focused);
    for (shape, _) in &frame.shapes {
        let (list, offset, _) = frame.crop(shape.inflate(1.0));
        assert_eq!(
            list.focus.is_some(),
            list.hits.iter().any(|h| h.id == focused)
        );
        for hit in &list.hits {
            let center = Point::new(
                (hit.rect.left + hit.rect.right) / 2.0,
                (hit.rect.top + hit.rect.bottom) / 2.0,
            );
            assert_eq!(
                frame
                    .target(Point::new(center.x + offset.x, center.y + offset.y))
                    .unwrap()
                    .0,
                hit.id
            );
        }
    }
}

#[test]
fn explicit_native_actions_bypass_custom_panels_and_hover_respects_overrides() {
    for action in [
        BuiltinAction::NativeStart,
        BuiltinAction::NativeNotifications,
        BuiltinAction::NativeQuickSettings,
    ] {
        assert_eq!(builtin_flyout(action), None);
    }
    assert_eq!(
        builtin_flyout(BuiltinAction::Calendar),
        Some(flyouts::Kind::Calendar)
    );
    let config = BarsConfig::parse(r#"{"modules":{"quick":{"click":{"left":"none"}}}}"#).unwrap();
    assert!(!opens_flyout(
        &Target::Module(ModuleRef::Builtin(Module::Quick)),
        &config
    ));
    assert!(opens_flyout(
        &Target::Module(ModuleRef::Builtin(Module::Clock)),
        &config
    ));
}

#[test]
fn liquid_material_uses_the_resolved_style_controls() {
    let config = BarsConfig::parse(r##"{"look":{"style":"liquid","glass":{"clarity":0.91,"tint":0.17,"blur":8,"refraction":0.3,"highlight":0.4}},"appearance":{"tint":"#123456"}}"##).unwrap();
    let tokens = look::Tokens::resolve(
        &config.top.look,
        &config.appearance,
        &providers::accent::read(),
    );
    let material = tokens.material.unwrap();
    assert_eq!(
        (
            material.clarity,
            material.blur,
            material.refraction,
            material.highlight
        ),
        (0.91, 8.0, 0.3, 0.4)
    );
    assert_eq!(material.tint, Rgba::rgb8([0x12, 0x34, 0x56], 0.17));
}

fn model() -> services::Model {
    let (tx, _) = std::sync::mpsc::channel();
    services::Model {
        foreground: Some(1),
        app_name: "File Explorer".into(),
        app_icon: None,
        media: None,
        volume: Some(providers::audio::Volume {
            level: 0.5,
            muted: false,
        }),
        network: None,
        battery: None,
        stats: None,
        workspaces: providers::desktops::Workspaces {
            count: 3,
            current: 0,
        },
        language: "ENG\nUS".into(),
        notifications: Some(2),
        tray: vec![],
        clock: windows::Win32::Foundation::SYSTEMTIME {
            wYear: 2026,
            wMonth: 11,
            wDay: 4,
            wDayOfWeek: 3,
            wHour: 13,
            wMinute: 24,
            ..Default::default()
        },
        custom: providers::custom::Custom::new(providers::wake::Waker::new(tx, 0)),
        trash_full: false,
    }
}
fn apps() -> Vec<Item> {
    ["Explorer", "Terminal", "Studio"]
        .into_iter()
        .enumerate()
        .map(|(i, name)| Item {
            key: name.into(),
            name: name.into(),
            path: format!("{name}.exe"),
            icon: None,
            pinned: i < 2,
            windows: if i == 2 {
                vec![]
            } else {
                vec![ItemWindow {
                    hwnd: i as isize + 1,
                    title: name.into(),
                    active: i == 0,
                    minimized: false,
                    attention: false,
                }]
            },
            aumid: None,
            exe: None,
        })
        .collect()
}

fn assert_shape_hit_pixels(frame: &view::Frame, image: &Image) {
    for &(rect, radius) in &frame.shapes {
        for y in rect.top.ceil() as u32..rect.bottom.floor().max(0.0) as u32 {
            for x in rect.left.ceil() as u32..rect.right.floor().max(0.0) as u32 {
                let px = x as f32 + 0.5;
                let py = y as f32 + 0.5;
                let dx = (radius - (px - rect.left).min(rect.right - px)).max(0.0);
                let dy = (radius - (py - rect.top).min(rect.bottom - py)).max(0.0);
                if dx * dx + dy * dy < (radius - 0.5).max(0.0).powi(2) || radius == 0.0 {
                    assert!(
                        image.pixel(x, y)[3] >= 1,
                        "surface body clicks through at {x},{y}"
                    );
                }
            }
        }
        assert!(
            frame.list.ops.iter().any(
                |op| matches!(op,Op::Fill { rect: r,color,.. } if *r == rect && *color == Rgba::HIT)
            ),
            "every shape explicitly fills HIT"
        );
    }
    for hit in &frame.list.hits {
        let p = Point::new(
            (hit.rect.left + hit.rect.right) / 2.0,
            (hit.rect.top + hit.rect.bottom) / 2.0,
        );
        assert!(
            frame.target(p).is_some(),
            "{} has no click target",
            hit.label
        );
        assert!(
            image.pixel(p.x as u32, p.y as u32)[3] > 0,
            "{} is invisible to Windows hit testing",
            hit.label
        );
    }
}

#[test]
fn bars_match_goldens_in_each_style_and_keep_glass_bodies_hit_testable() {
    let mut ui = Ui::new().unwrap();
    let model = model();
    let golden = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/bars");
    // Every kit style through the bar's look (look.rs), dark; and liquid light.
    for (name, style, family, scheme) in [
        ("liquid", NativeStyle::Liquid, Family::Macos, Scheme::Dark),
        ("liquid-light", NativeStyle::Liquid, Family::Macos, Scheme::Light),
        ("oneui", NativeStyle::Liquid, Family::Material, Scheme::Dark),
        ("fluent", NativeStyle::Liquid, Family::Fluent, Scheme::Dark),
        ("mono", NativeStyle::Mono, Family::Mono, Scheme::Dark),
        ("solid", NativeStyle::Solid, Family::Macos, Scheme::Dark),
    ] {
        let mut config = BarsConfig::parse(r#"{"top":{"layout":"floating","left":["start","workspaces","app"],"center":["clock"],"right":["network","volume","quick","power"]},"dock":{"extras":["trash"]}}"#).unwrap();
        config.resolve(Vec::new);
        for look in [&mut config.top.look, &mut config.dock.look] {
            look.style = Some(style);
            look.family = Some(family);
            look.scheme = Some(scheme);
        }
        let tokens = look::Tokens::resolve(
            &config.top.look,
            &Appearance::default(),
            &providers::accent::read(),
        );
        let mut images = view::Images::default();
        let top = view::top(
            1280,
            &config,
            &model,
            &tokens,
            1.0,
            &mut ui,
            &mut images,
            None,
        );
        let dock = view::dock(
            &config,
            &model,
            &apps(),
            &tokens,
            1.0,
            (1280, 80),
            &[],
            &[],
            &mut ui,
            &mut images,
            None,
        );
        for (surface, frame) in [("top", top), ("dock", dock)] {
            let image = render(&mut ui, &frame.list, frame.size.0, frame.size.1).unwrap();
            assert_shape_hit_pixels(&frame, &image);
            mypaper_ui::golden::check(&image, &golden.join(format!("{surface}-{name}.png")))
                .unwrap();
        }
    }
}

#[test]
fn islands_leave_the_gaps_click_through_and_minute_clock_damages_only_its_module() {
    let mut ui = Ui::new().unwrap();
    let mut model = model();
    let mut config = BarsConfig::parse(
        r#"{"top":{"layout":"islands","left":["start"],"center":["clock"],"right":["power"]}}"#,
    )
    .unwrap();
    config.resolve(Vec::new);
    let tokens = look::Tokens::resolve(
        &config.top.look,
        &Appearance::default(),
        &providers::accent::read(),
    );
    let mut images = view::Images::default();
    let a = view::top(
        1280,
        &config,
        &model,
        &tokens,
        1.0,
        &mut ui,
        &mut images,
        None,
    );
    let image = render(&mut ui, &a.list, a.size.0, a.size.1).unwrap();
    assert_shape_hit_pixels(&a, &image);
    let p = Point::new(250.0, 16.0);
    assert_eq!(image.pixel(p.x as u32, p.y as u32)[3], 0);
    assert!(a.target(p).is_none());
    let same = view::top(
        1280,
        &config,
        &model,
        &tokens,
        1.0,
        &mut ui,
        &mut images,
        None,
    );
    assert_eq!(same.list.damage(&a.list), None);
    model.clock.wMinute += 1;
    let b = view::top(
        1280,
        &config,
        &model,
        &tokens,
        1.0,
        &mut ui,
        &mut images,
        None,
    );
    let damage = b.list.damage(&a.list).unwrap();
    assert!(
        damage.width() < 250.0,
        "minute tick repaints the full bar: {damage:?}"
    );
}

#[test]
fn docks_on_all_edges_and_taskbar_module_clicks_have_alpha() {
    let mut ui = Ui::new().unwrap();
    let model = model();
    let apps = apps();
    for position in [
        config::DockPosition::Top,
        config::DockPosition::Bottom,
        config::DockPosition::Left,
        config::DockPosition::Right,
    ] {
        for style in [config::DockStyle::Floating, config::DockStyle::Taskbar] {
            let mut config = BarsConfig::parse("{}").unwrap();
            config.resolve(Vec::new);
            config.dock.position = position;
            config.dock.style = style;
            let tokens = look::Tokens::resolve(
                &config.dock.look,
                &Appearance::default(),
                &providers::accent::read(),
            );
            let mut images = view::Images::default();
            let frame = view::dock(
                &config,
                &model,
                &apps,
                &tokens,
                1.0,
                (1280, 1280),
                &[],
                &[],
                &mut ui,
                &mut images,
                None,
            );
            let image = render(&mut ui, &frame.list, frame.size.0, frame.size.1).unwrap();
            assert_shape_hit_pixels(&frame, &image);
        }
    }
}
