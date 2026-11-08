//! The component's actual view fixtures rendered by the production kit, without app input.
use crate::widgets::{
    model::{Model, Widget},
    time::Stamp,
    views,
};
use glaze_ui::{DisplayList, Op, Rect, Rgba, TextStyle, Tokens, Ui, golden, render};
use glaze_widgets::{layout::Placed, storage::Storage};
use serde_json::{Map, Value, json};
use std::{path::PathBuf, sync::Arc, time::Instant};

const NOW: i64 = 1_793_875_200_000;

fn model(face: &Value, name: &str) -> Model {
    let placed = Placed {
        instance: "grok-fixture".into(),
        widget: "clock".into(),
        cell: [0, 0],
        size: [8, 5],
        settings: Map::new(),
        appearance: Map::new(),
        own: Map::new(),
    };
    let mut m = Model::new(
        placed,
        &mut Storage::new(std::env::temp_dir().join("glaze-grok-plugin-fixture")),
        Stamp::at(NOW),
    )
    .unwrap();
    m.kind = Widget::Plugin;
    m.visible = true;
    m.data = json!({"title": format!("Grok · {name}"), "icon": "sparkles", "status": "live", "face": face});
    m
}

fn draw(ui: &mut Ui, model: &Model, size: (u32, u32), scale: f32) -> DisplayList {
    views::view(
        model,
        &Tokens::default(),
        views::Frame {
            size,
            scale,
            now: Stamp::at(NOW),
            instant: Instant::now(),
            editing: false,
        },
        &mut ui.text,
    )
}

fn add(
    ui: &mut Ui,
    sheet: &mut DisplayList,
    face: &Value,
    name: &str,
    size: (u32, u32),
    x: f32,
    y: f32,
) {
    let m = model(face, name);
    let list = draw(ui, &m, size, 1.0);
    assert!(
        list.damage(&draw(ui, &m, size, 1.0)).is_none(),
        "idle face repaints"
    );
    for hit in &list.hits {
        assert!(
            hit.rect.left >= 0.0
                && hit.rect.top >= 0.0
                && hit.rect.right <= size.0 as f32 + 0.5
                && hit.rect.bottom <= size.1 as f32 + 0.5,
            "{name}: {} {:?}",
            hit.label,
            hit.rect
        );
    }
    let rect = Rect::new(x, y, size.0 as f32, size.1 as f32);
    sheet.ops.push(Op::Fill {
        rect,
        radius: 22.0,
        color: Rgba::rgb8([44, 52, 64], 1.0),
    });
    sheet.ops.push(Op::Image {
        rect,
        image: Arc::new(render(ui, &list, size.0, size.1).unwrap()),
        opacity: 1.0,
        source: None,
    });
}

fn label(sheet: &mut DisplayList, text: &str, x: f32, y: f32) {
    sheet.ops.push(Op::Text {
        rect: Rect::new(x, y, 1100.0, 24.0),
        text: text.into(),
        style: TextStyle::new("Segoe UI Variable", 14.0),
        color: Rgba::rgb8([220, 228, 240], 1.0),
    });
}

#[test]
fn grok_component_sizes_and_states_render_on_the_kit() {
    let faces: Value = serde_json::from_str(include_str!("../golden/faces.json")).unwrap();
    let mut ui = Ui::new().unwrap();
    let mut sheet = DisplayList::default();
    sheet.ops.push(Op::Fill {
        rect: Rect::new(0.0, 0.0, 1176.0, 1425.0),
        radius: 0.0,
        color: Rgba::rgb8([20, 25, 33], 1.0),
    });
    for (row, (id, name)) in [
        ("routines", "Routines"),
        ("recent", "Recent"),
        ("prompt", "Prompt"),
    ]
    .into_iter()
    .enumerate()
    {
        let y = 20.0 + row as f32 * 380.0;
        label(
            &mut sheet,
            &format!("grok / {id} — compact · regular · large"),
            24.0,
            y,
        );
        for (x, size, class) in [
            (24.0, (180, 132), "small"),
            (228.0, (360, 220), "medium"),
            (612.0, (540, 330), "large"),
        ] {
            add(
                &mut ui,
                &mut sheet,
                &faces[id][class],
                name,
                size,
                x,
                y + 30.0,
            );
        }
        for width in 4..=12 {
            for height in 3..=10 {
                for scale in [1.0, 1.5] {
                    let size = (
                        (width as f32 * 44.0 * scale) as u32,
                        (height as f32 * 44.0 * scale) as u32,
                    );
                    let class = if width < 6 || height < 4 {
                        "small"
                    } else if height >= 7 {
                        "large"
                    } else {
                        "medium"
                    };
                    let list = draw(&mut ui, &model(&faces[id][class], name), size, scale);
                    for hit in &list.hits {
                        assert!(
                            hit.rect.left >= 0.0
                                && hit.rect.top >= 0.0
                                && hit.rect.right <= size.0 as f32 + 0.5
                                && hit.rect.bottom <= size.1 as f32 + 0.5,
                            "{id}/{width}x{height}: {} {:?}",
                            hit.label,
                            hit.rect
                        );
                    }
                }
            }
        }
    }
    for (state, title, x) in [
        ("setup", "SETUP", 24.0),
        ("signed-in", "SIGNED IN · NO ROUTINES", 412.0),
    ] {
        label(&mut sheet, title, x, 1160.0);
        add(
            &mut ui,
            &mut sheet,
            &faces[state]["medium"],
            "Routines",
            (352, 220),
            x,
            1190.0,
        );
    }
    label(&mut sheet, "ACCEPTED · RECEIPT", 800.0, 1160.0);
    add(
        &mut ui,
        &mut sheet,
        &faces["recent"]["medium"],
        "Recent",
        (352, 220),
        800.0,
        1190.0,
    );
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins/grok/golden/sizes.png");
    golden::check(&render(&mut ui, &sheet, 1176, 1425).unwrap(), &path).unwrap();
}

#[test]
fn grok_prompt_and_routine_hits_reach_the_component() {
    use views::plugin::{ACTION, Hit, action_at};
    let faces: Value = serde_json::from_str(include_str!("../golden/faces.json")).unwrap();
    let prompt = serde_json::from_value(faces["prompt"]["medium"].clone()).unwrap();
    assert_eq!(
        action_at(&prompt, ACTION),
        Some(Hit::Field("prompt".into()))
    );
    let routines = serde_json::from_value(faces["routines"]["medium"].clone()).unwrap();
    assert_eq!(
        action_at(&routines, ACTION),
        Some(Hit::Action("advanced".into()))
    );
    assert_eq!(
        action_at(&routines, ACTION + 1),
        Some(Hit::Action("run:routine-0".into()))
    );
}
