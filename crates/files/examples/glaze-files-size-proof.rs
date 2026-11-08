//! Manual visual fixture for index metadata, independent of the live index's current generation.
//! Run under the desktop lock; this window closes automatically after 20 seconds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![allow(
    dead_code,
    reason = "the fixture shares the production browser modules"
)]
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

fn main() -> anyhow::Result<()> {
    use std::{
        sync::Arc,
        time::{Duration, Instant},
    };
    use windows::Win32::{
        System::Ole::*,
        UI::{HiDpi::*, WindowsAndMessaging::*},
    };
    // SAFETY: initialize DPI/OLE before creating this fixture's windows, balancing OLE below.
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        OleInitialize(None)?;
    }
    let root = r"C:\glaze-targets\files\scratch\size-flags";
    std::fs::create_dir_all(root)?;
    let mut ui = glaze_ui::Ui::new()?;
    let mut app = app::App::new(root.into(), Some(glaze_ui::Scheme::Dark))?;
    let mut entries = Vec::new();
    for (name, bytes, unknown, excluded) in [
        ("Exact data", 1_073_741_824, false, false),
        ("Partial data", 4_509_715_661, true, false),
        ("node_modules", 0, true, true),
    ] {
        let path = format!("{root}\\{name}");
        app.sizes
            .insert(path.clone(), model::Size::indexed(bytes, unknown, excluded));
        entries.push(model::Entry {
            path,
            name: name.into(),
            directory: true,
            size: None,
            modified: 0,
            kind: "Folder".into(),
            hidden: false,
        });
    }
    app.entries = Arc::new(entries);
    app.shown = app.entries.clone();
    app.location = Some(shell::Location {
        path: root.into(),
        name: "Size metadata".into(),
        parent: None,
        filesystem: true,
    });
    app.loading = false;
    app.preferences.preview = false;
    app.preferences.view = model::View::Details;
    app.selection.choose(1, false, false);
    app.selection.choose(2, true, false);
    app.interaction.hover = Some(app::ITEM + 2);
    view::draw(&mut app, &mut ui)?;
    app.surface.show_active(false);
    let end = Instant::now() + Duration::from_secs(20);
    let mut message = MSG::default();
    while Instant::now() < end && !app.closing {
        // SAFETY: process only this fixture's UI queue; no synthetic input is generated.
        unsafe {
            while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        while let Some((_, event)) = glaze_ui::next_event() {
            if matches!(event, glaze_ui::Event::Close) {
                app.closing = true;
            }
        }
        // Keep fixture metadata fixed: deliberately do not consume live index replies.
        view::draw(&mut app, &mut ui)?;
        // SAFETY: a finite wait on the current thread's message queue, only in this manual fixture.
        unsafe {
            MsgWaitForMultipleObjectsEx(None, 1000, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
        }
    }
    drop(app);
    // SAFETY: all fixture OLE objects have been dropped.
    unsafe {
        OleUninitialize();
    }
    Ok(())
}
