//! The process that started the engine (core, which passes `--parent <its pid>`): its handle is
//! in the main loop's wait set, and once it has ended the engine exits the normal way (its
//! windows go, the desktop is restored). However core ends, crash or kill, no engine is left
//! drawing on the desktop. Like the app styler's and the index's `--parent`.

use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE, WAIT_OBJECT_0};
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_SYNCHRONIZE, WaitForSingleObject,
};

pub struct Parent(HANDLE);

impl Parent {
    /// Opens process `pid` to wait on, if it still runs and is older than us (a newer one reused
    /// the id of a parent that already ended). `None` means: the parent is gone.
    pub fn open(pid: u32) -> Option<Self> {
        // SAFETY: a wait/query-only handle, owned (and closed) by the `Parent`.
        let parent = Self(
            unsafe {
                OpenProcess(
                    PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
                    false,
                    pid,
                )
            }
            .ok()?,
        );
        // SAFETY: the pseudo-handle of this process.
        let ours = created(unsafe { GetCurrentProcess() })?;
        (created(parent.0)? <= ours && !parent.exited()).then_some(parent)
    }

    pub fn handle(&self) -> HANDLE {
        self.0
    }

    pub fn exited(&self) -> bool {
        // SAFETY: our own live handle; a zero-timeout check.
        unsafe { WaitForSingleObject(self.0, 0) == WAIT_OBJECT_0 }
    }
}

impl Drop for Parent {
    fn drop(&mut self) {
        // SAFETY: a handle we own.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

/// When the process was created (100 ns since 1601).
fn created(process: HANDLE) -> Option<u64> {
    let (mut creation, mut exit, mut kernel, mut user) = Default::default();
    // SAFETY: a live process handle and writable out parameters.
    unsafe { GetProcessTimes(process, &mut creation, &mut exit, &mut kernel, &mut user) }.ok()?;
    let FILETIME {
        dwLowDateTime: low,
        dwHighDateTime: high,
    } = creation;
    Some(u64::from(low) | (u64::from(high) << 32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_running_parent_no_newer_than_us_is_watched_and_others_are_not() {
        // Not newer than us: this very process.
        let parent = Parent::open(std::process::id()).expect("we run");
        assert!(!parent.exited());
        // Newer than us, as a process that reused a dead parent's id would be.
        let mut newer = std::process::Command::new("cmd")
            .args(["/c", "ping -n 3 127.0.0.1 >nul"])
            .spawn()
            .unwrap();
        assert!(Parent::open(newer.id()).is_none(), "newer than us");
        let id = newer.id();
        let _ = newer.wait();
        assert!(Parent::open(id).is_none(), "ended");
        assert!(Parent::open(0).is_none());
    }
}
