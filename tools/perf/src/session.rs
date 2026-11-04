//! A harness session under the desktop lock protocol (docs/performance.md, "Running it"):
//!
//! 1. take `%LOCALAPPDATA%\Temp\mypaper-tools\desktop.lock` (an atomic `mkdir`; retried every
//!    5 s; a lock older than 5 minutes is stale and removed);
//! 2. stop the user's mypaper: `WM_QUIT` to every thread of `mypaper-core`, then wait until no
//!    `mypaper-*` engine process is left;
//! 3. run;
//! 4. always put things back, also after an error, a panic or Ctrl+C: stop leftover test
//!    processes, replay test taskbar journals, restore the Windows wallpaper if it changed,
//!    restart the user's mypaper, release the lock.
//!
//! The cleanup lives in a global so the panic hook and the Ctrl+C handler can run it: release
//! builds abort on panic, so `Drop` alone wouldn't.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, Once};
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Context, Result, bail};
use windows::Win32::System::Console::SetConsoleCtrlHandler;
use windows::core::BOOL;

use crate::procs::{self, Handle};

const ENGINE: [&str; 5] = [
    "mypaper-core.exe",
    "mypaper-wallpaper.exe",
    "mypaper-widgets.exe",
    "mypaper-bars.exe",
    "mypaper-plates.exe",
];
const LOCK_RETRY: Duration = Duration::from_secs(5);
const LOCK_STALE: Duration = Duration::from_secs(5 * 60);
const LOCK_GIVE_UP: Duration = Duration::from_secs(20 * 60);
const STOP_TIMEOUT: Duration = Duration::from_secs(30);

/// What has to be put back when a session ends.
struct Cleanup {
    lock: PathBuf,
    /// The test binaries' folder: leftovers running from under it are ours to stop.
    stage: PathBuf,
    /// The mypaper-core to start again.
    user_core: Option<PathBuf>,
    /// The Windows wallpaper per monitor before the session.
    wallpapers: Vec<(String, String)>,
    /// Taskbar journals test runs may leave.
    journals: Vec<PathBuf>,
}

static PENDING: Mutex<Option<Cleanup>> = Mutex::new(None);

/// An active session. Ending it (or dropping it) runs the cleanup.
pub struct Session {
    pub started: Instant,
}

impl Session {
    /// Takes the lock and stops the user's mypaper. `restart` is started afterwards when no
    /// mypaper was running.
    pub fn begin(lock: &Path, stage: &Path, restart: Option<PathBuf>) -> Result<Self> {
        install_hooks();
        acquire(lock)?;
        let wallpapers = wallpaper::current().unwrap_or_else(|e| {
            eprintln!("warning: can't read the Windows wallpaper ({e:#}); it won't be guarded");
            Vec::new()
        });
        *pending() = Some(Cleanup {
            lock: lock.to_owned(),
            stage: stage.to_owned(),
            user_core: restart,
            wallpapers,
            journals: Vec::new(),
        });
        let session = Self {
            started: Instant::now(),
        };
        let running = stop_user_mypaper(stage)?;
        if let (Some(core), Some(cleanup)) = (running, pending().as_mut()) {
            cleanup.user_core = Some(core);
        }
        Ok(session)
    }

    /// A taskbar journal a test run may leave behind, replayed at the end.
    pub fn guard_journal(&self, path: PathBuf) {
        if let Some(cleanup) = pending().as_mut()
            && !cleanup.journals.contains(&path)
        {
            cleanup.journals.push(path);
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        run_cleanup();
    }
}

fn pending() -> std::sync::MutexGuard<'static, Option<Cleanup>> {
    PENDING
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Runs the pending cleanup, once.
fn run_cleanup() {
    let Some(cleanup) = pending().take() else {
        return;
    };
    let left = stop_test_processes(&cleanup.stage);
    if left > 0 {
        eprintln!("warning: {left} test processes are still running");
    }
    for journal in &cleanup.journals {
        let mut taskbar = mypaper_bars::taskbar::Taskbar::new(
            mypaper_bars::taskbar::NativeShell,
            journal.clone(),
        );
        match taskbar.restore() {
            Ok(true) => eprintln!("restored the taskbar from {}", journal.display()),
            Ok(false) => {}
            Err(e) => eprintln!(
                "error: restoring the taskbar from {}: {e:#}",
                journal.display()
            ),
        }
    }
    match wallpaper::restore(&cleanup.wallpapers) {
        Ok(0) => {}
        Ok(n) => eprintln!("warning: a test changed the Windows wallpaper; put {n} back"),
        Err(e) => eprintln!("error: restoring the Windows wallpaper: {e:#}"),
    }
    if let Some(core) = &cleanup.user_core {
        match restart(core) {
            Ok(()) => eprintln!("restarted {}", core.display()),
            Err(e) => eprintln!("error: restarting {}: {e:#}", core.display()),
        }
    }
    match std::fs::remove_dir(&cleanup.lock) {
        Ok(()) => {}
        Err(e) => eprintln!("error: releasing {}: {e}", cleanup.lock.display()),
    }
}

fn install_hooks() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let default = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            default(info);
            run_cleanup();
        }));
        // SAFETY: the handler is a plain function that lives for the whole process.
        if let Err(e) = unsafe { SetConsoleCtrlHandler(Some(on_ctrl), true) } {
            eprintln!("warning: no Ctrl+C handler: {e}");
        }
    });
}

unsafe extern "system" fn on_ctrl(_event: u32) -> BOOL {
    eprintln!("interrupted: cleaning up");
    run_cleanup();
    std::process::exit(130);
}

/// `mkdir` is atomic: whoever creates the folder holds the lock.
fn acquire(lock: &Path) -> Result<()> {
    std::fs::create_dir_all(lock.parent().context("lock has no parent")?)?;
    let gave_up = Instant::now() + LOCK_GIVE_UP;
    loop {
        match std::fs::create_dir(lock) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let age = std::fs::metadata(lock)
                    .and_then(|m| m.created())
                    .ok()
                    .and_then(|t| SystemTime::now().duration_since(t).ok());
                if age.is_some_and(|age| age > LOCK_STALE) {
                    eprintln!("removing a stale lock ({:?} old)", age.unwrap_or_default());
                    let _ = std::fs::remove_dir(lock);
                    continue;
                }
                if Instant::now() > gave_up {
                    bail!("{} is still held after {LOCK_GIVE_UP:?}", lock.display());
                }
                eprintln!("waiting for {}", lock.display());
                std::thread::sleep(LOCK_RETRY);
            }
            Err(e) => return Err(e).with_context(|| format!("creating {}", lock.display())),
        }
    }
}

/// Stops a running mypaper that isn't one of ours, and returns its core's path.
fn stop_user_mypaper(stage: &Path) -> Result<Option<PathBuf>> {
    let mut user_core = None;
    for proc in procs::list()
        .iter()
        .filter(|p| p.name.eq_ignore_ascii_case(ENGINE[0]))
    {
        let image = Handle::query(proc.pid).and_then(|h| h.image());
        if image.as_deref().is_some_and(|i| i.starts_with(stage)) {
            continue;
        }
        let threads = procs::post_quit(proc.pid);
        eprintln!(
            "stopping mypaper-core {} ({} threads told to quit)",
            proc.pid, threads
        );
        user_core = user_core.or(image);
    }
    let gone = procs::wait_until(STOP_TIMEOUT, || {
        for core in procs::list()
            .iter()
            .filter(|p| p.name.eq_ignore_ascii_case(ENGINE[0]))
        {
            procs::post_quit(core.pid);
        }
        !procs::list()
            .iter()
            .any(|p| ENGINE.iter().any(|e| p.name.eq_ignore_ascii_case(e)))
    });
    if !gone {
        bail!("mypaper didn't stop within {STOP_TIMEOUT:?}");
    }
    Ok(user_core)
}

/// Kills mypaper processes still running from under `stage` (only ours). Returns how many are
/// left afterwards.
pub fn stop_test_processes(stage: &Path) -> usize {
    let ours = || {
        procs::list()
            .into_iter()
            .filter(|p| ENGINE.iter().any(|e| p.name.eq_ignore_ascii_case(e)))
            .filter(|p| {
                Handle::query(p.pid)
                    .and_then(|h| h.image())
                    .is_some_and(|image| image.starts_with(stage))
            })
            .collect::<Vec<_>>()
    };
    let running = ours();
    if running.is_empty() {
        return 0;
    }
    for proc in &running {
        procs::post_quit(proc.pid);
    }
    if !procs::wait_until(Duration::from_secs(10), || ours().is_empty()) {
        for proc in ours() {
            eprintln!("killing test process {} ({})", proc.pid, proc.name);
            procs::kill(proc.pid);
        }
        procs::wait_until(Duration::from_secs(5), || ours().is_empty());
    }
    ours().len()
}

/// Starts `core` detached, from its own folder, with the harness's (the user's) environment.
fn restart(core: &Path) -> Result<()> {
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    use std::os::windows::process::CommandExt;
    Command::new(core)
        .current_dir(core.parent().context("core has no folder")?)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
        .spawn()?;
    Ok(())
}

/// The Windows desktop wallpaper per monitor (`IDesktopWallpaper`), read before a session and
/// put back after it if a test changed it.
mod wallpaper {
    use anyhow::Result;
    use windows::Win32::System::Com::{
        CLSCTX_ALL, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
    };
    use windows::Win32::UI::Shell::{DesktopWallpaper, IDesktopWallpaper};
    use windows::core::{HSTRING, PWSTR};

    fn take(text: PWSTR) -> String {
        // SAFETY: a COM-allocated string from IDesktopWallpaper, freed once here.
        unsafe {
            let value = text.to_string().unwrap_or_default();
            CoTaskMemFree(Some(text.0.cast()));
            value
        }
    }

    fn service() -> Result<IDesktopWallpaper> {
        // SAFETY: COM initialization (harmless if this thread already has it) and creation of
        // the shell's wallpaper object.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            Ok(CoCreateInstance(&DesktopWallpaper, None, CLSCTX_ALL)?)
        }
    }

    /// `(monitor id, wallpaper path)` for every monitor.
    pub fn current() -> Result<Vec<(String, String)>> {
        let desktop = service()?;
        // SAFETY: COM calls on a live object; strings are taken and freed by `take`.
        unsafe {
            (0..desktop.GetMonitorDevicePathCount()?)
                .map(|i| {
                    let monitor = take(desktop.GetMonitorDevicePathAt(i)?);
                    let path = take(desktop.GetWallpaper(&HSTRING::from(&monitor))?);
                    Ok((monitor, path))
                })
                .collect()
        }
    }

    /// Sets back every monitor's wallpaper that differs from `saved`. Returns how many.
    pub fn restore(saved: &[(String, String)]) -> Result<usize> {
        if saved.is_empty() {
            return Ok(0);
        }
        let desktop = service()?;
        let mut changed = 0;
        for (monitor, path) in saved {
            let monitor = HSTRING::from(monitor);
            // SAFETY: COM calls on a live object with valid strings.
            unsafe {
                let now = take(desktop.GetWallpaper(&monitor)?);
                if now != *path {
                    desktop.SetWallpaper(&monitor, &HSTRING::from(path))?;
                    changed += 1;
                }
            }
        }
        Ok(changed)
    }
}
