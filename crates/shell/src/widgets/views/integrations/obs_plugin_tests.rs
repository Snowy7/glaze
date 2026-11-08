//! The OBS plugin's faces drawn by the host, laid out like the native `obs-sizes.png` so the two
//! sheets compare side by side. Input is the plugin's own view golden (`plugins/obs/golden`).
use crate::widgets::{model::{Model, Widget}, time::Stamp, views};
use glaze_ui::{DisplayList, Op, Rect, Rgba, TextStyle, Tokens, Ui, golden, render};
use glaze_widgets::{layout::Placed, storage::Storage};
use serde_json::{Map, Value, json};
use std::{path::PathBuf, sync::Arc, time::Instant};

const NOW: i64 = 1_793_875_200_000;

fn model(face: &Value, title: &str) -> Model {
    let placed = Placed {
        instance: "obs-plugin".into(),
        widget: "clock".into(),
        cell: [0, 0],
        size: [8, 5],
        settings: Map::new(),
        appearance: Map::new(),
        own: Map::new(),
    };
    let storage = &mut Storage::new(std::env::temp_dir().join("glaze-plugin-fixture"));
    let mut m = Model::new(placed, storage, Stamp::at(NOW)).unwrap();
    m.kind = Widget::Plugin;
    m.visible = true;
    m.data = json!({ "title": title, "icon": "video", "status": "live", "face": face });
    m
}

fn draw(ui: &mut Ui, m: &Model, size: (u32, u32)) -> DisplayList {
    let frame = views::Frame { size, scale: 1.0, now: Stamp::at(NOW), instant: Instant::now(), editing: false };
    views::view(m, &Tokens::default(), frame, &mut ui.text)
}

#[test]
fn obs_plugin_faces_render_on_the_kit() {
    let faces: Value = serde_json::from_str(include_str!("../../../../../../plugins/obs/golden/faces.json")).unwrap();
    let names = [("controls", "Studio"), ("offline", "Not running")];
    let mut ui = Ui::new().unwrap();
    let height = names.len() as u32 * 380 + 20;
    let mut sheet = DisplayList::default();
    sheet.ops.push(Op::Fill { rect: Rect::new(0.0, 0.0, 1176.0, height as f32), radius: 0.0, color: Rgba::rgb8([20, 25, 33], 1.0) });
    for (row, (id, name)) in names.iter().enumerate() {
        let y = 20.0 + row as f32 * 380.0;
        sheet.ops.push(Op::Text {
            rect: Rect::new(24.0, y, 1000.0, 24.0),
            text: format!("obs plugin / {id} — compact · regular · large").into(),
            style: TextStyle::new("Segoe UI Variable", 14.0),
            color: Rgba::rgb8([220, 228, 240], 1.0),
        });
        for (x, size, class) in [(24.0, (180, 132), "small"), (228.0, (360, 220), "medium"), (612.0, (540, 330), "large")] {
            let m = model(&faces[id][class], &format!("OBS · {name}"));
            let list = draw(&mut ui, &m, size);
            assert!(list.damage(&draw(&mut ui, &m, size)).is_none(), "{id}/{class} idle damage");
            for hit in &list.hits {
                let inside = hit.rect.left >= 0.0 && hit.rect.top >= 0.0 && hit.rect.right <= size.0 as f32 + 0.5 && hit.rect.bottom <= size.1 as f32 + 0.5;
                assert!(inside, "{id}/{class}: {} {:?}", hit.label, hit.rect);
            }
            let image = render(&mut ui, &list, size.0, size.1).unwrap();
            let rect = Rect::new(x, y + 30.0, size.0 as f32, size.1 as f32);
            sheet.ops.push(Op::Fill { rect, radius: 22.0, color: Rgba::rgb8([44, 52, 64], 1.0) });
            sheet.ops.push(Op::Image { rect, image: Arc::new(image), opacity: 1.0, source: None });
        }
    }
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins/obs/golden/host-sizes.png");
    golden::check(&render(&mut ui, &sheet, 1176, height).unwrap(), &path).unwrap();
}
