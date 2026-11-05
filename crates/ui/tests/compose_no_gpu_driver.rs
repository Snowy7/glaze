//! Plan Rule 1 for DirectComposition: composing sprites over a surface (WARP device, DWM does the
//! scaling) loads no GPU vendor driver. Its own test binary, so nothing else is loaded.

use mypaper_ui::{Image, Kind, Rect, Role, Surface, compose::Compositor, modules};

#[test]
fn composing_sprites_over_a_surface_loads_no_gpu_driver() {
    let surface = Surface::new(Kind::Popup { activate: false }, "test", Role::Menu).unwrap();
    let compositor = Compositor::new(surface.hwnd()).unwrap();
    let image = Image {
        width: 48,
        height: 48,
        pixels: [255u8, 0, 0, 255].repeat(48 * 48),
    };
    let sprite = compositor.sprite(&image).unwrap();
    compositor.show(&[&sprite]).unwrap();
    for k in 0..4 {
        let s = 48.0 * (1.0 + k as f32 / 4.0);
        compositor
            .place(&sprite, Rect::new(k as f32 * 10.0, 0.0, s, s), 0.72)
            .unwrap();
        compositor.commit().unwrap();
    }

    let loaded = modules::loaded();
    let drivers: Vec<&String> = loaded
        .iter()
        .filter(|m| modules::is_gpu_driver(m))
        .collect();
    assert!(
        drivers.is_empty(),
        "GPU drivers loaded: {drivers:?}\nall modules: {loaded:?}"
    );
    assert!(loaded.iter().any(|m| m == "dcomp.dll"), "DirectComposition composed");
}
