//! Working-set trimming. Loading a theme touches memory the running
//! wallpaper never reads again: GPU driver and shader compiler start-up,
//! decoder set-up, the file just decoded. [`Trimmer`] hands those pages back
//! once per load, so the process settles at what it really uses.
//!
//! This empties the *working set* (what Task Manager shows as memory), not the
//! committed memory: the pages move to Windows' standby list and come back with
//! a cheap soft fault if they are touched again. Measured on a bare wgpu
//! window rendering at 60 fps, it takes the working set from ~200 MB to ~11 MB,
//! it stays there, and the one frame after the trim takes about 1 ms longer.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use windows::Win32::System::Threading::{GetCurrentProcess, SetProcessWorkingSetSize};

/// How long after a theme loads to trim, so the frames the load triggers have
/// touched what they need first.
const SETTLE: Duration = Duration::from_secs(2);

/// Trims the working set once per loaded theme.
pub struct Trimmer {
    theme: PathBuf,
    at: Option<Instant>,
}

impl Trimmer {
    /// For the theme just loaded from `theme`; the first trim follows [`SETTLE`] later.
    pub fn new(theme: &Path) -> Self {
        Self {
            theme: theme.to_path_buf(),
            at: Some(Instant::now() + SETTLE),
        }
    }

    /// Call once per loop turn with the folder of the theme now loaded. A
    /// different folder than last time restarts the countdown; when it runs
    /// out the working set is trimmed. Returns when the loop must wake for it.
    pub fn poll(&mut self, theme: &Path) -> Option<Instant> {
        self.poll_at(Instant::now(), theme, trim)
    }

    fn poll_at(&mut self, now: Instant, theme: &Path, trim: impl FnOnce()) -> Option<Instant> {
        if self.theme != theme {
            self.theme = theme.to_path_buf();
            self.at = Some(now + SETTLE);
        }
        if self.at.is_some_and(|at| now >= at) {
            self.at = None;
            trim();
        }
        self.at
    }
}

/// This process's private (committed) bytes in MiB: what releasing memory really gives back.
pub fn private_mib() -> f64 {
    use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX};
    let mut counters = PROCESS_MEMORY_COUNTERS_EX::default();
    // SAFETY: our own process; the struct's size goes with it.
    let _ = unsafe {
        GetProcessMemoryInfo(
            GetCurrentProcess(),
            (&raw mut counters).cast(),
            size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        )
    };
    counters.PrivateUsage as f64 / 1_048_576.0
}

/// Process CPU time, including the source and readback workers, in milliseconds.
pub fn cpu_ms() -> f64 {
    use windows::Win32::Foundation::FILETIME;
    use windows::Win32::System::Threading::GetProcessTimes;
    let mut created = FILETIME::default();
    let mut exited = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    // SAFETY: our process and initialized output structures.
    let _ = unsafe {
        GetProcessTimes(
            GetCurrentProcess(),
            &mut created,
            &mut exited,
            &mut kernel,
            &mut user,
        )
    };
    let ticks = |t: FILETIME| (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime);
    (ticks(kernel) + ticks(user)) as f64 / 10_000.0
}

/// Moves this process's pages out of its working set.
fn trim() {
    // SAFETY: `usize::MAX` for both limits asks Windows to trim our own
    // working set; no memory is freed or invalidated.
    let result = unsafe { SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX) };
    match result {
        Ok(()) => log::info!("trimmed the working set"),
        Err(e) => log::warn!("could not trim the working set: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trims_once_after_the_settle_time_and_again_for_a_new_theme() {
        let start = Instant::now();
        let mut trimmer = Trimmer::new(Path::new("a"));
        let mut trims = 0;
        let mut at = |secs: u64, theme: &str, trimmer: &mut Trimmer| {
            let now = start + Duration::from_secs(secs);
            trimmer.poll_at(now, Path::new(theme), || trims += 1)
        };
        // Not yet.
        assert!(at(1, "a", &mut trimmer).is_some());
        // Due: trims, and nothing is pending any more.
        assert!(at(3, "a", &mut trimmer).is_none());
        assert!(at(60, "a", &mut trimmer).is_none());
        // A new theme restarts the countdown.
        assert!(at(61, "b", &mut trimmer).is_some());
        assert!(at(64, "b", &mut trimmer).is_none());
        assert_eq!(trims, 2);
    }
}
