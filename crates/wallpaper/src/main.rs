//! mypaper-wallpaper: renders a theme's wallpaper behind the desktop icons.
//!
//! ```text
//! mypaper-wallpaper [--windowed] [THEME]
//! ```
//!
//! `--windowed` renders into a normal window instead of the desktop, for
//! development. Theme files hot-reload on save in both modes. `THEME` is a theme
//! folder or name (default `aurora`), see [`mypaper_theme::locate`].

mod engine;
mod gpu;
mod window;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Result, bail};
use mypaper_desktop::{DesktopHost, EXIT_CONFIG, WindowEvents, monitors};
use mypaper_theme::LoadedTheme;
use notify::Watcher;
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, PostThreadMessageW, TranslateMessage, WM_QUIT,
};

use crate::engine::{Engine, Flow};
use crate::window::{WM_RELOAD, Waiter, Window};

const USAGE: &str = "usage: mypaper-wallpaper [--windowed] [THEME]";

struct Args {
    windowed: bool,
    theme: PathBuf,
}

fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    mypaper_desktop::enable_dpi_awareness();
    mypaper_desktop::quit_on_ctrl_c();
    #[cfg(feature = "hotpatch")]
    dioxus_devtools::connect_subsecond();

    // Bad arguments or a broken theme will not fix themselves: exit with
    // EXIT_CONFIG so the supervisor reports it instead of restarting.
    let (args, theme) =
        match parse_args().and_then(|args| Ok((LoadedTheme::load(&args.theme)?, args))) {
            Ok((theme, args)) => (args, theme),
            Err(e) => {
                log::error!("{e:#}");
                return ExitCode::from(EXIT_CONFIG);
            }
        };
    match run(args, theme) {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            log::error!("{e:#}");
            ExitCode::FAILURE
        }
    }
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
    let theme = mypaper_theme::locate(theme.as_deref())?;
    Ok(Args { windowed, theme })
}

fn run(args: Args, theme: LoadedTheme) -> Result<u8> {
    log::info!("theme: {} ({})", theme.theme.name, args.theme.display());
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

    let mut engine = Engine::new(theme, windows, monitors)?;
    let _watcher = watch(&args.theme)?;
    let events = host.is_some().then(WindowEvents::install);
    let waiter = Waiter::new()?;

    let code = loop {
        if let Some(code) = pump(&mut engine) {
            break code;
        }
        match tick(&mut engine, events.as_ref()) {
            Flow::Wait(deadline) => waiter.wait(deadline),
            Flow::Exit(code) => break code,
        }
    };

    drop(engine); // destroys our windows
    if let Some(host) = host {
        host.restore();
    }
    Ok(code)
}

/// Drains the message queue. Returns the exit code once `WM_QUIT` arrives.
fn pump(engine: &mut Engine) -> Option<u8> {
    let mut msg = MSG::default();
    // SAFETY: standard message pump on the thread that owns our windows.
    unsafe {
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            match msg.message {
                WM_QUIT => return Some(msg.wParam.0 as u8),
                WM_RELOAD => engine.schedule_reload(),
                _ => {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        }
    }
    None
}

/// Wakes the main thread with `WM_RELOAD` whenever a theme file changes.
fn watch(dir: &Path) -> Result<notify::RecommendedWatcher> {
    // SAFETY: no preconditions.
    let thread = unsafe { GetCurrentThreadId() };
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if event.is_ok_and(|e| !e.kind.is_access()) {
            // SAFETY: posting to a thread id is always safe.
            let _ = unsafe { PostThreadMessageW(thread, WM_RELOAD, WPARAM(0), LPARAM(0)) };
        }
    })?;
    watcher.watch(dir, notify::RecursiveMode::Recursive)?;
    Ok(watcher)
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
