//! Internal-event regression fixture. Never sends input, activates, or touches another process.
#![windows_subsystem = "windows"]
use anyhow::{Context, Result, ensure};
use glaze_ui::{
    DisplayList, Entry, Event, Glass, Kind, Menu, Op, Point, Rect, Rgba, Role, Surface, Ui,
};
use std::{collections::HashMap, path::PathBuf, time::Instant};
use windows::Win32::{
    Foundation::*,
    Graphics::{Dwm::DwmFlush, Gdi::*},
    UI::WindowsAndMessaging::*,
};
#[link(name = "user32")]
unsafe extern "system" {
    fn PrintWindow(hwnd: HWND, dc: HDC, flags: u32) -> windows::core::BOOL;
}

fn pump(menu: &mut Menu<()>, ui: &mut Ui) {
    let mut msg = MSG::default();
    // SAFETY: dispatch only this fixture thread. UI input events from Windows are discarded.
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
}

fn content_pixels(surface: &Surface) -> Result<usize> {
    // SAFETY: PrintWindow only targets a verified HWND of this process, into our private DIB.
    unsafe {
        let mut pid = 0;
        GetWindowThreadProcessId(surface.hwnd(), Some(&mut pid));
        ensure!(pid == std::process::id(), "foreign capture refused");
        let mut rect = RECT::default();
        GetWindowRect(surface.hwnd(), &mut rect)?;
        let (w, h) = (
            (rect.right - rect.left).max(1),
            (rect.bottom - rect.top).max(1),
        );
        let dc = CreateCompatibleDC(None);
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let bitmap = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0)?;
        let old = SelectObject(dc, bitmap.into());
        let printed = PrintWindow(surface.hwnd(), dc, 2).as_bool();
        let pixels = std::slice::from_raw_parts(bits.cast::<u8>(), (w * h * 4) as usize);
        let count = pixels
            .chunks_exact(4)
            .filter(|p| p[0] > 20 || p[1] > 20 || p[2] > 20)
            .count();
        SelectObject(dc, old);
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(dc);
        ensure!(printed, "PrintWindow failed");
        Ok(count)
    }
}

fn edge_contrast(frost: HWND, backdrop_origin: (i32, i32)) -> Result<f32> {
    // SAFETY: each source pixel is checked against our PID before the DWM scanline is read.
    // One scanline readback per frame avoids serial GetPixel GPU stalls between samples.
    unsafe {
        let mut rect = RECT::default();
        GetWindowRect(frost, &mut rect)?;
        let (left, y, width) = (rect.left + 30, rect.bottom - 5, rect.right - rect.left - 60);
        if width < 4 {
            return Ok(0.0);
        }
        for x in left..left + width {
            let mut pid = 0;
            GetWindowThreadProcessId(WindowFromPoint(POINT { x, y }), Some(&mut pid));
            ensure!(pid == std::process::id(), "foreign pixel refused at {x},{y}; owner {pid}");
        }
        let dc = CreateCompatibleDC(None);
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -1,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let bitmap = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0)?;
        let old = SelectObject(dc, bitmap.into());
        let screen = GetDC(None);
        let copied = BitBlt(dc, 0, 0, width, 1, Some(screen), left, y, SRCCOPY);
        ReleaseDC(None, screen);
        let _ = GdiFlush();
        let pixels = std::slice::from_raw_parts(bits.cast::<u8>(), width as usize * 4);
        let mut total = 0.0;
        let mut count = 0;
        for offset in 1..width - 1 {
            if (left + offset - backdrop_origin.0).rem_euclid(16) == 0 {
                total += pixels[(offset as usize - 1) * 4]
                    .abs_diff(pixels[(offset as usize + 1) * 4]) as f32;
                count += 1;
            }
        }
        SelectObject(dc, old);
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(dc);
        copied?;
        Ok(if count > 0 { total / count as f32 } else { 0.0 })
    }
}

fn sample(
    stage: &str,
    index: usize,
    surfaces: &[&Surface],
    start: Instant,
    log: &mut Vec<serde_json::Value>,
    retained: &mut HashMap<usize, usize>,
    backdrop_origin: (i32, i32),
) -> Result<()> {
    // Synchronize to real DWM presents, rather than sampling a tight CPU-only loop.
    unsafe {
        DwmFlush()?;
        let mut foreground_pid=0;
        GetWindowThreadProcessId(GetForegroundWindow(),Some(&mut foreground_pid));
        ensure!(foreground_pid!=std::process::id(),"fixture took foreground");
    }
    let native = glaze_ui::platform::windows::glass::system();
    let mut checked = 0;
    let mut max_edge_contrast = 0.0f32;
    let mut min_content_pixels = usize::MAX;
    for surface in surfaces {
        let visible = surface.is_visible();
        let frost = native.window(surface.hwnd());
        if !visible {
            if let Some(frost) = frost {
                ensure!(
                    !unsafe { IsWindowVisible(frost) }.as_bool(),
                    "hidden content left visible frost"
                );
            }
            continue;
        }
        let frost = frost.ok_or_else(|| anyhow::anyhow!("visible content without frost"))?;
        ensure!(
            unsafe { IsWindowVisible(frost) }.as_bool(),
            "unfrosted frame"
        );
        let no_activate = unsafe { GetWindowLongPtrW(surface.hwnd(), GWL_EXSTYLE) } as u32
            & WS_EX_NOACTIVATE.0
            != 0;
        ensure!(no_activate, "fixture activated");
        let key = surface.hwnd().0 as usize;
        if let Some(previous) = retained.insert(key, frost.0 as usize) {
            ensure!(
                previous == frost.0 as usize,
                "frost recreated during retained repaint"
            );
        }
        let pixels = content_pixels(surface)?;
        ensure!(pixels > 30, "blank content frame");
        min_content_pixels = min_content_pixels.min(pixels);
        checked += 1;
        // The opaque fixture has 16px checks with 115/255 edges. Read only points verified
        // as owned by this PID, on the empty bottom inset of each frosted body. Real DWM
        // frost must suppress those edges; a merely visible but empty frost fails this check.
        let contrast = edge_contrast(frost, backdrop_origin)?;
        max_edge_contrast = max_edge_contrast.max(contrast);
        ensure!(
            contrast < 20.0,
            "unblurred DWM frame in {stage} at {index}: edge contrast {contrast}"
        );
    }
    log.push(serde_json::json!({"stage":stage,"frame":index,"elapsed_ms":start.elapsed().as_secs_f64()*1000.0,"checked_windows":checked,"min_content_pixels":min_content_pixels,"max_backdrop_edge_contrast":max_edge_contrast,"blank":0,"unfrosted":0}));
    Ok(())
}

fn body(width: f32, height: f32, hot: bool) -> DisplayList {
    let mut list = DisplayList::default();
    list.ops.push(Op::Fill {
        rect: Rect::new(0.0, 0.0, width, height),
        radius: 14.0,
        color: Rgba::HIT,
    });
    for i in 0..5 {
        list.ops.push(Op::Fill {
            rect: Rect::new(20.0 + i as f32 * 40.0, 15.0, 24.0, 24.0),
            radius: 6.0,
            color: Rgba::rgb8(if hot { [220, 180, 80] } else { [100, 180, 230] }, 1.0),
        });
    }
    list
}

fn main() {
    if let Err(error) = run() {
        if let Some(folder) = std::env::args_os().nth(1) {
            let _ = std::fs::write(
                PathBuf::from(folder).join("failure.json"),
                serde_json::json!({"error":format!("{error:#}")}).to_string(),
            );
        }
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let folder = PathBuf::from(std::env::args_os().nth(1).expect("evidence directory"));
    std::fs::create_dir_all(&folder)?;
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
        "Glaze stability backdrop",
        Role::Pane,
    )?;
    let mut background = DisplayList::default();
    for y in 0..48 {
        for x in 0..64 {
            background.ops.push(Op::Fill {
                rect: Rect::new(x as f32 * 16.0, y as f32 * 16.0, 16.0, 16.0),
                radius: 0.0,
                color: Rgba::rgb8(if (x + y) % 2 == 0 { [65; 3] } else { [180; 3] }, 1.0),
            });
        }
    }
    backdrop.present(&mut ui, origin, (1024, 768), background)?;
    backdrop.show();
    let look = glaze_shell::preview::flyouts::Look::load(&ui.text);
    let mut tokens = look.menu_tokens();
    tokens.shadow = false;
    let material = tokens.glass.expect("native material");
    let mut flyout = Surface::new(
        Kind::Popup { activate: false },
        "Glaze stability flyout",
        Role::Pane,
    )?;
    flyout.present(
        &mut ui,
        (origin.0 + 650, origin.1 + 50),
        (260, 150),
        body(260.0, 150.0, false),
    )?;
    flyout.set_glass(Some(Glass {
        body: Rect::new(0.0, 0.0, 260.0, 150.0),
        radius: 14.0,
        material,
    }));
    flyout.show();
    let mut dock = Surface::new(
        Kind::Popup { activate: false },
        "Glaze stability dock",
        Role::Toolbar,
    )?;
    let mut menu = Menu::new("Glaze stability menu")?;
    menu.set_no_activate(true);
    let items = vec![
        Entry::Submenu {
            label: "Pinned".into(),
            items: vec![
                Entry::action("Example folder", ()),
                Entry::action("Example document", ()),
            ],
        },
        Entry::action("Example app", ()),
        Entry::action("Close window", ()),
    ];
    let at = (origin.0 + 50, origin.1 + 50);
    let screen = glaze_ui::screen_at(at.0, at.1);
    let start = Instant::now();
    let mut log = vec![];
    let mut retained = HashMap::new();
    menu.open(&mut ui, items.clone(), at, screen, &tokens)?;
    for frame in 0..360 {
        let stage = match frame {
            0..=29 => "menu-open",
            30..=99 => "hover-and-submenus",
            100..=129 => "async-menu-update",
            130..=189 => "dock-reveal-hide",
            190..=289 => "magnification",
            _ => "flyout-repaint",
        };
        if (30..100).contains(&frame) {
            let root = menu.surfaces().next().unwrap();
            let id = root.id();
            let row = if frame % 20 < 10 { 0 } else { 1 };
            let screen = (
                at.0 + 80,
                at.1 + tokens.menu.pad as i32 + tokens.menu.row as i32 * row + 12,
            );
            menu.handle(
                &mut ui,
                id,
                &Event::Move(glaze_ui::platform::windows::surface::Pointer {
                    at: Point::new(80.0, 20.0),
                    screen,
                }),
            );
        }
        if frame == 100 {
            let mut updated = items.clone();
            updated.extend([
                Entry::action("Loaded destination", ()),
                Entry::action("Loaded app action", ()),
            ]);
            menu.update_entries(&mut ui, updated)?;
        }
        if (130..290).contains(&frame) {
            let n = (frame % 40) as f32;
            let width = if frame >= 190 {
                260.0 + (n / 40.0 * std::f32::consts::TAU).sin() * 35.0
            } else {
                260.0
            };
            let y = origin.1 + 550 + if frame < 190 { n as i32 } else { 0 };
            dock.present(
                &mut ui,
                (origin.0 + 300, y),
                (width.ceil() as u32, 70),
                body(width, 70.0, frame % 2 == 0),
            )?;
            dock.set_glass(Some(Glass {
                body: Rect::new(0.0, 0.0, width, 70.0),
                radius: 14.0,
                material,
            }));
            if frame % 40 == 39 {
                dock.hide();
            } else {
                dock.show();
            }
        }
        if frame >= 290 {
            flyout.present(
                &mut ui,
                (origin.0 + 650, origin.1 + 50),
                (260, 150),
                body(260.0, 150.0, frame % 2 == 0),
            )?;
        }
        pump(&mut menu, &mut ui);
        let mut surfaces: Vec<_> = menu.surfaces().collect();
        surfaces.extend([&flyout, &dock]);
        sample(
            stage,
            frame,
            &surfaces,
            start,
            &mut log,
            &mut retained,
            origin,
        ).with_context(||format!("{stage} frame {frame}"))?;
    }
    let summary = serde_json::json!({"frames":log.len(),"blank_frames":0,"unfrosted_frames":0,"foreground_untouched":true,"internal_events_only":true,"samples":log});
    std::fs::write(
        folder.join("frame-timing.json"),
        serde_json::to_vec_pretty(&summary)?,
    )?;
    let _ = std::fs::remove_file(folder.join("failure.json"));
    println!(
        "360 DWM-synchronized frames: no blank content, no missing frost, retained frost HWNDs"
    );
    Ok(())
}
