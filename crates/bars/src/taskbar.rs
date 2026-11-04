//! The native taskbar: kept, auto-hidden or hidden, and aligned, through supported APIs only
//! (`SHAppBarMessage`, `ShowWindow`, window styles, the `TaskbarAl` value Settings writes).
//!
//! Hidden means two things. The windows are `SW_HIDE`n, and they carry a **veil** ([`veil`]): a
//! layered, click-through style at alpha 0, so that when Explorer shows one anyway (leaving a
//! fullscreen app, a display change, the shell's own restarts) Windows has nothing to compose: it
//! is never seen, even for a frame, however late we notice and hide it again.
//!
//! Everything is reversible: before the first change of anything, its original is written to
//! `%LOCALAPPDATA%\mypaper\taskbar.json`. Applying `keep` (on exit, on the panic hotkey, by core
//! after a crash) puts each original back and deletes the journal. The logic runs against the
//! [`Shell`] trait, so it's tested without touching the real taskbar.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use windows::Win32::Foundation::{
    COLORREF, CloseHandle, ERROR_FILE_NOT_FOUND, HWND, LPARAM, WPARAM,
};
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_DWORD, RRF_RT_REG_DWORD, RegDeleteKeyValueW, RegGetValueW,
    RegSetKeyValueW,
};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::Shell::{
    ABM_GETSTATE, ABM_SETSTATE, ABS_AUTOHIDE, APPBARDATA, SHAppBarMessage,
};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowExW, GWL_EXSTYLE, GetLayeredWindowAttributes, GetShellWindow, GetWindowLongPtrW,
    GetWindowThreadProcessId, IsWindowVisible, LAYERED_WINDOW_ATTRIBUTES_FLAGS, LWA_ALPHA,
    SMTO_ABORTIFHUNG, SW_HIDE, SW_SHOWNA, SendMessageTimeoutW, SetLayeredWindowAttributes,
    SetWindowLongPtrW, ShowWindow, ShowWindowAsync, WM_SETTINGCHANGE, WS_EX_LAYERED,
    WS_EX_TRANSPARENT,
};
use windows::core::{PCWSTR, PWSTR, w};

use crate::config::{Alignment, TaskbarMode};

pub const JOURNAL: &str = "taskbar.json";

const ADVANCED: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced");
const TASKBAR_AL: PCWSTR = w!("TaskbarAl");

/// What the taskbar should look like.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Wanted {
    pub mode: TaskbarMode,
    /// `None`: Windows' own alignment.
    pub alignment: Option<Alignment>,
}

/// The taskbar as the operating system has it.
pub trait Shell {
    /// `ABM_GETSTATE`: `ABS_AUTOHIDE` and `ABS_ALWAYSONTOP` bits.
    fn appbar_state(&self) -> u32;
    fn set_appbar_state(&mut self, state: u32) -> Result<()>;
    /// Hides or shows every taskbar window (one per monitor).
    fn set_hidden(&mut self, hidden: bool) -> Result<()>;
    /// `TaskbarAl`, or `None` when the value is absent (Windows' default, centered).
    fn alignment(&self) -> Result<Option<u32>>;
    /// Writes `TaskbarAl`, or deletes it for `None`.
    fn set_alignment(&mut self, value: Option<u32>) -> Result<()>;
}

/// What a value was before we first changed it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Was<T> {
    pub was: T,
}

/// The undo journal: one entry per thing we changed, holding its original.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Journal {
    /// The app-bar state before we made the taskbar auto-hide.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub appbar: Option<Was<u32>>,
    /// We hid and veiled the taskbar windows (they were visible).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
    /// `TaskbarAl` before we changed it (`null`: absent).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alignment: Option<Was<Option<u32>>>,
}

impl Journal {
    fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Applies [`Wanted`] to a [`Shell`], journaled at `path`.
pub struct Taskbar<S> {
    shell: S,
    path: PathBuf,
}

impl Taskbar<NativeShell> {
    /// The real taskbar, journaled at `%LOCALAPPDATA%\mypaper\taskbar.json`.
    pub fn native() -> Result<Self> {
        Ok(Self::new(NativeShell, crate::files::local(JOURNAL)?))
    }
}

impl<S: Shell> Taskbar<S> {
    pub fn new(shell: S, path: PathBuf) -> Self {
        Self { shell, path }
    }

    /// Is there a journal, i.e. something to undo?
    pub fn pending(&self) -> bool {
        self.path.exists()
    }

    /// Makes the taskbar match `wanted`. Originals are journaled before the first change of
    /// each thing and kept through later changes, so `keep` always returns to how the user had
    /// it. Idempotent: call it again whenever Explorer may have reset something (it restarted, a
    /// monitor came back).
    pub fn apply(&mut self, wanted: Wanted) -> Result<()> {
        let mut journal = self.load()?.unwrap_or_default();
        let autohide = wanted.mode != TaskbarMode::Keep;
        let hide = wanted.mode == TaskbarMode::Hide;

        // Write-ahead: the originals are on disk before anything changes.
        let before = journal.clone();
        if autohide && journal.appbar.is_none() {
            journal.appbar = Some(Was {
                was: self.shell.appbar_state(),
            });
        }
        journal.hidden |= hide;
        if wanted.alignment.is_some() && journal.alignment.is_none() {
            journal.alignment = Some(Was {
                was: self.shell.alignment()?,
            });
        }
        if journal != before {
            self.save(&journal)?;
        }

        let mut failures = Vec::new();
        let mut attempt = |what: &str, result: Result<()>| match result {
            Ok(()) => true,
            Err(e) => {
                failures.push(format!("{what}: {e:#}"));
                false
            }
        };
        if hide {
            attempt("hiding", self.shell.set_hidden(true));
        } else if journal.hidden && attempt("showing", self.shell.set_hidden(false)) {
            journal.hidden = false;
        }
        if let Some(Was { was }) = journal.appbar {
            let target = if autohide { was | ABS_AUTOHIDE } else { was };
            let done = self.shell.appbar_state() == target
                || attempt("auto-hide", self.shell.set_appbar_state(target));
            if done && !autohide {
                journal.appbar = None;
            }
        }
        if let Some(Was { was }) = journal.alignment {
            let target = wanted.alignment.map(Alignment::value).or(was);
            let current = self.shell.alignment();
            let done = current.as_ref().is_ok_and(|c| *c == target)
                || attempt("alignment", self.shell.set_alignment(target));
            if done && wanted.alignment.is_none() {
                journal.alignment = None;
            }
        }

        if journal.is_empty() {
            match std::fs::remove_file(&self.path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => failures.push(format!("deleting the journal: {e}")),
            }
        } else if journal != before {
            self.save(&journal)?;
        }
        if !failures.is_empty() {
            bail!("taskbar: {}", failures.join("; "));
        }
        Ok(())
    }

    /// Undoes everything in the journal (if any). Returns whether there was a journal.
    pub fn restore(&mut self) -> Result<bool> {
        let pending = self.pending();
        if pending {
            self.apply(Wanted::default())?;
        }
        Ok(pending)
    }

    /// `None` if there is no journal. An unreadable one is set aside as `taskbar.json.bad`
    /// rather than blocking every start.
    fn load(&self) -> Result<Option<Journal>> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e).context("reading the taskbar journal"),
        };
        match serde_json::from_str(&text) {
            Ok(journal) => Ok(Some(journal)),
            Err(e) => {
                let bad = self.path.with_extension("json.bad");
                let _ = std::fs::rename(&self.path, &bad);
                bail!(
                    "the taskbar journal is corrupt ({e}); moved to {}",
                    bad.display()
                )
            }
        }
    }

    fn save(&self, journal: &Journal) -> Result<()> {
        let text = serde_json::to_string_pretty(journal)? + "\n";
        crate::files::write_atomic(&self.path, text.as_bytes())
    }
}

/// Puts the native taskbar back from a journal left by a bars process that didn't exit
/// cleanly (killed, crashed). Returns whether there was anything to undo.
pub fn replay() -> Result<bool> {
    Taskbar::native()?.restore()
}

/// The real taskbar.
pub struct NativeShell;

impl Shell for NativeShell {
    fn appbar_state(&self) -> u32 {
        let mut data = APPBARDATA {
            cbSize: size_of::<APPBARDATA>() as u32,
            ..Default::default()
        };
        // SAFETY: `data` is a valid APPBARDATA for the call.
        unsafe { SHAppBarMessage(ABM_GETSTATE, &mut data) as u32 }
    }

    fn set_appbar_state(&mut self, state: u32) -> Result<()> {
        let tray = explorer_tray().context("the taskbar isn't there (is Explorer running?)")?;
        let mut data = APPBARDATA {
            cbSize: size_of::<APPBARDATA>() as u32,
            hWnd: tray,
            lParam: LPARAM(state as isize),
            ..Default::default()
        };
        // SAFETY: `data` is a valid APPBARDATA for the call.
        unsafe { SHAppBarMessage(ABM_SETSTATE, &mut data) };
        Ok(())
    }

    fn set_hidden(&mut self, hidden: bool) -> Result<()> {
        let windows = taskbar_windows();
        if windows.is_empty() {
            bail!("the taskbar isn't there (is Explorer running?)");
        }
        for hwnd in windows {
            // Veiled before hidden, unveiled before shown: no moment in which it can be seen.
            if hidden {
                veil(hwnd);
            } else {
                unveil(hwnd);
            }
            // SAFETY: a taskbar window found just now; SW_SHOWNA never activates it.
            let _ = unsafe { ShowWindow(hwnd, if hidden { SW_HIDE } else { SW_SHOWNA }) };
        }
        Ok(())
    }

    fn alignment(&self) -> Result<Option<u32>> {
        let mut value = 0u32;
        let mut bytes = size_of::<u32>() as u32;
        // SAFETY: `value` and `bytes` outlive the call and match RRF_RT_REG_DWORD.
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                ADVANCED,
                TASKBAR_AL,
                RRF_RT_REG_DWORD,
                None,
                Some((&raw mut value).cast()),
                Some(&mut bytes),
            )
        };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        status.ok().context("reading TaskbarAl")?;
        Ok(Some(value))
    }

    fn set_alignment(&mut self, value: Option<u32>) -> Result<()> {
        // SAFETY: valid strings; the data pointer and size describe `value`.
        let status = unsafe {
            match value {
                Some(value) => RegSetKeyValueW(
                    HKEY_CURRENT_USER,
                    ADVANCED,
                    TASKBAR_AL,
                    REG_DWORD.0,
                    Some((&raw const value).cast()),
                    size_of::<u32>() as u32,
                ),
                None => RegDeleteKeyValueW(HKEY_CURRENT_USER, ADVANCED, TASKBAR_AL),
            }
        };
        if !(status.is_ok() || value.is_none() && status == ERROR_FILE_NOT_FOUND) {
            status.ok().context("writing TaskbarAl")?;
        }
        // What Settings announces after changing taskbar settings, sent to the taskbars only:
        // a broadcast waits on every window in the session, which can take seconds.
        for hwnd in taskbar_windows() {
            // SAFETY: a static string that outlives the call; the timeout bounds a hung Explorer.
            unsafe {
                SendMessageTimeoutW(
                    hwnd,
                    WM_SETTINGCHANGE,
                    WPARAM(0),
                    LPARAM(w!("TraySettings").as_ptr() as isize),
                    SMTO_ABORTIFHUNG,
                    500,
                    None,
                )
            };
        }
        Ok(())
    }
}

/// `hide` mode's fast path, when Explorer showed a taskbar again (leaving a fullscreen app, a
/// display change, a restarted Explorer): veils every taskbar window (so none can be seen), hides
/// the ones that are visible and says how many were. No journal I/O; the journal already says
/// hidden. Also reports whether auto-hide is still set, so the caller can re-apply the mode when
/// Explorer reset it.
pub fn hide_visible() -> (usize, bool) {
    let windows = taskbar_windows();
    // All veils first: the hides below are only housekeeping.
    for &hwnd in &windows {
        veil(hwnd);
    }
    let shown = windows
        .into_iter()
        // SAFETY: read-only query, then a hide, on taskbar windows found just now.
        .filter(|&hwnd| unsafe { IsWindowVisible(hwnd) }.as_bool())
        .inspect(|&hwnd| {
            // SAFETY: as above. Async: a busy Explorer mustn't hold up the bars' loop.
            let _ = unsafe { ShowWindowAsync(hwnd, SW_HIDE) };
        })
        .count();
    (shown, NativeShell.appbar_state() & ABS_AUTOHIDE != 0)
}

/// The extended styles of the veil: layered (at alpha 0, set by [`veil`]) and click-through.
const VEIL: u32 = WS_EX_LAYERED.0 | WS_EX_TRANSPARENT.0;

fn ex_style(hwnd: HWND) -> u32 {
    // SAFETY: a read-only query; a stale handle reads as 0.
    unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32 }
}

/// Makes `hwnd` invisible and click-through, whether Explorer shows it or not: a layered window
/// at alpha 0 composes to nothing. Explorer shows, hides, moves and re-clips its taskbar, but
/// never touches its extended style (measured on build 26300: the veil survived Start, Search,
/// Quick Settings, Task View and fullscreen exits; `DWMWA_CLOAK` is refused cross-process with
/// `E_ACCESSDENIED`, and an empty window region is overwritten by auto-hide's own). A window that
/// is already layered by someone else is left alone.
fn veil(hwnd: HWND) {
    let style = ex_style(hwnd);
    if style & WS_EX_LAYERED.0 != 0 {
        return;
    }
    // SAFETY: style changes on another process's top-level window are allowed at the same
    // integrity level; a layered window shows nothing until its attributes are set, which is
    // what we want in between.
    unsafe {
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, (style | VEIL) as isize);
        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 0, LWA_ALPHA);
    }
}

/// Is `hwnd` carrying our veil: layered, click-through and fully transparent? (A taskbar is never
/// that otherwise, so it also tells our veil from someone else's layering.)
fn is_veiled(hwnd: HWND) -> bool {
    if ex_style(hwnd) & VEIL != VEIL {
        return false;
    }
    let (mut alpha, mut flags) = (u8::MAX, LAYERED_WINDOW_ATTRIBUTES_FLAGS(0));
    // SAFETY: the out pointers outlive the call.
    let read =
        unsafe { GetLayeredWindowAttributes(hwnd, None, Some(&mut alpha), Some(&mut flags)) };
    read.is_ok() && alpha == 0 && flags.0 & LWA_ALPHA.0 != 0
}

/// Takes our veil off `hwnd` (if it's ours): the window is as Explorer had it.
fn unveil(hwnd: HWND) {
    if is_veiled(hwnd) {
        // SAFETY: as in `veil`.
        unsafe { SetWindowLongPtrW(hwnd, GWL_EXSTYLE, (ex_style(hwnd) & !VEIL) as isize) };
    }
}

/// The desktop shell's primary tray, independent of Z-order or other Explorer processes.
pub fn explorer_tray() -> Option<HWND> {
    let mut shell_pid = 0;
    // SAFETY: read-only queries; the shell HWND is null while Explorer is down.
    unsafe { GetWindowThreadProcessId(GetShellWindow(), Some(&mut shell_pid)) };
    if shell_pid == 0 {
        return None;
    }
    explorer_windows(w!("Shell_TrayWnd"))
        .into_iter()
        .find(|&hwnd| {
            let mut pid = 0;
            // SAFETY: read-only query on a just-found taskbar.
            unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
            pid == shell_pid
        })
}

/// Every top-level window of `class` that `explorer.exe` owns.
fn explorer_windows(class: PCWSTR) -> Vec<HWND> {
    let mut found = Vec::new();
    let mut after = None;
    // SAFETY: lookups by class name and a read-only query; a stale handle just ends the loop.
    while let Ok(hwnd) = unsafe { FindWindowExW(None, after, class, PCWSTR::null()) } {
        after = Some(hwnd);
        let mut pid = 0;
        // SAFETY: a read-only query on the handle just found; `pid` outlives the call.
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        if pid != std::process::id() && is_explorer(pid) {
            found.push(hwnd);
        }
    }
    found
}

/// Explorer's `Shell_TrayWnd` and every `Shell_SecondaryTrayWnd` (never the tray host's, or
/// another shell's). Found by class with `FindWindowEx`, not enumerated: while Start, Search,
/// Quick Settings or Task View is open, Explorer moves the main taskbar into a higher z-band
/// (`GetWindowBand` 6 instead of 1), and `EnumWindows` skips such windows, so a taskbar found
/// that way could not be hidden then.
pub fn taskbar_windows() -> Vec<HWND> {
    let mut found = explorer_windows(w!("Shell_TrayWnd"));
    found.extend(explorer_windows(w!("Shell_SecondaryTrayWnd")));
    found
}

/// Is `class` a taskbar window's class?
pub fn is_taskbar_class(class: &str) -> bool {
    matches!(class, "Shell_TrayWnd" | "Shell_SecondaryTrayWnd")
}

/// Whether process `pid` runs `explorer.exe`.
fn is_explorer(pid: u32) -> bool {
    // SAFETY: a query handle we close; the buffer outlives the call.
    unsafe {
        let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return false;
        };
        let mut buffer = [0u16; 1024];
        let mut len = buffer.len() as u32;
        let read = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut len,
        );
        let _ = CloseHandle(process);
        read.is_ok() && is_explorer_path(&String::from_utf16_lossy(&buffer[..len as usize]))
    }
}

/// `C:\Windows\explorer.exe`, whatever the folder or case.
fn is_explorer_path(path: &str) -> bool {
    path.rsplit(['\\', '/'])
        .next()
        .is_some_and(|name| name.eq_ignore_ascii_case("explorer.exe"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knows_explorer_by_its_image() {
        assert!(is_explorer_path(r"C:\Windows\explorer.exe"));
        assert!(is_explorer_path(r"C:\WINDOWS\EXPLORER.EXE"));
        assert!(!is_explorer_path(r"C:\Program Files\RetroBar\RetroBar.exe"));
        assert!(!is_explorer_path(r"C:\Tools\myexplorer.exe"));
        assert!(!is_explorer_path(""));
    }

    /// The veil goes on and comes off a real window exactly, and leaves other people's layering
    /// alone. (Our own window stands in for a taskbar.)
    #[test]
    fn the_veil_round_trips_on_a_real_window() {
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, WINDOW_EX_STYLE, WS_POPUP,
        };
        // SAFETY: a plain hidden window of a system class, destroyed below.
        let create = |ex: WINDOW_EX_STYLE| unsafe {
            CreateWindowExW(
                ex,
                w!("STATIC"),
                w!(""),
                WS_POPUP,
                0,
                0,
                10,
                10,
                None,
                None,
                None,
                None,
            )
            .unwrap()
        };

        let plain = create(WINDOW_EX_STYLE(0x88)); // tool window, topmost: like a taskbar
        let before = ex_style(plain);
        assert!(!is_veiled(plain));
        veil(plain);
        assert!(is_veiled(plain));
        assert_eq!(ex_style(plain), before | VEIL);
        veil(plain); // idempotent
        unveil(plain);
        assert_eq!(ex_style(plain), before, "restored exactly");
        unveil(plain); // nothing to undo

        // Someone else's layering is neither replaced nor removed.
        let theirs = create(WS_EX_LAYERED);
        // SAFETY: our own window.
        unsafe { SetLayeredWindowAttributes(theirs, COLORREF(0), 200, LWA_ALPHA).unwrap() };
        let before = ex_style(theirs);
        veil(theirs);
        unveil(theirs);
        assert_eq!(ex_style(theirs), before);

        for hwnd in [plain, theirs] {
            // SAFETY: windows created above, on this thread.
            let _ = unsafe { DestroyWindow(hwnd) };
        }
    }

    /// A taskbar in memory.
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct Fake {
        state: u32,
        hidden: bool,
        alignment: Option<u32>,
        writes: usize,
    }

    impl Shell for Fake {
        fn appbar_state(&self) -> u32 {
            self.state
        }
        fn set_appbar_state(&mut self, state: u32) -> Result<()> {
            self.writes += 1;
            self.state = state;
            Ok(())
        }
        fn set_hidden(&mut self, hidden: bool) -> Result<()> {
            self.hidden = hidden;
            Ok(())
        }
        fn alignment(&self) -> Result<Option<u32>> {
            Ok(self.alignment)
        }
        fn set_alignment(&mut self, value: Option<u32>) -> Result<()> {
            self.writes += 1;
            self.alignment = value;
            Ok(())
        }
    }

    const ALWAYS_ON_TOP: u32 = 2;

    fn user_taskbar() -> Fake {
        Fake {
            state: ALWAYS_ON_TOP,
            hidden: false,
            alignment: None,
            writes: 0,
        }
    }

    fn wanted(mode: TaskbarMode, alignment: Option<Alignment>) -> Wanted {
        Wanted { mode, alignment }
    }

    fn taskbar(name: &str) -> Taskbar<Fake> {
        let dir = crate::files::temp_dir(name);
        Taskbar::new(user_taskbar(), dir.join(JOURNAL))
    }

    fn same_state(a: &Fake, b: &Fake) -> bool {
        (a.state, a.hidden, a.alignment) == (b.state, b.hidden, b.alignment)
    }

    #[test]
    fn hide_then_keep_restores_exactly_and_removes_the_journal() {
        let mut bar = taskbar("hide-keep");
        bar.apply(wanted(TaskbarMode::Hide, Some(Alignment::Left)))
            .unwrap();
        assert_eq!(bar.shell.state, ALWAYS_ON_TOP | ABS_AUTOHIDE);
        assert!(bar.shell.hidden);
        assert_eq!(bar.shell.alignment, Some(0));
        assert!(bar.pending());

        bar.apply(Wanted::default()).unwrap();
        assert!(same_state(&bar.shell, &user_taskbar()));
        assert!(!bar.pending());
    }

    #[test]
    fn originals_are_kept_through_later_changes() {
        let mut bar = taskbar("originals");
        bar.apply(wanted(TaskbarMode::Autohide, Some(Alignment::Left)))
            .unwrap();
        bar.apply(wanted(TaskbarMode::Hide, Some(Alignment::Center)))
            .unwrap();
        bar.apply(wanted(TaskbarMode::Autohide, None)).unwrap();
        assert!(!bar.shell.hidden, "leaving hide shows the taskbar again");
        assert_eq!(bar.shell.alignment, None, "dropping alignment restores it");
        assert_eq!(bar.shell.state, ALWAYS_ON_TOP | ABS_AUTOHIDE);
        assert!(bar.restore().unwrap());
        assert!(same_state(&bar.shell, &user_taskbar()));
        assert!(!bar.restore().unwrap(), "nothing left to undo");
    }

    #[test]
    fn keep_without_alignment_touches_nothing_and_writes_no_journal() {
        let mut bar = taskbar("keep");
        bar.apply(Wanted::default()).unwrap();
        bar.apply(Wanted::default()).unwrap();
        assert_eq!(bar.shell.writes, 0);
        assert!(!bar.pending());
    }

    #[test]
    fn applying_again_is_idempotent() {
        let mut bar = taskbar("idempotent");
        bar.apply(wanted(TaskbarMode::Autohide, Some(Alignment::Left)))
            .unwrap();
        let writes = bar.shell.writes;
        bar.apply(wanted(TaskbarMode::Autohide, Some(Alignment::Left)))
            .unwrap();
        assert_eq!(bar.shell.writes, writes, "already as wanted: no writes");
    }

    #[test]
    fn a_crashed_run_is_replayed_by_another_process() {
        let mut crashed = taskbar("replay");
        crashed
            .apply(wanted(TaskbarMode::Hide, Some(Alignment::Left)))
            .unwrap();
        // The process dies; the taskbar stays as it left it. A new controller (core) replays.
        let left_behind = crashed.shell.clone();
        let mut core = Taskbar::new(left_behind, crashed.path.clone());
        assert!(core.restore().unwrap());
        assert!(same_state(&core.shell, &user_taskbar()));
        assert!(!core.pending());
    }

    #[test]
    fn the_journal_round_trips_and_a_corrupt_one_is_set_aside() {
        let journal = Journal {
            appbar: Some(Was { was: 2 }),
            hidden: true,
            alignment: Some(Was { was: None }),
        };
        let text = serde_json::to_string(&journal).unwrap();
        assert_eq!(
            text,
            r#"{"appbar":{"was":2},"hidden":true,"alignment":{"was":null}}"#
        );
        assert_eq!(serde_json::from_str::<Journal>(&text).unwrap(), journal);
        assert_eq!(
            serde_json::from_str::<Journal>("{}").unwrap(),
            Journal::default()
        );

        let bar = taskbar("corrupt");
        std::fs::write(&bar.path, "{ not json").unwrap();
        assert!(bar.load().is_err());
        assert!(!bar.pending());
        assert!(bar.path.with_extension("json.bad").exists());
    }
}
