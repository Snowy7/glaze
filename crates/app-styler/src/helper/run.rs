//! The helper's run loop: follow the config, keep each app's rules file current, attach to
//! Explorer and Settings as they appear, and stop styling an app that keeps crashing.
//!
//! Event-driven only: it sleeps in `MsgWaitForMultipleObjects` on the attached processes and its
//! message queue, which a window-shown WinEvent hook and the config/palette file watches feed.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use mypaper_app_styler::presets::{self, Colors};
use mypaper_app_styler::{Attach, Framework};
use mypaper_config::{Config, Paint};
use mypaper_theme::LoadedTheme;
use windows::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HWND, LPARAM, WAIT_OBJECT_0, WPARAM,
};
use windows::Win32::System::Threading::{
    CreateMutexW, GetCurrentThreadId, GetExitCodeProcess, INFINITE, OpenProcess,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
};
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, EVENT_OBJECT_SHOW, EnumChildWindows, EnumWindows, GA_ROOT, GetAncestor,
    GetClassNameW, GetWindowThreadProcessId, IsWindowVisible, MSG, MsgWaitForMultipleObjects,
    OBJID_WINDOW, PM_REMOVE, PeekMessageW, PostThreadMessageW, QS_ALLINPUT, TranslateMessage,
    WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS, WM_APP, WM_QUIT,
};
use windows_core::{BOOL, w};

use super::{app_of, attach, data_dir, log_path, tap_path};

/// `config.json` or `palette.json` changed.
const WM_RESOLVE: u32 = WM_APP + 1;

/// A styled process that dies this soon after we attached counts against its app.
const CRASH_WINDOW: Duration = Duration::from_secs(60);
/// That many such deaths and the app isn't styled again this session.
const CRASH_LIMIT: usize = 2;

thread_local! {
    /// Windows shown since the last pass (filled by the WinEvent callback).
    static SHOWN: RefCell<Vec<HWND>> = const { RefCell::new(Vec::new()) };
}

/// Runs until the flag goes off or the process is told to quit. `preview`: style with that
/// preset (under the user's `look.apps`) whatever the flag and the theme say, to try one.
pub fn run(preview: Option<String>) -> Result<()> {
    // SAFETY: a named mutex held for the life of the process.
    let _single = unsafe { CreateMutexW(None, true, w!("Local\\mypaper-styler"))? };
    // SAFETY: reads this thread's last error.
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        bail!("mypaper-styler is already running");
    }
    // SAFETY: no preconditions.
    let thread = unsafe { GetCurrentThreadId() };
    let post = move || {
        // SAFETY: posting to a thread id is always safe; failure means it is gone.
        let _ = unsafe { PostThreadMessageW(thread, WM_RESOLVE, WPARAM(0), LPARAM(0)) };
    };
    let _config = mypaper_desktop::watch_config(post)
        .inspect_err(|e| log::warn!("rules won't follow config.json: {e:#}"))
        .ok();
    let _palette = mypaper_desktop::watch_file(&palette_path()?, post)
        .inspect_err(|e| log::warn!("rules won't follow the wallpaper: {e:#}"))
        .ok();

    let mut helper = Helper::new(preview)?;
    if !helper.resolve() {
        log::info!("experimental.app_styler is off; exiting");
        return Ok(());
    }
    // SAFETY: an out-of-context hook with a 'static callback, removed below.
    let hook = unsafe {
        SetWinEventHook(
            EVENT_OBJECT_SHOW,
            EVENT_OBJECT_SHOW,
            None,
            Some(on_show),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        )
    };
    if hook.is_invalid() {
        bail!("SetWinEventHook failed");
    }
    sweep();
    helper.consider_shown();
    log::info!("app styler running");

    let result = helper.serve();
    // SAFETY: the hook from above.
    let _ = unsafe { UnhookWinEvent(hook) };
    // Exiting is what makes every TAP put its app back.
    result
}

/// `%LOCALAPPDATA%\mypaper\palette.json`, which the wallpaper writes on every theme (re)load.
fn palette_path() -> Result<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA").context("LOCALAPPDATA is not set")?;
    Ok(PathBuf::from(local).join("mypaper").join("palette.json"))
}

struct Helper {
    /// `--preview PRESET`: ignore the flag and the theme's `apps`.
    preview: Option<String>,
    tap: PathBuf,
    dir: PathBuf,
    /// Attached processes by pid.
    attached: HashMap<u32, Attached>,
    /// Pids that aren't Explorer or Settings (or were refused), not asked again.
    refused: HashSet<u32>,
    /// Pids that are, but aren't attached yet (Explorer before its first folder window). Only a
    /// filter: attaching asks the allowlist again.
    seen: HashMap<u32, &'static str>,
    /// Deaths soon after attaching, per app.
    strikes: HashMap<&'static str, usize>,
    disabled: HashSet<&'static str>,
}

struct Attached {
    app: &'static str,
    process: HANDLE,
    at: Instant,
    frameworks: Vec<Framework>,
}

impl Drop for Attached {
    fn drop(&mut self) {
        // SAFETY: opened in `attach_to`, closed once.
        let _ = unsafe { CloseHandle(self.process) };
    }
}

impl Helper {
    fn new(preview: Option<String>) -> Result<Self> {
        Ok(Self {
            tap: tap_path()?,
            preview,
            dir: data_dir()?,
            attached: HashMap::new(),
            refused: HashSet::new(),
            seen: HashMap::new(),
            strikes: HashMap::new(),
            disabled: HashSet::new(),
        })
    }

    /// Re-reads the config, theme and palette and rewrites each app's rules file if it changed.
    /// Returns whether the styler is still enabled.
    fn resolve(&mut self) -> bool {
        let config = match Config::load() {
            Ok(config) => config,
            Err(e) => {
                log::warn!("config.json: {e:#}; rules unchanged");
                return true;
            }
        };
        if !config.experimental.app_styler && self.preview.is_none() {
            return false;
        }
        let theme = mypaper_theme::locate(Some(&config.theme))
            .and_then(LoadedTheme::load)
            .inspect_err(|e| log::warn!("theme: {e:#}; using the look's rules alone"))
            .ok();
        let palette = read_palette(theme.as_ref().map(|t| t.dir.as_path()));
        let accent = match config.look.accent {
            Some(Paint::Color(c)) => Some(c.0),
            _ => palette.map(|p| p.0),
        };
        let colors = Colors {
            accent,
            dominant: palette.map(|p| p.1),
        };
        let theme_apps = theme.as_ref().and_then(|t| t.theme.apps.as_ref());
        for app in mypaper_config::apps::APPS {
            let rules = match &self.preview {
                Some(name) => {
                    let user = mypaper_config::AppsStyle {
                        preset: Some(name.clone()),
                        ..config.look.apps.clone()
                    };
                    presets::resolve(app, None, &user, colors)
                }
                None => presets::resolve(app, theme_apps, &config.look.apps, colors),
            };
            let text = serde_json::to_string_pretty(&rules).unwrap_or_default();
            let path = self.dir.join(format!("{app}.json"));
            if std::fs::read_to_string(&path).ok().as_deref() != Some(text.as_str())
                && let Err(e) = write_atomic(&path, &text)
            {
                log::warn!("writing {}: {e:#}", path.display());
            }
        }
        true
    }

    fn serve(&mut self) -> Result<()> {
        loop {
            let pids: Vec<u32> = self.attached.keys().copied().collect();
            let handles: Vec<HANDLE> = pids.iter().map(|p| self.attached[p].process).collect();
            let wait_on = (!handles.is_empty()).then_some(handles.as_slice());
            // SAFETY: the handles stay open while their entries exist.
            let woke = unsafe { MsgWaitForMultipleObjects(wait_on, false, INFINITE, QS_ALLINPUT) };
            let index = woke.0.wrapping_sub(WAIT_OBJECT_0.0) as usize;
            if index < pids.len() {
                self.exited(pids[index]);
            }
            let mut msg = MSG::default();
            // SAFETY: a standard message pump on this thread.
            unsafe {
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    match msg.message {
                        WM_QUIT => return Ok(()),
                        WM_RESOLVE => {
                            if !self.resolve() {
                                log::info!("experimental.app_styler turned off; exiting");
                                return Ok(());
                            }
                        }
                        _ => {
                            let _ = TranslateMessage(&msg);
                            DispatchMessageW(&msg);
                        }
                    }
                }
            }
            self.consider_shown();
        }
    }

    /// Looks at the windows shown since last time: attaches to their process if it is one of
    /// the styled apps and isn't attached for the framework it needs yet.
    fn consider_shown(&mut self) {
        let shown = SHOWN.with_borrow_mut(std::mem::take);
        for hwnd in shown {
            let mut pid = 0;
            // SAFETY: a valid out pointer; a stale window just yields 0.
            unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
            if pid == 0 || self.refused.contains(&pid) {
                continue;
            }
            let app = match self
                .attached
                .get(&pid)
                .map(|a| a.app)
                .or_else(|| self.seen.get(&pid).copied())
            {
                Some(app) => app,
                None => match app_of(pid) {
                    Some(app) => {
                        self.seen.insert(pid, app);
                        app
                    }
                    None => {
                        self.refused.insert(pid);
                        continue;
                    }
                },
            };
            if self.disabled.contains(app) {
                continue;
            }
            let framework = match app {
                // Settings is system XAML throughout.
                "settings" => Framework::System,
                // Explorer's folder windows are WinUI 3; its system XAML is the taskbar, which
                // isn't ours to style. Wait for a folder window: WinUI loads with the first.
                _ if root_class(hwnd).as_deref() == Some("CabinetWClass") => Framework::WinUi,
                _ => continue,
            };
            if self
                .attached
                .get(&pid)
                .is_some_and(|a| a.frameworks.contains(&framework))
            {
                continue;
            }
            self.attach_to(pid, app, framework);
        }
    }

    fn attach_to(&mut self, pid: u32, app: &'static str, framework: Framework) {
        // Pids are reused: ask the allowlist again right before touching the process.
        if app_of(pid) != Some(app) {
            self.refused.insert(pid);
            return;
        }
        let data = Attach {
            helper: std::process::id(),
            framework,
            rules: self.dir.join(format!("{app}.json")),
            log: log_path().ok(),
            dump: None,
        };
        if let Err(e) = attach::attach(pid, framework, &self.tap, &data) {
            // WinUI not loaded yet in a fresh Explorer: a later window will try again.
            log::debug!("{app} ({pid}): {e:#}");
            return;
        }
        log::info!("styling {app} ({pid}, {framework})");
        if let Some(entry) = self.attached.get_mut(&pid) {
            entry.frameworks.push(framework);
            return;
        }
        // SAFETY: a handle to wait on and read the exit code; closed by `Attached`.
        let process = unsafe {
            OpenProcess(
                PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
                false,
                pid,
            )
        };
        match process {
            Ok(process) => {
                self.attached.insert(
                    pid,
                    Attached {
                        app,
                        process,
                        at: Instant::now(),
                        frameworks: vec![framework],
                    },
                );
            }
            Err(e) => log::warn!("can't watch {app} ({pid}): {e}"),
        }
    }

    /// An attached process ended. A crash soon after we attached is a strike against its app.
    fn exited(&mut self, pid: u32) {
        self.seen.remove(&pid);
        let Some(entry) = self.attached.remove(&pid) else {
            return;
        };
        let mut code = 0;
        // SAFETY: a live handle and a valid out pointer.
        let _ = unsafe { GetExitCodeProcess(entry.process, &mut code) };
        // NTSTATUS errors and warnings (access violation, fail-fast, heap corruption, ...); a
        // normal close or a Task Manager restart isn't one.
        let crashed = code >= 0x8000_0000;
        let soon = entry.at.elapsed() < CRASH_WINDOW;
        log::info!("{} ({pid}) exited with {code:#x}", entry.app);
        if crashed && soon {
            let strikes = self.strikes.entry(entry.app).or_default();
            *strikes += 1;
            log::warn!(
                "{} crashed {:?} after we attached ({} of {CRASH_LIMIT})",
                entry.app,
                entry.at.elapsed(),
                strikes
            );
            if *strikes >= CRASH_LIMIT {
                log::error!("not styling {} again this session", entry.app);
                self.disabled.insert(entry.app);
            }
        }
    }
}

/// The class of `hwnd`'s top-level window.
fn root_class(hwnd: HWND) -> Option<String> {
    // SAFETY: plain queries; a stale handle just fails.
    let root = unsafe { GetAncestor(hwnd, GA_ROOT) };
    let mut buffer = [0u16; 64];
    // SAFETY: the buffer is passed with its length.
    let len = unsafe { GetClassNameW(root, &mut buffer) } as usize;
    (len > 0).then(|| String::from_utf16_lossy(&buffer[..len]))
}

/// The wallpaper's (accent, dominant) colours, if `palette.json` is for the active theme.
fn read_palette(theme: Option<&Path>) -> Option<([u8; 3], [u8; 3])> {
    #[derive(serde::Deserialize)]
    struct File {
        theme: PathBuf,
        dominant: mypaper_config::Color,
        accent: mypaper_config::Color,
    }
    let text = std::fs::read_to_string(palette_path().ok()?).ok()?;
    let file: File = serde_json::from_str(&text).ok()?;
    let same = |a: &Path, b: &Path| {
        let norm = |p: &Path| {
            p.to_string_lossy()
                .replace('/', "\\")
                .trim_end_matches('\\')
                .to_lowercase()
        };
        norm(a) == norm(b)
    };
    theme
        .is_none_or(|t| same(t, &file.theme))
        .then_some((file.accent.0, file.dominant.0))
}

fn write_atomic(path: &Path, text: &str) -> Result<()> {
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, text)?;
    std::fs::rename(&temp, path)?;
    Ok(())
}

/// Queues every visible top-level window, and the app windows inside UWP frames (Settings'
/// `CoreWindow` lives in an `ApplicationFrameWindow` that ApplicationFrameHost owns), for the
/// first pass.
fn sweep() {
    unsafe extern "system" fn visit(hwnd: HWND, _: LPARAM) -> BOOL {
        // SAFETY: a plain query.
        if unsafe { IsWindowVisible(hwnd) }.as_bool() {
            SHOWN.with_borrow_mut(|s| s.push(hwnd));
            if root_class(hwnd).as_deref() == Some("ApplicationFrameWindow") {
                // SAFETY: as for the top level; children are queued the same way.
                let _ = unsafe { EnumChildWindows(Some(hwnd), Some(child), LPARAM(0)) };
            }
        }
        true.into()
    }
    unsafe extern "system" fn child(hwnd: HWND, _: LPARAM) -> BOOL {
        SHOWN.with_borrow_mut(|s| s.push(hwnd));
        true.into()
    }
    // SAFETY: the callbacks only touch a thread-local on this thread.
    let _ = unsafe { EnumWindows(Some(visit), LPARAM(0)) };
}
unsafe extern "system" fn on_show(
    _: HWINEVENTHOOK,
    _: u32,
    hwnd: HWND,
    object: i32,
    _: i32,
    _: u32,
    _: u32,
) {
    if object == OBJID_WINDOW.0 && !hwnd.is_invalid() {
        SHOWN.with_borrow_mut(|s| {
            // Bounded: the loop drains it after every wake.
            if s.len() < 256 {
                s.push(hwnd);
            }
        });
    }
}
