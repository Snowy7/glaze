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
        ("oneui", NativeStyle::OneUi, Family::Material, Scheme::Dark),
        ("fluent", NativeStyle::Fluent, Family::Fluent, Scheme::Dark),
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

/// Every preset in every style, top bar and dock, over a wallpaper, one PNG per preset and
/// style. Glass is a stand-in (the style's tint at its strength, a rim line where it has a
/// highlight); layout, sizes and which material each style picks are exact.
#[test]
#[ignore = "writes images: MYPAPER_BAR_GRID=<dir> [MYPAPER_BAR_BACKDROP=<image>]"]
fn preset_style_grid() {
    let Ok(dir) = std::env::var("MYPAPER_BAR_GRID") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    let mut ui = Ui::new().unwrap();
    let backdrop = std::env::var("MYPAPER_BAR_BACKDROP")
        .ok()
        .and_then(|p| Image::load(&ui, std::path::Path::new(&p)).ok())
        .map(std::sync::Arc::new);
    let mut model = model();
    model.notifications = None;
    let (width, height) = (1280_u32, 300_u32);
    for preset in mypaper_bars::presets::Preset::ALL {
        let preset_name = serde_json::to_value(preset).unwrap();
        for style in NativeStyle::ALL {
            let style_name = serde_json::to_value(style).unwrap();
            let mut config = BarsConfig::parse(&format!(
                r#"{{"preset":{preset_name},"look":{{"style":{style_name},"scheme":"dark"}},
                    "top":{{"enabled":true}},"dock":{{"enabled":true,"extras":["trash"]}}}}"#
            ))
            .unwrap();
            config.resolve(Vec::new);
            let mut images = view::Images::default();
            let system = providers::accent::read();
            let top_tokens = look::Tokens::resolve(&config.top.look, &Appearance::default(), &system);
            let dock_tokens = look::Tokens::resolve(&config.dock.look, &Appearance::default(), &system);
            let top = view::top(width, &config, &model, &top_tokens, 1.0, &mut ui, &mut images, None);
            let dock = view::dock(
                &config, &model, &apps(), &dock_tokens, 1.0, (width, 120), &[], &[], &mut ui,
                &mut images, None,
            );
            let mut list = mypaper_ui::DisplayList::default();
            if let Some(image) = &backdrop {
                let k = (image.width as f32 / width as f32).min(image.height as f32 / height as f32);
                list.ops.push(Op::Image {
                    rect: mypaper_ui::Rect::new(0.0, 0.0, width as f32, height as f32),
                    image: image.clone(),
                    opacity: 1.0,
                    source: Some(mypaper_ui::Rect::new(0.0, 0.0, width as f32 * k, height as f32 * k)),
                });
            }
            let dock_y = height as f32 - dock.size.1 as f32;
            for (frame, tokens, dy) in [(&top, &top_tokens, 0.0), (&dock, &dock_tokens, dock_y)] {
                let dx = ((width - frame.size.0) / 2) as f32;
                if let Some(m) = tokens.material {
                    for &(rect, radius) in &frame.shapes {
                        // Stand-in glass: the tint at its strength, a frosted veil when clear.
                        let veil = (1.0 - m.clarity).max(m.tint.a).clamp(0.25, 0.92);
                        list.ops.push(Op::Fill {
                            rect: rect.offset(dx, dy),
                            radius,
                            color: m.tint.with_alpha(veil),
                        });
                        if m.highlight > 0.0 {
                            list.ops.push(Op::Stroke {
                                rect: rect.offset(dx, dy).inflate(-0.5),
                                radius,
                                width: 1.0,
                                color: Rgba::rgb8([255; 3], 0.35 * m.highlight),
                            });
                        }
                    }
                }
                list.ops.extend(frame.list.translated(dx, dy).ops);
            }
            let image = render(&mut ui, &list, width, height).unwrap();
            let name = format!(
                "{}-{}.png",
                preset_name.as_str().unwrap(),
                style_name.as_str().unwrap()
            );
            mypaper_ui::golden::write(&image, &dir.join(name)).unwrap();
        }
    }
}

/// The docs' headless renders (`docs/media/native/bar-*.png`): the macOS preset's top bar and
/// dock at 100 % and 150 %, dark and light, over `MYPAPER_BAR_BACKDROP` (a wallpaper PNG) with a
/// flat stand-in for the engine's glass. Run with `MYPAPER_BAR_SHOTS=<dir>`.
/// `MYPAPER_BAR_CONTENT_ONLY=1` draws only the bars' content on transparency, to lay over the
/// engine's real glass from its frame lab, and prints each bar's glass shapes and material.
#[test]
#[ignore = "writes images: MYPAPER_BAR_SHOTS=<dir> [MYPAPER_BAR_BACKDROP=<png>]"]
fn headless_bar_renders() {
    let Ok(dir) = std::env::var("MYPAPER_BAR_SHOTS") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    let mut ui = Ui::new().unwrap();
    let backdrop = std::env::var("MYPAPER_BAR_BACKDROP")
        .ok()
        .and_then(|p| Image::load(&ui, std::path::Path::new(&p)).ok())
        .map(std::sync::Arc::new);
    let mut model = model();
    model.notifications = None;
    for scheme in ["dark", "light"] {
        let mut config = BarsConfig::parse(&format!(
            r#"{{"preset":"macos","look":{{"style":"liquid","scheme":"{scheme}"}},
                "top":{{"left":["start","search","workspaces","app"],"center":[],
                        "right":["media","network","volume","quick","clock","power"]}},
                "dock":{{"enabled":true,"extras":["desktop","trash"],"magnify":false}}}}"#
        ))
        .unwrap();
        config.resolve(Vec::new);
        for scale in [1.0_f32, 1.5] {
            let width = (1280.0 * scale) as u32;
            let mut images = view::Images::default();
            let top_tokens = look::Tokens::resolve(
                &config.top.look,
                &Appearance::default(),
                &providers::accent::read(),
            );
            let dock_tokens = look::Tokens::resolve(
                &config.dock.look,
                &Appearance::default(),
                &providers::accent::read(),
            );
            let top = view::top(width, &config, &model, &top_tokens, scale, &mut ui, &mut images, None);
            let dock = view::dock(
                &config, &model, &apps(), &dock_tokens, scale, (width, 400), &[], &[], &mut ui,
                &mut images, None,
            );
            let content_only = std::env::var_os("MYPAPER_BAR_CONTENT_ONLY").is_some();
            for (name, frame, tokens) in [("top", top, &top_tokens), ("dock", dock, &dock_tokens)] {
                let (w, h) = frame.size;
                let mut list = mypaper_ui::DisplayList::default();
                if content_only {
                    eprintln!("{name} {scheme} {scale}: shapes {:?}, material {:?}", frame.shapes, tokens.material);
                    list.ops.extend(frame.list.ops.iter().cloned());
                    let image = render(&mut ui, &list, w, h).unwrap();
                    let pct = (scale * 100.0) as u32;
                    mypaper_ui::golden::write(&image, &dir.join(format!("content-{name}-{scheme}-{pct}.png")))
                        .unwrap();
                    continue;
                }
                if let Some(image) = &backdrop {
                    list.ops.push(Op::Image {
                        rect: mypaper_ui::Rect::new(0.0, 0.0, w as f32, h as f32),
                        image: image.clone(),
                        opacity: 1.0,
                        source: Some(mypaper_ui::Rect::new(0.0, 720.0, w as f32 / scale, h as f32 / scale)),
                    });
                }
                for &(rect, radius) in &frame.shapes {
                    let tint = tokens.theme.color.surface;
                    list.ops.push(Op::Fill { rect, radius, color: tint.with_alpha(0.55) });
                }
                list.ops.extend(frame.list.ops.iter().cloned());
                let image = render(&mut ui, &list, w, h).unwrap();
                let pct = (scale * 100.0) as u32;
                mypaper_ui::golden::write(&image, &dir.join(format!("bar-{name}-{scheme}-{pct}.png")))
                    .unwrap();
            }
        }
    }
}

/// The README's desktop scene, bars half (`docs/media/readme`): the macOS preset's top bar and
/// dock on a 2560×1440 screen at 150 %, dark, content only on transparency, plus their glass
/// plates in the frame lab's `MYPAPER_LAB_PLATES` form (each bar's own material). Run with
/// `MYPAPER_README_SCENE=<dir>` (the widgets half is in `widgets::tests`).
#[test]
#[ignore = "writes images: MYPAPER_README_SCENE=<dir>"]
fn readme_scene_bars() {
    let Ok(dir) = std::env::var("MYPAPER_README_SCENE") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    let (width, height, scale) = (2560_u32, 1440_u32, 1.5_f32);
    let mut ui = Ui::new().unwrap();
    let mut model = model();
    model.notifications = None;
    // The widgets half's clock (`widgets::tests::now`), so the bar and the clock widget agree.
    (model.clock.wHour, model.clock.wMinute) = (14, 37);
    let mut config = BarsConfig::parse(
        r#"{"preset":"macos","look":{"style":"liquid","scheme":"dark"},
            "dock":{"enabled":true,"extras":["desktop","trash"],"magnify":false}}"#,
    )
    .unwrap();
    config.resolve(Vec::new);
    let system = providers::accent::read();
    let top_tokens = look::Tokens::resolve(&config.top.look, &Appearance::default(), &system);
    let dock_tokens = look::Tokens::resolve(&config.dock.look, &Appearance::default(), &system);
    let mut images = view::Images::default();
    let top = view::top(width, &config, &model, &top_tokens, scale, &mut ui, &mut images, None);
    let dock = view::dock(
        &config, &model, &readme_apps(&dir), &dock_tokens, scale, (width, 400), &[], &[], &mut ui,
        &mut images, None,
    );
    let mut list = mypaper_ui::DisplayList::default();
    let mut plates = String::new();
    let dock_y = height as f32 - dock.size.1 as f32;
    for (frame, tokens, dy) in [(&top, &top_tokens, 0.0), (&dock, &dock_tokens, dock_y)] {
        let dx = ((width - frame.size.0) / 2) as f32;
        // The bar's own material where it resolves one (blur in physical pixels), else the
        // engine's default glass.
        let m = tokens.material.map_or_else(mypaper_glass_ipc::Material::default, |m| {
            mypaper_glass_ipc::Material {
                clarity: m.clarity,
                blur: m.blur * scale,
                refraction: m.refraction,
                highlight: m.highlight,
                tint: [m.tint.r, m.tint.g, m.tint.b, m.tint.a],
            }
        });
        for &(rect, radius) in &frame.shapes {
            let (r, [tr, tg, tb, ta]) = (rect.offset(dx, dy), m.tint);
            plates.push_str(&format!(
                "{},{},{},{},{radius},{},{},{},{},{tr},{tg},{tb},{ta}\n",
                r.left,
                r.top,
                r.width(),
                r.height(),
                m.clarity,
                m.blur,
                m.refraction,
                m.highlight
            ));
        }
        list.ops.extend(frame.list.translated(dx, dy).ops);
    }
    let image = render(&mut ui, &list, width, height).unwrap();
    std::fs::create_dir_all(&dir).unwrap();
    mypaper_ui::golden::write(&image, &dir.join("bars.png")).unwrap();
    std::fs::write(dir.join("bars.plates"), plates).unwrap();
}

/// The README dock's apps: stock Windows programs and Studio, with the icons the shell draws for
/// them (written as PNGs under `dir/icons`); an icon that can't be read drops its app, so the
/// dock never shows a letter placeholder.
fn readme_apps(dir: &std::path::Path) -> Vec<Item> {
    let _apartment = mypaper_desktop::desktop_wallpaper::Apartment::enter().unwrap();
    let studio = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/logo/png/mark-256.png");
    let apps = [
        ("File Explorer", r"C:\Windows\explorer.exe", true, true),
        ("Microsoft Edge", r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe", true, true),
        ("PowerShell", r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe", true, true),
        ("Notepad", r"C:\Windows\notepad.exe", true, false),
        ("Task Manager", r"C:\Windows\System32\Taskmgr.exe", true, false),
    ];
    let mut items: Vec<Item> = apps
        .into_iter()
        .enumerate()
        .filter_map(|(i, (name, exe, pinned, open))| {
            let icon = crate::desktop_menu::shell_menu::file_icon(std::path::Path::new(exe), 256)?;
            let png = dir.join("icons").join(format!("{i}.png"));
            mypaper_ui::golden::write(&icon, &png).ok()?;
            Some(Item {
                key: name.into(),
                name: name.into(),
                path: exe.into(),
                icon: Some(png.display().to_string()),
                pinned,
                windows: if open {
                    vec![ItemWindow {
                        hwnd: i as isize + 1,
                        title: name.into(),
                        active: i == 0,
                        minimized: false,
                        attention: false,
                    }]
                } else {
                    vec![]
                },
                aumid: None,
                exe: None,
            })
        })
        .collect();
    items.push(Item {
        key: "mypaper Studio".into(),
        name: "mypaper Studio".into(),
        path: "mypaper-studio.exe".into(),
        icon: Some(studio.display().to_string()),
        pinned: true,
        windows: vec![],
        aumid: None,
        exe: None,
    });
    items
}
