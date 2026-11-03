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

mod control;
mod engine;
mod gpu;
mod hold;
mod sources;
mod window;

use std::process::ExitCode;
use std::time::Instant;

use anyhow::{Result, bail};
use mypaper_config::Config;
use mypaper_desktop::{DesktopHost, EXIT_CONFIG, WindowEvents, monitors};
use mypaper_theme::LoadedTheme;
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, TranslateMessage, WM_QUIT,
};

use crate::control::Live;
use crate::engine::{Engine, Flow};
use crate::hold::Fact;
use crate::window::{ControlWindow, WM_CONFIG, WM_FACT, WM_RELOAD, Waiter, Window};

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
    dioxus_devtools::connect_subsecond();

    // Bad arguments, config or theme will not fix themselves: exit with
    // EXIT_CONFIG so the supervisor reports it instead of restarting.
    let (args, config, theme) = match startup() {
        Ok(started) => started,
        Err(e) => {
            log::error!("{e:#}");
            return ExitCode::from(EXIT_CONFIG);
        }
    };
    match run(args, config, theme) {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            log::error!("{e:#}");
            ExitCode::FAILURE
        }
    }
}

/// Reads the arguments and config, and loads the theme they select.
fn startup() -> Result<(Args, Config, LoadedTheme)> {
    let args = parse_args()?;
    let config = Config::load()?;
    let name = args.theme.as_deref().unwrap_or(&config.theme);
    let theme = LoadedTheme::load(mypaper_theme::locate(Some(name))?)?;
    Ok((args, config, theme))
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

fn run(args: Args, config: Config, theme: LoadedTheme) -> Result<u8> {
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

    let _control = ControlWindow::new()?;
    let mut engine = Engine::new(theme, windows, monitors)?;
    // Facts first, so the initial hold is logged once.
    engine.set_fact(Fact::OnBattery, mypaper_desktop::on_battery());
    engine.apply_config(&config);
    let mut live = Live::new(config, engine.theme_dir())?;
    let events = host.is_some().then(WindowEvents::install);
    let waiter = Waiter::new()?;

    let code = loop {
        if let Some(code) = pump(&mut engine, &mut live) {
            break code;
        }
        live.apply_due(&mut engine);
        match tick(&mut engine, events.as_ref()) {
            Flow::Wait(deadline) => waiter.wait(earliest(deadline, live.deadline())),
            Flow::Exit(code) => break code,
        }
    };

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
