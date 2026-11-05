use super::*;
use mypaper_ui::calendar::Date;
use mypaper_ui::{DisplayList, Op, Point, golden, render};

fn fixture(id: &str, root: PathBuf) -> Model {
    let registry = registry::builtins();
    let manifest = &registry.get(id).unwrap().manifest;
    let mut layout = Layout::default();
    let instance = layout.add(id, "fixture", [0, 0], manifest.size.default);
    let grid = Grid::new(
        &layout.grid,
        [96.0; 2],
        RectF {
            x: 0.0,
            y: 0.0,
            width: 2560.0,
            height: 1440.0,
        },
    );
    let appearance = look::defaults();
    let placed = layout::place(layout.get(&instance).unwrap(), manifest, &grid, &appearance);
    let mut m = Model::new(placed, &mut Storage::new(root), now()).unwrap();
    m.visible = true;
    if m.kind == Widget::Calendar {
        m.placed
            .settings
            .insert("weekStart".into(), json!("monday"));
    }
    m.note =
        "# A quiet place\nIdeas, reminders and things to build.\n\nمرحبا بالعالم\n日本語のメモ"
            .into();
    m.tasks = vec![
        Task {
            text: "Plan the native desktop".into(),
            done: true,
        },
        Task {
            text: "Write something useful".into(),
            done: false,
        },
        Task {
            text: "Take a break".into(),
            done: false,
        },
    ];
    m.data = match m.kind {
        Widget::Media => {
            json!({"title":"A Walk in the Rain","artist":"City Lights","playing":false,"position":68,"duration":213})
        }
        Widget::Weather => {
            json!({"current":{"location":{"name":"Oslo"},"temperature":27,"condition":"Clear sky","code":0},"forecast":{"days":[{"date":"2026-11-04","max":28,"min":22},{"date":"2026-11-05","max":29,"min":23},{"date":"2026-11-06","max":28,"min":21}]}})
        }
        Widget::Rss => {
            json!({"items":[{"title":"A simpler native desktop takes shape","source":"mypaper","published":1793790000,"link":"https://example.com/one"},{"title":"Software rendering, small surfaces and a sleeping event loop","source":"Engineering","published":1793788000,"link":"https://example.com/two"}]})
        }
        Widget::Crypto => {
            json!({"coins":[{"id":"bitcoin","name":"Bitcoin","price":71543.25,"change":1.42},{"id":"ethereum","name":"Ethereum","price":2641.80,"change":-0.81},{"id":"solana","name":"Solana","price":164.55,"change":2.35}]})
        }
        _ => Value::Null,
    };
    m.stats = Some(mypaper_widgets::stats::SystemStats {
        cpu: 23.0,
        memory: mypaper_widgets::stats::Usage {
            used: 7_000_000_000,
            total: 16_000_000_000,
        },
        gpu: Some(12.0),
        net: Some(mypaper_widgets::stats::Net {
            up: 14000.0,
            down: 280000.0,
        }),
        disks: None,
        battery: None,
        uptime: 12345.0,
        network: None,
    });
    for i in 0..80 {
        m.counts
            .insert(now().date.add_days(-i).key(), (i % 6) as u32);
    }
    m.photo.current = Some(Arc::new(mypaper_ui::Image {
        width: 320,
        height: 200,
        pixels: (0..200)
            .flat_map(|y| {
                (0..320).flat_map(move |x| {
                    [
                        (80 + y / 3) as u8,
                        (60 + x / 4) as u8,
                        (30 + y / 4) as u8,
                        255,
                    ]
                })
            })
            .collect(),
    }));
    m
}
fn now() -> Stamp {
    Stamp {
        date: Date {
            year: 2026,
            month: 11,
            day: 4,
        },
        hour: 14,
        minute: 37,
        second: 0,
        ms: 1793792220000,
    }
}
fn temp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("mypaper-n4-{name}-{}", std::process::id()))
}

#[test]
fn every_native_widget_has_a_golden_and_hit_testable_body() {
    let mut ui = Ui::new().unwrap();
    let root = temp("goldens");
    let stamp = now();
    let instant = Instant::now();
    for id in registry::builtins().widgets.keys() {
        let m = fixture(id, root.clone());
        let size = match m.kind {
            Widget::Clock => (320, 180),
            Widget::Calendar => (320, 310),
            Widget::Quote | Widget::Countdown => (320, 128),
            _ => (360, 240),
        };
        let tokens = tokens(&m.placed.appearance);
        let mut list = view::view(
            &m,
            &tokens,
            view::Frame {
                size,
                scale: 1.0,
                now: stamp,
                instant,
                editing: false,
            },
            &mut ui.text,
        );
        assert!(
            list.hits.iter().any(|h| h.id == view::BODY),
            "{id}: body hit"
        );
        let image = render(&mut ui, &list, size.0, size.1).unwrap();
        for y in 24..size.1 - 24 {
            for x in 24..size.0 - 24 {
                assert!(image.pixel(x, y)[3] >= 1, "{id}: alpha hole at {x},{y}");
            }
        }
        // A neutral backdrop makes the transparent content readable in the reviewed PNG.
        list.ops.insert(
            0,
            Op::Fill {
                rect: Rect::new(0.0, 0.0, size.0 as f32, size.1 as f32),
                radius: 0.0,
                color: Rgba::rgb8([36, 43, 53], 1.0),
            },
        );
        let image = render(&mut ui, &list, size.0, size.1).unwrap();
        golden::check(
            &image,
            &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("src/widgets/golden/{id}.png")),
        )
        .unwrap();
    }
}

#[test]
fn scheduled_ticks_stop_when_hidden_and_minute_clock_never_ticks_faster() {
    let instant = Instant::now();
    let now = now();
    let mut clock = fixture("clock", temp("scheduling"));
    clock.schedule(now, instant);
    assert_eq!(clock.next, Some(instant + Duration::from_secs(60)));
    clock.visible = false;
    clock.schedule(now, instant);
    assert_eq!(clock.next, None);
    let mut timer = fixture("timer", temp("scheduling"));
    timer.schedule(now, instant);
    assert_eq!(timer.next, None);
    timer.timer.ends_at = Some(now.ms + 125_000);
    timer.schedule(now, instant);
    assert_eq!(timer.next, Some(instant + Duration::from_secs(1)));
    let mut media = fixture("now-playing", temp("media-scheduling"));
    media.schedule(now, instant);
    assert_eq!(media.next, None, "paused media has no progress timer");
    media.data["playing"] = json!(true);
    media.schedule(now, instant);
    assert_eq!(
        media.next, None,
        "playing media changes only on SMTC events"
    );
    let mut system = fixture("system-monitor", temp("scheduling"));
    system.schedule(now, instant);
    assert_eq!(
        system.next, None,
        "stats service owns the 2 s sampling deadline"
    );
    let mut photo = fixture("photo", temp("scheduling"));
    photo.schedule(now, instant);
    assert_eq!(photo.next, None, "a single still photo has no frame timer");
}

#[test]
fn existing_notes_todo_timer_and_replacement_ids_load_without_rewriting_storage() {
    let root = temp("old-data");
    let mut storage = Storage::new(root.clone());
    storage
        .set("notes-1", "text", Some(json!("مرحبا · 日本語 · 한글")))
        .unwrap();
    storage
        .set(
            "todo-1",
            "items",
            Some(json!([{"text":"A saved task","done":true}])),
        )
        .unwrap();
    storage.set("timer-1","state",Some(json!({"mode":"focus","total":1500000,"remaining":450000,"endsAt":null,"finishedAt":null,"sessions":3}))).unwrap();
    let mut layout=Layout::parse(r#"{"instances":[{"instance":"notes-1","widget":"notes","monitor":"gone","cell":[0,0],"size":[2,2]},{"instance":"todo-1","widget":"todo","monitor":"gone","cell":[2,0],"size":[2,2]},{"instance":"timer-1","widget":"timer","monitor":"gone","cell":[4,0],"size":[2,2]},{"instance":"analog-clock-1","widget":"analog-clock","monitor":"gone","cell":[6,0],"size":[2,2]}]}"#).unwrap();
    let registry = registry::builtins();
    assert!(layout.migrate(|id| registry.replacement(id)));
    assert_eq!(layout.instances[3].instance, "analog-clock-1");
    assert_eq!(layout.instances[3].settings["variant"], "analog");
    let grid = Grid::new(
        &layout.grid,
        [96.0; 2],
        RectF {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
        },
    );
    let models: Vec<_> = layout
        .instances
        .iter()
        .map(|i| {
            Model::new(
                layout::place(
                    i,
                    &registry.get(&i.widget).unwrap().manifest,
                    &grid,
                    &look::defaults(),
                ),
                &mut storage,
                now(),
            )
            .unwrap()
        })
        .collect();
    assert_eq!(models[0].note, "مرحبا · 日本語 · 한글");
    assert!(models[1].tasks[0].done);
    assert_eq!(models[2].timer.remaining, 450_000);
    assert_eq!(models[2].timer.sessions, 3);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn widget_actions_have_click_targets_and_edit_controls_cover_their_shapes() {
    let mut ui = Ui::new().unwrap();
    let instant = Instant::now();
    for (id, targets) in [
        ("clock", vec![view::BODY]),
        ("calendar", vec![view::PREVIOUS, view::NEXT, view::PRIMARY]),
        ("countdown", vec![view::BODY]),
        ("system-monitor", vec![view::BODY]),
        (
            "media-pill",
            vec![view::PRIMARY, view::PREVIOUS, view::NEXT],
        ),
        ("weather", vec![view::PRIMARY]),
        ("photo", vec![view::PRIMARY]),
        ("notes", vec![view::FIELD]),
        ("rss", vec![view::ITEM]),
        ("crypto", vec![view::ITEM]),
        ("heatmap", vec![view::ITEM]),
        (
            "now-playing",
            vec![view::PRIMARY, view::PREVIOUS, view::NEXT],
        ),
        ("timer", vec![view::PRIMARY, view::RESET, view::NEXT]),
        (
            "todo",
            vec![view::ITEM, view::EDIT_TASK, view::DELETE_TASK, view::FIELD],
        ),
        ("launcher", vec![view::ITEM, view::FIELD]),
        ("quote", vec![view::PRIMARY]),
        ("quick-toggles", vec![view::ITEM, view::ITEM + 7]),
    ] {
        let m = fixture(id, temp("hits"));
        let list = view::view(
            &m,
            &tokens(&m.placed.appearance),
            view::Frame {
                size: (420, 300),
                scale: 1.0,
                now: now(),
                instant,
                editing: false,
            },
            &mut ui.text,
        );
        for target in targets {
            let hit = list
                .hits
                .iter()
                .find(|h| h.id == target)
                .unwrap_or_else(|| panic!("{id}: hit {target}"));
            let p = Point::new(
                hit.rect.left + hit.rect.width() / 2.0,
                hit.rect.top + hit.rect.height() / 2.0,
            );
            assert_eq!(
                list.hit_at(p).unwrap().id,
                target,
                "{id}: target overlapped"
            );
        }
        let list = view::view(
            &m,
            &tokens(&m.placed.appearance),
            view::Frame {
                size: (420, 300),
                scale: 1.0,
                now: now(),
                instant,
                editing: true,
            },
            &mut ui.text,
        );
        for id in [view::REMOVE, view::DUPLICATE, view::RESIZE] {
            assert!(list.hits.iter().any(|h| h.id == id));
        }
    }
}

#[test]
fn desktop_kind_is_a_top_level_layered_tool_window() {
    use windows::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GWL_STYLE, GetParent, GetWindowLongPtrW, WS_CHILD, WS_EX_LAYERED,
        WS_EX_TOOLWINDOW, WS_POPUP,
    };
    let mut ui = Ui::new().unwrap();
    let parent = Surface::new(
        Kind::Popup { activate: false },
        "Hidden test parent",
        Role::Pane,
    )
    .unwrap();
    let mut child = Surface::new(
        Kind::DesktopChild {
            parent: parent.hwnd(),
        },
        "Hidden widget child",
        Role::Pane,
    )
    .unwrap();
    // SAFETY: read-only style/parent queries on windows owned by this test.
    unsafe {
        assert!(GetParent(child.hwnd()).is_err());
        assert_eq!(
            GetWindowLongPtrW(child.hwnd(), GWL_STYLE) as u32 & WS_CHILD.0,
            0
        );
        assert_ne!(
            GetWindowLongPtrW(child.hwnd(), GWL_STYLE) as u32 & WS_POPUP.0,
            0
        );
        assert_ne!(
            GetWindowLongPtrW(child.hwnd(), GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW.0,
            0
        );
        assert_ne!(
            GetWindowLongPtrW(child.hwnd(), GWL_EXSTYLE) as u32 & WS_EX_LAYERED.0,
            0
        );
    }
    let frame = DisplayList {
        ops: vec![Op::Fill {
            rect: Rect::new(0.0, 0.0, 40.0, 40.0),
            radius: 8.0,
            color: Rgba::HIT,
        }],
        ..Default::default()
    };
    assert!(
        child
            .present(&mut ui, (0, 0), (40, 40), frame.clone())
            .unwrap(),
        "ULW succeeds for a top-level desktop surface"
    );
    assert!(
        !child.present(&mut ui, (0, 0), (40, 40), frame).unwrap(),
        "identical child content never presents"
    );
}

#[test]
#[ignore = "read-only compatibility check against this user's actual layout and data"]
fn existing_user_layout_and_data_load_read_only() {
    let layout = Layout::load(&files::layout().unwrap()).unwrap();
    let registry = registry::builtins();
    let monitors = monitor_infos();
    let primary = monitors.iter().find(|m| m.primary).unwrap();
    let mut storage = Storage::new(files::user("widget-data").unwrap());
    let mut loaded = 0;
    for instance in &layout.instances {
        let Some(installed) = registry.get(&instance.widget) else {
            continue;
        };
        let monitor = monitors
            .iter()
            .find(|m| m.device == instance.monitor)
            .unwrap_or(primary);
        let placed = layout::place(
            instance,
            &installed.manifest,
            &grid_for(&layout, monitor),
            &look::defaults(),
        );
        Model::new(placed, &mut storage, Stamp::now()).unwrap();
        loaded += 1;
    }
    println!(
        "read-only existing layout: {loaded}/{} native instances loaded",
        layout.instances.len()
    );
    assert_eq!(loaded, layout.instances.len());
}
