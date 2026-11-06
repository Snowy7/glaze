//! The helper's parts: which processes it may touch, attaching, and the run loop.

pub mod attach;
pub mod run;

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;

use anyhow::{Context, Result};
use mypaper_inject::signature::microsoft_signed;
use mypaper_inject::{Target, Verdict, evaluate};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, WAIT_TIMEOUT};
use windows::Win32::Storage::Packaging::Appx::GetPackageFamilyName;
use windows::Win32::System::SystemInformation::GetWindowsDirectoryW;
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
    QueryFullProcessImageNameW, WaitForSingleObject,
};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowExW, GetShellWindow, GetWindowThreadProcessId,
};
use windows_core::{PWSTR, w};

/// The styled apps, by image path under the Windows directory, and the name their rules use.
/// Narrower than the injection allowlist on purpose: only these two, and only if that allowlist
/// also says yes.
const APPS: [(&str, &str); 2] = [
    (r"explorer.exe", "explorer"),
    (r"ImmersiveControlPanel\SystemSettings.exe", "settings"),
];

/// `%LOCALAPPDATA%\mypaper\app-styler`: the rules files and the log.
pub fn data_dir() -> Result<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA").context("LOCALAPPDATA is not set")?;
    let dir = PathBuf::from(local).join("mypaper").join("app-styler");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// The TAP DLL to attach: a copy of the one next to this exe, named by its content hash.
///
/// An app keeps a loaded TAP until it exits, and loading the same path again hands back that
/// old module. A new build therefore gets a new name and loads beside the old one, and the
/// shipped DLL is never mapped into an app, so deploying can replace it. Copies no process has
/// mapped any more are deleted here (deleting a mapped one fails, which is the check).
pub fn tap_path() -> Result<PathBuf> {
    let source = std::env::current_exe()?.with_file_name("mypaper_app_styler.dll");
    let bytes = std::fs::read(&source).with_context(|| format!("reading {}", source.display()))?;
    // FNV-1a: only needs to tell builds apart.
    let hash = bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    });
    let dir = data_dir()?;
    let name = format!("tap-{hash:016x}.dll");
    let staged = dir.join(&name);
    for entry in std::fs::read_dir(&dir)?.flatten() {
        let file = entry.file_name();
        let file = file.to_string_lossy();
        if file.starts_with("tap-") && file.ends_with(".dll") && file != name {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    // An existing copy is only used if it is still exactly this build (the folder is writable).
    if std::fs::read(&staged).ok().as_deref() != Some(bytes.as_slice()) {
        let temp = dir.join(format!("{name}.tmp"));
        std::fs::write(&temp, &bytes)?;
        std::fs::rename(&temp, &staged)
            .with_context(|| format!("replacing {}", staged.display()))?;
    }
    Ok(staged)
}

/// A process the allowlist accepted, held open. While a handle to a process is open its pid
/// can't be given to another process, so attaching by this pid reaches this process and no
/// other. The handle also waits for its exit (`SYNCHRONIZE`) and reads its exit code.
pub struct Pinned {
    /// "explorer" or "settings".
    pub app: &'static str,
    process: Process,
}

impl Pinned {
    pub fn handle(&self) -> HANDLE {
        self.process.0
    }

    /// Whether the process hasn't exited.
    pub fn running(&self) -> bool {
        // SAFETY: a live handle; a zero timeout only polls.
        let waited = unsafe { WaitForSingleObject(self.process.0, 0) };
        waited == WAIT_TIMEOUT
    }
}

/// Opens `pid` and checks *that* process: the app it is ("explorer" or "settings"), if the
/// injection allowlist (with the signature check) accepts it and it is one of the two styled
/// apps. Anything else is refused.
pub fn pin(pid: u32) -> Option<Pinned> {
    // SAFETY: a query and wait handle, closed by `Process`.
    let handle = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            false,
            pid,
        )
    }
    .ok()?;
    let process = Process(handle);
    let image = process.image()?;
    let windows = windows_dir()?;
    let app = APPS
        .iter()
        .find(|(rel, _)| {
            windows
                .join(rel)
                .to_str()
                .zip(image.to_str())
                .is_some_and(|(want, got)| want.eq_ignore_ascii_case(got))
        })
        .map(|(_, app)| *app)?;
    let family = process.package_family();
    let target = Target {
        image: &image,
        package_family: family.as_deref(),
        microsoft_signed: microsoft_signed(&image),
    };
    let pinned = Pinned { app, process };
    (evaluate(&target, &windows) == Verdict::Allow && pinned.running()).then_some(pinned)
}

pub fn check_allowed(pid: u32) -> Result<Pinned> {
    let pinned =
        pin(pid).with_context(|| format!("process {pid} is not Explorer or Settings; refused"))?;
    if pinned.app == "explorer" && is_shell(pid) != Some(false) {
        anyhow::bail!("explorer {pid} is (or may be) the shell's Explorer; never attached");
    }
    Ok(pinned)
}

/// Whether `pid` is the shell's Explorer: it owns the shell window (the desktop), `Progman` or
/// a taskbar. XAML Diagnostics keeps every window tree it has seen until the process exits, and
/// the shell's Explorer runs for the whole session, so it is never attached. `None` when there
/// is no shell window right now (Explorer restarting): nobody can tell, so don't attach either.
pub fn is_shell(pid: u32) -> Option<bool> {
    let owner = |hwnd: HWND| {
        let mut owner = 0;
        // SAFETY: a valid out pointer; a stale window just yields 0.
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut owner)) };
        owner
    };
    // SAFETY: no preconditions.
    let shell = unsafe { GetShellWindow() };
    if shell.is_invalid() {
        return None;
    }
    if owner(shell) == pid {
        return Some(true);
    }
    // Every top-level window of these classes (a desktop, a taskbar per screen).
    for class in [w!("Progman"), w!("Shell_TrayWnd"), w!("Shell_SecondaryTrayWnd")] {
        let mut after = None;
        // SAFETY: plain lookups; `after` is the previous match (or none for the first).
        while let Ok(found) = unsafe { FindWindowExW(None, after, class, None) } {
            if owner(found) == pid {
                return Some(true);
            }
            after = Some(found);
        }
    }
    Some(false)
}

struct Process(HANDLE);

impl Drop for Process {
    fn drop(&mut self) {
        // SAFETY: opened by us, closed once.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

impl Process {
    fn image(&self) -> Option<PathBuf> {
        let mut buffer = [0u16; 1024];
        let mut len = buffer.len() as u32;
        // SAFETY: the buffer and its capacity describe the same memory.
        unsafe {
            QueryFullProcessImageNameW(
                self.0,
                PROCESS_NAME_WIN32,
                PWSTR(buffer.as_mut_ptr()),
                &mut len,
            )
        }
        .ok()?;
        Some(PathBuf::from(OsString::from_wide(&buffer[..len as usize])))
    }

    fn package_family(&self) -> Option<String> {
        let mut buffer = [0u16; 128];
        let mut len = buffer.len() as u32;
        // SAFETY: the buffer and its capacity describe the same memory.
        let status =
            unsafe { GetPackageFamilyName(self.0, &mut len, Some(PWSTR(buffer.as_mut_ptr()))) };
        status
            .is_ok()
            .then(|| String::from_utf16_lossy(&buffer[..(len as usize).saturating_sub(1)]))
    }
}

fn windows_dir() -> Option<PathBuf> {
    let mut buffer = [0u16; 260];
    // SAFETY: the buffer is passed with its length.
    let len = unsafe { GetWindowsDirectoryW(Some(&mut buffer)) } as usize;
    (len > 0 && len <= buffer.len()).then(|| PathBuf::from(OsString::from_wide(&buffer[..len])))
}
