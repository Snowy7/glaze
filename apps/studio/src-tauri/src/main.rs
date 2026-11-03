//! mypaper Studio: the desktop control app. It only edits `config.json` and
//! theme folders and starts or stops `mypaper-core.exe`; the engine watches
//! the config and applies changes live, so there is no other IPC.

// No console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod autostart;
mod capture;
mod commands;
mod engine;
mod manifest;
mod shell;
mod source;
mod themes;
mod thumbs;
mod watch;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let window = capture::build_main_window(app.handle())?;
            shell::match_chrome(&window);
            // Live updates are a nicety: Studio still works if watching fails.
            if let Err(e) = watch::start(app.handle().clone()) {
                eprintln!("not watching the config folder: {e:#}");
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_config,
            commands::update_config,
            commands::reset_config,
            commands::list_themes,
            commands::inspect_source,
            commands::inspect_url,
            commands::create_theme,
            commands::generate_preview,
            commands::duplicate_theme,
            commands::get_style,
            commands::set_style,
            commands::engine_status,
            commands::start_engine,
            commands::stop_engine,
            commands::get_autostart,
            commands::set_autostart,
        ])
        .run(tauri::generate_context!())
        .expect("error while running mypaper Studio");
}
