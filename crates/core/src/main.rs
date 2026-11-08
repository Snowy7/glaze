//! glaze-core supervises the wallpaper engine and native shell, restores the taskbar after
//! shell exit, and owns the tray, styling journal and panic hotkey.
//!
//! ```text
//! glaze-core [THEME]
//! ```
//!
//! The wallpaper follows `config.json` on its own (theme, pausing), so core
//! passes it nothing; an explicit `THEME` is forwarded as a run-only override.
//! Core owns the tray icon (pause, themes, studio, restart, quit).
//!
//! **Panic hotkey: Ctrl+Alt+Shift+Backspace** stops everything and restores
//! the stock desktop, whatever state Glaze is in.

// No console window in release builds; logs go to a file (see init_logging).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod autostart;
mod bars;
mod child;
mod clean_env;
mod companions;
mod desktop_extras;
mod restart;
mod styling;
mod system;
mod tray;

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use glaze_config::Config;
use glaze_desktop::{DesktopHost, startup};
use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, HANDLE, WAIT_OBJECT_0};
use windows::Win32::System::Threading::{CreateMutexW, INFINITE};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, RegisterHotKey, UnregisterHotKey, VK_BACK,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, MSG, MsgWaitForMultipleObjects, PM_REMOVE, PeekMessageW, QS_ALLINPUT,
    TranslateMessage, WM_HOTKEY, WM_QUIT,
};
use windows::core::w;

use crate::child::{Outcome, Supervised};
use crate::desktop_extras::DesktopExtras;
use crate::styling::{Styling, WM_STYLE_CONFIG, WM_STYLE_PALETTE};
use crate::tray::{Tray, WM_RESTART_WALLPAPER};

/// Pause before relaunching after "Restart wallpaper".
const MANUAL_RESTART_DELAY: Duration = Duration::from_millis(250);

const PANIC_HOTKEY: i32 = 1;

fn main() -> ExitCode {
    // `--autostart on|off|status` configures "Start with Windows" and exits (see autostart.rs).
    if let Some(code) = autostart::cli(std::env::args().skip(1)) {
        return code;
    }
    // Before any thread: everything Glaze launches inherits this environment (clean_env.rs).
    clean_env::reset();
    glaze_desktop::init_logging();
    glaze_desktop::quit_on_ctrl_c();
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            log::error!("{e:#}");
            ExitCode::FAILURE
        }
    }
}

/// Startup order (docs/performance.md, "Startup"): the children start first, so they work
/// while core sets up its own tray and styling. Before the spawn, core only checks it's the one
/// instance, takes the panic hotkey and waits for Explorer if it is not up yet. The shell is
/// spawned immediately afterward and owns the early taskbar hide.
fn run() -> Result<()> {
    startup::ready("core", "main_ms");
    // SAFETY: named mutex we hold for the lifetime of the process.
    let _instance = unsafe { CreateMutexW(None, true, w!("Local\\glaze-core"))? };
    // SAFETY: reads the calling thread's last error.
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        bail!("Glaze is already running");
    }
    // SAFETY: thread-level hotkey; WM_HOTKEY arrives in our message queue.
    unsafe {
        RegisterHotKey(
            None,
            PANIC_HOTKEY,
            MOD_CONTROL | MOD_ALT | MOD_SHIFT | MOD_NOREPEAT,
            VK_BACK.0.into(),
        )
        .context("panic hotkey Ctrl+Alt+Shift+Backspace is taken by another app")?;
    }
    let exe = std::env::current_exe()?;
    let wallpaper_exe = exe.with_file_name("glaze-wallpaper.exe");
    if !wallpaper_exe.exists() {
        bail!(
            "{} not found; build the workspace first (cargo build)",
            wallpaper_exe.display()
        );
    }
    bars::wait_for_shell();
    bars::stop_leftover()?;
    // The shell paints before acquiring the taskbar journal and hiding Explorer. Start it
    // before the wallpaper and all of core's optional work.
    startup::ready("core", "shell_spawn_begin_ms");
    if let Err(e) = glaze_desktop::tiling_recovery::restore_pending() {
        log::error!("recovering tiling before shell startup: {e:#}");
    }
    let mut shell = start_optional("shell", exe.with_file_name("glaze-shell.exe"));
    startup::ready("core", "shell_spawn_ms");

    // The wallpaper is essential: when it stops for good, so does glaze. It reads the config
    // itself and exits with EXIT_CONFIG on a bad one, which stops core too. Only an explicit
    // theme override is forwarded, as an absolute path.
    // `--parent` makes it exit with core however core ends (a crash or a kill too), so no
    // orphaned engine keeps drawing on the desktop; the shell watches its parent on its own.
    let theme_override = std::env::args().nth(1);
    let mut wallpaper_args = vec![
        PathBuf::from("--parent"),
        PathBuf::from(std::process::id().to_string()),
    ];
    if let Some(name) = &theme_override {
        wallpaper_args.push(glaze_theme::locate(Some(name)).context("theme to start with")?);
    }
    let mut wallpaper = Supervised::new("wallpaper", wallpaper_exe, wallpaper_args);
    wallpaper.start()?;
    startup::ready("core", "spawn_ms");
    log::info!("running; panic hotkey: Ctrl+Alt+Shift+Backspace");

    // Failing to show a tray icon must not stop the wallpaper.
    let _tray = Tray::new()
        .inspect_err(|e| log::warn!("no tray icon: {e:#}"))
        .ok();

    // Native Windows styling of the active theme. Created after the single-instance check: it
    // undoes a crashed run's leftovers, which a second instance must never do to a live one.
    // Dropping it (any exit path) restores everything.
    let config = Config::load()
        .inspect_err(|e| log::warn!("config.json: {e:#}"))
        .unwrap_or_default();
    let theme = theme_override.as_deref().unwrap_or(&config.theme);
    log::info!("theme: {theme}");
    let mut styling = Styling::new(&config.theme)
        .inspect_err(|e| log::warn!("no window styling: {e:#}"))
        .ok();
    if let Some(styling) = &mut styling {
        styling.set_theme(theme);
    }

    // The classic-menu switch (opt-in), via config.json (docs/desktop-extras.md). The desktop
    // right-click menu is the native shell's.
    let mut extras = DesktopExtras::new(&config);
    let mut system = system::SystemSettings::new(&config);
    let index_exe = exe.with_file_name("glaze-index.exe");
    let mut file_index = config
        .experimental
        .file_index
        .then(|| start_optional("index", index_exe.clone()))
        .flatten();
    // The app styler (experimental; docs/windows-styling.md, "App styler") runs only while its
    // flag is on. Stopping it, by any path, is what puts Explorer and Settings back.
    let styler_exe = exe.with_file_name("glaze-styler.exe");
    let mut app_styler = config
        .experimental
        .app_styler
        .then(|| start_optional("styler", styler_exe.clone()))
        .flatten();

    let panicked = loop {
        let children = [
            Some(&wallpaper),
            shell.as_ref(),
            file_index.as_ref(),
            app_styler.as_ref(),
        ];
        let handles: Vec<HANDLE> = children
            .iter()
            .flatten()
            .filter_map(|c| c.handle())
            .collect();
        let owners: Vec<&'static str> = children
            .iter()
            .flatten()
            .filter(|c| c.handle().is_some())
            .map(|c| c.name)
            .collect();
        let next_restart = children
            .iter()
            .flatten()
            .filter_map(|c| c.restart_at())
            .min();
        let timeout = next_restart.map_or(INFINITE, |t| {
            t.saturating_duration_since(Instant::now())
                .as_millis()
                .min(u128::from(INFINITE - 1)) as u32
        });
        let wait_on = (!handles.is_empty()).then_some(handles.as_slice());
        // SAFETY: each handle stays valid while its child is alive (owned by `Supervised`).
        let woke = unsafe { MsgWaitForMultipleObjects(wait_on, false, timeout, QS_ALLINPUT) };

        let exited = (woke.0.wrapping_sub(WAIT_OBJECT_0.0) as usize) < handles.len();
        if exited {
            let index = woke.0.wrapping_sub(WAIT_OBJECT_0.0) as usize;
            match owners[index] {
                name if name == wallpaper.name => {
                    if wallpaper.on_exit()? == Outcome::Stopped {
                        break false;
                    }
                }
                name if shell.as_ref().is_some_and(|s| s.name == name) => {
                    if let Err(e) = glaze_desktop::tiling_recovery::restore_pending() {
                        log::error!("recovering tiling after shell exit: {e:#}");
                    }
                    // The exited shell released its lease. Its replacement may paint while
                    // core owns the journal, then waits before changing Explorer or the journal.
                    let journal = glaze_bars::taskbar::JournalLease::acquire()?;
                    if let Err(e) = glaze_bars::taskbar::Taskbar::native()
                        .and_then(|mut t| t.restore_visibility())
                    {
                        log::warn!("revealing the native taskbar after shell exit: {e:#}");
                    }
                    if let Some(child) = &mut shell {
                        if child.on_exit()? == Outcome::Stopped {
                            shell = None;
                        } else if let Err(e) = child.restart_if_due() {
                            log::warn!("{e:#}; continuing without the shell");
                            shell = None;
                        }
                    }
                    // The new shell presents first, then waits on this lease before any
                    // journal access. Explorer's synchronous restore overlaps its renderer startup.
                    bars::replay_journal();
                    drop(journal);
                }
                "index" => {
                    if let Some(child) = &mut file_index
                        && child.on_exit()? == Outcome::Stopped
                    {
                        file_index = None;
                    }
                }
                "styler" => {
                    if let Some(child) = &mut app_styler
                        && child.on_exit()? == Outcome::Stopped
                    {
                        app_styler = None;
                    }
                }
                _ => {}
            }
        }
        wallpaper.restart_if_due()?;
        if let Some(child) = &mut shell
            && let Err(e) = child.restart_if_due()
        {
            log::warn!("{e:#}; continuing without the shell");
            shell = None;
        }
        let pumped = pump();
        if let Some(child) = &mut file_index
            && let Err(error) = child.restart_if_due()
        {
            log::warn!("{error:#}; continuing without file search");
            file_index = None;
        }
        if let Some(child) = &mut app_styler
            && let Err(error) = child.restart_if_due()
        {
            log::warn!("{error:#}; continuing without the app styler");
            app_styler = None;
        }
        match pumped.signal {
            Some(Signal::Panic) => {
                log::warn!("panic hotkey: stopping everything");
                break true;
            }
            Some(Signal::Quit) => break false,
            Some(Signal::Restart) => {
                log::info!("restarting the wallpaper");
                wallpaper.restart_after(MANUAL_RESTART_DELAY);
                // A killed wallpaper cannot clean up after itself.
                if let Ok(host) = DesktopHost::find() {
                    host.restore();
                }
            }
            None => {}
        }
        if let Some(styling) = &mut styling {
            if pumped.config_changed {
                styling.follow_config();
            }
            if pumped.palette_changed {
                styling.palette_changed();
            }
        }
        if pumped.config_changed {
            extras.follow_config();
            system.follow_config();
            if let Ok(config) = Config::load() {
                if config.experimental.file_index && file_index.is_none() {
                    file_index = start_optional("index", index_exe.clone());
                } else if !config.experimental.file_index {
                    if let Some(child) = &mut file_index {
                        child.stop();
                    }
                    file_index = None;
                }
                if config.experimental.app_styler && app_styler.is_none() {
                    app_styler = start_optional("styler", styler_exe.clone());
                } else if !config.experimental.app_styler {
                    if let Some(child) = &mut app_styler {
                        child.stop();
                    }
                    app_styler = None;
                }
            }
        }
    };

    // On-demand apps are not supervised/restarted, but belong to this desktop's lifetime.
    // Close them while the shell and wallpaper still serve their final saves/cleanup.
    companions::close(&exe);
    // Stop and reap first; only then may core replay the shell's taskbar journal.
    if let Some(child) = &mut shell {
        child.stop_gracefully(bars::ask_to_close, bars::GRACE);
    }
    bars::replay_journal();
    wallpaper.stop();
    if let Err(e) = glaze_desktop::tiling_recovery::restore_pending() {
        log::error!("recovering tiling on shutdown: {e:#}");
    }
    if let Some(child) = &mut file_index {
        child.stop();
    }
    if let Some(child) = &mut app_styler {
        child.stop();
    }
    if let Some(styling) = &mut styling {
        styling.stop();
    }
    system.stop();
    if let Ok(host) = DesktopHost::find() {
        host.restore();
    }
    // The stock desktop includes the user's own Windows wallpaper (a quit keeps the theme's
    // still, which is what Windows shows until Glaze starts again).
    if panicked && let Err(e) = glaze_desktop::desktop_wallpaper::restore() {
        log::error!("putting the Windows wallpaper back: {e:#}");
    }
    // SAFETY: unregisters the hotkey this thread registered.
    let _ = unsafe { UnregisterHotKey(None, PANIC_HOTKEY) };
    log::info!("desktop restored");
    Ok(())
}

/// Starts an optional child when its executable is there. `None` when it's missing or didn't
/// start. The index and the app styler get `--parent <core's pid>` and exit with core, however
/// core ends (the styler's exit is what puts Explorer and Settings back).
fn start_optional(name: &'static str, exe: PathBuf) -> Option<Supervised> {
    if !exe.exists() {
        return None;
    }
    let args = if matches!(name, "index" | "styler") {
        vec![
            PathBuf::from("--parent"),
            PathBuf::from(std::process::id().to_string()),
        ]
    } else {
        Vec::new()
    };
    let mut child = Supervised::new(name, exe, args);
    match child.start() {
        Ok(()) => Some(child),
        Err(e) => {
            log::warn!("{e:#}; continuing without the {name}");
            None
        }
    }
}

/// What the message queue asked of the supervisor, least urgent first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Signal {
    Restart,
    Quit,
    Panic,
}

/// What one drain of the message queue found.
#[derive(Debug, Default)]
struct Pumped {
    /// The most urgent request in the queue.
    signal: Option<Signal>,
    /// `config.json` changed: the theme (and its styling) may have too.
    config_changed: bool,
    /// The wallpaper wrote a new `palette.json`.
    palette_changed: bool,
}

/// Drains the message queue (running the tray's window procedure and any
/// window-event callbacks of the styling).
fn pump() -> Pumped {
    let mut msg = MSG::default();
    let mut pumped = Pumped::default();
    // SAFETY: standard message pump on the thread that owns the tray window.
    unsafe {
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            let requested = match msg.message {
                WM_HOTKEY if msg.wParam.0 == PANIC_HOTKEY as usize => Signal::Panic,
                WM_QUIT => Signal::Quit,
                WM_RESTART_WALLPAPER => Signal::Restart,
                WM_STYLE_CONFIG => {
                    pumped.config_changed = true;
                    continue;
                }
                WM_STYLE_PALETTE => {
                    pumped.palette_changed = true;
                    continue;
                }
                _ => {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                    continue;
                }
            };
            pumped.signal = pumped.signal.max(Some(requested));
        }
    }
    pumped
}
