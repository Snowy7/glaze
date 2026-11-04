//! Plan Rule 1: drawing with `mypaper-ui` loads no GPU vendor driver. An integration test, so
//! it runs in a process of its own with nothing else loaded.

use mypaper_ui::{
    DisplayList, Kind, Op, Rect, Rgba, Role, Surface, TextStyle, Ui, modules, render,
};

#[test]
fn drawing_text_and_shapes_on_a_surface_loads_no_gpu_driver() {
    let mut ui = Ui::new().unwrap();
    let style = TextStyle::new("Segoe UI Variable Text", 14.0);
    let (w, h) = ui.text.measure("Refresh", &style);
    let list = DisplayList {
        ops: vec![
            Op::Fill {
                rect: Rect::new(0.0, 0.0, 200.0, 40.0),
                radius: 8.0,
                color: Rgba::new(0.1, 0.1, 0.1, 0.9),
            },
            Op::Text {
                rect: Rect::new(10.0, 10.0, w, h),
                text: "Refresh".into(),
                style,
                color: Rgba::new(1.0, 1.0, 1.0, 1.0),
            },
        ],
        ..Default::default()
    };
    render(&mut ui, &list, 200, 40).unwrap();
    let mut surface = Surface::new(Kind::Popup { activate: false }, "test", Role::Menu).unwrap();
    surface.present(&mut ui, (0, 0), (200, 40), list).unwrap();

    let loaded = modules::loaded();
    let drivers: Vec<&String> = loaded
        .iter()
        .filter(|m| modules::is_gpu_driver(m))
        .collect();
    assert!(
        drivers.is_empty(),
        "GPU drivers loaded: {drivers:?}\nall modules: {loaded:?}"
    );
    assert!(
        loaded.iter().any(|m| m == "d2d1.dll"),
        "Direct2D did the drawing"
    );
}
