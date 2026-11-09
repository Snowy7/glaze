//! Live flat jump-list proof. All windows are ours, no-activate, and every destination label is synthetic.
#![windows_subsystem = "windows"]
use anyhow::{Result, ensure};
use glaze_ui::menu::{Attachment, Side};
use glaze_ui::{DisplayList, Entry, Event, Glass, Kind, Menu, Op, Rect, Rgba, Role, Surface, Ui};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use windows::Win32::{Foundation::RECT, UI::WindowsAndMessaging::*};

fn pump(menu: &mut Menu<()>, ui: &mut Ui, ms: u64) {
    let until = Instant::now() + Duration::from_millis(ms);
    while Instant::now() < until {
        let mut msg = MSG::default();
        // SAFETY: dispatch only our message queue, discarding real input events.
        unsafe {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        while let Some((id, event)) = glaze_ui::next_event() {
            if matches!(event, Event::Frame) && menu.owns(id) {
                menu.handle(ui, id, &event);
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn bounds(surface: &Surface) -> Result<RECT> {
    let mut r = RECT::default();
    unsafe {
        GetWindowRect(surface.hwnd(), &mut r)?;
    }
    Ok(r)
}

fn main() {
    if let Err(error)=run() {
        if let Some(root)=std::env::args_os().nth(1){let _=std::fs::write(PathBuf::from(root).join("failure.json"),serde_json::json!({"error":error.to_string()}).to_string());}
    }
}
fn run() -> Result<()> {
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
    let origin = (monitor.work.x + 100, monitor.work.y + 40);
    let width = 900u32;
    let height = (monitor.work.height - 80).min(1150) as u32;
    let mut backdrop = Surface::new(
        Kind::Popup { activate: false },
        "Glaze test backdrop",
        Role::Pane,
    )?;
    let mut background = DisplayList::default();
    for y in 0..height.div_ceil(12) {
        for x in 0..width.div_ceil(12) {
            background.ops.push(Op::Fill {
                rect: Rect::new(x as f32 * 12.0, y as f32 * 12.0, 12.0, 12.0),
                radius: 0.0,
                color: Rgba::rgb8([if (x + y) % 2 == 0 { 80 } else { 152 }; 3], 1.0),
            });
        }
    }
    for row in 0..height / 32 {
        background.ops.push(Op::Text {
            rect: Rect::new(12.0, row as f32 * 32.0, 880.0, 28.0),
            text: "Glaze test backdrop - stand-in content - 1234567890".into(),
            style: glaze_ui::TextStyle::new("Segoe UI", 17.0),
            color: Rgba::rgb8([240; 3], 1.0),
        });
    }
    backdrop.present(&mut ui, origin, (width, height), background)?;
    backdrop.show();
    let helium = std::env::args().nth(2).as_deref() == Some("helium");
    let app_name = if helium { "Helium" } else { "File Explorer" };
    let app = glaze_bars::apps::Item {
        key: String::new(),
        name: app_name.into(),
        path: PathBuf::from(std::env::var_os("APPDATA").unwrap())
            .join(r"Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar")
            .join(format!("{app_name}.lnk"))
            .to_string_lossy()
            .into(),
        icon: None,
        pinned: true,
        windows: vec![],
        aumid: None,
        exe: None,
    };
    let look = glaze_shell::preview::flyouts::Look::load(&ui.text);
    let mut tokens = look.menu_tokens();
    tokens.shadow = false;
    glaze_shell::bars::jump_menu::style(&mut tokens);
    let s = monitor.dpi as f32 / 96.0 * tokens.scale;
    let (mut sections, app_icon) = std::thread::spawn(move || {
        let mut sections = glaze_bars::jump_list::load(&app);
        let icon =
            glaze_bars::jump_list::load_icons(&mut sections, &app.path, (16.0 * s).round() as u32);
        (sections, icon)
    })
    .join()
    .unwrap();
    for (category, section) in sections.iter_mut().enumerate() {
        if !matches!(
            section.name.as_str(),
            "Pinned" | "Recent" | "Frequent" | "Tasks"
        ) {
            section.name = format!("App category {}", category + 1);
        }
        for (index, item) in section.items.iter_mut().enumerate() {
            if let Some(item) = item {
                item.name = if section.name == "Tasks" {
                    if index == 0 {
                        "New window".into()
                    } else {
                        "New private window".into()
                    }
                } else {
                    format!(
                        "Example {} {}{}",
                        section.name.to_lowercase(),
                        index + 1,
                        if index == 0 {
                            " - a long stand-in label to check ellipsis"
                        } else {
                            ""
                        }
                    )
                };
            }
        }
    }
    let counts: Vec<_> = sections
        .iter()
        .map(|s| (s.name.clone(), s.items.iter().flatten().count()))
        .collect();
    let mut footer = vec![
        Entry::action(app_name, ()),
        Entry::action("Unpin from taskbar", ()).with_icon(glaze_ui::MenuIcon::Glyph('\u{E718}')),
        Entry::action("Close window", ()).with_icon(glaze_ui::MenuIcon::Glyph('\u{E711}')),
    ];
    if let Some(icon) = app_icon.as_deref() {
        footer[0] = footer[0]
            .clone()
            .with_icon(glaze_shell::bars::jump_menu::icon(icon));
    }
    let mut entries = glaze_shell::bars::jump_menu::rows(sections, |_| ());
    if !entries.is_empty() {
        entries.push(Entry::Separator);
    }
    entries.extend(footer.clone());
    ensure!(
        !entries
            .iter()
            .any(|e| matches!(e.plain(), Entry::Submenu { .. })),
        "jump list must be flat"
    );
    let dock_origin = (origin.0 + 210, origin.1 + height as i32 - 100);
    let mut dock = Surface::new(
        Kind::Popup { activate: false },
        "Glaze test dock",
        Role::Toolbar,
    )?;
    let mut list = DisplayList::default();
    list.ops.push(Op::Fill {
        rect: Rect::new(0.0, 20.0, 480.0, 62.0),
        radius: 16.0,
        color: Rgba::rgb8([52; 3], 0.65),
    });
    for i in 0..7 {
        list.ops.push(Op::Text {
            rect: Rect::new(
                14.0 + i as f32 * 66.0,
                if i == 3 { 0.0 } else { 28.0 },
                52.0,
                52.0,
            ),
            text: if i == 3 {
                if helium { "\u{E774}" } else { "\u{E8B7}" }
            } else {
                "\u{E7C3}"
            }
            .into(),
            style: glaze_ui::TextStyle::new("Segoe MDL2 Assets", if i == 3 { 48.0 } else { 35.0 }),
            color: Rgba::rgb8([230; 3], 1.0),
        });
    }
    dock.present(&mut ui, dock_origin, (480, 82), list)?;
    dock.set_glass_style(tokens.glass_style);
    dock.set_glass(tokens.glass.map(|material| Glass {
        body: Rect::new(0.0, 20.0, 480.0, 62.0),
        radius: 16.0,
        material,
    }));
    dock.show();
    let bar = Rect::new(dock_origin.0 as f32, dock_origin.1 as f32, 480.0, 82.0);
    let attachment = Attachment {
        item: Rect::new(
            dock_origin.0 as f32 + 212.0,
            dock_origin.1 as f32,
            52.0,
            52.0,
        ),
        bar,
        side: Side::Above,
        gap: (6.0 * s).round() as i32,
    };
    let mut menu = Menu::new("Glaze jump-list evidence")?;
    menu.set_no_activate(true);
    menu.set_width(Some(280.0));
    let screen = glaze_ui::screen_at(dock_origin.0, dock_origin.1);
    // Exercise basic-only, a short Recent section, then the asynchronous full expansion.
    menu.open_attached(&mut ui, footer.clone(), attachment, screen, &tokens)?;
    let mut variants = vec![];
    for (rows, name) in [
        (footer.clone(), "no-list"),
        (
            vec![
                Entry::Header("Recent".into()),
                Entry::action("Example recent 1", ()),
                Entry::action("Example recent 2", ()),
                Entry::Separator,
            ]
            .into_iter()
            .chain(footer)
            .collect(),
            "few-recent",
        ),
        (entries, "full-list"),
    ] {
        menu.update_entries(&mut ui, rows)?;
        pump(&mut menu, &mut ui, 80);
        let surface = menu.surfaces().next().unwrap();
        let r = bounds(surface)?;
        ensure!(
            r.bottom <= bar.top as i32 - attachment.gap,
            "menu overlaps dock after {name}"
        );
        ensure!(
            r.left >= monitor.work.x
                && r.right <= monitor.work.x + monitor.work.width
                && r.top >= monitor.work.y,
            "menu outside work area"
        );
        variants.push(serde_json::json!({"variant":name,"menu":[r.left,r.top,r.right,r.bottom],"dockVisibleTop":bar.top,"gap":bar.top as i32-r.bottom}));
    }
    let ellipsis = menu
        .surfaces()
        .next()
        .unwrap()
        .shown()
        .ops
        .iter()
        .any(|op| matches!(op,Op::Text{text,..} if text.ends_with('\u{2026}')));
    ensure!(ellipsis, "long stand-in labels must be ellipsized");
    let raised = Attachment {
        bar: attachment.bar.offset(0.0, -24.0),
        item: attachment.item.offset(0.0, -24.0),
        ..attachment
    };
    menu.reanchor(&mut ui, raised)?;
    ensure!(
        bounds(menu.surfaces().next().unwrap())?.bottom <= raised.bar.top as i32 - raised.gap,
        "magnified dock overlaps menu"
    );
    menu.reanchor(&mut ui, attachment)?;
    pump(&mut menu, &mut ui, 1200);
    let native = glaze_ui::platform::windows::glass::system();
    let surface = menu.surfaces().next().unwrap();
    std::fs::write(root.join("ready.json"),serde_json::json!({"pid":std::process::id(),"backdrop":backdrop.hwnd().0 as usize,"menu":surface.hwnd().0 as usize,"dock":dock.hwnd().0 as usize,"frost":native.window(surface.hwnd()).map(|h|h.0 as usize),"dockFrost":native.window(dock.hwnd()).map(|h|h.0 as usize),"rect":[origin.0,origin.1,width,height],"sections":counts,"variants":variants}).to_string())?;
    for _ in 0..400 {
        if root.join("stop").exists() {
            break;
        }
        pump(&mut menu, &mut ui, 100);
    }
    let _ = std::fs::remove_file(root.join("stop"));
    menu.close();pump(&mut menu,&mut ui,250);
    // The real Win32 wait has no ticker/deadline. Keep dock frost and the populated icon cache alive.
    let start=Instant::now();let mut frames=0;let mut timers=0;let mut waits=0;
    while start.elapsed()<Duration::from_secs(10){
        let remaining=Duration::from_secs(10).saturating_sub(start.elapsed());
        unsafe {MsgWaitForMultipleObjectsEx(None,remaining.as_millis().max(1) as u32,QS_ALLINPUT,MWMO_INPUTAVAILABLE);}
        waits+=1;
        let mut msg=MSG::default();
        unsafe {while PeekMessageW(&mut msg,None,0,0,PM_REMOVE).as_bool(){if msg.message==WM_TIMER{timers+=1;}let _=TranslateMessage(&msg);DispatchMessageW(&msg);}}
        while let Some((_,event))=glaze_ui::next_event(){if matches!(event,Event::Frame){frames+=1;}}
    }
    std::fs::write(root.join("idle.json"),serde_json::json!({"seconds":start.elapsed().as_secs_f64(),"frameEvents":frames,"timerMessages":timers,"waitReturns":waits,"menuOpen":menu.is_open(),"dockFrostAlive":native.window(dock.hwnd()).is_some(),"iconCachePopulated":app_icon.is_some()}).to_string())?;
    ensure!(frames==0&&timers==0,"closed menu kept frame or timer work alive");
    Ok(())
}
