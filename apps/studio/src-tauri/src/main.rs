//! mypaper Studio: the desktop control app. It edits `config.json`, theme
//! folders, `bars.json` and `widgets.json`, and starts or stops
//! `mypaper-core.exe`; the programs watch those files and apply changes live.
//! The one exception to "files only" is the widget host's edit mode, which is
//! toggled with a registered window message.

// No console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod autostart;
mod bars;
mod capture;
mod commands;
mod engine;
mod fsio;
mod icons;
mod look;
mod manifest;
mod shell;
mod source;
mod themes;
mod thumbs;
mod watch;
mod widgets;

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
            commands::get_skin,
            commands::set_skin,
            commands::import_skin_css,
            commands::active_skins,
            commands::get_bars,
            commands::set_bars,
            commands::reset_bars,
            commands::list_monitors,
            commands::dock_icon,
            commands::list_widgets,
            commands::get_layout,
            commands::set_widgets_appearance,
            commands::remove_instance,
            commands::reset_instance_look,
            commands::toggle_widget_edit,
            commands::components,
            commands::engine_status,
            commands::start_engine,
            commands::stop_engine,
            commands::get_autostart,
            commands::set_autostart,
        ])
        .run(tauri::generate_context!())
        .expect("error while running mypaper Studio");
}
