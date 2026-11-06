//! mypaper-shell: the native shell (docs/ui.md). One UI thread on `mypaper-ui`, whose loop sleeps
//! in `MsgWaitForMultipleObjectsEx` until a message, an input event or its supervisor's exit
//! wakes it. It hosts the native top bar, dock and desktop right-click menu.
//!
//! Core starts it as a supervised child. It exits when core does (it waits on its parent
//! process), and on `WM_CLOSE` to its inbox window (class `mypaper.shell.inbox`). A second
//! shell started while one runs exits at once.

// No console window in release builds; logs go to a file (see init_logging).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// The shell's modules are a library (`lib.rs`) so Studio can draw previews with them.
use mypaper_shell::{bars, desktop_menu, flyouts, inbox, integrations, look, overlay_client, studio_pick, widgets};

mod wakes;

use std::process::ExitCode;
use std::time::Instant;

use anyhow::{Context, Result};
use mypaper_config::Config;
use mypaper_desktop::desktop_view::Com;
use mypaper_desktop::{ConfigWatcher, WM_FACT, startup, watch_config};
use mypaper_tweaks::palette::{self, PaletteWatcher};
use mypaper_ui::{Ui, modules, next_event};
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_ABANDONED, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    CreateMutexW, GetCurrentProcessId, INFINITE, OpenProcess, PROCESS_SYNCHRONIZE,
    WaitForSingleObject,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, MSG, MWMO_INPUTAVAILABLE, MsgWaitForMultipleObjectsEx, PM_REMOVE,
    PeekMessageW, QS_ALLINPUT, TranslateMessage, WM_MOUSEMOVE, WM_NCMOUSELEAVE,
    WM_NCMOUSEMOVE, WM_QUIT,
};
use windows::core::w;

use crate::desktop_menu::{DesktopMenu, Settings};
use crate::inbox::{Inbox, Msg, WM_CONFIG, WM_PALETTE};
use crate::wakes::{Cause, Wakes};

fn main() -> ExitCode {
    mypaper_desktop::init_logging();
    mypaper_desktop::enable_dpi_awareness();
    mypaper_desktop::quit_on_ctrl_c();
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            log::error!("{e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    startup::ready("shell", "main_ms");
    if !single_instance()? {
        log::info!("another mypaper-shell is running; leaving it be");
        return Ok(());
    }
    let parent = parent_process();
    anyhow::ensure!(parent.is_some(), "could not watch the shell supervisor");
    let bars_start = bars::prepare()?;
    startup::ready("shell", "prepared_ms");
    let _com = Com::init();
    let mut ui = Ui::new()?;
    // Window events arrive on a thread of their own; a batch that changed something posts
    // WM_NULL here (a posted message, so it runs the loop's work at once).
    let win_events = mypaper_desktop::win_events::WinEvents::install();
    startup::ready("shell", "factories_ms");
    overlay_client::install();
    let bars = bars::Bars::new(&mut ui, bars_start)?;
    let inbox = Inbox::new()?;
    let _watchers = watch_files();
    let mut shell = Shell {
        flyouts: flyouts::Flyouts::new(&ui, inbox.hwnd())?,
        inbox: inbox.hwnd(),
        menu: None,
        bars,
        widgets: None,
        pending_widget_facts: Vec::new(),
        studio: studio_pick::StudioPick::new()?,
    };
    shell.follow_config(&ui);
    shell.bars.tick(&mut ui);
    startup::ready("shell", "ready_ms");
    log::info!("running");

    let mut msg = MSG::default();
    let mut wakes = Wakes::new();
    // Bars::new has already presented its first frame. Yield through the message loop before
    // any widget windows, fonts, providers or fetchers are initialized.
    let mut widgets_pending = true;
    // The work after wakes that only ran sent messages and WinEvent callbacks, until the next
    // other wake or this deadline (see COALESCE).
    let mut coalesced: Option<Instant> = None;
    // When the widgets last polled (see POINTER_POLL).
    let mut widgets_polled = Instant::now();
    'run: loop {
        let start = Instant::now();
        // Order matters: the parent, then the widgets' frame timer, then the bars' handles.
        let mut handles: Vec<(HANDLE, &'static str)> = parent.iter().map(|&h| (h, "parent")).collect();
        handles.extend(shell.widgets.as_ref().map(|w| (w.timer_handle(), "widgets.frame")));
        handles.extend(
            shell
                .bars
                .handles()
                .into_iter()
                .enumerate()
                .map(|(i, h)| (h, if i == 0 { "bars.ticker" } else { "bars.desktops" })),
        );
        handles.push((shell.flyouts.handle(), "flyouts.ticker"));
        let raw: Vec<HANDLE> = handles.iter().map(|h| h.0).collect();
        // The nearest deadline, and whose it is (0 widgets, 1 bars, 2 flyouts, 3 coalesced).
        let (timeout, due) = if widgets_pending {
            (0, 4)
        } else {
            [
                shell.widgets.as_ref().map_or(INFINITE, widgets::Widgets::timeout_ms),
                shell.bars.timeout(),
                shell.flyouts.timeout_ms(),
                coalesced.map_or(INFINITE, |at| {
                    wait_ms(at.saturating_duration_since(Instant::now()))
                }),
                shell.studio.timeout_ms(),
            ]
            .into_iter()
            .zip(0..)
            .min()
            .unwrap_or((INFINITE, 4))
        };
        wakes.spent("wait setup", start);
        // SAFETY: the parent's handle stays open for the whole loop.
        let woke = unsafe {
            MsgWaitForMultipleObjectsEx(
                (!raw.is_empty()).then_some(raw.as_slice()),
                timeout,
                QS_ALLINPUT,
                MWMO_INPUTAVAILABLE,
            )
        };
        let mut at = Instant::now();
        wakes.resume();
        if parent.is_some() && woke == WAIT_OBJECT_0 {
            log::info!("the process that started the shell exited; stopping");
            break;
        }
        if let Some(widgets) = &mut shell.widgets
            && woke.0 == WAIT_OBJECT_0.0 + u32::from(parent.is_some())
        {
            widgets.frame_ready();
        }
        let signalled = woke.0.wrapping_sub(WAIT_OBJECT_0.0) as usize;
        let mut cause = if woke == WAIT_TIMEOUT {
            Some(Cause::Timeout(match due {
                0 => "widgets",
                1 => shell.bars.timeout_source(),
                2 => "flyouts",
                3 => "coalesced",
                _ => "startup",
            }))
        } else {
            handles.get(signalled).map(|h| Cause::Handle(h.1))
        };
        // Every posted message so far was the pointer moving over (or leaving) a bar or the dock.
        let mut bar_pointer = true;
        // SAFETY: standard message pump on the thread that owns every window of the shell.
        unsafe {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                wakes.message(&msg);
                bar_pointer &= matches!(
                    msg.message,
                    WM_MOUSEMOVE | WM_NCMOUSEMOVE | WM_MOUSELEAVE | WM_NCMOUSELEAVE
                ) && shell.bars.owns_hwnd(msg.hwnd);
                if cause.is_none() && signalled == handles.len() {
                    // Woken by this posted message, not by a sent one.
                    cause = Some(Cause::Message);
                }
                if msg.message == WM_QUIT {
                    break 'run;
                }
                if let Some(widgets) = &mut shell.widgets {
                    widgets.raw_message(&msg);
                }
                if shell.bars.message(&msg) {
                    continue;
                }
                if msg.hwnd.is_invalid() && msg.message == WM_FACT {
                    // Posted to the thread by the menu's control window (power and session).
                    shell.fact(&mut ui, msg.wParam.0, msg.lParam.0);
                    continue;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        at = wakes.spent("pump", at);
        let sent_only = cause.is_none();
        wakes.wake(cause.unwrap_or(Cause::Sent));
        if wakes.report(|| {
            let bars: Vec<_> = bars::Bars::take_tick_costs()
                .iter()
                .map(|(part, d, cycles)| {
                    format!("{part} {:.1}/{:.1}", d.as_secs_f64() * 1e3, *cycles as f64 / 1e6)
                })
                .collect();
            format!(
                "bars.tick ms/Mcycles: {}; bar stats (samples, changes, sample_us, all bar repaints) since start: {:?}; WinEvents (hooks, callbacks, subscribers): {:?}; widgets since start: {:?}",
                bars.join(", "),
                shell.bars.stats_counts(),
                win_events.counts(),
                shell.widgets.as_ref().map(widgets::Widgets::costs)
            )
        }) {
            // A suppressed minute still ends the bars' timing interval.
            let _ = bars::Bars::take_tick_costs();
        }
        if sent_only && *coalesced.get_or_insert(at + COALESCE) > at {
            continue;
        }
        coalesced = None;
        wakes.worked();
        // A sweep over the dock posts ~250 moves a second; the widgets don't see them, so they
        // poll at most every POINTER_POLL then (their own deadlines and handles still wake them).
        let poll_widgets = !(bar_pointer && cause == Some(Cause::Message))
            || widgets_polled.elapsed() >= POINTER_POLL;
        if poll_widgets {
            widgets_polled = at;
        }
        at = shell.drain(&mut ui, &mut wakes, at, poll_widgets);
        shell.studio_tick(&mut ui);
        shell.bars.tick(&mut ui);
        wakes.spent("bars.tick", at);
        if widgets_pending {
            widgets_pending = false;
            startup::ready("shell", "widgets_init_ms");
            shell.widgets =
                widgets::Widgets::new(&mut ui, inbox.hwnd(), &shell.pending_widget_facts)
                    .inspect_err(|e| log::error!("widgets not started: {e:#}"))
                    .ok();
            shell.pending_widget_facts.clear();
            startup::ready("shell", "widgets_ready_ms");
        }
    }
    // Core gives the shell three seconds to restore the taskbar and exit: each part's teardown
    // is timed, in the order the fields used to drop.
    log::info!("stopping");
    let mut at = Instant::now();
    let mut stopped = Vec::new();
    let mut part = |name: &'static str, at: &mut Instant| {
        let now = Instant::now();
        stopped.push(format!("{name} {} ms", (now - *at).as_millis()));
        *at = now;
    };
    let Shell {
        bars,
        menu,
        widgets,
        flyouts,
        ..
    } = shell;
    drop(bars);
    part("bars", &mut at);
    drop(menu);
    part("menu", &mut at);
    drop(widgets);
    part("widgets", &mut at);
    drop(flyouts);
    part("flyouts", &mut at);
    // Every lease is gone with them: the providers' workers are joined now, while they run.
    integrations::shutdown();
    part("integrations", &mut at);
    drop(win_events);
    part("window events", &mut at);
    if let Some(parent) = parent {
        // SAFETY: our own handle, closed once.
        let _ = unsafe { CloseHandle(parent) };
    }
    log::info!("stopped ({})", stopped.join(", "));
    Ok(())
}

/// How long the loop's work may wait after a wake that only ran sent messages (other processes'
/// messages to the shell's windows). Window events no longer arrive here (see
/// `mypaper_desktop::win_events`). Posted messages (input), handles and deadlines still run the
/// work at once.
const COALESCE: std::time::Duration = std::time::Duration::from_millis(25);

/// How often the widgets poll while the only input is the pointer over the bars or the dock.
const POINTER_POLL: std::time::Duration = std::time::Duration::from_millis(100);
/// `WM_MOUSELEAVE` (its binding sits behind a `windows` feature the shell does not enable).
const WM_MOUSELEAVE: u32 = 0x02A3;

/// A wait in whole milliseconds, rounded up so a deadline has passed when it returns.
fn wait_ms(duration: std::time::Duration) -> u32 {
    duration.as_nanos().div_ceil(1_000_000).min(u128::from(INFINITE - 1)) as u32
}

/// What the shell hosts.
struct Shell {
    studio: studio_pick::StudioPick,
    bars: bars::Bars,
    inbox: windows::Win32::Foundation::HWND,
    /// The desktop right-click menu, while `desktop_menu.enabled`.
    menu: Option<DesktopMenu>,
    widgets: Option<widgets::Widgets>,
    /// Calendar, quick settings, notifications, launcher and the other panels the bars open.
    flyouts: flyouts::Flyouts,
    pending_widget_facts: Vec<(usize, isize)>,
}

impl Shell {
    fn studio_tick(&mut self, ui: &mut Ui) {
        let mut targets = Vec::new();
        if self.studio.active() {
            targets = self.bars.studio_targets();
            if let Some(widgets) = &self.widgets { targets.extend(widgets.studio_targets()); }
        }
        let revision = self.studio.active().then(|| self.widgets.as_ref().map(widgets::Widgets::studio_revision)).flatten();
        self.studio.tick(ui, &targets, self.bars.config(), revision);
        let warm = self.studio.take_preview_warm();
        if warm { self.bars.warm_studio_preview(); }
        self.bars.studio_preview(self.studio.preview());
        self.bars.studio_session(self.studio.active());
        if let Some(widgets) = &mut self.widgets {
            let (preview, resize) = self.studio.previews();
            widgets.studio_preview(ui, preview, resize, warm);
            widgets.studio_session(ui, self.studio.active(), self.studio.lifted());
        }
    }

    /// Handles everything queued: inbox messages and input, in order, until both are empty.
    /// Handling one may call into Explorer and so let more arrive; they are handled here too.
    /// Adds the time each part took to `wakes`, from `at`; returns when it finished.
    fn drain(&mut self, ui: &mut Ui, wakes: &mut Wakes, mut at: Instant, poll_widgets: bool) -> Instant {
        if integrations::process() {
            self.bars.integration_changed();
        }
        at = wakes.spent("integrations", at);
        let mut config_changed = false;
        loop {
            if let Some(msg) = inbox::next() {
                if let Some(widgets) = &mut self.widgets {
                    widgets.handle(ui, msg);
                }
                match msg.message {
                    WM_CONFIG | WM_PALETTE => {
                        config_changed = true;
                        self.bars.config_changed();
                    }
                    _ => {
                        if let Some(menu) = &mut self.menu {
                            menu.handle(ui, msg);
                        }
                    }
                }
            } else if let Some((id, event)) = next_event() {
                if self.studio.active() {
                    let point = match event {
                        mypaper_ui::Event::Move(p) | mypaper_ui::Event::Press(p, _) => {
                            Some(mypaper_ui::Point::new(p.screen.0 as f32, p.screen.1 as f32))
                        }
                        _ => None,
                    };
                    let target = self.widgets.as_ref()
                        .and_then(|w| w.studio_targets().into_iter().find(|t| t.surface == id))
                        .or_else(|| point.and_then(|p| self.bars.studio_hit(id, p)));
                    let move_surface = match &event {
                        mypaper_ui::Event::Press(p, mypaper_ui::Button::Left) => self.widgets.as_ref()
                            .and_then(|w| w.studio_move_surface(id, p.at)),
                        _ => None,
                    };
                    if self.studio.event(id, &event, target, move_surface) { continue; }
                }
                if self.flyouts.owns(id) {
                    self.flyouts.event(ui, &mut self.bars, id, &event);
                    continue;
                }
                self.bars.event(ui, id, &event);
                if let Some(widgets) = &mut self.widgets {
                    widgets.on_event(ui, id, &event);
                }
                if let Some(menu) = &mut self.menu {
                    menu.on_event(ui, id, &event);
                }
            } else {
                break;
            }
        }
        at = wakes.spent("events", at);
        if poll_widgets && let Some(widgets) = &mut self.widgets {
            widgets.poll(ui);
        }
        at = wakes.spent("widgets.poll", at);
        // What the bars asked to open, provider answers, and the open flyout's motion.
        self.flyouts.process(ui, &mut self.bars);
        at = wakes.spent("flyouts.process", at);
        // The menu's look follows the theme and its palette, so both count.
        if config_changed {
            self.follow_config(ui);
            self.flyouts.config_changed(ui, self.inbox);
        }
        at
    }

    fn fact(&mut self, ui: &mut Ui, wparam: usize, lparam: isize) {
        integrations::fact(wparam, lparam);
        self.bars.fact(wparam, lparam);
        if let Some(widgets) = &mut self.widgets {
            widgets.fact(wparam, lparam);
        } else {
            self.pending_widget_facts.push((wparam, lparam));
        }
        if let Some(menu) = &mut self.menu {
            menu.handle(
                ui,
                Msg {
                    message: WM_FACT,
                    wparam,
                    lparam,
                },
            );
        }
    }

    /// Brings the desktop menu in line with `config.json` and the theme. Nothing runs, and no
    /// hook is installed, unless `desktop_menu.enabled`.
    fn follow_config(&mut self, ui: &Ui) {
        let config = match Config::load() {
            Ok(config) => config,
            Err(e) => {
                log::warn!("config.json: {e:#}; keeping the current settings");
                return;
            }
        };
        if !config.desktop_menu.enabled {
            if self.menu.take().is_some() {
                log::info!("desktop right-click menu off");
            }
            return;
        }
        let settings = Settings {
            menu: config.desktop_menu.clone(),
            tokens: look::tokens(&config.theme, &ui.text),
        };
        match &mut self.menu {
            Some(menu) => menu.update(settings),
            None => match DesktopMenu::start(settings, self.inbox) {
                Ok(menu) => {
                    log::info!("desktop right-click menu on");
                    self.menu = Some(menu);
                    let drivers = modules::gpu_drivers();
                    if !drivers.is_empty() {
                        log::error!("GPU drivers loaded into the shell: {drivers:?}");
                    }
                }
                Err(e) => log::warn!("desktop right-click menu not started: {e:#}"),
            },
        }
    }
}

/// Takes the shell's named mutex, waiting a moment for a shell that is just exiting. `false`
/// when another shell holds it.
fn single_instance() -> Result<bool> {
    // SAFETY: a named mutex held (by never closing it) for the life of the process.
    unsafe {
        let mutex = CreateMutexW(None, false, w!("Local\\mypaper-shell")).context("mutex")?;
        let waited = WaitForSingleObject(mutex, 2000);
        Ok(waited == WAIT_OBJECT_0 || waited == WAIT_ABANDONED)
    }
}

/// `config.json` and `palette.json` changes, as inbox messages.
fn watch_files() -> (Option<ConfigWatcher>, Option<PaletteWatcher>) {
    let config = watch_config(|| inbox::post(WM_CONFIG, 0))
        .inspect_err(|e| log::warn!("not watching config.json: {e:#}"))
        .ok();
    let palette = palette::watch(|| inbox::post(WM_PALETTE, 0))
        .inspect_err(|e| log::warn!("not watching palette.json: {e:#}"))
        .ok();
    (config, palette)
}

/// A handle that is signalled when the process that started us exits (core, or a terminal).
fn parent_process() -> Option<HANDLE> {
    // SAFETY: a process snapshot read and closed here; the opened handle is the caller's.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).ok()?;
        let me = GetCurrentProcessId();
        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut parent = None;
        let mut more = Process32FirstW(snapshot, &mut entry).is_ok();
        while more {
            if entry.th32ProcessID == me {
                parent = Some(entry.th32ParentProcessID);
                break;
            }
            more = Process32NextW(snapshot, &mut entry).is_ok();
        }
        let _ = CloseHandle(snapshot);
        OpenProcess(PROCESS_SYNCHRONIZE, false, parent?)
            .inspect_err(|e| log::warn!("not watching the parent process: {e}"))
            .ok()
    }
}
