//! Compiled by the shell's plugin view tests, so the golden uses the production kit renderer.
use super::*;

fn docker_model(face: &Value) -> Model {
    let mut model = model(face, "Docker");
    model.data["icon"] = "layers".into();
    model
}

#[test]
fn docker_plugin_size_sheet_and_action_hits() {
    use views::plugin::{ACTION, Hit, action_at};
    let faces: Value = serde_json::from_str(include_str!("../golden/faces.json")).unwrap();
    let live: glaze_plugin_sdk::Face =
        serde_json::from_value(faces["containers"]["large"].clone()).unwrap();
    assert_eq!(
        action_at(&live, ACTION),
        Some(Hit::Action("stop:web".into()))
    );
    assert_eq!(
        action_at(&live, ACTION + 2),
        Some(Hit::Action("start:worker".into()))
    );
    let mut ui = Ui::new().unwrap();
    let mut sheet = DisplayList::default();
    sheet.ops.push(Op::Fill {
        rect: Rect::new(0.0, 0.0, 996.0, 740.0),
        radius: 0.0,
        color: Rgba::rgb8([20, 25, 33], 1.0),
    });
    sheet.ops.push(Op::Text {
        rect: Rect::new(24.0, 16.0, 940.0, 28.0),
        text: "DOCKER PLUGIN / small · medium · large".into(),
        style: TextStyle::new("Segoe UI Variable", 18.0),
        color: Rgba::rgb8([235, 240, 248], 1.0),
    });
    for (id, class, size, x, y) in [
        ("containers", "small", (180, 180), 24.0, 58.0),
        ("containers", "medium", (360, 180), 228.0, 58.0),
        ("containers", "large", (360, 360), 612.0, 58.0),
        ("setup", "medium", (300, 210), 24.0, 486.0),
        ("disconnected", "medium", (300, 210), 348.0, 486.0),
        ("empty", "medium", (300, 210), 672.0, 486.0),
    ] {
        let model = docker_model(&faces[id][class]);
        let list = draw(&mut ui, &model, size);
        assert!(list.damage(&draw(&mut ui, &model, size)).is_none());
        if id == "containers" {
            assert!(
                list.hits.iter().any(|hit| hit.id == ACTION && hit.enabled),
                "{class}: first container action must fit"
            );
        }
        for hit in &list.hits {
            assert!(
                hit.rect.left >= 0.0
                    && hit.rect.top >= 0.0
                    && hit.rect.right <= size.0 as f32
                    && hit.rect.bottom <= size.1 as f32,
                "{id}/{class}: {} {:?}",
                hit.label,
                hit.rect
            );
            if id == "disconnected" && hit.id >= ACTION {
                assert!(!hit.enabled);
            }
        }
        let rect = Rect::new(x, y, size.0 as f32, size.1 as f32);
        sheet.ops.push(Op::Fill {
            rect,
            radius: 22.0,
            color: Rgba::rgb8([44, 52, 64], 1.0),
        });
        sheet.ops.push(Op::Image {
            rect,
            image: Arc::new(render(&mut ui, &list, size.0, size.1).unwrap()),
            opacity: 1.0,
            source: None,
        });
    }
    for (x, label) in [(24.0, "SETUP"), (348.0, "DISCONNECTED"), (672.0, "EMPTY")] {
        sheet.ops.push(Op::Text {
            rect: Rect::new(x, 449.0, 300.0, 23.0),
            text: label.into(),
            style: TextStyle::new("Segoe UI Variable", 12.0),
            color: Rgba::rgb8([159, 172, 190], 1.0),
        });
    }
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins/docker/golden/sizes.png");
    golden::check(&render(&mut ui, &sheet, 996, 740).unwrap(), &path).unwrap();
}
