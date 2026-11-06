//! The helper's run loop: follow the config, keep each app's rules file current, attach to
//! Explorer and Settings as they appear, and stop styling an app that keeps crashing.
//!
//! Event-driven only: it sleeps in `MsgWaitForMultipleObjects` on core (when core started it),
//! the attached processes, their sessions' `ended` events and its message queue, which a
//! window-shown WinEvent hook, a window-destroyed hook per attached Explorer and the
//! config/palette file watches feed. The only timeout is a pending recycle's.
//!
//! **Recycling Explorer's session.** XAML Diagnostics keeps every tree it has reported for as
//! long as a session holds it, closed windows included (docs/windows-styling.md, "Explorer's
//! memory"). So when one of Explorer's folder windows or XAML island hosts closes, and nothing
//! else closes for [`RECYCLE_QUIET`], the helper sets the session's `recycle` event: the TAP hands
//! the look over and lets go of XAML Diagnostics (which frees the closed trees), sets `ended`, and
//! the helper attaches a new session that takes the look over.

use std::cell::RefCell;
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use mypaper_app_styler::presets::{self, Colors};
use mypaper_app_styler::{Attach, Framework, SessionEvent, stop_marker};
use mypaper_config::Config;
use mypaper_theme::LoadedTheme;
use windows::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, FILETIME, GetLastError, HANDLE, HWND, LPARAM,
    WAIT_ABANDONED, WAIT_OBJECT_0, WPARAM,
};
use windows::Win32::System::Threading::{
    CreateMutexW, EVENT_MODIFY_STATE, GetCurrentProcess, GetCurrentThreadId, GetExitCodeProcess,
    GetProcessTimes, INFINITE, OpenEventW, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_SYNCHRONIZE, SYNCHRONIZATION_SYNCHRONIZE, SetEvent, WaitForSingleObject,
};
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    CHILDID_SELF, DispatchMessageW, EVENT_OBJECT_DESTROY, EVENT_OBJECT_SHOW, EnumChildWindows,
    EnumWindows, GA_ROOT, GetAncestor, GetClassNameW, GetWindowThreadProcessId, IsWindowVisible,
    MSG, MsgWaitForMultipleObjects, OBJID_WINDOW, PM_REMOVE, PeekMessageW, PostThreadMessageW,
    QS_ALLINPUT, TranslateMessage, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS, WM_APP,
    WM_QUIT,
};
use windows_core::{BOOL, PCWSTR, w};

use super::{Pinned, attach, data_dir, pin, tap_path};

/// `config.json` or `palette.json` changed.
const WM_RESOLVE: u32 = WM_APP + 1;

/// A styled process that dies this soon after we attached counts against its app.
const CRASH_WINDOW: Duration = Duration::from_secs(60);
/// That many such deaths and the app isn't styled again this session.
const CRASH_LIMIT: usize = 2;
/// Deaths of one app this close together are one incident (one Explorer restart ends every
/// Explorer process at once), so they count once.
const INCIDENT: Duration = Duration::from_secs(5);

/// After one of Explorer's folder windows or island hosts closes, how long nothing else may close
/// before its session is recycled (closing several windows recycles once).
const RECYCLE_QUIET: Duration = Duration::from_secs(2);
/// How long a session may take to end before a new one attaches anyway.
const SESSION_END: Duration = Duration::from_secs(15);
/// Explorer's windows whose closing leaves a XAML tree that a session keeps: folder windows and
/// the WinUI 3 island hosts inside them (Home, Gallery, the header).
const HOSTS: [&str; 2] = [
    "CabinetWClass",
    "Microsoft.UI.Content.DesktopChildSiteBridge",
];

thread_local! {
    /// Windows shown since the last pass (filled by the WinEvent callback).
    static SHOWN: RefCell<Vec<HWND>> = const { RefCell::new(Vec::new()) };
    /// Windows of attached Explorers destroyed since the last pass.
    static DESTROYED: RefCell<Vec<HWND>> = const { RefCell::new(Vec::new()) };
}

/// How long a new helper waits for the previous one to finish exiting (core restarted).
const PREVIOUS_EXIT: u32 = 10_000;

/// Runs until the flag goes off, the process is told to quit, or `parent` (core, which started
/// us) exits. `preview`: style with that preset (under the user's `look.apps`) whatever the flag
/// and the theme say, to try one. `only`: style that process alone (for tests in a separate
/// Explorer).
pub fn run(preview: Option<String>, parent: Option<u32>, only: Option<u32>) -> Result<()> {
    // Watch core first: if it is already gone there is nothing to do.
    let parent = match parent.map(open_parent) {
        Some(Some(handle)) => Some(handle),
        Some(None) => {
            log::info!("core has already exited; not starting");
            return Ok(());
        }
        None => None,
    };
    // SAFETY: a named mutex held for the life of the process.
    let single = unsafe { CreateMutexW(None, true, w!("Local\\mypaper-styler"))? };
    // SAFETY: reads this thread's last error.
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        // A helper whose core just exited is still putting the apps back; wait for it.
        // SAFETY: the mutex handle from above, kept for the life of the process.
        let waited = unsafe { WaitForSingleObject(single, PREVIOUS_EXIT) };
        if waited != WAIT_OBJECT_0 && waited != WAIT_ABANDONED {
            bail!("mypaper-styler is already running");
        }
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

    let mut helper = Helper::new(preview, parent, only)?;
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
    helper.finish_recycles();
    // Exiting is what makes every TAP put its app back.
    result
}

/// A named event of a TAP session, closed on drop.
struct Event(HANDLE);

impl Drop for Event {
    fn drop(&mut self) {
        // SAFETY: opened by us, closed once.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

impl Event {
    /// Opens a session's event, if it exists.
    fn open(event: SessionEvent, pid: u32, framework: Framework) -> Option<Self> {
        let name: Vec<u16> = event
            .name(pid, framework)
            .encode_utf16()
            .chain([0])
            .collect();
        // SAFETY: a NUL-terminated name; closed by `Event`.
        unsafe {
            OpenEventW(
                SYNCHRONIZATION_SYNCHRONIZE | EVENT_MODIFY_STATE,
                false,
                PCWSTR(name.as_ptr()),
            )
        }
        .ok()
        .map(Self)
    }

    /// Waits for it to be set, at most `timeout`.
    fn wait(&self, timeout: Duration) -> bool {
        // SAFETY: a live handle.
        let waited = unsafe { WaitForSingleObject(self.0, timeout.as_millis() as u32) };
        waited == WAIT_OBJECT_0
    }
}

/// A WinEvent hook, removed on drop.
struct Hook(HWINEVENTHOOK);

impl Drop for Hook {
    fn drop(&mut self) {
        // SAFETY: our hook, removed once.
        let _ = unsafe { UnhookWinEvent(self.0) };
    }
}

/// A wait handle on core, `pid`, if that process is still the one that started us: a process
/// created after us under a reused pid can't be. `None` when core is gone.
fn open_parent(pid: u32) -> Option<Process> {
    // SAFETY: a wait and query handle, closed by `Process`.
    let handle = unsafe {
        OpenProcess(
            PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
            false,
            pid,
        )
    }
    .ok()?;
    let parent = Process(handle);
    let created = |process: HANDLE| {
        let (mut created, mut exited, mut kernel, mut user) = Default::default();
        // SAFETY: a live handle and valid out pointers.
        unsafe { GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user) }
            .ok()
            .map(|()| filetime(created))
    };
    // SAFETY: the pseudo handle needs no closing.
    let ours = created(unsafe { GetCurrentProcess() })?;
    (created(parent.0)? <= ours).then_some(parent)
}

fn filetime(time: FILETIME) -> u64 {
    (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime)
}

/// An owned process handle, closed on drop.
struct Process(HANDLE);

impl Drop for Process {
    fn drop(&mut self) {
        // SAFETY: opened by us, closed once.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

/// `%LOCALAPPDATA%\mypaper\palette.json`, which the wallpaper writes on every theme (re)load.
fn palette_path() -> Result<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA").context("LOCALAPPDATA is not set")?;
    Ok(PathBuf::from(local).join("mypaper").join("palette.json"))
}

struct Helper {
    /// `--preview PRESET`: ignore the flag and the theme's `apps`.
    preview: Option<String>,
    /// Core, when it started us: we exit with it (which puts the apps back).
    parent: Option<Process>,
    tap: PathBuf,
    dir: PathBuf,
    /// Attached processes by pid, pinned (see [`Pinned`]) from before the TAP loads, so a crash
    /// while it loads is seen too.
    attached: HashMap<u32, Attached>,
    /// Pids that aren't Explorer or Settings (or were refused), not asked again.
    refused: HashSet<u32>,
    /// Pids that are, but aren't attached yet (Explorer before its first folder window). Only a
    /// filter: attaching asks the allowlist again.
    seen: HashMap<u32, &'static str>,
    /// Incidents of deaths soon after attaching, per app, and when the last one was.
    strikes: HashMap<&'static str, (usize, Instant)>,
    disabled: HashSet<&'static str>,
    /// `--only PID`: no other process is styled.
    only: Option<u32>,
}

struct Attached {
    process: Pinned,
    /// When the last attach started.
    at: Instant,
    sessions: Vec<TapSession>,
    /// Explorer: its folder windows and island hosts we know of (see [`HOSTS`]).
    hosts: HashSet<isize>,
    /// Explorer: this process's window-destroyed events, while its WinUI 3 session can recycle.
    destroy_hook: Option<Hook>,
    /// When to recycle its sessions (a host closed, and nothing since).
    recycle_at: Option<Instant>,
}

/// A TAP session in an attached process.
struct TapSession {
    framework: Framework,
    /// The session's events (see [`SessionEvent`]); `None` if they never appeared (it can't be
    /// recycled then).
    recycle: Option<Event>,
    ended: Option<Event>,
    /// Asked to recycle: attach again once it has ended.
    recycling: bool,
}

/// What woke the serve loop, by handle index.
#[derive(Clone, Copy)]
enum Wake {
    Parent,
    Process(u32),
    Ended(u32, Framework),
}

impl Helper {
    fn new(preview: Option<String>, parent: Option<Process>, only: Option<u32>) -> Result<Self> {
        let dir = data_dir()?;
        // A fresh session starts clean: stop markers from an earlier one's crash guard go.
        for app in mypaper_config::apps::APPS {
            let _ = std::fs::remove_file(stop_marker(&dir, &rules_name(app)));
        }
        Ok(Self {
            tap: tap_path()?,
            preview,
            parent,
            dir,
            attached: HashMap::new(),
            refused: HashSet::new(),
            seen: HashMap::new(),
            strikes: HashMap::new(),
            disabled: HashSet::new(),
            only,
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
        // The same `$accent` / `$dominant` the native look and Windows styling use.
        let shared = mypaper_theme::windows::resolve(
            &config.look,
            theme.as_ref().map(|t| &t.theme),
            palette.map(|(accent, dominant)| mypaper_theme::windows::Palette {
                accent: mypaper_config::Color(accent),
                dominant: mypaper_config::Color(dominant),
            }),
            mypaper_tweaks::unmanaged_system_prefers_dark(),
        );
        let colors = Colors {
            accent: shared.accent.map(|c| c.0),
            dominant: shared.dominant.map(|c| c.0),
        };
        let theme_apps = theme.as_ref().and_then(|t| t.theme.apps.as_ref());
        for app in mypaper_config::apps::APPS {
            // The crash guard gave up on it: its rules aren't published any more.
            if self.disabled.contains(app) {
                continue;
            }
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
            let path = self.dir.join(rules_name(app));
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
            // Core first (when it started us), then the attached processes and their sessions'
            // `ended` events.
            let mut handles: Vec<HANDLE> = Vec::new();
            let mut wakes: Vec<Wake> = Vec::new();
            if let Some(parent) = &self.parent {
                handles.push(parent.0);
                wakes.push(Wake::Parent);
            }
            for (&pid, entry) in &self.attached {
                handles.push(entry.process.handle());
                wakes.push(Wake::Process(pid));
                for session in &entry.sessions {
                    if let Some(ended) = &session.ended {
                        handles.push(ended.0);
                        wakes.push(Wake::Ended(pid, session.framework));
                    }
                }
            }
            let timeout = self
                .attached
                .values()
                .filter_map(|e| e.recycle_at)
                .min()
                .map_or(INFINITE, |at| {
                    at.saturating_duration_since(Instant::now()).as_millis() as u32 + 1
                });
            let wait_on = (!handles.is_empty()).then_some(handles.as_slice());
            // SAFETY: the handles stay open while their entries exist.
            let woke = unsafe { MsgWaitForMultipleObjects(wait_on, false, timeout, QS_ALLINPUT) };
            let index = woke.0.wrapping_sub(WAIT_OBJECT_0.0) as usize;
            match wakes.get(index) {
                Some(Wake::Parent) => {
                    // Quit, panic hotkey or a crash of core: nothing supervises us any more.
                    log::info!("core exited; exiting (the apps get their look back)");
                    return Ok(());
                }
                Some(&Wake::Process(pid)) => self.exited(pid),
                Some(&Wake::Ended(pid, framework)) => self.session_ended(pid, framework),
                None => {} // messages, or a recycle is due
            }
            let mut msg = MSG::default();
            // SAFETY: a standard message pump on this thread.
            unsafe {
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    match msg.message {
                        WM_QUIT => return Ok(()),
                        WM_RESOLVE => {
                            // A config writer that isn't atomic can show a moment with no file
                            // (defaults: off). Only an "off" that is still off a second later
                            // counts; core stops us when the flag really goes off anyway.
                            if !self.resolve() && {
                                std::thread::sleep(Duration::from_secs(1));
                                !self.resolve()
                            } {
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
            self.consider_destroyed();
            self.recycle_due();
        }
    }

    /// A host window of an attached Explorer closed: its tree is now kept only by the session,
    /// so recycle it once nothing else has closed for a moment.
    fn consider_destroyed(&mut self) {
        let destroyed = DESTROYED.with_borrow_mut(std::mem::take);
        for hwnd in destroyed {
            for entry in self.attached.values_mut() {
                if entry.hosts.remove(&(hwnd.0 as isize)) {
                    entry.recycle_at = Some(Instant::now() + RECYCLE_QUIET);
                }
            }
        }
    }

    /// Asks the sessions whose recycle is due to end and hand their look over.
    fn recycle_due(&mut self) {
        let now = Instant::now();
        for (pid, entry) in &mut self.attached {
            if !entry.recycle_at.is_some_and(|at| at <= now) {
                continue;
            }
            entry.recycle_at = None;
            for session in &mut entry.sessions {
                if let (false, Some(recycle)) = (session.recycling, &session.recycle) {
                    // SAFETY: a live event handle.
                    if unsafe { SetEvent(recycle.0) }.is_ok() {
                        log::debug!(
                            "recycling {} ({pid}, {})",
                            entry.process.app,
                            session.framework
                        );
                        session.recycling = true;
                    }
                }
            }
        }
    }

    /// A session ended: attach a new one if it was recycled (it has let go of XAML Diagnostics
    /// by now, so the new one is safe).
    fn session_ended(&mut self, pid: u32, framework: Framework) {
        let Some(entry) = self.attached.get_mut(&pid) else {
            return;
        };
        let Some(at) = entry.sessions.iter().position(|s| s.framework == framework) else {
            return;
        };
        let app = entry.process.app;
        let session = entry.sessions.remove(at);
        if session.recycling {
            drop(session);
            self.attach_to(pid, app, framework);
        } else {
            log::info!("{app} ({pid}): the {framework} session ended");
        }
    }

    /// Exiting: a session asked to recycle has handed its look over and is ending. Attach once
    /// more, so that a session takes the look over and, seeing us gone, puts it back.
    fn finish_recycles(&mut self) {
        let pending: Vec<(u32, &'static str, Framework)> = self
            .attached
            .iter()
            .flat_map(|(&pid, e)| {
                e.sessions
                    .iter()
                    .filter(|s| s.recycling)
                    .map(move |s| (pid, e.process.app, s.framework))
            })
            .collect();
        for (pid, app, framework) in pending {
            if let Some(entry) = self.attached.get_mut(&pid) {
                entry.sessions.retain(|s| s.framework != framework);
            }
            log::info!("{app} ({pid}): finishing a recycle before exiting");
            self.attach_to(pid, app, framework);
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
            if pid == 0 || self.refused.contains(&pid) || self.only.is_some_and(|o| o != pid) {
                continue;
            }
            if let Some(entry) = self.attached.get_mut(&pid)
                && entry.destroy_hook.is_some()
                && is_host(hwnd)
            {
                entry.hosts.insert(hwnd.0 as isize);
            }
            let app = match self
                .attached
                .get(&pid)
                .map(|a| a.process.app)
                .or_else(|| self.seen.get(&pid).copied())
            {
                Some(app) => app,
                None => match pin(pid) {
                    Some(process) => {
                        self.seen.insert(pid, process.app);
                        process.app
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
                .is_some_and(|a| a.sessions.iter().any(|s| s.framework == framework))
            {
                continue;
            }
            self.attach_to(pid, app, framework);
        }
    }

    fn attach_to(&mut self, pid: u32, app: &'static str, framework: Framework) {
        // Pin the process and ask the allowlist about that very process, then keep it pinned
        // (pids are reused, but not while we hold a handle) and watched from before the TAP
        // loads: a crash while it loads counts too.
        if let Entry::Vacant(slot) = self.attached.entry(pid) {
            match pin(pid) {
                Some(process) if process.app == app => {
                    slot.insert(Attached {
                        process,
                        at: Instant::now(),
                        sessions: Vec::new(),
                        hosts: HashSet::new(),
                        destroy_hook: None,
                        recycle_at: None,
                    });
                }
                _ => {
                    self.seen.remove(&pid);
                    self.refused.insert(pid);
                    return;
                }
            }
        }
        let Some(entry) = self.attached.get_mut(&pid) else {
            return;
        };
        if !entry.process.running() {
            return self.exited(pid);
        }
        // A session of this process that is still ending (ours being recycled, or a helper's
        // before us) would switch the new one off when it finally lets go: wait for it.
        if let Some(previous) = Event::open(SessionEvent::Ended, pid, framework)
            && !previous.wait(SESSION_END)
        {
            log::warn!("{app} ({pid}): the last {framework} session hasn't ended; attaching anyway");
        }
        entry.at = Instant::now();
        let data = Attach {
            helper: std::process::id(),
            framework,
            dir: self.dir.clone(),
            rules: rules_name(app),
            dump: false,
        };
        match attach::attach(pid, framework, &self.tap, &data) {
            Ok(()) => {
                let (recycle, ended) = session_events(pid, framework);
                if ended.is_none() {
                    log::warn!("{app} ({pid}): the session shows no events; it won't recycle");
                }
                log::info!("styling {app} ({pid}, {framework})");
                entry.sessions.push(TapSession {
                    framework,
                    recycle,
                    ended,
                    recycling: false,
                });
                if framework == Framework::WinUi && entry.destroy_hook.is_none() {
                    entry.destroy_hook = destroy_hook(pid);
                    entry.hosts = hosts_of(pid);
                }
            }
            Err(e) => {
                // WinUI not loaded yet in a fresh Explorer: a later window will try again.
                log::debug!("{app} ({pid}): {e:#}");
                if !entry.process.running() {
                    // It died while we attached: the crash guard decides whether that was us.
                    self.exited(pid);
                } else if entry.sessions.is_empty() {
                    self.attached.remove(&pid);
                }
            }
        }
    }

    /// An attached process ended. A crash soon after we attached is a strike against its app.
    fn exited(&mut self, pid: u32) {
        self.seen.remove(&pid);
        let Some(entry) = self.attached.remove(&pid) else {
            return;
        };
        let app = entry.process.app;
        let mut code = 0;
        // SAFETY: a live handle and a valid out pointer.
        let _ = unsafe { GetExitCodeProcess(entry.process.handle(), &mut code) };
        // NTSTATUS errors and warnings (access violation, fail-fast, heap corruption, ...) and
        // being ended with TerminateProcess(-1); a normal close or Task Manager's Explorer
        // restart isn't one.
        let crashed = code >= 0x8000_0000;
        let soon = entry.at.elapsed() < CRASH_WINDOW;
        log::info!("{app} ({pid}) exited with {code:#x}");
        if !(crashed && soon) {
            return;
        }
        let (strikes, last) = self
            .strikes
            .entry(app)
            .or_insert((0, Instant::now() - INCIDENT * 2));
        if last.elapsed() < INCIDENT {
            log::warn!(
                "{app} also died {:?} after we attached: same incident",
                entry.at.elapsed()
            );
            return;
        }
        *strikes += 1;
        *last = Instant::now();
        log::warn!(
            "{app} crashed {:?} after we attached ({} of {CRASH_LIMIT})",
            entry.at.elapsed(),
            strikes
        );
        if *strikes >= CRASH_LIMIT {
            self.disable(app);
        }
    }

    /// Stops styling `app` for this session: no new attaches, no more rules for it, and its
    /// sessions still running (Explorer can have several processes) put it back and detach
    /// when they see its stop marker.
    fn disable(&mut self, app: &'static str) {
        log::error!("not styling {app} again this session");
        self.disabled.insert(app);
        let marker = stop_marker(&self.dir, &rules_name(app));
        if let Err(e) = std::fs::write(&marker, b"crash guard\n") {
            log::warn!("writing {}: {e}", marker.display());
        }
        self.attached.retain(|_, a| a.process.app != app);
    }
}

/// An app's rules file name in the styler's folder.
fn rules_name(app: &str) -> String {
    format!("{app}.json")
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

/// A new session's events. The TAP makes them while our attach call is in progress, but the call
/// can return before that: give it a few seconds (only right after attaching).
fn session_events(pid: u32, framework: Framework) -> (Option<Event>, Option<Event>) {
    for _ in 0..50 {
        if let Some(ended) = Event::open(SessionEvent::Ended, pid, framework) {
            return (Event::open(SessionEvent::Recycle, pid, framework), Some(ended));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    (None, None)
}

/// Window-destroyed events from `pid` alone, for [`on_destroy`].
fn destroy_hook(pid: u32) -> Option<Hook> {
    // SAFETY: an out-of-context hook with a 'static callback, removed by `Hook`.
    let hook = unsafe {
        SetWinEventHook(
            EVENT_OBJECT_DESTROY,
            EVENT_OBJECT_DESTROY,
            None,
            Some(on_destroy),
            pid,
            0,
            WINEVENT_OUTOFCONTEXT,
        )
    };
    if hook.is_invalid() {
        log::warn!("explorer ({pid}): no window-destroyed hook; its session won't recycle");
        return None;
    }
    Some(Hook(hook))
}

fn class_of(hwnd: HWND) -> Option<String> {
    let mut buffer = [0u16; 64];
    // SAFETY: the buffer is passed with its length.
    let len = unsafe { GetClassNameW(hwnd, &mut buffer) } as usize;
    (len > 0).then(|| String::from_utf16_lossy(&buffer[..len]))
}

fn is_host(hwnd: HWND) -> bool {
    class_of(hwnd).is_some_and(|c| HOSTS.contains(&c.as_str()))
}

/// The host windows `pid` has now: its folder windows and the island hosts inside them.
fn hosts_of(pid: u32) -> HashSet<isize> {
    struct Find {
        pid: u32,
        found: HashSet<isize>,
    }
    unsafe extern "system" fn top(hwnd: HWND, data: LPARAM) -> BOOL {
        // SAFETY: `data` is the `Find` below, alive for the enumeration.
        let find = unsafe { &mut *(data.0 as *mut Find) };
        let mut pid = 0;
        // SAFETY: a plain query.
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        if pid == find.pid && class_of(hwnd).as_deref() == Some("CabinetWClass") {
            find.found.insert(hwnd.0 as isize);
            // SAFETY: as above; the children go through `child`.
            let _ = unsafe { EnumChildWindows(Some(hwnd), Some(child), data) };
        }
        true.into()
    }
    unsafe extern "system" fn child(hwnd: HWND, data: LPARAM) -> BOOL {
        // SAFETY: as in `top`.
        let find = unsafe { &mut *(data.0 as *mut Find) };
        if is_host(hwnd) {
            find.found.insert(hwnd.0 as isize);
        }
        true.into()
    }
    let mut find = Find {
        pid,
        found: HashSet::new(),
    };
    // SAFETY: the callbacks only touch `find`, which outlives the call.
    let _ = unsafe { EnumWindows(Some(top), LPARAM(&mut find as *mut Find as isize)) };
    find.found
}

unsafe extern "system" fn on_destroy(
    _: HWINEVENTHOOK,
    _: u32,
    hwnd: HWND,
    object: i32,
    child: i32,
    _: u32,
    _: u32,
) {
    if object == OBJID_WINDOW.0 && child == CHILDID_SELF as i32 && !hwnd.is_invalid() {
        DESTROYED.with_borrow_mut(|d| {
            // Bounded: the loop drains it after every wake.
            if d.len() < 1024 {
                d.push(hwnd);
            }
        });
    }
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
