//! Input-free live evidence: production menu, weather and overflow views, with fixture-owned
//! opaque backdrops so no other app's pixels need to be captured. Run explicitly after deploy.
use super::*;
use glaze_ui::{DisplayList, Entry, MenuIcon, Op, Rgba, Tokens};
use windows::Win32::UI::WindowsAndMessaging::*;

fn pump(ms: u64) {
    let until = Instant::now() + Duration::from_millis(ms);
    while Instant::now() < until {
        let mut msg = MSG::default();
        // SAFETY: pump only this fixture thread; no input is generated or forwarded to apps.
        unsafe {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        std::thread::sleep(Duration::from_millis(8));
    }
}

fn menu<C>(
    ui: &mut Ui,
    surface: &mut Surface,
    entries: &[Entry<C>],
    tokens: &Tokens,
    at: (i32, i32),
) {
    let layout = glaze_ui::menu::layout(entries, tokens, 1.0, &mut ui.text);
    let margin = if tokens.shadow {
        tokens.menu.shadow.ceil()
    } else {
        0.0
    };
    let size = (
        (layout.width + margin * 2.0).ceil() as u32,
        (layout.height + margin * 2.0).ceil() as u32,
    );
    let body = Rect::new(margin, margin, layout.width, layout.height);
    surface.set_glass(tokens.glass.map(|material| Glass {
        body,
        radius: tokens.radius,
        material,
    }));
    surface.set_glass_style(tokens.glass_style);
    let list = glaze_ui::menu::view(entries, &layout, None, tokens);
    surface.present(ui, at, size, list).unwrap();
    surface.show();
}

#[test]
#[ignore = "input-free visual fixture; requires deployed engine and GLAZE_MENU_GLASS_CAPTURE"]
fn live_menu_glass() {
    let root =
        PathBuf::from(std::env::var_os("GLAZE_MENU_GLASS_CAPTURE").expect("capture folder"));
    std::fs::create_dir_all(&root).unwrap();
    for name in ["live-ready.json", "desktop-captured", "tray-captured"] {
        let _ = std::fs::remove_file(root.join(name));
    }
    glaze_desktop::enable_dpi_awareness();
    let _com = glaze_desktop::desktop_view::Com::init();
    let _ = glaze_ui::set_identity(glaze_ui::Identity {
        prefix: "glaze.ui",
        desktop_classes: &[],
    });
    let mut ui = Ui::new().unwrap();
    crate::overlay_client::install();
    let look = Look::load(&ui.text);
    look.install();
    let monitor = glaze_desktop::monitor_infos()
        .into_iter()
        .find(|m| m.primary)
        .unwrap();
    let origin = (monitor.work.x + 80, monitor.work.y + 80);
    let mut backdrop = Surface::new(
        SurfaceKind::Popup { activate: false },
        "Glaze glass evidence backdrop",
        Role::Pane,
    )
    .unwrap();
    let mut list = DisplayList::default();
    for y in 0..44 {
        for x in 0..75 {
            list.ops.push(Op::Fill {
                rect: Rect::new(x as f32 * 16.0, y as f32 * 16.0, 16.0, 16.0),
                radius: 0.0,
                color: Rgba::rgb8(
                    if (x + y) % 2 == 0 {
                        [64, 92, 120]
                    } else {
                        [164, 139, 106]
                    },
                    1.0,
                ),
            });
        }
    }
    backdrop
        .present(&mut ui, origin, (1200, 704), list)
        .unwrap();
    backdrop.show();
    let mut desktop = Surface::new(
        SurfaceKind::Popup { activate: false },
        "Glaze desktop menu evidence",
        Role::Menu,
    )
    .unwrap();
    let entries = crate::preview::menu::build(
        &glaze_config::DesktopMenu::default(),
        crate::preview::menu::ViewState {
            icons_visible: true,
            ..Default::default()
        },
    );
    let tokens = look.menu_tokens();
    menu(
        &mut ui,
        &mut desktop,
        &entries,
        &tokens,
        (origin.0 + 16, origin.1 + 16),
    );
    let mut widget = Surface::new(
        SurfaceKind::Popup { activate: false },
        "Glaze weather evidence",
        Role::Pane,
    )
    .unwrap();
    let registry = glaze_widgets::registry::builtins();
    let manifest = &registry.get("weather").unwrap().manifest;
    let mut layout = glaze_widgets::layout::Layout::default();
    let id = layout.add("weather", "fixture", [0, 0], manifest.size.default);
    let grid = glaze_widgets::layout::Grid::new(
        &layout.grid,
        [96.0; 2],
        glaze_widgets::layout::RectF {
            x: 0.0,
            y: 0.0,
            width: 1200.0,
            height: 704.0,
        },
    );
    let placed = glaze_widgets::layout::place(
        layout.get(&id).unwrap(),
        manifest,
        &grid,
        &glaze_widgets::look::defaults(),
    );
    let now = crate::preview::widgets::Stamp::now();
    let mut model = crate::preview::widgets::Model::new(
        placed,
        &mut glaze_widgets::storage::Storage::new(root.join("data")),
        now,
    )
    .unwrap();
    model.visible = true;
    model.data = serde_json::json!({"current":{"location":{"name":"Oslo"},"temperature":35,"condition":"Clear sky","code":0},"forecast":{"days":[{"date":"2026-11-08","max":36,"min":27,"code":0},{"date":"2026-11-09","max":35,"min":26,"code":2},{"date":"2026-11-10","max":34,"min":26,"code":3}]}});
    let mut widget_tokens = crate::preview::widgets::tokens(&model.placed.appearance);
    let material = crate::glass::liquid_material(glaze_widgets::look::DEFAULT_TINT);
    widget_tokens.glass = Some(material);
    let list = crate::preview::widgets::view(
        &model,
        &widget_tokens,
        crate::preview::widgets::Frame {
            size: (520, 320),
            scale: 1.0,
            now,
            instant: Instant::now(),
            editing: false,
        },
        &mut ui.text,
    );
    widget.set_glass(Some(Glass {
        body: Rect::new(0.0, 0.0, 520.0, 320.0),
        radius: 22.0,
        material,
    }));
    widget
        .present(&mut ui, (origin.0 + 340, origin.1 + 30), (520, 320), list)
        .unwrap();
    widget.show();
    pump(1200);
    std::fs::write(root.join("live-ready.json"), serde_json::json!({"phase":"desktop","pid":std::process::id(),"backdrop":backdrop.hwnd().0 as usize,"rect":[origin.0,origin.1,origin.0+900,origin.1+660]}).to_string()).unwrap();
    for _ in 0..150 {
        if root.join("desktop-captured").exists() {
            break;
        }
        pump(100);
    }
    desktop.hide();
    widget.hide();
    let mut overflow = Surface::new(
        SurfaceKind::Popup { activate: false },
        "Glaze overflow evidence",
        Role::Dialog,
    )
    .unwrap();
    let items = (0..5)
        .map(|i| glaze_bars::tray_data::TrayItem {
            id: i.to_string(),
            name: format!("Tray app {i}"),
            icon: None,
            pinned: false,
        })
        .collect();
    let panel = super::tray::Overflow::with(items);
    let (list, size) = build(
        &panel,
        &mut ui.text,
        &look,
        1.0,
        700.0,
        (None, None, None),
        true,
    );
    overflow.set_glass(look.glass.map(|material| Glass {
        body: Rect::new(0.0, 0.0, size.0 as f32, size.1 as f32),
        radius: look.radius,
        material,
    }));
    overflow.set_glass_style(look.glass_style);
    overflow
        .present(&mut ui, (origin.0 + 300, origin.1 + 60), size, list)
        .unwrap();
    overflow.show();
    pump(800);
    let mut tray = Surface::new(
        SurfaceKind::Popup { activate: false },
        "Glaze tray menu evidence",
        Role::Menu,
    )
    .unwrap();
    let entries = vec![
        Entry::action("Pause", ()).with_icon(MenuIcon::Glyph('\u{E769}')),
        Entry::action("Themes", ()),
        Entry::action("Edit widgets", ()),
        Entry::action("Show overlay widgets", ()),
        Entry::action("Bars", ()),
        Entry::Separator,
        Entry::action("Open Studio", ()),
        Entry::action("Restart wallpaper", ()),
        Entry::Separator,
        Entry::action("Quit", ()),
    ];
    menu(
        &mut ui,
        &mut tray,
        &entries,
        &tokens,
        (origin.0 + 320, origin.1 + 75),
    );
    pump(1200);
    // Exercise the actual troublesome operation while the menu is visible.
    // SAFETY: our own parent flyout, without activation, movement or input.
    unsafe {
        SetWindowPos(
            overflow.hwnd(),
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
        )
        .unwrap();
    }
    pump(250);
    std::fs::write(root.join("live-ready.json"), serde_json::json!({"phase":"tray","pid":std::process::id(),"backdrop":backdrop.hwnd().0 as usize,"menu":tray.hwnd().0 as usize,"overflow":overflow.hwnd().0 as usize,"rect":[origin.0+270,origin.1+30,origin.0+650,origin.1+450]}).to_string()).unwrap();
    for _ in 0..150 {
        if root.join("tray-captured").exists() {
            break;
        }
        pump(100);
    }
}
