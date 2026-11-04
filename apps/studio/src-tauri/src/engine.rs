//! The engine is `mypaper-core.exe` (the supervisor) and the
//! `mypaper-wallpaper.exe` it runs. Studio starts and stops them and asks
//! whether they're running; everything else goes through `config.json`.

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde::Serialize;
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
    TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE, TerminateProcess, WaitForSingleObject,
};
use windows::Win32::UI::WindowsAndMessaging::{PostThreadMessageW, WM_QUIT};
use windows::core::Owned;

const CORE_EXE: &str = "mypaper-core.exe";
const WALLPAPER_EXE: &str = "mypaper-wallpaper.exe";

/// No console window, even for debug builds (which are console programs).
/// `DETACHED_PROCESS` would instead make the wallpaper child open its own console.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineStatus {
    /// `mypaper-core.exe` is running.
    pub running: bool,
    pub wallpaper_running: bool,
    /// The core exe Start would launch, if one can be found.
    pub core_path: Option<String>,
}

pub fn status() -> EngineStatus {
    let processes = processes();
    let running = |exe: &str| processes.iter().any(|p| p.name.eq_ignore_ascii_case(exe));
    EngineStatus {
        running: running(CORE_EXE),
        wallpaper_running: running(WALLPAPER_EXE),
        core_path: find_core().map(|p| p.display().to_string()),
    }
}

/// One of mypaper's programs and whether it is running.
#[derive(Debug, Serialize)]
pub struct Component {
    pub id: &'static str,
    pub exe: &'static str,
    pub running: bool,
    /// Where it was found beside Studio or in a build folder; `None` if it isn't built or installed.
    pub path: Option<String>,
}

const COMPONENTS: [(&str, &str); 4] = [
    ("core", CORE_EXE),
    ("wallpaper", WALLPAPER_EXE),
    ("widgets", "mypaper-widgets.exe"),
    ("bars", "mypaper-bars.exe"),
];

pub fn components() -> Vec<Component> {
    let processes = processes();
    let studio = std::env::current_exe().ok();
    COMPONENTS
        .iter()
        .map(|&(id, exe)| Component {
            id,
            exe,
            running: processes.iter().any(|p| p.name.eq_ignore_ascii_case(exe)),
            path: studio
                .as_deref()
                .and_then(|studio| find_exe_from(studio, exe))
                .map(|p| p.display().to_string()),
        })
        .collect()
}

/// `mypaper-core.exe` beside Studio (installed layout), else the newest build
/// in a `target\release` or `target\debug` of any parent folder (dev layout).
pub fn find_core() -> Option<PathBuf> {
    find_exe_from(&std::env::current_exe().ok()?, CORE_EXE)
}

fn find_exe_from(exe: &Path, name: &str) -> Option<PathBuf> {
    let beside = exe.with_file_name(name);
    if beside.is_file() {
        return Some(beside);
    }
    exe.ancestors()
        .skip(1)
        .flat_map(|dir| {
            ["release", "debug"].map(|profile| dir.join("target").join(profile).join(name))
        })
        .filter(|path| path.is_file())
        .max_by_key(|path| std::fs::metadata(path).and_then(|m| m.modified()).ok())
}

/// Spawns the supervisor, which reads `config.json` itself. Does nothing if it's already running.
pub fn start() -> Result<()> {
    if status().running {
        return Ok(());
    }
    let exe = find_core().with_context(|| {
        format!("{CORE_EXE} not found beside Studio or in a target\\release or target\\debug folder; build the workspace first")
    })?;
    let mut child = Command::new(&exe)
        .current_dir(exe.parent().unwrap_or(Path::new(".")))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP)
        .spawn()
        .with_context(|| format!("starting {}", exe.display()))?;

    // Core exits right away on a bad config, a missing theme or a second instance.
    let deadline = Instant::now() + Duration::from_millis(1200);
    while Instant::now() < deadline {
        if let Some(exit) = child.try_wait()? {
            bail!(
                "{CORE_EXE} exited immediately ({exit}); see %LOCALAPPDATA%\\mypaper\\mypaper.log (release builds) or run it from a console"
            );
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    // Core outlives Studio on purpose, so the handle is just dropped.
    Ok(())
}

/// Stops core and the wallpaper, nothing else. Asks nicely first (`WM_QUIT`
/// makes core kill its child and restore the desktop), then terminates what's left.
pub fn stop() -> Result<()> {
    let ours = || {
        processes()
            .into_iter()
            .filter(|p| {
                [CORE_EXE, WALLPAPER_EXE]
                    .iter()
                    .any(|exe| p.name.eq_ignore_ascii_case(exe))
            })
            .collect::<Vec<_>>()
    };
    for process in ours() {
        post_quit(process.pid);
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline && !ours().is_empty() {
        std::thread::sleep(Duration::from_millis(50));
    }
    for process in ours() {
        terminate(process.pid)?;
    }
    Ok(())
}

struct Process {
    pid: u32,
    name: String,
}

fn processes() -> Vec<Process> {
    // SAFETY: plain snapshot call; the handle is owned and closed on drop.
    let Ok(snapshot) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else {
        return Vec::new();
    };
    // SAFETY: `snapshot` is a fresh handle that nothing else closes.
    let snapshot = unsafe { Owned::new(snapshot) };
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut found = Vec::new();
    // SAFETY: `entry.dwSize` is set and the snapshot is valid for these calls.
    let mut more = unsafe { Process32FirstW(*snapshot, &mut entry) }.is_ok();
    while more {
        let len = entry
            .szExeFile
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(entry.szExeFile.len());
        found.push(Process {
            pid: entry.th32ProcessID,
            name: String::from_utf16_lossy(&entry.szExeFile[..len]),
        });
        // SAFETY: as above.
        more = unsafe { Process32NextW(*snapshot, &mut entry) }.is_ok();
    }
    found
}

/// Posts `WM_QUIT` to every thread of the process; only threads with a message queue see it.
fn post_quit(pid: u32) {
    // SAFETY: plain snapshot call; the handle is owned and closed on drop.
    let Ok(snapshot) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) }) else {
        return;
    };
    // SAFETY: `snapshot` is a fresh handle that nothing else closes.
    let snapshot = unsafe { Owned::new(snapshot) };
    let mut entry = THREADENTRY32 {
        dwSize: size_of::<THREADENTRY32>() as u32,
        ..Default::default()
    };
    // SAFETY: `entry.dwSize` is set and the snapshot is valid for these calls.
    let mut more = unsafe { Thread32First(*snapshot, &mut entry) }.is_ok();
    while more {
        if entry.th32OwnerProcessID == pid {
            // SAFETY: posting a message to a thread id has no memory-safety requirements.
            let _ =
                unsafe { PostThreadMessageW(entry.th32ThreadID, WM_QUIT, WPARAM(0), LPARAM(0)) };
        }
        // SAFETY: as above.
        more = unsafe { Thread32Next(*snapshot, &mut entry) }.is_ok();
    }
}

fn terminate(pid: u32) -> Result<()> {
    // SAFETY: opens a handle we own and close on drop.
    let process = unsafe { OpenProcess(PROCESS_TERMINATE | PROCESS_SYNCHRONIZE, false, pid) };
    // The process may have exited since the snapshot; that is the goal anyway.
    let Ok(process) = process else { return Ok(()) };
    // SAFETY: a fresh handle that nothing else closes.
    let process = unsafe { Owned::new(process) };
    // SAFETY: `process` is a valid handle with terminate and synchronize access.
    unsafe {
        TerminateProcess(*process, 1).with_context(|| format!("terminating process {pid}"))?;
        WaitForSingleObject(*process, 2000);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::tests::temp_dir;

    #[test]
    fn finds_core_beside_studio_or_in_a_parent_target() {
        let root = temp_dir("core");
        let studio = root
            .join("apps")
            .join("studio")
            .join("src-tauri")
            .join("target")
            .join("debug")
            .join("mypaper-studio.exe");
        std::fs::create_dir_all(studio.parent().unwrap()).unwrap();
        assert_eq!(find_exe_from(&studio, CORE_EXE), None);

        let built = root.join("target").join("debug").join(CORE_EXE);
        std::fs::create_dir_all(built.parent().unwrap()).unwrap();
        std::fs::write(&built, "").unwrap();
        assert_eq!(find_exe_from(&studio, CORE_EXE), Some(built));
        assert_eq!(
            find_exe_from(&studio, "mypaper-bars.exe"),
            None,
            "each program is looked up by its own name"
        );

        let beside = studio.with_file_name(CORE_EXE);
        std::fs::write(&beside, "").unwrap();
        assert_eq!(find_exe_from(&studio, CORE_EXE), Some(beside));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn process_list_includes_this_test_process() {
        let me = std::process::id();
        assert!(processes().iter().any(|p| p.pid == me));
    }
}
