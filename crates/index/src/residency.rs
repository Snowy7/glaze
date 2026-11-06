//! Maintenance views retain at most four 8 MiB windows. VirtualUnlock on unlocked pages
//! removes them from this process's working set without invalidating borrowed references.
//! https://learn.microsoft.com/en-us/windows/win32/api/memoryapi/nf-memoryapi-virtualunlock

use std::collections::VecDeque;
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};

pub(crate) const WINDOW: usize = 8 * 1024 * 1024;
const RETAINED: usize = 4;

#[derive(Default)]
pub(crate) struct Windows {
    resident: Mutex<VecDeque<(usize, usize)>>,
    last: AtomicUsize,
    writable: bool,
}

fn release(address: usize, length: usize, writable: bool) {
    if writable {
        // SAFETY: the evicted range belongs to a live writable file mapping. Write dirty
        // pages back before trimming, so the RAM is reclaimable rather than merely hidden.
        let _ =
            unsafe { windows::Win32::System::Memory::FlushViewOfFile(address as *const _, length) };
    }
    // SAFETY: every entry names a page-aligned range inside a live owned file mapping.
    // ERROR_NOT_LOCKED is the documented success path for removing unlocked resident pages.
    let _ = unsafe { windows::Win32::System::Memory::VirtualUnlock(address as *const _, length) };
}

impl Windows {
    pub fn writable() -> Self {
        Self {
            writable: true,
            ..Self::default()
        }
    }
    pub fn touch(&self, base: *const u8, mapped_len: usize, offset: usize, length: usize) {
        if length == 0 {
            return;
        }
        debug_assert!(offset <= mapped_len && length <= mapped_len - offset);
        for chunk in offset / WINDOW..=(offset + length - 1) / WINDOW {
            let start = chunk * WINDOW;
            let address = base as usize + start;
            if self.last.load(Ordering::Relaxed) == address {
                continue;
            }
            let mut resident = self.resident.lock().expect("resident windows");
            if let Some(at) = resident.iter().position(|(p, _)| *p == address) {
                let window = resident.remove(at).expect("existing window");
                resident.push_back(window);
            } else {
                if resident.len() == RETAINED {
                    let (old, len) = resident.pop_front().expect("full resident windows");
                    release(old, len, self.writable);
                }
                resident.push_back((address, WINDOW.min(mapped_len - start)));
            }
            self.last.store(address, Ordering::Relaxed);
        }
    }

    pub fn release(&self) {
        let mut resident = self.resident.lock().expect("resident windows");
        for (address, length) in resident.drain(..) {
            release(address, length, self.writable);
        }
        self.last.store(0, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evicted_dirty_windows_remain_valid_and_only_four_are_retained() {
        let path =
            std::env::temp_dir().join(format!("mypaper-index-windows-{}.bin", std::process::id()));
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        file.set_len((WINDOW * 6) as u64).unwrap();
        // SAFETY: exclusively owned scratch file, never truncated while mapped.
        let mut map = unsafe { memmap2::MmapMut::map_mut(&file).unwrap() };
        let windows = Windows::writable();
        for chunk in 0..6 {
            let at = chunk * WINDOW;
            windows.touch(map.as_ptr(), map.len(), at, WINDOW);
            for page in (at..at + WINDOW).step_by(4096) {
                map[page] = chunk as u8 + 1;
            }
            assert!(windows.resident.lock().unwrap().len() <= RETAINED);
        }
        windows.release();
        for chunk in 0..6 {
            let at = chunk * WINDOW;
            windows.touch(map.as_ptr(), map.len(), at, WINDOW);
            for page in (at..at + WINDOW).step_by(4096) {
                assert_eq!(map[page], chunk as u8 + 1);
            }
        }
        windows.release();
        drop(map);
        drop(file);
        std::fs::remove_file(path).unwrap();
    }
}
