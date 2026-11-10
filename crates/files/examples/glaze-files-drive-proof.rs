//! No-activate production-view fixture; never injects input or persists a user's session.
#![allow(dead_code)]
#[path = "../src/app.rs"]
mod app;
#[path = "../src/backdrop.rs"]
mod backdrop;
#[path = "../src/backend.rs"]
mod backend;
#[path = "../src/look.rs"]
mod look;
#[path = "../src/model.rs"]
mod model;
#[path = "../src/native.rs"]
mod native;
#[path = "../src/preview.rs"]
mod preview;
#[path = "../src/shell.rs"]
mod shell;
#[path = "../src/tabs.rs"]
mod tabs;
#[path = "../src/transfer.rs"]
mod transfer;
#[path = "../src/view.rs"]
mod view;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use windows::Win32::{
    System::Ole::*,
    UI::{HiDpi::*, WindowsAndMessaging::*},
};

fn main() -> anyhow::Result<()> {
    let output = PathBuf::from(std::env::args().nth(1).expect("output directory"));
    std::fs::create_dir_all(&output)?;
    // A private profile is installed before COM, windows or workers exist.
    let profile = output.join("profile");
    std::fs::create_dir_all(profile.join("glaze"))?;
    std::fs::write(
        profile.join("glaze/files.json"),
        br#"{"pins":[],"preview":false}"#,
    )?;
    unsafe {
        std::env::set_var("APPDATA", &profile);
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)?;
        OleInitialize(None)?;
    }
    let _ = glaze_ui::set_identity(glaze_ui::Identity {
        prefix: "glaze.ui",
        desktop_classes: &["glaze.wallpaper.glass"],
    });
    let mut ui = glaze_ui::Ui::new()?;
    let mut app = app::App::new(
        "shell:MyComputerFolder".into(),
        Some(glaze_ui::Scheme::Dark),
    )?;
    let hwnd = app.surface.hwnd();
    unsafe {
        SetWindowLongPtrW(
            hwnd,
            GWL_EXSTYLE,
            GetWindowLongPtrW(hwnd, GWL_EXSTYLE) | WS_EX_NOACTIVATE.0 as isize,
        );
    }
    app.surface.place((80, 150, 1180, 760));
    // This fixture uses a matching flat frost so no other application's content is captured.
    app.theme.glass = None;
    view::draw(&mut app, &mut ui)?;
    app.surface.show();
    let end = Instant::now() + Duration::from_secs(30);
    let mut saved = false;
    while Instant::now() < end {
        unsafe {
            let mut message = MSG::default();
            while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                DispatchMessageW(&message);
            }
        }
        app.replies()?;
        if !app.loading && !app.drive_spaces.is_empty() {
            app.interaction.hover = Some(app::CLOSE);
            view::draw(&mut app, &mut ui)?;
            if !saved {
                let (width, height) = app.surface.client_size();
                let image = glaze_ui::render(&mut ui, app.surface.shown(), width, height)?;
                glaze_ui::golden::write(&image, &output.join("this-pc-canvas.png"))?;
                std::fs::write(
                    output.join("ready.json"),
                    serde_json::to_vec_pretty(&serde_json::json!({
                        "pid": std::process::id(), "hwnd": hwnd.0 as usize, "width": width, "height": height,
                        "drive_count": app.drive_spaces.len(), "corner_radius": app.theme.radius.surface * app.surface.scale()
                    }))?,
                )?;
                saved = true;
            }
        }
        unsafe {
            MsgWaitForMultipleObjectsEx(None, 100, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
        }
    }
    anyhow::ensure!(saved, "drive capacities did not arrive");
    drop(app);
    unsafe {
        OleUninitialize();
    }
    Ok(())
}
