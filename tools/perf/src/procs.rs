//! Processes: listing them, the tree under one, their CPU times, memory, image and command line,
//! and asking a glaze-core to quit the way the lock protocol says (`WM_QUIT` to every thread).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use windows::Wdk::System::Threading::{NtQueryInformationProcess, PROCESSINFOCLASS};
use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE, LPARAM, UNICODE_STRING, WPARAM};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
    TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
};
use windows::Win32::System::ProcessStatus::{
    GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
};
use windows::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, PROCESS_ACCESS_RIGHTS, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE, PROCESS_VM_READ,
    QueryFullProcessImageNameW, TerminateProcess,
};
use windows::Win32::UI::WindowsAndMessaging::{PostThreadMessageW, WM_QUIT};
use windows::core::PWSTR;

use crate::cpu::Times;

/// `ProcessCommandLineInformation` (Windows 8.1+).
const COMMAND_LINE_INFORMATION: PROCESSINFOCLASS = PROCESSINFOCLASS(60);

/// A running process as a snapshot saw it.
#[derive(Debug, Clone)]
pub struct Proc {
    pub pid: u32,
    pub parent: u32,
    /// The executable's file name, e.g. `msedgewebview2.exe`.
    pub name: String,
}

/// Every process right now.
pub fn list() -> Vec<Proc> {
    let mut out = Vec::new();
    // SAFETY: a snapshot we close below; the entry is sized as the API requires.
    unsafe {
        let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return out;
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut more = Process32FirstW(snapshot, &mut entry).is_ok();
        while more {
            let len = entry
                .szExeFile
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(entry.szExeFile.len());
            out.push(Proc {
                pid: entry.th32ProcessID,
                parent: entry.th32ParentProcessID,
                name: String::from_utf16_lossy(&entry.szExeFile[..len]),
            });
            more = Process32NextW(snapshot, &mut entry).is_ok();
        }
        let _ = CloseHandle(snapshot);
    }
    out
}

/// `root` and everything it started, parents before children. Windows keeps a dead parent's id
/// in its children's entries and reuses ids, so an unrelated old process can name one of ours as
/// its parent: a process only counts as a child when it was created after its parent
/// (`created`: creation time by pid; unknown times don't count).
pub fn tree(root: u32, all: &[Proc], created: impl Fn(u32) -> Option<u64>) -> Vec<Proc> {
    let mut found: Vec<Proc> = all.iter().filter(|p| p.pid == root).cloned().collect();
    let mut next = 0;
    while next < found.len() {
        let parent = found[next].pid;
        let born = created(parent);
        let children: Vec<Proc> = all
            .iter()
            .filter(|p| p.parent == parent && !found.iter().any(|f| f.pid == p.pid))
            .filter(|p| born.zip(created(p.pid)).is_some_and(|(b, c)| c >= b))
            .cloned()
            .collect();
        found.extend(children);
        next += 1;
    }
    found
}

/// The creation time of `pid` (FILETIME ticks), if it can be read.
pub fn created(pid: u32) -> Option<u64> {
    Handle::query(pid)?.times().map(|t| t.created)
}

/// Memory of one process.
#[derive(Debug, Clone, Copy, Default, PartialEq, serde::Serialize)]
pub struct Memory {
    /// Committed private memory ("private bytes").
    pub private: u64,
    /// What Task Manager shows as memory.
    pub working_set: u64,
}

/// An open process handle, closed on drop.
pub struct Handle(HANDLE);

impl Handle {
    /// Opens `pid` for queries (times, memory, image, command line).
    pub fn query(pid: u32) -> Option<Self> {
        Self::open(pid, PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ)
            .or_else(|| Self::open(pid, PROCESS_QUERY_LIMITED_INFORMATION))
    }

    fn open(pid: u32, access: PROCESS_ACCESS_RIGHTS) -> Option<Self> {
        // SAFETY: no preconditions; the handle is owned by the returned value.
        unsafe { OpenProcess(access, false, pid) }.ok().map(Self)
    }

    pub fn times(&self) -> Option<Times> {
        let mut created = FILETIME::default();
        let (mut exited, mut kernel, mut user) = Default::default();
        // SAFETY: a live handle; the out-pointers outlive the call.
        unsafe { GetProcessTimes(self.0, &mut created, &mut exited, &mut kernel, &mut user) }
            .ok()?;
        Some(Times {
            created: ticks(created),
            busy: ticks(kernel) + ticks(user),
        })
    }

    pub fn memory(&self) -> Option<Memory> {
        let mut counters = PROCESS_MEMORY_COUNTERS_EX {
            cb: size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
            ..Default::default()
        };
        // SAFETY: `counters` is the EX struct, which starts with the plain one; cb says which.
        unsafe {
            GetProcessMemoryInfo(
                self.0,
                (&raw mut counters).cast::<PROCESS_MEMORY_COUNTERS>(),
                counters.cb,
            )
        }
        .ok()?;
        Some(Memory {
            private: counters.PrivateUsage as u64,
            working_set: counters.WorkingSetSize as u64,
        })
    }

    /// The full path of the executable.
    pub fn image(&self) -> Option<PathBuf> {
        let mut buffer = [0u16; 1024];
        let mut len = buffer.len() as u32;
        // SAFETY: the buffer and its length outlive the call.
        unsafe {
            QueryFullProcessImageNameW(
                self.0,
                PROCESS_NAME_WIN32,
                PWSTR(buffer.as_mut_ptr()),
                &mut len,
            )
        }
        .ok()?;
        Some(PathBuf::from(String::from_utf16_lossy(
            &buffer[..len as usize],
        )))
    }

    pub fn command_line(&self) -> Option<String> {
        let mut needed = 0u32;
        // SAFETY: a size query with no buffer.
        let _ = unsafe {
            NtQueryInformationProcess(
                self.0,
                COMMAND_LINE_INFORMATION,
                std::ptr::null_mut(),
                0,
                &mut needed,
            )
        };
        if needed == 0 {
            return None;
        }
        // u64 elements: aligned for the UNICODE_STRING the buffer starts with.
        let mut buffer = vec![0u64; (needed as usize).div_ceil(8)];
        // SAFETY: the buffer holds `needed` bytes.
        let status = unsafe {
            NtQueryInformationProcess(
                self.0,
                COMMAND_LINE_INFORMATION,
                buffer.as_mut_ptr().cast(),
                needed,
                &mut needed,
            )
        };
        if !status.is_ok() {
            return None;
        }
        // SAFETY: on success the buffer starts with a UNICODE_STRING pointing into the buffer.
        let text = unsafe {
            let string = &*buffer.as_ptr().cast::<UNICODE_STRING>();
            if string.Buffer.is_null() {
                return None;
            }
            std::slice::from_raw_parts(string.Buffer.0, usize::from(string.Length) / 2)
        };
        Some(String::from_utf16_lossy(text))
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: we own the handle.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

/// FILETIME as a number of 100 ns ticks.
pub fn ticks(time: FILETIME) -> u64 {
    u64::from(time.dwHighDateTime) << 32 | u64::from(time.dwLowDateTime)
}

/// FILETIME ticks (since 1601) as Unix microseconds.
pub fn ticks_to_epoch_us(ticks: u64) -> u64 {
    const UNIX_EPOCH_TICKS: u64 = 116_444_736_000_000_000;
    ticks.saturating_sub(UNIX_EPOCH_TICKS) / 10
}

/// Posts `WM_QUIT` to every thread of `pid`. Returns how many threads it reached.
pub fn post_quit(pid: u32) -> usize {
    let mut posted = 0;
    // SAFETY: a snapshot we close below; the entry is sized as the API requires.
    unsafe {
        let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) else {
            return 0;
        };
        let mut entry = THREADENTRY32 {
            dwSize: size_of::<THREADENTRY32>() as u32,
            ..Default::default()
        };
        let mut more = Thread32First(snapshot, &mut entry).is_ok();
        while more {
            if entry.th32OwnerProcessID == pid
                && PostThreadMessageW(entry.th32ThreadID, WM_QUIT, WPARAM(0), LPARAM(0)).is_ok()
            {
                posted += 1;
            }
            more = Thread32Next(snapshot, &mut entry).is_ok();
        }
        let _ = CloseHandle(snapshot);
    }
    posted
}

/// Ends `pid` right away. Only for the harness's own test processes.
pub fn kill(pid: u32) -> bool {
    let Some(handle) = Handle::open(pid, PROCESS_TERMINATE) else {
        return false;
    };
    // SAFETY: a live handle opened for termination.
    unsafe { TerminateProcess(handle.0, 1) }.is_ok()
}

/// Polls `done` every 50 ms until it holds or `timeout` passes. Returns whether it held.
pub fn wait_until(timeout: Duration, mut done: impl FnMut() -> bool) -> bool {
    let until = Instant::now() + timeout;
    loop {
        if done() {
            return true;
        }
        if Instant::now() >= until {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The value of `--user-data-dir=` in a Chromium command line, if any. WebView2 puts its
/// profile in an `EBWebView` folder under the app's user data folder; this returns the app's.
pub fn user_data_dir(command_line: &str) -> Option<String> {
    let rest = &command_line[command_line.find("--user-data-dir=")? + "--user-data-dir=".len()..];
    let value = match rest.strip_prefix('"') {
        Some(quoted) => &quoted[..quoted.find('"')?],
        None => rest.split_whitespace().next()?,
    };
    let value = value.trim_end_matches(['\\', '/']);
    let value = value
        .strip_suffix("EBWebView")
        .map_or(value, |parent| parent.trim_end_matches(['\\', '/']));
    Some(value.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proc(pid: u32, parent: u32) -> Proc {
        Proc {
            pid,
            parent,
            name: format!("p{pid}"),
        }
    }

    #[test]
    fn a_tree_is_the_root_and_its_descendants_parents_first() {
        let all = [proc(1, 0), proc(2, 1), proc(3, 2), proc(4, 9), proc(5, 1)];
        let created = |pid: u32| Some(u64::from(pid));
        let pids: Vec<u32> = tree(1, &all, created).iter().map(|p| p.pid).collect();
        assert_eq!(pids, [1, 2, 5, 3]);
        assert!(tree(42, &all, created).is_empty());
    }

    #[test]
    fn processes_older_than_their_parent_id_are_not_its_children() {
        // 7 names 2 as its parent, but is older: its real parent died and 2 reused the id.
        let all = [proc(1, 0), proc(2, 1), proc(7, 2), proc(8, 7)];
        let created = |pid: u32| Some(if pid == 7 { 0 } else { u64::from(pid) * 10 });
        let pids: Vec<u32> = tree(1, &all, created).iter().map(|p| p.pid).collect();
        assert_eq!(pids, [1, 2]);
    }

    #[test]
    fn finds_the_app_user_data_folder_in_webview2_command_lines() {
        let quoted = r#""msedgewebview2.exe" --type=browser --user-data-dir="C:\Users\x\AppData\Local\glaze\webview2-bars\EBWebView" --x"#;
        assert_eq!(
            user_data_dir(quoted).as_deref(),
            Some(r"C:\Users\x\AppData\Local\glaze\webview2-bars")
        );
        let bare = r"msedgewebview2.exe --user-data-dir=C:\tmp\webview2-shell --type=renderer";
        assert_eq!(
            user_data_dir(bare).as_deref(),
            Some(r"C:\tmp\webview2-shell")
        );
        assert_eq!(user_data_dir("msedgewebview2.exe --type=gpu-process"), None);
    }

    #[test]
    fn filetime_ticks_convert_to_unix_microseconds() {
        assert_eq!(ticks_to_epoch_us(116_444_736_000_000_000 + 25), 2);
    }
}
