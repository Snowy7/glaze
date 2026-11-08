#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod backdrop;
mod backend;
mod look;
mod model;
mod native;
mod preview;
mod shell;
mod tabs;
mod transfer;
mod view;

fn main() -> std::process::ExitCode {
    env_logger::init();
    // Glaze's window names, as the shell and Studio set them.
    let _ = glaze_ui::set_identity(glaze_ui::Identity {
        prefix: "glaze.ui",
        desktop_classes: &["glaze.wallpaper.glass"],
    });
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            log::error!("Files: {error:#}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run() -> anyhow::Result<()> {
    use glaze_ui::{FileWatch, Scheme, next_event};
    use windows::Win32::{
        Foundation::*,
        System::Ole::*,
        UI::{HiDpi::*, WindowsAndMessaging::*},
    };
    // SAFETY: before creating windows; OLE is paired with uninitialization after they close.
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        OleInitialize(None)?;
    }
    let mut args = std::env::args().skip(1);
    let mut path = String::new();
    let mut scheme = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--light" => scheme = Some(Scheme::Light),
            "--dark" => scheme = Some(Scheme::Dark),
            "--restore-routing" => {
                let mut config = glaze_config::Config::load()?;
                config.experimental.files_default = false;
                config.save()?;
                glaze_tweaks::folder_routing::FolderRouting::new()?
                    .set(false, &std::env::current_exe()?)?;
                // SAFETY: this balances the successful OleInitialize before the early CLI return.
                unsafe {
                    OleUninitialize();
                }
                return Ok(());
            }
            "--" => {
                if let Some(value) = args.next() {
                    path = value;
                }
                break;
            }
            _ => path = arg,
        }
    }
    let mut ui = glaze_ui::Ui::new()?;
    let mut app = app::App::new(path, scheme)?;
    set_window_icons(app.surface.hwnd(), app.surface.scale())?;
    let hwnd = app.surface.hwnd().0 as usize;
    let reload = WM_APP + 172;
    let mut watches = Vec::new();
    for path in [
        glaze_config::path(),
        glaze_bars::config::path(),
        glaze_tweaks::palette::path(),
    ]
    .into_iter()
    .flatten()
    {
        watches.push(FileWatch::new(&path, move || {
            // SAFETY: the callback posts no borrowed pointers; a closed window rejects the wake.
            let _ = unsafe { PostMessageW(Some(HWND(hwnd as _)), reload, WPARAM(0), LPARAM(0)) };
        })?);
    }
    view::draw(&mut app, &mut ui)?;
    app.surface.show_active(false);
    let state = std::rc::Rc::new(std::cell::RefCell::new((app, ui)));
    let weak = std::rc::Rc::downgrade(&state);
    native::set_geometry_hook(Some(Box::new(move || {
        if let Some(state) = weak.upgrade()
            && let Ok(mut state) = state.try_borrow_mut()
        {
            let (app, ui) = &mut *state;
            if let Err(error) = view::draw(app, ui) {
                log::warn!("Files resize: {error:#}");
            }
        }
    })));
    let mut message = MSG::default();
    while !state.borrow().0.closing {
        let mut redraw = false;
        // SAFETY: message storage is live and each retrieved message is dispatched on its owning UI thread.
        unsafe {
            while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                if message.message == WM_QUIT {
                    state.borrow_mut().0.closing = true;
                    break;
                }
                if message.message == reload {
                    state.borrow_mut().0.reload_look()?;
                    redraw = true;
                }
                if message.message == backdrop::CHANGED {
                    redraw = true;
                }
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        let mut borrowed = state.borrow_mut();
        let (app, ui) = &mut *borrowed;
        while let Some((id, event)) = next_event() {
            match app.event(ui, id, event) {
                Ok(changed) => redraw |= changed,
                Err(error) => {
                    app.error = Some(format!("{error:#}"));
                    redraw = true;
                }
            }
        }
        while let Some(input) = native::next() {
            let result = match input {
                native::Input::Character(character) => {
                    app.type_select(character);
                    Ok(())
                }
                native::Input::Back(forward) => {
                    if let Some(path) = app.history.go(forward) {
                        app.navigate(path, false)
                    } else {
                        Ok(())
                    }
                }
                native::Input::Key(key, modifiers) => app.key(ui, key, modifiers),
            };
            if let Err(error) = result {
                app.error = Some(format!("{error:#}"));
            }
            redraw = true;
        }
        redraw |= app.replies()?;
        let (moving, next) = app.tick();
        redraw |= moving;
        if redraw && !app.closing {
            view::draw(app, ui)?;
        }
        if app.closing {
            break;
        }
        // No timer or periodic wake while idle. Scroll animation alone requests finite waits.
        let timeout = next.map_or(u32::MAX, |d| {
            d.as_millis().clamp(1, u32::MAX as u128) as u32
        });
        drop(borrowed);
        // SAFETY: the thread waits on its own message queue with no borrowed kernel-handle array.
        unsafe {
            MsgWaitForMultipleObjectsEx(None, timeout, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
        }
    }
    transfer::flush();
    native::set_geometry_hook(None);
    drop(watches);
    drop(state);
    // SAFETY: all OLE windows and data objects have been released before balancing initialization.
    unsafe {
        OleUninitialize();
    }
    Ok(())
}

/// Match executable lookup, the window class and both task-switcher icon sizes.
fn set_window_icons(hwnd: windows::Win32::Foundation::HWND, scale: f32) -> anyhow::Result<()> {
    use anyhow::Context;
    use windows::Win32::{
        Foundation::{LPARAM, WPARAM},
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::{
            GCLP_HICON, GCLP_HICONSM, ICON_BIG, ICON_SMALL, IMAGE_ICON, LR_SHARED, LoadImageW,
            SendMessageW, SetClassLongPtrW, WM_SETICON,
        },
    };
    // SAFETY: resource 101 is embedded in our exe. LR_SHARED keeps the icons alive
    // for the window/class lifetime; Windows owns them, so do not destroy them.
    unsafe {
        let instance = GetModuleHandleW(None)?;
        for (kind, class, size) in [
            (ICON_BIG, GCLP_HICON, 32.0),
            (ICON_SMALL, GCLP_HICONSM, 16.0),
        ] {
            let size = (size * scale).round() as i32;
            let icon = LoadImageW(
                Some(instance.into()),
                windows::core::PCWSTR(101usize as *const u16),
                IMAGE_ICON,
                size,
                size,
                LR_SHARED,
            )
            .context("Files icon resource")?;
            SetClassLongPtrW(hwnd, class, icon.0 as isize);
            SendMessageW(
                hwnd,
                WM_SETICON,
                Some(WPARAM(kind as usize)),
                Some(LPARAM(icon.0 as isize)),
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod icon_tests {
    /// Run under the desktop lock. Creates only a hidden icon proof window,
    /// using the same embedded resource and setup function as Files startup.
    #[test]
    #[ignore = "live icon proof requires the desktop lock"]
    fn embedded_icons_on_a_nonactivating_window() {
        use glaze_ui::{Kind, Role, Surface, Ui};
        use windows::Win32::UI::WindowsAndMessaging::*;
        let _ui = Ui::new().unwrap();
        let surface = Surface::new(
            Kind::App { min: (200, 120) },
            "Files icon proof",
            Role::Window,
        )
        .unwrap();
        super::set_window_icons(surface.hwnd(), surface.scale()).unwrap();
        // SAFETY: queries only the HWND created on this test thread.
        unsafe {
            for (kind, class) in [(ICON_BIG, GCLP_HICON), (ICON_SMALL, GCLP_HICONSM)] {
                let window = SendMessageW(
                    surface.hwnd(),
                    WM_GETICON,
                    Some(windows::Win32::Foundation::WPARAM(kind as usize)),
                    None,
                );
                assert_ne!(window.0, 0);
                assert_eq!(window.0 as usize, GetClassLongPtrW(surface.hwnd(), class));
            }
        }
    }
}
