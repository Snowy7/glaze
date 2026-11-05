//! The T3 plugin's faces drawn by the host, laid out like the native `t3-sizes.png` so the two
//! sheets compare side by side. Input is the plugin's own view golden (`plugins/t3/golden`).
use crate::widgets::{model::{Model, Widget}, time::Stamp, views};
use mypaper_ui::{DisplayList, Op, Rect, Rgba, TextStyle, Tokens, Ui, golden, render};
use mypaper_widgets::{layout::Placed, storage::Storage};
use serde_json::{Map, Value, json};
use std::{path::PathBuf, sync::Arc, time::Instant};

const NOW: i64 = 1_793_875_200_000;

#[path = "../../../../../plugins/docker/src/host_tests.rs"]
mod docker;

fn model(face: &Value, title: &str) -> Model {
    let placed = Placed {
        instance: "t3-plugin".into(),
        widget: "clock".into(),
        cell: [0, 0],
        size: [8, 5],
        settings: Map::new(),
        appearance: Map::new(),
        own: Map::new(),
    };
    let storage = &mut Storage::new(std::env::temp_dir().join("mypaper-plugin-fixture"));
    let mut m = Model::new(placed, storage, Stamp::at(NOW)).unwrap();
    m.kind = Widget::Plugin;
    m.visible = true;
    m.data = json!({ "title": title, "icon": "terminal", "status": "live", "face": face });
    m
}

fn draw(ui: &mut Ui, m: &Model, size: (u32, u32)) -> DisplayList {
    draw_with(ui, m, size, &Tokens::default())
}

fn draw_with(ui: &mut Ui, m: &Model, size: (u32, u32), tokens: &Tokens) -> DisplayList {
    let frame = views::Frame { size, scale: 1.0, now: Stamp::at(NOW), instant: Instant::now(), editing: false };
    views::view(m, tokens, frame, &mut ui.text)
}

/// The cells of a T3 sheet row: compact, regular, large, and a 150-DIP square (the smallest
/// tile that still gets the regular face).
const CELLS: [(f32, (u32, u32), &str); 4] = [(24.0, (180, 132), "small"), (228.0, (360, 220), "medium"), (612.0, (540, 330), "large"), (1176.0, (150, 150), "medium")];
const SHEET_W: u32 = 1350;

/// Draws one labelled row of `faces` (`{ size: Face }`) at `y`, light or dark, and checks
/// every cell's hits stay inside it and an idle redraw damages nothing.
fn sheet_row(ui: &mut Ui, sheet: &mut DisplayList, y: f32, caption: &str, faces: &Value, title: &str, light: bool) {
    let tokens = if light { Tokens { colors: mypaper_ui::Colors::new(false, [0, 120, 215]), ..Tokens::default() } } else { Tokens::default() };
    sheet.ops.push(Op::Text {
        rect: Rect::new(24.0, y, 1000.0, 24.0),
        text: caption.to_owned().into(),
        style: TextStyle::new("Segoe UI Variable", 14.0),
        color: Rgba::rgb8([220, 228, 240], 1.0),
    });
    for (x, size, class) in CELLS {
        let m = model(&faces[class], title);
        let list = draw_with(ui, &m, size, &tokens);
        assert!(list.damage(&draw_with(ui, &m, size, &tokens)).is_none(), "{caption}/{class} idle damage");
        for hit in &list.hits {
            let inside = hit.rect.left >= 0.0 && hit.rect.top >= 0.0 && hit.rect.right <= size.0 as f32 + 0.5 && hit.rect.bottom <= size.1 as f32 + 0.5;
            assert!(inside, "{caption}/{class}: {} {:?}", hit.label, hit.rect);
        }
        let image = render(ui, &list, size.0, size.1).unwrap();
        let rect = Rect::new(x, y + 30.0, size.0 as f32, size.1 as f32);
        let backdrop = if light { Rgba::rgb8([206, 212, 222], 1.0) } else { Rgba::rgb8([44, 52, 64], 1.0) };
        sheet.ops.push(Op::Fill { rect, radius: 22.0, color: backdrop });
        sheet.ops.push(Op::Image { rect, image: Arc::new(image), opacity: 1.0, source: None });
    }
}

fn sheet(rows: usize) -> (DisplayList, u32) {
    let height = rows as u32 * 380 + 20;
    let mut sheet = DisplayList::default();
    sheet.ops.push(Op::Fill { rect: Rect::new(0.0, 0.0, SHEET_W as f32, height as f32), radius: 0.0, color: Rgba::rgb8([20, 25, 33], 1.0) });
    (sheet, height)
}

/// The edge cases (`plugins/t3/golden/edges.json`): empty, one, many, stale, connecting and
/// unavailable, each on the faces it matters for.
#[test]
fn t3_plugin_edge_cases_render_on_the_kit() {
    let edges: Value = serde_json::from_str(include_str!("../../../../../plugins/t3/golden/edges.json")).unwrap();
    let rows = [
        ("empty", "running", "Running now"),
        ("empty", "attention", "Needs you"),
        ("empty", "limits", "Limits"),
        ("empty", "usage", "Usage"),
        ("one", "running", "Running now"),
        ("one", "limits", "Limits"),
        ("many", "running", "Running now"),
        ("many", "attention", "Needs you"),
        ("many", "limits", "Limits"),
        ("stale", "limits", "Limits"),
        ("connecting", "limits", "Limits"),
        ("unavailable", "running", "Running now"),
    ];
    let mut ui = Ui::new().unwrap();
    let (mut sheet, height) = self::sheet(rows.len());
    for (row, (case, face, name)) in rows.iter().enumerate() {
        let y = 20.0 + row as f32 * 380.0;
        sheet_row(&mut ui, &mut sheet, y, &format!("t3 edge / {case} / {face} — compact · regular · large · 150"), &edges[case][face], &format!("T3 · {name}"), false);
    }
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/widgets/views/plugin-t3-edges.png");
    golden::check(&render(&mut ui, &sheet, SHEET_W, height).unwrap(), &path).unwrap();
}

/// Hits map back to the plugin's actions in tree order; a `field` asks for text entry.
#[test]
fn hits_map_back_to_plugin_actions() {
    use views::plugin::{ACTION, Hit, action_at};
    let faces: Value = serde_json::from_str(include_str!("../../../../../plugins/t3/golden/faces.json")).unwrap();
    let face = |id: &str| serde_json::from_value::<mypaper_plugin_sdk::Face>(faces[id]["medium"].clone()).unwrap();
    let attention = face("attention");
    assert_eq!(action_at(&attention, ACTION), Some(Hit::Action("thread:thread-0".into())));
    assert_eq!(action_at(&attention, ACTION + 1), Some(Hit::Action("approve:thread-0:approval".into())));
    assert_eq!(action_at(&attention, ACTION + 2), Some(Hit::Action("deny:thread-0:approval".into())));
    assert_eq!(action_at(&attention, ACTION + 3), None);
    assert_eq!(action_at(&attention, ACTION - 1), Some(Hit::Action("refresh".into())));
    let new_thread = face("new");
    for (offset, expected) in [(0, "project"), (1, "model")] {
        assert!(matches!(
            action_at(&new_thread, ACTION + offset),
            Some(Hit::Select { action, options, selected })
                if action == expected && options.iter().any(|option| Some(&option.value) == selected.as_ref())
        ));
    }
    assert_eq!(action_at(&new_thread, ACTION + 2), Some(Hit::Field("send".into())));
    assert_eq!(action_at(&new_thread, ACTION + 3), None);
}

#[test]
fn t3_plugin_faces_render_on_the_kit() {
    let faces: Value = serde_json::from_str(include_str!("../../../../../plugins/t3/golden/faces.json")).unwrap();
    let names = [("running", "Running now"), ("usage", "Usage"), ("limits", "Limits"), ("attention", "Needs you"), ("new", "New thread")];
    // The same faces again on a light widget.
    let light = [("limits", "Limits"), ("running", "Running now"), ("usage", "Usage")];
    let mut ui = Ui::new().unwrap();
    let (mut sheet, height) = self::sheet(names.len() + light.len());
    let rows = names.iter().map(|n| (n, false)).chain(light.iter().map(|n| (n, true)));
    for (row, ((id, name), light)) in rows.enumerate() {
        let y = 20.0 + row as f32 * 380.0;
        let scheme = if light { "light" } else { "dark" };
        sheet_row(&mut ui, &mut sheet, y, &format!("t3 plugin / {id} ({scheme}) — compact · regular · large · 150"), &faces[id], &format!("T3 · {name}"), light);
    }
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/widgets/views/plugin-t3-sizes.png");
    golden::check(&render(&mut ui, &sheet, SHEET_W, height).unwrap(), &path).unwrap();
}

#[path = "../../../../../plugins/discord/src/host_tests.rs"]
mod discord;
