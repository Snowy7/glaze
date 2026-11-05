//! mypaper-wallpaper: renders a theme's wallpaper behind the desktop icons.
//!
//! ```text
//! mypaper-wallpaper [--windowed] [THEME]
//! ```
//!
//! The theme and pause settings come from `config.json` and follow it live:
//! switching the theme or pausing needs no restart. `THEME` (a theme folder or
//! name, see [`mypaper_theme::locate`]) overrides the config's theme for this
//! run, for development; later theme changes in the config still apply.
//! `--windowed` renders into a normal window instead of the desktop. Theme
//! files hot-reload on save in both modes.

// No console window in release builds; logs go to a file (see init_logging).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// Proving that wgpu's buffers can move to the snapshot thread (`Send`) walks a deep type graph.
#![recursion_limit = "256"]

mod backdrop;
mod control;
mod engine;
mod glass_pass;
mod material;
mod gpu;
#[cfg(test)]
mod lab;
// Glass overlays, driven by the loop below; their measuring calls serve the spike examples.
#[allow(dead_code)]
mod overlay;
mod palette;
mod preview;
mod snapshot;
mod sources;
mod timing;
mod trim;
mod window;

use std::process::ExitCode;
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use mypaper_config::Config;
use mypaper_desktop::{
    ControlWindow, DesktopEvents, DesktopHost, EXIT_CONFIG, EXIT_RESTART, WindowEvents, hold,
    monitors, startup,
};
use mypaper_theme::LoadedTheme;
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, TranslateMessage, WM_QUIT,
};

use crate::control::Live;
use crate::engine::{Engine, Flow};
use crate::hold::Fact;
use crate::trim::Trimmer;
use crate::window::{CONTROL_CLASS, WM_CONFIG, WM_FACT, WM_RELOAD, Waiter, Window};

const USAGE: &str = "usage: mypaper-wallpaper [--windowed] [THEME]";

struct Args {
    windowed: bool,
    /// Overrides `Config::theme` for this run.
    theme: Option<String>,
}

fn main() -> ExitCode {
    mypaper_desktop::init_logging();
    mypaper_desktop::enable_dpi_awareness();
    mypaper_desktop::quit_on_ctrl_c();
    #[cfg(feature = "hotpatch")]
    connect_hotpatch();

    // Bad arguments, config or theme will not fix themselves: exit with
    // EXIT_CONFIG so the supervisor reports it instead of restarting.
    let args = match parse_args() {
        Ok(args) => args,
        Err(e) => {
            log::error!("{e:#}");
            return ExitCode::from(EXIT_CONFIG);
        }
    };
    let (config, theme) = match load(&args) {
        Ok(loaded) => loaded,
        Err(e) => {
            log::error!("{e:#}");
            return ExitCode::from(EXIT_CONFIG);
        }
    };
    let id = theme
        .dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    let settings = config.engine.resolve(id, theme.theme.engine);
    // Settings select the one device; opening still overlaps creation of the desktop windows.
    let gpu = gpu::Opening::start(settings.gpu);
    match run(args, config, theme, gpu) {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            log::error!("{e:#}");
            ExitCode::FAILURE
        }
    }
}

/// Reads the config and loads the theme it (or the arguments) select.
fn load(args: &Args) -> Result<(Config, LoadedTheme)> {
    let config = Config::load()?;
    let name = args.theme.as_deref().unwrap_or(&config.theme);
    let theme = LoadedTheme::load(mypaper_theme::locate(Some(name))?)?;
    Ok((config, theme))
}

fn parse_args() -> Result<Args> {
    let mut windowed = false;
    let mut theme = None;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--windowed" => windowed = true,
            "-h" | "--help" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            flag if flag.starts_with('-') => bail!("unknown flag {flag}\n{USAGE}"),
            name => theme = Some(name.to_owned()),
        }
    }
    Ok(Args { windowed, theme })
}

fn run(args: Args, config: Config, theme: LoadedTheme, gpu: gpu::Opening) -> Result<u8> {
    log::info!("theme: {} ({})", theme.theme.name, theme.dir.display());
    window::init_com_apartment()?;
    window::register_class()?;

    let host = if args.windowed {
        None
    } else {
        Some(DesktopHost::find()?)
    };
    let (windows, monitors) = match &host {
        Some(host) => {
            log::info!("desktop host: {host:?}");
            let monitors = monitors();
            let windows = monitors
                .iter()
                .map(|m| Window::desktop(host, *m))
                .collect::<Result<Vec<_>>>()?;
            (windows, Some(monitors))
        }
        None => (
            vec![Window::dev(&format!(
                "mypaper - {} (dev)",
                theme.theme.name
            ))?],
            None,
        ),
    };
    let desktop_events = host.map(DesktopEvents::new).transpose()?;

    let _control = ControlWindow::new(CONTROL_CLASS)?;
    startup::ready("wallpaper", "windows_ms");
    let mut engine = Engine::new(theme, gpu, windows, monitors, config.engine.clone())?;
    // Facts first, so the initial hold is logged once.
    engine.set_fact(Fact::OnBattery, mypaper_desktop::on_battery());
    engine.apply_config(&config);
    engine.set_static_snapshot(config.static_snapshot);
    let mut live = Live::new(config, engine.theme_dir())?;
    let events = host.is_some().then(WindowEvents::install);
    let waiter = Waiter::new()?;
    // Glass under the shell's popups (desktop mode only: a dev window must not take requests).
    let mut overlays = host
        .is_some()
        .then(|| overlay::Overlays::new(engine.overlay_gpu()))
        .transpose()
        .inspect_err(|e| log::error!("glass overlays unavailable: {e:#}"))
        .ok()
        .flatten();

    let mut trimmer = Trimmer::new(engine.theme_dir());
    startup::ready("wallpaper", "loop_ms");
    let mut desktop_lost = false;
    let mut desktop_at = None;
    let code = loop {
        if let Some(code) = pump(&mut engine, &mut live) {
            break code;
        }
        // Requests and capture frames arrived as messages in that pump.
        let warm_at = overlays.as_mut().and_then(|o| {
            o.tick();
            o.deadline()
        });
        if let Some(events) = &desktop_events {
            desktop_lost |= events.take_lost();
            if events.take_ready() {
                desktop_at = Some(Instant::now() + Duration::from_millis(250));
            }
            if desktop_at.is_some_and(|at| at <= Instant::now()) {
                desktop_at = None;
                match DesktopHost::find_once() {
                    Ok(current) if desktop_lost || Some(current) != host => {
                        log::info!(
                            "Explorer restarted: restarting wallpaper to reattach to {current:?}"
                        );
                        break EXIT_RESTART;
                    }
                    Ok(_) => {}
                    Err(_) => {
                        desktop_lost = true;
                        events.set_host(None);
                    }
                }
            }
            if desktop_lost {
                // Keep the GPU away from orphaned HWNDs until the replacement shell is ready.
                waiter.wait(desktop_at, &[]);
                continue;
            }
        }
        live.apply_due(&mut engine);
        let trim_at = trimmer.poll(engine.theme_dir());
        match tick(&mut engine, events.as_ref()) {
            Flow::Wait(deadline) => waiter.wait(
                earliest(
                    earliest(desktop_at, warm_at),
                    earliest(earliest(deadline, trim_at), live.deadline()),
                ),
                &engine.glass_handles(),
            ),
            Flow::Exit(code) => break code,
        }
    };

    drop(overlays);
    drop(engine); // destroys our windows
    if let Some(host) = host {
        host.restore();
    }
    Ok(code)
}

fn earliest(a: Option<Instant>, b: Option<Instant>) -> Option<Instant> {
    [a, b].into_iter().flatten().min()
}

/// Drains the message queue. Returns the exit code once `WM_QUIT` arrives.
fn pump(engine: &mut Engine, live: &mut Live) -> Option<u8> {
    let mut msg = MSG::default();
    // SAFETY: standard message pump on the thread that owns our windows.
    unsafe {
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            match msg.message {
                WM_QUIT => return Some(msg.wParam.0 as u8),
                WM_RELOAD => engine.schedule_reload(),
                WM_CONFIG => live.schedule(),
                WM_FACT => {
                    if let Some(fact) = Fact::from_wparam(msg.wParam.0) {
                        engine.set_fact(fact, msg.lParam.0 != 0);
                    }
                }
                _ => {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        }
    }
    None
}

/// Connects to `dx serve --hotpatch` and logs each patch it applies: how
/// many functions the jump table maps, and whether `Engine::tick` (the entry
/// point [`tick`] calls through it) is one of them.
#[cfg(feature = "hotpatch")]
fn connect_hotpatch() {
    dioxus_devtools::connect_subsecond();
    subsecond::register_handler(std::sync::Arc::new(|| {
        let entry = Engine::tick as fn(&mut Engine, Option<&WindowEvents>) -> Flow;
        // SAFETY: only read for logging, right after the patch was applied.
        let table = unsafe { subsecond::get_jump_table() };
        log::info!(
            "hot-patch applied: {} functions mapped, Engine::tick at {:#x} is {}",
            table.map_or(0, |t| t.map.len()),
            entry as usize,
            if table.is_some_and(|t| t.map.contains_key(&(entry as usize as u64))) {
                "mapped (patched)"
            } else {
                "NOT mapped (unchanged)"
            }
        );
    }));
}

/// Runs one engine tick. When hot-patching, the call goes through
/// subsecond's jump table, so the latest compiled `Engine::tick` (and
/// everything it calls) runs. Subsecond only redirects pointer-sized
/// callables, hence the plain `fn` pointer rather than a closure.
fn tick(engine: &mut Engine, events: Option<&WindowEvents>) -> Flow {
    #[cfg(feature = "hotpatch")]
    {
        let tick: fn(&mut Engine, Option<&WindowEvents>) -> Flow = Engine::tick;
        subsecond::HotFn::current(tick).call((engine, events))
    }
    #[cfg(not(feature = "hotpatch"))]
    engine.tick(events)
}
