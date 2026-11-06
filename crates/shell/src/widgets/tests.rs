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
    m.events = vec![
        json!({"date":"2026-11-04","time":"09:30","title":"Design review","color":0}),
        json!({"date":"2026-11-04","time":"16:00","title":"Gym","color":1}),
        json!({"date":"2026-11-06","title":"Dad's birthday","color":4}),
        json!({"date":"2026-11-12","time":"19:00","title":"Dinner with Sam","color":2}),
    ];
    m.selected = now().date;
    m.month = now().date;
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
            let codes = [0, 2, 3, 61, 95, 71, 45];
            let days: Vec<_> = (0..7)
                .map(|i| json!({"date": now().date.add_days(i).key(), "max": 28 + i % 3, "min": 21 + i % 2, "code": codes[i as usize]}))
                .collect();
            let hourly: Vec<_> = (0..12)
                .map(|i| json!({"time": format!("2026-11-04T{:02}:00", (14 + i) % 24), "temperature": 27 - i / 3, "code": (i % 4).min(3), "is_day": i < 4}))
                .collect();
            json!({"current":{"location":{"name":"Oslo"},"temperature":27,"condition":"Clear sky","code":0},"forecast":{"days":days,"hourly":hourly}})
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

#[test]
#[ignore = "compares real meter repaint strategies under the desktop lock"]
fn profile_system_repaint_cost() {
    use windows::Win32::{
        Foundation::FILETIME,
        System::Threading::{GetCurrentProcess, GetProcessTimes},
    };
    let busy = || {
        let (mut a, mut b, mut k, mut u) = (
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
        );
        // SAFETY: read-only accounting of this test process into valid outputs.
        unsafe { GetProcessTimes(GetCurrentProcess(), &mut a, &mut b, &mut k, &mut u).unwrap() };
        let ticks = |t: FILETIME| (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime);
        ticks(k) + ticks(u)
    };
    let root = temp("meter-repaint-cost");
    std::fs::create_dir_all(&root).unwrap();
    let mut ui = Ui::new().unwrap();
    let mut m = fixture("system-monitor", root.clone());
    m.placed.appearance.insert("style".into(), json!("liquid"));
    let tokens = tokens(&m.placed.appearance);
    let size = (m.placed.size[0] * 96 - 8, m.placed.size[1] * 96 - 8);
    let frames: Vec<_> = (0..60)
        .map(|i| {
            let stats = m.stats.as_mut().unwrap();
            stats.cpu = (i % 12) as f64;
            stats.gpu = Some((17 + i % 7) as f64);
            stats.net = Some(mypaper_widgets::stats::Net {
                down: (i % 8) as f64 * 1000.0,
                up: (i % 5) as f64 * 1000.0,
            });
            view::view(
                &m,
                &tokens,
                view::Frame {
                    size,
                    scale: 1.0,
                    now: now(),
                    instant: Instant::now(),
                    editing: false,
                },
                &mut ui.text,
            )
        })
        .collect();
    for sparse in [false, true] {
        let mut surface = Surface::new(
            Kind::Popup { activate: false },
            "Widget repaint probe",
            Role::Pane,
        )
        .unwrap();
        for pass in 0..2 {
            let count = if pass == 0 { 120 } else { 1000 };
            let at = Instant::now();
            let before = busy();
            for i in 0..count {
                let list = frames[i % frames.len()].clone();
                if sparse {
                    surface.present_sparse(&mut ui, (0, 0), size, list).unwrap();
                } else {
                    surface.present(&mut ui, (0, 0), size, list).unwrap();
                }
            }
            if pass != 0 {
                println!(
                    "meter sparse={sparse} wall_us={} cpu_us={}",
                    at.elapsed().as_micros() / count as u128,
                    (busy() - before) / (count as u64 * 10)
                );
            }
        }
    }
    for capacity in [64, 192] {
        let mut ui = Ui::new().unwrap();
        ui.text.set_capacity(capacity);
        for pass in 0..2 {
            let count = if pass == 0 { 120 } else { 1000 };
            let at = Instant::now();
            let before = busy();
            for i in 0..count {
                let stats = m.stats.as_mut().unwrap();
                stats.cpu = (i % 12) as f64;
                stats.gpu = Some((17 + i % 7) as f64);
                stats.net = Some(mypaper_widgets::stats::Net {
                    down: (i % 60) as f64 * 1000.0,
                    up: (i % 17) as f64 * 1000.0,
                });
                std::hint::black_box(view::view(
                    &m,
                    &tokens,
                    view::Frame {
                        size,
                        scale: 1.0,
                        now: now(),
                        instant: Instant::now(),
                        editing: false,
                    },
                    &mut ui.text,
                ));
            }
            if pass != 0 {
                println!(
                    "meter view cache={capacity} wall_us={} cpu_us={}",
                    at.elapsed().as_micros() / count as u128,
                    (busy() - before) / (count as u64 * 10)
                );
            }
        }
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn media_timeline_updates_reuse_artwork_and_removing_art_clears_its_material() {
    let ui = Ui::new().unwrap();
    let root = temp("media-art-cache");
    let path = root.join("cover.png");
    let image = mypaper_ui::Image {
        width: 2,
        height: 2,
        pixels: [40, 80, 160, 255].repeat(4),
    };
    golden::write(&image, &path).unwrap();
    let uri = format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(std::fs::read(&path).unwrap())
    );
    let mut m = fixture("now-playing", root.clone());
    let mut data = json!({"artwork":uri,"position":0,"playing":true});
    let widget = Rect::new(0.0, 0.0, 300.0, 150.0);
    update_artwork(&mut m, &ui, &data, widget, 1.0);
    let art = m.artwork.clone().unwrap();
    let material = m.artwork_backdrop.clone().unwrap();
    // The blur backdrop has the plate's shape: its corners are cut, its middle is opaque.
    assert_eq!((material.width, material.height), (192, 96));
    assert_eq!(material.pixels[3], 0, "the corner is transparent");
    let middle = ((48 * 192 + 96) * 4 + 3) as usize;
    assert_eq!(material.pixels[middle], 255);
    m.data = data.clone();
    data["position"] = json!(30);
    data["playing"] = json!(false);
    update_artwork(&mut m, &ui, &data, widget, 1.0);
    assert!(Arc::ptr_eq(&art, m.artwork.as_ref().unwrap()));
    assert!(Arc::ptr_eq(&material, m.artwork_backdrop.as_ref().unwrap()));
    data["artwork"] = Value::Null;
    update_artwork(&mut m, &ui, &data, widget, 1.0);
    assert!(m.artwork.is_none() && m.artwork_backdrop.is_none() && m.artwork_tint.is_none());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_bleed_photo_is_cropped_to_the_body_and_rounded_like_the_plate() {
    let root = temp("photo-round");
    let m = fixture("photo", root.clone());
    let photo = mypaper_ui::Image { width: 300, height: 200, pixels: [90, 90, 90, 255].repeat(300 * 200) };
    // A square widget: a square body, so a square crop with transparent corners.
    let out = photo_bitmap(&m, Rect::new(0.0, 0.0, 200.0, 200.0), 1.0, photo);
    assert_eq!((out.width, out.height), (200, 200));
    assert_eq!(out.pixels[3], 0, "the corner is cut");
    assert_eq!(out.pixels[((100 * 200 + 100) * 4 + 3) as usize], 255);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn widget_styles_preserve_none_solid_and_explicit_text_controls() {
    let mut ui = Ui::new().unwrap();
    let mut m = fixture("quote", temp("appearance"));
    for style in ["minimal", "depth", "paper", "aero", "liquid"] {
        m.placed.appearance.insert("style".into(), json!(style));
        for surface in ["none", "solid"] {
            m.placed.appearance.insert("surface".into(), json!(surface));
            let tokens = tokens(&m.placed.appearance);
            let list = view::view(
                &m,
                &tokens,
                view::Frame {
                    size: (320, 128),
                    scale: 1.0,
                    now: now(),
                    instant: Instant::now(),
                    editing: false,
                },
                &mut ui.text,
            );
            let image = render(&mut ui, &list, 320, 128).unwrap();
            assert_eq!(
                image.pixel(8, 40)[3],
                if surface == "none" { 1 } else { 255 },
                "{style}/{surface}"
            );
            // The text tone picks the theme's scheme; its label is what the words are drawn in.
            let label = view::theme(&m, &tokens).color.label;
            assert!(
                list.ops
                    .iter()
                    .any(|op| matches!(op,Op::Text {color,..} if *color==label)),
                "{style}: explicit text colors survive"
            );
        }
    }
}

#[test]
fn explicit_widget_corners_and_edge_reach_the_drawn_body_even_for_pill_variants() {
    let mut ui = Ui::new().unwrap();
    let mut model = fixture("media-pill", temp("look-shape"));
    for radius in [0.0, 32.0] {
        model.corner_override = Some(radius);
        for edge in [false, true] {
            model.edge_override = Some(edge);
            let rect = Rect::new(0.0, 0.0, 320.0, 128.0);
            assert_eq!(view::plate_radius(&model, rect, 1.5), radius * 1.5);
            let list = view::view(&model, &tokens(&model.placed.appearance), view::Frame {
                size:(320,128), scale:1.0, now:now(), instant:Instant::now(), editing:false,
            }, &mut ui.text);
            assert!(list.ops.iter().any(|op| matches!(op, Op::Fill {rect: r, radius: actual, color} if *r == rect && *actual == radius && *color == Rgba::HIT)));
            assert_eq!(list.ops.iter().any(|op| matches!(op, Op::Stroke {rect:r,..} if *r == rect.inflate(-0.5))), edge);
        }
    }
}

#[test]
fn metric_invalidation_uses_displayed_precision_and_stable_trace_identity() {
    let mut m = fixture("system-monitor", temp("metrics"));
    let before = metrics::visible(&m);
    m.stats.as_mut().unwrap().cpu += 0.1;
    assert_eq!(
        before,
        metrics::visible(&m),
        "subprecision changes do not repaint"
    );
    m.stats.as_mut().unwrap().cpu += 1.0;
    assert_ne!(before, metrics::visible(&m));
    m.placed.settings.insert("cpu".into(), json!(false));
    let selected = metrics::visible(&m);
    assert_eq!(selected[0].id, "ram");
    m.history.insert("ram".into(), [12.0, 42.0].into());
    assert_eq!(m.history[&selected[0].id][1], 42.0);
    assert!(metrics::visible(&m).iter().any(|metric| metric.id == "net"));
}

#[test]
fn activity_credits_follow_insertions_and_deletions_and_long_break_resets_cycle() {
    let mut todo = fixture("todo", temp("activity-indices"));
    todo.counted.insert(0, "2026-11-03".into());
    todo.counted.insert(2, "2026-11-04".into());
    todo.insert_counted(0);
    assert_eq!(todo.counted[&1], "2026-11-03");
    todo.remove_counted(2);
    assert_eq!(todo.counted[&1], "2026-11-03");
    assert_eq!(todo.counted[&2], "2026-11-04");
    let mut timer = Timer::idle(&todo.placed.settings, "long", 4);
    timer = timer.enter(&todo.placed.settings, "focus");
    assert_eq!(timer.sessions, 0);
}

#[test]
fn event_editor_validates_date_time_and_preserves_legacy_wire_data() {
    let mut draft = model::EventDraft {
        title: "Appointment".into(),
        date: "2026-11-04".into(),
        time: "09:30".into(),
        color: 3,
        part: model::EventPart::Title,
    };
    let event = draft.event().unwrap();
    assert_eq!(event["color"], 3);
    assert_eq!(event["time"], "09:30");
    draft.time = "25:01".into();
    assert!(draft.event().is_err());
    draft.time = String::new();
    draft.date = "2026-02-30".into();
    assert!(draft.event().is_err());
    draft.date = "2026-02-28".into();
    assert!(draft.event().is_ok());
    assert_eq!(view::price_label(0.00001234, "usd"), "$0.0000123");
    assert_eq!(view::price_label(0.00000000001234, "usd"), "$1.23e-11");
    assert_eq!(view::price_label(1234.2, "jpy"), "¥1234");
}

#[test]
fn long_content_scrolls_to_the_last_item_without_hits_outside_the_viewport() {
    let mut ui = Ui::new().unwrap();
    for id in ["notes", "todo", "rss", "calendar", "launcher"] {
        let mut m = fixture(id, temp("scroll-content"));
        match m.kind {
            Widget::Notes => m.note = (0..80).map(|i| format!("* [ ] Task {i}\n")).collect(),
            Widget::Todo => {
                m.tasks = (0..80)
                    .map(|i| Task {
                        text: format!("Task {i}"),
                        done: false,
                    })
                    .collect()
            }
            Widget::Rss => {
                m.data["items"]=json!((0..30).map(|i|json!({"title":format!("Headline {i}"),"link":format!("https://example.com/{i}")})).collect::<Vec<_>>());
                m.placed.settings.insert("maxItems".into(), json!(30));
            }
            Widget::Calendar => {
                m.placed.settings.insert("variant".into(), json!("agenda"));
                m.events = (0..80)
                    .map(|i| json!({"date":"2026-11-05","title":format!("Event {i}")}))
                    .collect();
            }
            Widget::Launcher => {
                m.placed.settings.insert("variant".into(), json!("list"));
                m.placed.settings.insert("shortcuts".into(),json!((0..24).map(|i|json!({"label":format!("App {i}"),"target":"https://example.com"})).collect::<Vec<_>>()));
            }
            _ => unreachable!(),
        }
        let tokens = tokens(&m.placed.appearance);
        let frame = || view::Frame {
            size: (320, 240),
            scale: 1.0,
            now: now(),
            instant: Instant::now(),
            editing: false,
        };
        let first = view::view(&m, &tokens, frame(), &mut ui.text);
        assert!(m.scroll_max.get() > 0.0, "{id} has overflow");
        m.scroll = m.scroll_max.get();
        let last = view::view(&m, &tokens, frame(), &mut ui.text);
        assert!(last.damage(&first).is_some(), "{id}: scroll repaints");
        let expected = match m.kind {
            Widget::Notes => view::NOTE_TASK + 79,
            Widget::Todo => view::ITEM + 79,
            Widget::Rss => view::ITEM + 29,
            Widget::Calendar => view::EDIT_TASK + 79,
            Widget::Launcher => view::ITEM + 23,
            _ => unreachable!(),
        };
        assert!(
            last.hits.iter().any(|h| h.id == expected),
            "{id}: final item is reachable"
        );
        assert!(
            last.hits
                .iter()
                .all(|h| h.rect.top >= 0.0 && h.rect.bottom <= 240.0),
            "{id}: clipped hit geometry"
        );
    }
}
fn temp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("mypaper-n4-{name}-{}", std::process::id()))
}

/// A vertical gradient as an image (premultiplied BGRA), for a stand-in wallpaper.
fn gradient(width: u32, height: u32, top: [u8; 3], bottom: [u8; 3]) -> mypaper_ui::Image {
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        let t = y as f32 / height.max(2) as f32;
        let c = |i: usize| (top[i] as f32 + (bottom[i] as f32 - top[i] as f32) * t) as u8;
        for _ in 0..width {
            pixels.extend_from_slice(&[c(2), c(1), c(0), 255]);
        }
    }
    mypaper_ui::Image {
        width,
        height,
        pixels,
    }
}

/// One widget at one size in one scheme, over its plate's stand-in glass (the engine draws
/// the real glass): its content as an image and its plate's radius.
fn widget_shot(ui: &mut Ui, m: &mut Model, size: (u32, u32), dark: bool) -> (mypaper_ui::Image, f32) {
    m.placed
        .appearance
        .insert("text".into(), json!(if dark { "light" } else { "dark" }));
    let tokens = tokens(&m.placed.appearance);
    let list = view::view(
        m,
        &tokens,
        view::Frame {
            size,
            scale: 1.0,
            now: now(),
            instant: Instant::now(),
            editing: false,
        },
        &mut ui.text,
    );
    let rect = Rect::new(0.0, 0.0, size.0 as f32, size.1 as f32);
    (
        render(ui, &list, size.0, size.1).unwrap(),
        view::plate_radius(m, rect, 1.0),
    )
}

/// Paint that leaves a widget's frame (a pixel of antialiasing allowed): the first such op.
fn overflow(list: &DisplayList, size: (u32, u32)) -> Option<String> {
    let frame = Rect::new(0.0, 0.0, size.0 as f32, size.1 as f32).inflate(1.0);
    list.ops.iter().find_map(|op| {
        let (kind, rect) = match op {
            Op::Text { rect, text, .. } => (format!("text {text:?}"), *rect),
            Op::Emphasis { rect, text, .. } => (format!("text {text:?}"), *rect),
            Op::Fill { rect, .. } => ("fill".into(), *rect),
            Op::Stroke { rect, .. } => ("stroke".into(), *rect),
            Op::Icon { rect, .. } => ("icon".into(), *rect),
            Op::Image { rect, .. } | Op::RoundedImage { rect, .. } => ("image".into(), *rect),
            Op::Clipped { rect, .. } | Op::TextField { rect, .. } => ("group".into(), *rect),
            Op::Arc { center, radius, .. } => (
                "arc".into(),
                Rect::new(center.x - radius, center.y - radius, radius * 2.0, radius * 2.0),
            ),
            Op::Line { from, to, .. } => (
                "line".into(),
                Rect {
                    left: from.x.min(to.x),
                    top: from.y.min(to.y),
                    right: from.x.max(to.x),
                    bottom: from.y.max(to.y),
                },
            ),
            _ => return None,
        };
        // A text box is a line box: its leading may pass the frame, its glyphs may not.
        let rect = if kind.starts_with("text") {
            let lead = rect.height() * 0.15;
            Rect { top: rect.top + lead, bottom: rect.bottom - lead, ..rect }
        } else {
            rect
        };
        let outside = rect.left < frame.left
            || rect.top < frame.top
            || rect.right > frame.right
            || rect.bottom > frame.bottom;
        (outside && rect.width() > 0.0 && rect.height() > 0.0).then(|| {
            format!(
                "{kind} at {:.0},{:.0} {:.0}×{:.0}",
                rect.left,
                rect.top,
                rect.width(),
                rect.height()
            )
        })
    })
}

/// The size sheet as a check: every widget at every size from its min to its max (one cell at a
/// time), on the fine grid of a 96 DIP and a 75 DIP (Windows' default) icon cell, renders
/// without panicking and keeps its paint inside its frame. Integration views are listed, not
/// failed: other owners fix those (MYPAPER_SIZES=all fails them too).
#[test]
fn every_widget_renders_inside_its_frame_at_every_size() {
    let mut ui = Ui::new().unwrap();
    let root = temp("dense-sizes");
    let strict_all = std::env::var("MYPAPER_SIZES").is_ok_and(|v| v == "all");
    let integrations = ["t3"];
    let mut failures = Vec::new();
    let mut reported = Vec::new();
    for (id, installed) in &registry::builtins().widgets {
        let mut m = fixture(id, root.clone());
        let tokens = tokens(&m.placed.appearance);
        for size in installed.manifest.size.sizes([16, 16]) {
            m.placed.size = size;
            for (cell, gap) in [(44.0_f32, 8.0_f32), (33.5, 8.0)] {
                let px = |n: u32| (n as f32 * cell + (n - 1) as f32 * gap).round() as u32;
                let frame = (px(size[0]), px(size[1]));
                let list = view::view(
                    &m,
                    &tokens,
                    view::Frame {
                        size: frame,
                        scale: 1.0,
                        now: now(),
                        instant: Instant::now(),
                        editing: false,
                    },
                    &mut ui.text,
                );
                if let Some(what) = overflow(&list, frame) {
                    let line = format!("{id} {}×{} ({cell} DIP cells, {}×{} px): {what}", size[0], size[1], frame.0, frame.1);
                    if integrations.contains(&id.as_str()) && !strict_all {
                        reported.push(line);
                    } else {
                        failures.push(line);
                    }
                }
            }
        }
    }
    let _ = std::fs::remove_dir_all(root);
    for line in &reported {
        eprintln!("integration view overflows: {line}");
    }
    assert!(failures.is_empty(), "{} overflow(s):
{}", failures.len(), failures.join("
"));
}

/// Size sheets for review: every widget at every allowed size, dark over a night backdrop and
/// light over a day one, each on a stand-in for the engine's glass (its tint and shadow).
#[test]
#[ignore = "writes docs/media/native/widget-*.png; run when widget views change"]
fn widget_size_sheets() {
    // The fine grid of a 96 DIP icon cell.
    const CELL: u32 = 44;
    const GAP: u32 = 8;
    let px = |n: u32| n * CELL + (n - 1) * GAP;
    let mut ui = Ui::new().unwrap();
    let root = temp("sheets");
    let only = std::env::var("MYPAPER_SHEET").ok();
    let registry = registry::builtins();
    for (id, installed) in &registry.widgets {
        // The integration widgets keep their own goldens (`views/integrations`).
        let integration = ["t3"].contains(&id.as_str());
        if integration || only.as_deref().is_some_and(|o| o != id) {
            continue;
        }
        let mut sizes: Vec<_> = installed.manifest.size.sizes([16, 16]).collect();
        sizes.sort_by_key(|s| (s[1], s[0]));
        let rows: Vec<u32> = sizes.iter().map(|s| s[1]).collect::<BTreeSet<_>>().into_iter().collect();
        let margin = 24.0;
        let label = 22.0;
        let row_w = |h: u32| -> f32 {
            sizes
                .iter()
                .filter(|s| s[1] == h)
                .map(|s| px(s[0]) as f32 + margin)
                .sum::<f32>()
                + margin
        };
        let width = rows.iter().map(|&h| row_w(h)).fold(0.0, f32::max).ceil();
        let half = rows
            .iter()
            .map(|&h| px(h) as f32 + label + margin)
            .sum::<f32>()
            + margin;
        let height = (half * 2.0).ceil();
        let mut list = DisplayList::default();
        let mut m = fixture(id, root.clone());
        for (pass, dark) in [true, false].into_iter().enumerate() {
            let top = pass as f32 * half;
            let (from, to) = if dark {
                ([22, 30, 58], [40, 70, 96])
            } else {
                ([150, 190, 230], [228, 214, 196])
            };
            list.ops.push(Op::Image {
                rect: Rect::new(0.0, top, width, half),
                image: Arc::new(gradient(8, 64, from, to)),
                opacity: 1.0,
                source: None,
            });
            let theme = view::theme(&m, &tokens(&m.placed.appearance));
            let glass = if dark {
                Rgba::rgb8([28, 28, 30], 0.62)
            } else {
                Rgba::rgb8([245, 245, 247], 0.62)
            };
            let mut y = top + margin;
            for &h in &rows {
                let mut x = margin;
                for size in sizes.iter().filter(|s| s[1] == h) {
                    let (w, hh) = (px(size[0]), px(size[1]));
                    let (image, radius) = widget_shot(&mut ui, &mut m, (w, hh), dark);
                    let rect = Rect::new(x, y + label, w as f32, hh as f32);
                    list.ops.push(Op::Text {
                        rect: Rect::new(x + 2.0, y, 80.0, 18.0),
                        text: format!("{}×{}", size[0], size[1]).into(),
                        style: theme.font.style(mypaper_ui::TextRole::Footnote, 1.0),
                        color: if dark {
                            Rgba::new(1.0, 1.0, 1.0, 0.7)
                        } else {
                            Rgba::new(0.0, 0.0, 0.0, 0.6)
                        },
                    });
                    list.ops.push(Op::Shadow {
                        rect,
                        radius,
                        spread: 14.0,
                        drop: 4.0,
                        color: Rgba::new(0.0, 0.0, 0.0, if dark { 0.3 } else { 0.12 }),
                    });
                    list.ops.push(Op::Fill {
                        rect,
                        radius,
                        color: glass,
                    });
                    list.ops.push(Op::Image {
                        rect,
                        image: Arc::new(image),
                        opacity: 1.0,
                        source: None,
                    });
                    x += w as f32 + margin;
                }
                y += px(h) as f32 + label + margin;
            }
        }
        let image = render(&mut ui, &list, width as u32, height as u32).unwrap();
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../../docs/media/native/widget-{id}.png"));
        golden::write(&image, &path).unwrap();
        eprintln!("wrote {}", path.display());
    }
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn every_native_widget_has_a_golden_and_hit_testable_body() {
    let mut ui = Ui::new().unwrap();
    let root = temp("goldens");
    let stamp = now();
    let instant = Instant::now();
    for id in registry::builtins().widgets.keys() {
        eprintln!("Checking golden: {id}");
        let m = fixture(id, root.clone());
        if m.kind == Widget::Plugin {
            continue;
        }
        let size = match m.kind {
            Widget::Clock => (320, 180),
            Widget::Calendar => (320, 310),
            Widget::Media if id == "media-pill" => (360, 96),
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
        let radius = view::plate_radius(&m, Rect::new(0.0, 0.0, size.0 as f32, size.1 as f32), 1.0)
            .min(size.0 as f32 / 2.0)
            .min(size.1 as f32 / 2.0);
        for y in 24..size.1 - 24 {
            for x in 24..size.0 - 24 {
                let px = x as f32 + 0.5;
                let py = y as f32 + 0.5;
                let dx = px - px.clamp(radius, size.0 as f32 - radius);
                let dy = py - py.clamp(radius, size.1 as f32 - radius);
                if dx * dx + dy * dy > radius * radius {
                    continue;
                }
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
    assert_eq!(
        clock.next,
        Some(instant + Duration::from_secs(60) + time::CLOCK_SLACK)
    );
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
        media.next,
        Some(instant + Duration::from_secs(1) + time::CLOCK_SLACK),
        "visible elapsed/progress interpolates without polling SMTC"
    );
    media.data["duration"] = Value::Null;
    media.schedule(now, instant);
    assert_eq!(
        media.next, None,
        "unsupported/live timelines need no interpolation tick"
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
        WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP,
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
        assert_eq!(
            GetWindowLongPtrW(child.hwnd(), GWL_EXSTYLE) as u32 & WS_EX_NOACTIVATE.0,
            0,
            "clicks activate keyboard editing; the position guard preserves desktop order"
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

/// The event editor once gave its field and its "Title: …" button the same id; AccessKit
/// panics on a duplicate child and the release build aborted (0xC0000409) as soon as a UI
/// Automation client read the widget. Every calendar view gives each hit its own id.
#[test]
fn calendar_views_and_the_event_editor_give_every_hit_its_own_id() {
    let mut ui = Ui::new().unwrap();
    let mut m = fixture("calendar", temp("calendar-hit-ids"));
    let tokens = tokens(&m.placed.appearance);
    for variant in ["month", "strip", "agenda"] {
        for editing in [false, true] {
            m.placed.settings.insert("variant".into(), json!(variant));
            m.event_draft = editing.then(|| model::EventDraft {
                title: "Dentist".into(),
                date: "2026-11-05".into(),
                time: String::new(),
                color: 0,
                part: model::EventPart::Title,
            });
            for size in [(160, 160), (320, 310), (420, 420)] {
                let frame = view::Frame {
                    size,
                    scale: 1.0,
                    now: now(),
                    instant: Instant::now(),
                    editing: false,
                };
                let list = view::view(&m, &tokens, frame, &mut ui.text);
                let mut seen = std::collections::HashSet::new();
                for hit in &list.hits {
                    assert!(
                        seen.insert(hit.id),
                        "{variant} (editor: {editing}) at {size:?}: hit {} ({}) is not unique",
                        hit.id,
                        hit.label
                    );
                }
            }
        }
    }
}

/// The README's desktop scene, widgets half (`docs/media/readme`): a few built-in widgets at
/// their default sizes on a 2560×1440 screen at 150 %, content only on transparency, plus their
/// glass plates in the frame lab's `MYPAPER_LAB_PLATES` form, so the engine's real glass can be
/// drawn under them; `widgets-editing.png` is the same in edit mode. Run with `MYPAPER_README_SCENE=<dir>` (the bars half is in `bars::tests`).
#[test]
#[ignore = "writes images: MYPAPER_README_SCENE=<dir>"]
fn readme_scene_widgets() {
    let Ok(dir) = std::env::var("MYPAPER_README_SCENE") else {
        return;
    };
    let dir = PathBuf::from(dir);
    const SCALE: f32 = 1.5;
    let px = |n: u32| ((n as f32 * 44.0 + (n - 1) as f32 * 8.0) * SCALE).round() as u32;
    let mut ui = Ui::new().unwrap();
    let root = temp("readme-scene");
    let registry = registry::builtins();
    // Two columns from the top-left, like a desktop: (widget, column, row order).
    let columns: [&[&str]; 2] = [&["clock", "weather", "system-monitor"], &["calendar", "now-playing"]];
    let (left, top, gap) = (60.0_f32, 110.0_f32, 36.0_f32);
    let (mut list, mut editing_list) = (DisplayList::default(), DisplayList::default());
    let mut plates = String::new();
    let material = mypaper_glass_ipc::Material::default();
    let mut x = left;
    for column in columns {
        let mut y = top;
        let mut column_w = 0.0_f32;
        for id in column {
            let installed = &registry.widgets[*id];
            let mut m = fixture(id, root.clone());
            m.placed.size = installed.manifest.size.default;
            m.placed.appearance.insert("text".into(), json!("light"));
            let size = (px(m.placed.size[0]), px(m.placed.size[1]));
            let tokens = tokens(&m.placed.appearance);
            for (editing, list) in [(false, &mut list), (true, &mut editing_list)] {
                let frame = view::Frame { size, scale: SCALE, now: now(), instant: Instant::now(), editing };
                list.ops.extend(view::view(&m, &tokens, frame, &mut ui.text).translated(x, y).ops);
            }
            let rect = Rect::new(x, y, size.0 as f32, size.1 as f32);
            let radius = view::plate_radius(&m, Rect::new(0.0, 0.0, rect.width(), rect.height()), SCALE);
            let [r, g, b, a] = material.tint;
            plates.push_str(&format!(
                "{x},{y},{},{},{radius},{},{},{},{},{r},{g},{b},{a}\n",
                rect.width(),
                rect.height(),
                material.clarity,
                material.blur,
                material.refraction,
                material.highlight
            ));
            y += size.1 as f32 + gap;
            column_w = column_w.max(size.0 as f32);
        }
        x += column_w + gap;
    }
    let image = render(&mut ui, &list, 2560, 1440).unwrap();
    std::fs::create_dir_all(&dir).unwrap();
    golden::write(&image, &dir.join("widgets.png")).unwrap();
    let image = render(&mut ui, &editing_list, 2560, 1440).unwrap();
    golden::write(&image, &dir.join("widgets-editing.png")).unwrap();
    std::fs::write(dir.join("widgets.plates"), plates).unwrap();
    let _ = std::fs::remove_dir_all(root);
}
