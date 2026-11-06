#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod model;
mod shell;
mod backend;
mod transfer;
mod native;
mod look;
mod app;
mod view;

fn main() -> std::process::ExitCode {
    env_logger::init();
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => { log::error!("Files: {error:#}"); std::process::ExitCode::FAILURE }
    }
}

fn run() -> anyhow::Result<()> {
    use mypaper_ui::{FileWatch, Scheme, next_event};
    use windows::Win32::{Foundation::*, System::Ole::*, UI::{HiDpi::*, WindowsAndMessaging::*}};
    // SAFETY: before creating windows; OLE is paired with uninitialization after they close.
    unsafe { let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2); OleInitialize(None)?; }
    let mut args = std::env::args().skip(1);
    let mut path = "shell:profile".to_owned();
    let mut scheme = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--light" => scheme = Some(Scheme::Light), "--dark" => scheme = Some(Scheme::Dark),
            "--restore-routing" => {
                let mut config = mypaper_config::Config::load()?;
                config.experimental.files_default = false;
                config.save()?;
                mypaper_tweaks::folder_routing::FolderRouting::new()?.set(false, &std::env::current_exe()?)?;
                unsafe { OleUninitialize(); } return Ok(());
            }
            "--" => { if let Some(value) = args.next() { path = value; } break; }
            _ => path = arg,
        }
    }
    let mut ui = mypaper_ui::Ui::new()?;
    let mut app = app::App::new(path, scheme)?;
    let hwnd = app.surface.hwnd().0 as usize;
    let reload = WM_APP + 172;
    let mut watches = Vec::new();
    for path in [mypaper_config::path(), mypaper_bars::config::path()].into_iter().flatten() {
        watches.push(FileWatch::new(&path, move || { let _ = unsafe { PostMessageW(Some(HWND(hwnd as _)), reload, WPARAM(0), LPARAM(0)) }; })?);
    }
    view::draw(&mut app, &mut ui)?;
    app.surface.show_active(false);
    let mut message = MSG::default();
    while !app.closing {
        let mut redraw = false;
        unsafe {
            while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                if message.message == WM_QUIT { app.closing = true; break; }
                if message.message == reload { app.reload_look()?; redraw = true; }
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        while let Some((id, event)) = next_event() {
            match app.event(&mut ui, id, event) { Ok(changed) => redraw |= changed, Err(error) => { app.error = Some(format!("{error:#}")); redraw = true; } }
        }
        while let Some(input) = native::next() {
            let result = match input {
                native::Input::Character(character) => { app.type_select(character); Ok(()) }
                native::Input::Back(forward) => if let Some(path) = app.history.go(forward) { app.navigate(path, false) } else { Ok(()) },
                native::Input::Key(key, modifiers) => app.key(&mut ui, key, modifiers),
            };
            if let Err(error) = result { app.error = Some(format!("{error:#}")); }
            redraw = true;
        }
        redraw |= app.replies()?;
        let (moving, next) = app.tick(); redraw |= moving;
        if redraw && !app.closing { view::draw(&mut app, &mut ui)?; }
        if app.closing { break; }
        // No timer or periodic wake while idle. Scroll animation alone requests finite waits.
        let timeout = next.map_or(u32::MAX, |d| d.as_millis().clamp(1, u32::MAX as u128) as u32);
        unsafe { MsgWaitForMultipleObjectsEx(None, timeout, QS_ALLINPUT, MWMO_INPUTAVAILABLE); }
    }
    transfer::flush();
    drop(watches); drop(app); drop(ui);
    unsafe { OleUninitialize(); }
    Ok(())
}
