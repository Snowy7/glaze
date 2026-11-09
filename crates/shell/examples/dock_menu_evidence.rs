//! No-input, no-activate popup evidence over an opaque, fixture-owned busy window.
//! Destination labels are synthetic; real user destinations are never painted or logged.
use anyhow::Result;
use glaze_ui::{DisplayList, Entry, Glass, Kind, Op, Rect, Rgba, Role, Surface, Tokens, Ui};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use windows::Win32::UI::WindowsAndMessaging::*;

fn pump(ms: u64) {
    let until = Instant::now() + Duration::from_millis(ms);
    while Instant::now() < until {
        let mut msg = MSG::default();
        // SAFETY: only our own message queue; no input or activation.
        unsafe {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn panel(
    ui: &mut Ui,
    entries: &[Entry<()>],
    tokens: &Tokens,
    at: (i32, i32),
    hover: Option<usize>,
) -> Result<Surface> {
    let mut surface = Surface::new(
        Kind::Popup { activate: false },
        "Glaze dock menu evidence",
        Role::Menu,
    )?;
    let layout = glaze_ui::menu::layout(entries, tokens, 1.0, &mut ui.text);
    let margin = if tokens.shadow {
        tokens.menu.shadow.ceil()
    } else {
        0.0
    };
    surface.set_glass(tokens.glass.map(|material| Glass {
        body: Rect::new(margin, margin, layout.width, layout.height),
        radius: tokens.radius,
        material,
    }));
    surface.set_glass_style(tokens.glass_style);
    let list = glaze_ui::menu::view(entries, &layout, hover, tokens);
    surface.present(
        ui,
        at,
        (
            (layout.width + margin * 2.0).ceil() as u32,
            (layout.height + margin * 2.0).ceil() as u32,
        ),
        list,
    )?;
    surface.show();
    Ok(surface)
}

fn main() -> Result<()> {
    let root = PathBuf::from(std::env::args_os().nth(1).expect("evidence folder"));
    std::fs::create_dir_all(&root)?;
    glaze_desktop::enable_dpi_awareness();
    let _com = glaze_desktop::desktop_view::Com::init();
    let mut ui = Ui::new()?;
    glaze_shell::overlay_client::install();
    let monitor = glaze_desktop::monitor_infos()
        .into_iter()
        .find(|m| m.primary)
        .unwrap();
    let origin = (monitor.work.x + 100, monitor.work.y + 100);
    let mut backdrop = Surface::new(
        Kind::Popup { activate: false },
        "Glaze busy evidence window",
        Role::Pane,
    )?;
    let mut background = DisplayList::default();
    for y in 0..50 {
        for x in 0..80 {
            background.ops.push(Op::Fill {
                rect: Rect::new(x as f32 * 12.0, y as f32 * 12.0, 12.0, 12.0),
                radius: 0.0,
                color: Rgba::rgb8([if (x + y) % 2 == 0 { 64 } else { 112 } + if (x / 16 + y / 12) % 2 == 0 { 0 } else { 96 }; 3], 1.0),
            });
        }
    }
    for row in 0..18 {
        background.ops.push(Op::Text {
            rect: Rect::new(12.0, row as f32 * 32.0, 930.0, 28.0),
            text: "Glaze backdrop — sharp text outside the menu • 1234567890".into(),
            style: glaze_ui::TextStyle::new("Segoe UI", 17.0),
            color: Rgba::rgb8([245; 3], 1.0),
        });
    }
    backdrop.present(&mut ui, origin, (960, 600), background)?;
    backdrop.show();
    let look = glaze_shell::preview::flyouts::Look::load(&ui.text);
    let mut tokens = look.menu_tokens();
    tokens.shadow = false;
    let helium = std::env::args().nth(2).as_deref() == Some("helium");
    let app_name = if helium { "Helium" } else { "File Explorer" };
    let path = PathBuf::from(std::env::var_os("APPDATA").unwrap())
        .join(r"Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar")
        .join(format!("{app_name}.lnk"));
    let app = glaze_bars::apps::Item {
        key: String::new(),
        name: app_name.into(),
        path: path.to_string_lossy().into(),
        icon: None,
        pinned: true,
        windows: vec![],
        aumid: None,
        exe: None,
    };
    let sections = glaze_bars::jump_list::load(&app);
    let counts: Vec<_> = sections
        .iter()
        .enumerate()
        .map(|(i, s)| {
            (
                match s.name.as_str() {
                    "Pinned" | "Recent" | "Frequent" | "Tasks" => s.name.clone(),
                    _ => format!("App category {}", i + 1),
                },
                s.items.len(),
            )
        })
        .collect();
    let mut entries: Vec<_> = counts
        .iter()
        .map(|(name, count)| Entry::Submenu {
            label: name.clone(),
            items: (0..*count)
                .map(|i| Entry::action(&format!("Example item {}", i + 1), ()))
                .collect(),
        })
        .collect();
    entries.extend([
        Entry::Separator,
        Entry::action(app_name, ()),
        Entry::action("Unpin", ()),
        Entry::action("Close window", ()),
    ]);
    let selected = if helium {
        counts
            .iter()
            .position(|(name, _)| name == "Tasks")
            .unwrap_or(0)
    } else {
        0
    };
    let menu = panel(
        &mut ui,
        &entries,
        &tokens,
        (origin.0 + 65, origin.1 + 65),
        Some(selected),
    )?;
    let subentries: Vec<_> = sections
        .get(selected)
        .map(|s| {
            s.items
                .iter()
                .enumerate()
                .map(|(i, item)| {
                    let label = if s.name == "Tasks" {
                        item.as_ref()
                            .map(|item| item.name.as_str())
                            .filter(|name| {
                                matches!(
                                    *name,
                                    "New window" | "New incognito window" | "New private window"
                                )
                            })
                            .map(str::to_owned)
                            .unwrap_or_else(|| format!("App action {}", i + 1))
                    } else {
                        format!("Example pinned folder {}", i + 1)
                    };
                    Entry::action(&label, ())
                })
                .collect()
        })
        .unwrap_or_default();
    let submenu = panel(
        &mut ui,
        &subentries,
        &tokens,
        (origin.0 + 345, origin.1 + 65 + selected as i32 * 32),
        None,
    )?;
    pump(1800);
    let native = glaze_ui::platform::windows::glass::system();
    let frost = native.window(menu.hwnd()).map(|h| h.0 as usize);
    std::fs::write(root.join("ready.json"), serde_json::json!({"pid":std::process::id(),"backdrop":backdrop.hwnd().0 as usize,"menu":menu.hwnd().0 as usize,"submenu":submenu.hwnd().0 as usize,"frost":frost,"rect":[origin.0,origin.1,960,600],"sections":counts}).to_string())?;
    for _ in 0..400 {
        if root.join("stop").exists() {
            break;
        }
        pump(100);
    }
    Ok(())
}
