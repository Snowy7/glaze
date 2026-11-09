//! `glaze-core --restore`: stops a running Glaze and undoes every system change any run left
//! behind, with no UI. The installer, the uninstaller and "Restore Windows defaults" call it;
//! the contract (exit codes, the printed line, what it covers) is in docs/restore.md.
//!
//! Every step replays a journal written before the change it undoes, so the user's own values
//! come back exactly, not Windows' defaults. Steps are independent: one failing skips nothing.

use std::process::ExitCode;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde::Serialize;
use windows::Win32::Foundation::{
    CloseHandle, HANDLE, LPARAM, WAIT_ABANDONED, WAIT_OBJECT_0, WPARAM,
};
use windows::Win32::System::Threading::{
    CreateMutexW, OpenProcess, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE, TerminateProcess,
    WaitForSingleObject,
};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, GetWindowThreadProcessId, PostThreadMessageW, WM_HOTKEY,
};
use windows::core::{PCWSTR, w};

/// How long a running core gets to stop (it gives the shell 3 s of that) before it is killed.
const STOP_GRACE: Duration = Duration::from_secs(15);

/// The line `--restore` prints.
#[derive(Debug, Default, Serialize)]
struct Report {
    /// A running Glaze was stopped first.
    stopped: bool,
    /// Steps that had something to undo and undid it.
    restored: Vec<&'static str>,
    failed: Vec<Failure>,
}

#[derive(Debug, Serialize)]
struct Failure {
    step: &'static str,
    error: String,
}

/// Handles `--restore`; `None` for any other command line.
pub fn cli(mut args: impl Iterator<Item = String>) -> Option<ExitCode> {
    if args.next().as_deref() != Some("--restore") {
        return None;
    }
    crate::autostart::ensure_console();
    glaze_desktop::init_logging();
    log::info!("--restore: stopping Glaze and undoing its system changes");
    let mut report = Report::default();
    let code = match stop_running() {
        Ok((stopped, _instance)) => {
            report.stopped = stopped;
            restore_all(&mut report);
            u8::from(!report.failed.is_empty())
        }
        Err(e) => {
            report.failed.push(Failure {
                step: "stop",
                error: format!("{e:#}"),
            });
            2
        }
    };
    for failure in &report.failed {
        log::error!("--restore: {}: {}", failure.step, failure.error);
    }
    log::info!(
        "--restore: done (restored: {:?})",
        report.restored
    );
    println!(
        "{}",
        serde_json::to_string(&report).unwrap_or_else(|_| "{}".into())
    );
    Some(ExitCode::from(code))
}

/// Core's single-instance mutex, held until the restore is done, so a core started meanwhile
/// (at sign-in, by the user) refuses to run instead of re-applying what we undo.
struct Instance(HANDLE);

impl Drop for Instance {
    fn drop(&mut self) {
        // SAFETY: our own handle, closed once; closing releases ownership.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

/// Stops a running core the way the panic hotkey does (it restores everything it applied and
/// puts the user's wallpaper back), kills it if it hasn't stopped after [`STOP_GRACE`], and takes
/// its single-instance mutex. Returns whether there was a core to stop.
fn stop_running() -> Result<(bool, Instance)> {
    // SAFETY: opens (or creates) a named mutex; the handle is owned by `Instance`.
    let mutex = Instance(unsafe { CreateMutexW(None, false, w!("Local\\glaze-core"))? });
    let started = Instant::now();
    let mut core: Option<HANDLE> = None;
    let result = loop {
        // SAFETY: a wait on our own handle; an abandoned mutex (a killed core) is ours too.
        let status = unsafe { WaitForSingleObject(mutex.0, 100) };
        if status == WAIT_OBJECT_0 || status == WAIT_ABANDONED {
            break Ok(core.is_some());
        }
        if core.is_none() {
            core = ask_core_to_stop();
        }
        if started.elapsed() > STOP_GRACE {
            let Some(process) = core else {
                break Err(anyhow::anyhow!(
                    "Glaze is running but did not stop and has no tray window to ask"
                ));
            };
            log::warn!("--restore: Glaze did not stop in {STOP_GRACE:?}; ending it");
            // SAFETY: a handle opened with PROCESS_TERMINATE, closed below.
            if let Err(e) = unsafe { TerminateProcess(process, 1) } {
                break Err(e).context("ending a running Glaze");
            }
            // SAFETY: as above; its mutex is abandoned once it is gone.
            let status = unsafe { WaitForSingleObject(mutex.0, 5000) };
            if status == WAIT_OBJECT_0 || status == WAIT_ABANDONED {
                break Ok(true);
            }
            break Err(anyhow::anyhow!("a running Glaze did not exit"));
        }
    };
    if let Some(process) = core {
        // SAFETY: opened by `ask_core_to_stop`, closed once.
        let _ = unsafe { CloseHandle(process) };
    }
    result.map(|stopped| (stopped, mutex))
}

/// Finds core by its tray window and posts it the panic hotkey. Returns a handle to wait on
/// and end it with.
fn ask_core_to_stop() -> Option<HANDLE> {
    // SAFETY: a lookup, a query and a thread post; nothing is activated.
    unsafe {
        let tray = FindWindowW(w!("glaze.tray"), PCWSTR::null()).ok()?;
        let mut pid = 0;
        let thread = GetWindowThreadProcessId(tray, Some(&mut pid));
        if pid == std::process::id() {
            return None;
        }
        let process = OpenProcess(PROCESS_SYNCHRONIZE | PROCESS_TERMINATE, false, pid).ok()?;
        log::info!("--restore: asking the running Glaze (pid {pid}) to stop");
        let posted = PostThreadMessageW(
            thread,
            WM_HOTKEY,
            WPARAM(crate::PANIC_HOTKEY as usize),
            LPARAM(0),
        );
        if let Err(e) = posted {
            log::warn!("--restore: could not ask Glaze to stop: {e}");
        }
        Some(process)
    }
}

/// Replays every journal. Each step records what it did in `report`.
fn restore_all(report: &mut Report) {
    let mut step = |name: &'static str, run: &mut dyn FnMut() -> Result<bool>| match run() {
        Ok(true) => report.restored.push(name),
        Ok(false) => {}
        Err(e) => report.failed.push(Failure {
            step: name,
            error: format!("{e:#}"),
        }),
    };
    // A shell outliving its core (or hung) holds the taskbar journal: stop it first.
    step("shell", &mut || crate::bars::stop_leftover());
    step("taskbar", &mut || {
        let _lease = glaze_bars::taskbar::JournalLease::acquire()?;
        glaze_bars::replay()
    });
    step("styling", &mut restore_styling);
    step("preferences", &mut || {
        glaze_tweaks::system::SystemTweaks::new()?.restore()
    });
    step("classic-menu", &mut || {
        let mut menu = glaze_tweaks::classic_menu::ClassicMenu::new()?;
        Ok(menu.set(false)? == glaze_tweaks::classic_menu::Change::Applied)
    });
    step("folder-routing", &mut || {
        let files = std::env::current_exe()?.with_file_name("glaze-files.exe");
        glaze_tweaks::folder_routing::FolderRouting::new()?.set(false, &files)
    });
    step("wallpaper", &mut || glaze_desktop::desktop_wallpaper::restore());
    step("desktop", &mut || {
        // Repaints the legacy desktop layout's WorkerW, which keeps the engine's last frame.
        if let Ok(host) = glaze_desktop::DesktopHost::find_once() {
            host.restore();
        }
        Ok(false)
    });
}

/// The theme's Windows styling (accent, dark mode, cursors, DWM frames on allowlisted windows):
/// creating the controller undoes a crashed run's journal; dropping it waits for the worker.
fn restore_styling() -> Result<bool> {
    let journal = glaze_tweaks::palette::local_dir()?.join("undo.json");
    let pending = journal.exists();
    let mut tweaks = glaze_tweaks::Tweaks::new()?;
    tweaks.deactivate();
    drop(tweaks);
    if journal.exists() {
        bail!("{} could not be fully undone", journal.display());
    }
    Ok(pending)
}
