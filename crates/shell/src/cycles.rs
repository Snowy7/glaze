//! CPU cycles of the calling thread, for the wake report's per-part costs (counted only while the
//! report is on, see `wakes.rs`).

use std::sync::atomic::{AtomicBool, Ordering};

use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::Threading::GetCurrentThread;

/// Whether parts count their cycles (the wake report is on).
static ON: AtomicBool = AtomicBool::new(false);

pub fn set_counting(on: bool) {
    ON.store(on, Ordering::Relaxed);
}

pub fn counting() -> bool {
    ON.load(Ordering::Relaxed)
}

/// The calling thread's CPU cycles so far (user and kernel).
pub fn thread() -> u64 {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn QueryThreadCycleTime(thread: HANDLE, cycles: *mut u64) -> i32;
    }
    let mut cycles = 0;
    // SAFETY: the current thread's pseudo handle and a local out value.
    unsafe { QueryThreadCycleTime(GetCurrentThread(), &mut cycles) };
    cycles
}
