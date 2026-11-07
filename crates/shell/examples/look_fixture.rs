//! Isolated, no-input live evidence for Look. Uses production views and the config watcher;
//! no providers, app-bar reservations, taskbar changes, or glass-plate snapshot writes.
use anyhow::{Result, ensure};
use mypaper_bars::BarsConfig;
use mypaper_config::Config;
use mypaper_shell::{
    glass::Family,
    look::settings,
    preview::{bars, flyouts, widgets},
};
use mypaper_ui::{DisplayList, Entry, FileWatch, Glass, Kind, Rect, Role, Surface, Ui};
use mypaper_widgets::{
    layout::{Grid, Layout, RectF},
    registry,
    storage::Storage,
};
use std::{
    io::Write,
    path::PathBuf,
    time::{Duration, Instant},
};
use windows::Win32::{
    Foundation::*,
    System::{SystemInformation::GetLocalTime, Threading::GetCurrentThreadId},
    UI::WindowsAndMessaging::*,
};

fn main() -> Result<()> {
    let root = PathBuf::from(std::env::var_os("MYPAPER_LOOK_FIXTURE").ok_or_else(|| {
        anyhow::anyhow!("MYPAPER_LOOK_FIXTURE must name the isolated config folder")
    })?);
    ensure!(
        mypaper_config::path()? == root.join("config.json")
            && root.to_string_lossy().contains("look-fixture"),
        "fixture APPDATA must be isolated"
    );
    run(root)
}

fn run(root: PathBuf) -> Result<()> {
    mypaper_desktop::enable_dpi_awareness();
    let _com = mypaper_desktop::desktop_view::Com::init();
    let mut ui = Ui::new()?;
    mypaper_shell::overlay_client::install();
    let inbox = mypaper_shell::inbox::Inbox::new()?;
    let _watch = mypaper_desktop::watch_config(|| {
        mypaper_shell::inbox::post(mypaper_shell::inbox::WM_CONFIG, 0)
    })?;
    let thread = unsafe { GetCurrentThreadId() };
    let _stop = FileWatch::new(&root.join("stop"), move || {
        let _ = unsafe { PostThreadMessageW(thread, WM_QUIT, WPARAM(0), LPARAM(0)) };
    })?;
    let monitor = mypaper_desktop::monitor_infos()
        .into_iter()
        .find(|m| m.primary)
        .ok_or_else(|| anyhow::anyhow!("no primary monitor"))?;
    let origin = (
        monitor.work.x + monitor.work.width - 650,
        monitor.work.y + 70,
    );
    // An opaque fixture-owned backdrop keeps transparent text screenshots free of other apps.
    let mut backdrop = Surface::new(
        Kind::Popup { activate: false },
        "mypaper Look fixture backdrop",
        Role::Pane,
    )?;
    let mut background = DisplayList::default();
    for row in 0..16 {
        for col in 0..13 {
            background.ops.push(mypaper_ui::Op::Fill {
                rect: Rect::new(col as f32 * 50.0, row as f32 * 50.0, 50.0, 50.0),
                radius: 0.0,
                color: mypaper_ui::Rgba::rgb8(
                    if (col + row) % 2 == 0 {
                        [105, 135, 165]
                    } else {
                        [150, 175, 195]
                    },
                    1.0,
                ),
            });
        }
    }
    backdrop.present(
        &mut ui,
        (origin.0 - 10, origin.1 - 10),
        (650, 800),
        background,
    )?;
    backdrop.show();
    let mut windows = (0..6)
        .map(|i| {
            Surface::new(
                Kind::Popup { activate: false },
                &format!("mypaper Look fixture {i}"),
                Role::Pane,
            )
        })
        .collect::<Result<Vec<_>>>()?;
    let field = mypaper_ui::TextField::new(windows[5].hwnd(), Rect::new(14.0, 5.0, 590.0, 30.0), false, 14.0, mypaper_ui::Rgba::rgb8([255;3], 1.0))?;
    field.set_text("Editable text keeps its content")?;
    field.prepare()?;
    let mut revisions = 0u64;
    let until = Instant::now() + Duration::from_secs(115);
    let mut dirty = true;
    loop {
        if dirty {
            let config = Config::load()?;
            revisions += 1;
            let report = draw(&mut ui, &mut windows, &field, &config, origin, &root)?;
            let mut log = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(root.join("revisions.jsonl"))?;
            writeln!(
                log,
                "{}",
                serde_json::json!({"revision":revisions,"pid":std::process::id(),"look":config.look,"surfaces":report})
            )?;
            dirty = false;
        }
        let remaining = until.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        unsafe {
            MsgWaitForMultipleObjectsEx(
                None,
                remaining.as_millis().min(u32::MAX as u128) as u32,
                QS_ALLINPUT,
                MWMO_INPUTAVAILABLE,
            );
        }
        let mut msg = MSG::default();
        while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
            if msg.message == WM_QUIT {
                return Ok(());
            }
            unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        while let Some(msg) = mypaper_shell::inbox::next() {
            dirty |= msg.message == mypaper_shell::inbox::WM_CONFIG;
        }
        while mypaper_ui::next_event().is_some() {}
    }
    drop(inbox);
    Ok(())
}

fn draw(
    ui: &mut Ui,
    windows: &mut [Surface],
    field: &mypaper_ui::TextField,
    config: &Config,
    origin: (i32, i32),
    root: &std::path::Path,
) -> Result<Vec<serde_json::Value>> {
    let user = &config.look;
    mypaper_ui::theme::set_active(
        match settings::style(user, Family::Widgets).unwrap_or_default() {
            mypaper_bars::style::NativeStyle::Liquid => mypaper_ui::Style::Liquid,
            mypaper_bars::style::NativeStyle::OneUi => mypaper_ui::Style::OneUi,
            mypaper_bars::style::NativeStyle::Fluent => mypaper_ui::Style::Fluent,
            mypaper_bars::style::NativeStyle::Solid => mypaper_ui::Style::Solid,
            mypaper_bars::style::NativeStyle::Mono => mypaper_ui::Style::Mono,
        },
    );
    let system = mypaper_bars::accent::read();
    let mut config_bars = BarsConfig::parse(
        r#"{"top":{"layout":"strip","left":["start","app"],"center":["clock"],"right":["volume","power"]},"dock":{"pinned":[],"extras":["desktop","trash"]}}"#,
    )?;
    config_bars.resolve(Vec::new);
    let clock = unsafe { GetLocalTime() };
    let model = bars::sample_model(clock, "Look settings");
    let mut report = Vec::new();
    for (i, key) in ["top", "dock"].iter().enumerate() {
        let mut look = if i == 0 {
            config_bars.top.look.clone()
        } else {
            config_bars.dock.look.clone()
        };
        let mut a = mypaper_theme::Appearance::default();
        settings::bar(
            user,
            Family::Bars,
            &config_bars,
            key,
            &mut look,
            &mut a,
            &system,
            &ui.text,
        );
        let tokens = bars::Tokens::resolve_settings(&look, &a, &system, user, &ui.text);
        let mut images = bars::Images::default();
        let frame = if i == 0 {
            bars::top(
                620,
                &config_bars,
                &model,
                &tokens,
                tokens.scale,
                ui,
                &mut images,
                None,
            )
        } else {
            bars::dock(
                &config_bars,
                &model,
                &[],
                &tokens,
                tokens.scale,
                (620, 110),
                &[],
                &[],
                ui,
                &mut images,
                None,
            )
        };
        let body = frame.shapes.first().copied();
        let glass = body.and_then(|(body, radius)| {
            tokens.material.map(|material| Glass {
                body,
                radius,
                material,
            })
        });
        present(
            ui,
            &mut windows[i],
            frame.list,
            frame.size,
            (origin.0, origin.1 + i as i32 * 110),
            glass,
            root,
            key,
            &mut report,
        )?;
    }
    for (i, bare) in [false, true].iter().enumerate() {
        let registry = registry::builtins();
        let mut layout = Layout::default();
        let id = layout.add(
            "quote",
            "fixture",
            [0, 0],
            registry.get("quote").unwrap().manifest.size.default,
        );
        let mut base = mypaper_widgets::look::defaults();
        if *bare {
            base.surface = Some(mypaper_theme::Surface::None);
        }
        let mut a = settings::appearance(user, Family::Widgets, &base, *bare, &system);
        if let Some(name) = a.font.as_mut() {
            *name = settings::font(name, |f| ui.text.has_family(f));
        }
        let a = mypaper_widgets::look::resolve(
            &Default::default(),
            &Default::default(),
            &a,
            mypaper_theme::Color([80, 140, 240]),
        );
        let grid = Grid::new(
            &layout.grid,
            [96.0; 2],
            RectF {
                x: 0.0,
                y: 0.0,
                width: 620.0,
                height: 180.0,
            },
        );
        let placed = mypaper_widgets::layout::place(
            layout.get(&id).unwrap(),
            &registry.get("quote").unwrap().manifest,
            &grid,
            &a,
        );
        let mut model = widgets::Model::new(
            placed,
            &mut Storage::new(root.join("data")),
            widgets::Stamp::now(),
        )?;
        model.corner_override = user.shape.corners;
        model.edge_override = user.shape.edge;
        let mut tokens = widgets::tokens(&model.placed.appearance);
        tokens.icon_set = settings::icons(user, &ui.text);
        let size = (620, 150);
        let list = widgets::view(
            &model,
            &tokens,
            widgets::Frame {
                size,
                scale: tokens.scale,
                now: widgets::Stamp::now(),
                instant: Instant::now(),
                editing: false,
            },
            &mut ui.text,
        );
        let glass = (!bare)
            .then(|| mypaper_ui::theme::material(mypaper_ui::theme::active(), tokens.material.tint))
            .flatten()
            .map(|material| Glass {
                body: Rect::new(0.0, 0.0, 620.0, 150.0),
                radius: tokens.radius * tokens.scale,
                material,
            });
        present(
            ui,
            &mut windows[i + 2],
            list,
            size,
            (origin.0, origin.1 + 230 + i as i32 * 170),
            glass,
            root,
            if *bare { "bare" } else { "widget" },
            &mut report,
        )?;
    }
    let mut bar_look = config_bars.top.look.clone();
    let mut a = mypaper_theme::Appearance::default();
    settings::bar(
        user,
        Family::Menus,
        &config_bars,
        "top",
        &mut bar_look,
        &mut a,
        &system,
        &ui.text,
    );
    let panel = flyouts::Look::resolve_settings(
        &bar_look,
        &a,
        &system,
        &mypaper_shell::glass::knobs(user, Family::Menus),
        user,
        &ui.text,
    );
    let tokens = panel.menu_tokens();
    let entries = vec![
        Entry::action("Settings", ()).with_icon(mypaper_ui::MenuIcon::Glyph('\u{E713}')),
        Entry::action("Refresh", ()).with_icon(mypaper_ui::MenuIcon::Glyph('\u{E72C}')),
    ];
    let layout = mypaper_ui::menu::layout(&entries, &tokens, tokens.scale, &mut ui.text);
    let list = mypaper_ui::menu::view(&entries, &layout, None, &tokens);
    let size = (
        (layout.width + 28.0 * tokens.scale).ceil() as u32,
        (layout.height + 28.0 * tokens.scale).ceil() as u32,
    );
    let body = Rect::new(
        14.0 * tokens.scale,
        14.0 * tokens.scale,
        layout.width,
        layout.height,
    );
    let glass = tokens.glass.map(|material| Glass {
        body,
        radius: tokens.radius * tokens.scale,
        material,
    });
    present(
        ui,
        &mut windows[4],
        list,
        size,
        (origin.0, origin.1 + 580),
        glass,
        root,
        "menu",
        &mut report,
    )?;
    let name = settings::font(user.text.font.as_deref().unwrap_or("Segoe UI"), |f| ui.text.has_family(f));
    field.set_style(&mypaper_ui::TextStyle::new(&name, 14.0 * user.text.size.unwrap_or(1.0)), mypaper_ui::Rgba::rgb8([255;3],1.0));
    let mut input = DisplayList::default();
    input.ops.push(mypaper_ui::Op::Fill {rect:Rect::new(0.0,0.0,620.0,40.0),radius:user.shape.corners.unwrap_or(8.0),color:mypaper_ui::Rgba::rgb8([28,28,30],1.0)});
    input.ops.push(mypaper_ui::Op::TextField {rect:Rect::new(14.0,5.0,590.0,30.0),field:field.clone(),revision:field.revision()});
    present(ui,&mut windows[5],input,(620,40),(origin.0,origin.1+745),None,root,"field",&mut report)?;
    Ok(report)
}

#[allow(clippy::too_many_arguments)]
fn present(
    ui: &mut Ui,
    surface: &mut Surface,
    list: DisplayList,
    size: (u32, u32),
    at: (i32, i32),
    glass: Option<Glass>,
    root: &std::path::Path,
    name: &str,
    report: &mut Vec<serde_json::Value>,
) -> Result<()> {
    let radius = glass.as_ref().map(|g| g.radius);
    surface.set_glass(glass);
    surface.present(ui, at, size, list)?;
    surface.show();
    mypaper_ui::golden::write(
        &mypaper_ui::render(ui, surface.shown(), size.0, size.1)?,
        &root.join(format!("{name}.png")),
    )?;
    report.push(serde_json::json!({"name":name,"hwnd":surface.hwnd().0 as usize,"at":at,"size":size,"glass_radius":radius}));
    Ok(())
}
