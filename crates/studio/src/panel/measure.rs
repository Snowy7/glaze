//! Read-only measurements of this Studio process, never another window or desktop component.
#[derive(Debug, Clone, Copy)]
pub struct Memory {
    pub working_mb: f64,
    pub private_mb: f64,
}

pub fn memory(fixture: bool) -> Option<Memory> {
    if fixture {
        return Some(Memory {
            working_mb: 32.0,
            private_mb: 24.0,
        });
    }
    use windows::Win32::System::{
        ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX},
        Threading::GetCurrentProcess,
    };
    let mut counters = PROCESS_MEMORY_COUNTERS_EX {
        cb: size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        ..Default::default()
    };
    // SAFETY: the current-process pseudo handle needs no close; the writable buffer has the declared size.
    unsafe { GetProcessMemoryInfo(GetCurrentProcess(), (&raw mut counters).cast(), counters.cb) }
        .ok()?;
    Some(Memory {
        working_mb: counters.WorkingSetSize as f64 / 1048576.0,
        private_mb: counters.PrivateUsage as f64 / 1048576.0,
    })
}
