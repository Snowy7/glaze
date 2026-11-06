//! The helper's parts: which processes it may touch, attaching, and the run loop.

pub mod attach;
pub mod run;

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;

use anyhow::{Context, Result};
use mypaper_inject::signature::microsoft_signed;
use mypaper_inject::{Target, Verdict, evaluate};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Storage::Packaging::Appx::GetPackageFamilyName;
use windows::Win32::System::SystemInformation::GetWindowsDirectoryW;
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows_core::PWSTR;

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
    if !staged.exists() {
        let temp = dir.join(format!("{name}.tmp"));
        std::fs::write(&temp, &bytes)?;
        std::fs::rename(&temp, &staged)?;
    }
    Ok(staged)
}

/// The app `pid` is ("explorer" or "settings"), if the injection allowlist (with the signature
/// check) accepts it and it is one of the two styled apps. Anything else is refused.
pub fn app_of(pid: u32) -> Option<&'static str> {
    // SAFETY: a query-only handle, closed by `Process`.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
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
    (evaluate(&target, &windows) == Verdict::Allow).then_some(app)
}

pub fn check_allowed(pid: u32) -> Result<&'static str> {
    app_of(pid).with_context(|| format!("process {pid} is not Explorer or Settings; refused"))
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
