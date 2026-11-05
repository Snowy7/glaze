//! Tailscale SDK trees rendered by the actual host, including disconnected and stale states.
use crate::widgets::{model::{Model, Widget}, time::Stamp, views};
use mypaper_ui::{DisplayList, Op, Rect, Rgba, TextStyle, Tokens, Ui, golden, render};
use mypaper_widgets::{layout::Placed, storage::Storage};
use serde_json::{Map, Value, json};
use std::{path::PathBuf, sync::Arc, time::Instant};
const NOW: i64 = 1_793_875_200_000;

#[test]
fn tailscale_plugin_faces_render_on_the_kit() {
    let live = std::env::var_os("MYPAPER_TAILSCALE_LIVE_FACES").map(PathBuf::from);
    let faces: Value = match &live {
        Some(path) => serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap(),
        None => serde_json::from_str(include_str!("../../../../../plugins/tailscale/golden/faces.json")).unwrap(),
    };
    let mut ui = Ui::new().unwrap();
    let names: &[&str] = if live.is_some() { &["connection", "peers", "traffic"] } else { &["connection", "peers", "traffic", "disconnected", "unavailable", "empty"] };
    let sizes = if live.is_some() { vec![(24.0, (540, 330), "large")] } else { vec![(24.0, (180, 132), "small"), (228.0, (360, 220), "medium"), (612.0, (540, 330), "large")] };
    let width = if live.is_some() { 588 } else { 1176 };
    let height = names.len() as u32 * 380 + 20;
    let mut sheet = DisplayList::default();
    sheet.ops.push(Op::Fill { rect: Rect::new(0.0, 0.0, width as f32, height as f32), radius: 0.0, color: Rgba::rgb8([20, 25, 33], 1.0) });
    for (row, name) in names.iter().enumerate() {
        let y = 20.0 + row as f32 * 380.0;
        sheet.ops.push(Op::Text { rect: Rect::new(24.0, y, 1000.0, 24.0), text: format!("tailscale / {name} — compact · regular · large").into(), style: TextStyle::new("Segoe UI Variable", 14.0), color: Rgba::rgb8([220, 228, 240], 1.0) });
        for &(x, size, class) in &sizes {
            let placed = Placed { instance: "tailscale-plugin".into(), widget: "clock".into(), cell: [0, 0], size: [8, 5], settings: Map::new(), appearance: Map::new(), own: Map::new() };
            let mut m = Model::new(placed, &mut Storage::new(std::env::temp_dir().join("mypaper-tailscale-fixture")), Stamp::at(NOW)).unwrap();
            m.kind = Widget::Plugin;
            m.visible = true;
            let face = if live.is_some() { &faces[name] } else { &faces[name][class] };
            m.data = json!({"title":"Tailscale", "icon":"globe", "status":"live", "face":face});
            let frame = || views::Frame { size, scale: 1.0, now: Stamp::at(NOW), instant: Instant::now(), editing: false };
            let list = views::view(&m, &Tokens::default(), frame(), &mut ui.text);
            let same = views::view(&m, &Tokens::default(), frame(), &mut ui.text);
            assert!(list.damage(&same).is_none(), "{name}/{class}: idle damage");
            for hit in &list.hits {
                assert!(hit.rect.left >= 0.0 && hit.rect.top >= 0.0 && hit.rect.right <= size.0 as f32 + 0.5 && hit.rect.bottom <= size.1 as f32 + 0.5, "{name}/{class}: {} {:?}", hit.label, hit.rect);
            }
            if *name == "connection" || *name == "disconnected" || *name == "empty" {
                assert!(list.hits.iter().any(|h| h.label.as_ref() == "Connect" || h.label.as_ref() == "Disconnect"), "{name}/{class}: toggle clipped");
            }
            let image = render(&mut ui, &list, size.0, size.1).unwrap();
            let rect = Rect::new(x, y + 30.0, size.0 as f32, size.1 as f32);
            sheet.ops.push(Op::Fill { rect, radius: 22.0, color: Rgba::rgb8([44, 52, 64], 1.0) });
            sheet.ops.push(Op::Image { rect, image: Arc::new(image), opacity: 1.0, source: None });
        }
    }
    let path = live.map(|p| p.with_extension("png")).unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins/tailscale/golden/sizes.png"));
    golden::check(&render(&mut ui, &sheet, width, height).unwrap(), &path).unwrap();
}
