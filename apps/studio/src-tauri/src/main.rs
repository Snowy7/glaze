//! mypaper Studio: the desktop control app. It only edits `config.json` and
//! theme folders and starts or stops `mypaper-core.exe`; the engine watches
//! the config and applies changes live, so there is no other IPC.

// No console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod autostart;
mod commands;
mod engine;
mod shell;
mod source;
mod themes;
mod watch;

use tauri::Manager;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            if let Some(window) = app.get_webview_window("main") {
                app.manage(shell::apply_backdrop(&window));
            }
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
            commands::create_theme,
            commands::engine_status,
            commands::start_engine,
            commands::stop_engine,
            commands::get_autostart,
            commands::set_autostart,
            commands::shell_info,
        ])
        .run(tauri::generate_context!())
        .expect("error while running mypaper Studio");
}
