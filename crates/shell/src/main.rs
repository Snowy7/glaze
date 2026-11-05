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
use mypaper_shell::{bars, desktop_menu, flyouts, inbox, integrations, look, overlay_client, widgets};

use std::process::ExitCode;

use anyhow::{Context, Result};
use mypaper_config::Config;
use mypaper_desktop::desktop_view::Com;
use mypaper_desktop::{ConfigWatcher, WM_FACT, startup, watch_config};
use mypaper_tweaks::palette::{self, PaletteWatcher};
use mypaper_ui::{Ui, modules, next_event};
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_ABANDONED, WAIT_OBJECT_0};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    CreateMutexW, GetCurrentProcessId, OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, MSG, MWMO_INPUTAVAILABLE, MsgWaitForMultipleObjectsEx, PM_REMOVE,
    PeekMessageW, QS_ALLINPUT, TranslateMessage, WM_QUIT,
};
use windows::core::w;

use crate::desktop_menu::{DesktopMenu, Settings};
use crate::inbox::{Inbox, Msg, WM_CONFIG, WM_PALETTE};

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
        widgets: widgets::Widgets::new(&mut ui, inbox.hwnd())
            .inspect_err(|e| log::error!("widgets not started: {e:#}"))
            .ok(),
    };
    shell.follow_config(&ui);
    shell.bars.tick(&mut ui);
    startup::ready("shell", "ready_ms");
    log::info!("running");

    let mut msg = MSG::default();
    'run: loop {
        // Order matters: the parent, then the widgets' frame timer, then the bars' handles.
        let handles: Vec<HANDLE> = parent
            .iter()
            .copied()
            .chain(shell.widgets.as_ref().map(widgets::Widgets::timer_handle))
            .chain(shell.bars.handles())
            .chain([shell.flyouts.handle()])
            .collect();
        // SAFETY: the parent's handle stays open for the whole loop.
        let woke = unsafe {
            MsgWaitForMultipleObjectsEx(
                (!handles.is_empty()).then_some(handles.as_slice()),
                shell
                    .widgets
                    .as_ref()
                    .map_or(
                        windows::Win32::System::Threading::INFINITE,
                        widgets::Widgets::timeout_ms,
                    )
                    .min(shell.bars.timeout())
                    .min(shell.flyouts.timeout_ms()),
                QS_ALLINPUT,
                MWMO_INPUTAVAILABLE,
            )
        };
        if parent.is_some() && woke == WAIT_OBJECT_0 {
            log::info!("the process that started the shell exited; stopping");
            break;
        }
        if let Some(widgets) = &mut shell.widgets
            && woke.0 == WAIT_OBJECT_0.0 + u32::from(parent.is_some())
        {
            widgets.frame_ready();
        }
        // SAFETY: standard message pump on the thread that owns every window of the shell.
        unsafe {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
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
        shell.drain(&mut ui);
        shell.bars.tick(&mut ui);
    }
    drop(shell);
    if let Some(parent) = parent {
        // SAFETY: our own handle, closed once.
        let _ = unsafe { CloseHandle(parent) };
    }
    log::info!("stopped");
    Ok(())
}

/// What the shell hosts.
struct Shell {
    bars: bars::Bars,
    inbox: windows::Win32::Foundation::HWND,
    /// The desktop right-click menu, while `desktop_menu.enabled`.
    menu: Option<DesktopMenu>,
    widgets: Option<widgets::Widgets>,
    /// Calendar, quick settings, notifications, launcher and the other panels the bars open.
    flyouts: flyouts::Flyouts,
}

impl Shell {
    /// Handles everything queued: inbox messages and input, in order, until both are empty.
    /// Handling one may call into Explorer and so let more arrive; they are handled here too.
    fn drain(&mut self, ui: &mut Ui) {
        if integrations::process() {
            self.bars.integration_changed();
        }
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
        if let Some(widgets) = &mut self.widgets {
            widgets.poll(ui);
        }
        // What the bars asked to open, provider answers, and the open flyout's motion.
        self.flyouts.process(ui, &mut self.bars);
        // The menu's look follows the theme and its palette, so both count.
        if config_changed {
            self.follow_config(ui);
            self.flyouts.config_changed(ui, self.inbox);
        }
    }

    fn fact(&mut self, ui: &mut Ui, wparam: usize, lparam: isize) {
        integrations::fact(wparam, lparam);
        self.bars.fact(wparam, lparam);
        if let Some(widgets) = &mut self.widgets {
            widgets.fact(wparam, lparam);
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
