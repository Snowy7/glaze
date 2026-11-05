//! Latest placement per client, shared across processes. Only the wake is a window message;
//! no pointers cross the process boundary and a busy engine never holds up the shell.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering, fence};

use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, INVALID_HANDLE_VALUE, LPARAM, WPARAM};
use windows::Win32::System::Memory::{
    CreateFileMappingW, FILE_MAP_ALL_ACCESS, MEMORY_MAPPED_VIEW_ADDRESS, MapViewOfFile,
    OpenFileMappingW, PAGE_READWRITE, UnmapViewOfFile,
};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};
use windows::core::HSTRING;

use super::{Request, Wire};

pub const WM_PLACEMENT: u32 = WM_APP + 0x4F2;
const WORDS: usize = size_of::<Wire>() / size_of::<u32>();

#[repr(C)]
struct Shared {
    sequence: AtomicU64,
    wake: AtomicBool,
    words: [AtomicU32; WORDS],
}

pub struct Channel {
    mapping: HANDLE,
    view: MEMORY_MAPPED_VIEW_ADDRESS,
    sequence: u64,
}

impl Channel {
    /// The shell creates this before Open; the engine opens the same mapping at intake.
    pub fn open(client: HWND, create: bool) -> windows::core::Result<Self> {
        let name = HSTRING::from(format!(
            "Local\\mypaper.overlay.placement.v1.{}",
            client.0 as usize
        ));
        // SAFETY: a page-file mapping starts zeroed, has allocation alignment, and remains
        // mapped for this owner's lifetime. Both processes compile this same fixed ABI.
        unsafe {
            let mapping = if create {
                CreateFileMappingW(
                    INVALID_HANDLE_VALUE,
                    None,
                    PAGE_READWRITE,
                    0,
                    size_of::<Shared>() as u32,
                    &name,
                )?
            } else {
                OpenFileMappingW(FILE_MAP_ALL_ACCESS.0, false, &name)?
            };
            let view = MapViewOfFile(mapping, FILE_MAP_ALL_ACCESS, 0, 0, size_of::<Shared>());
            if view.Value.is_null() {
                let error = windows::core::Error::from_thread();
                let _ = CloseHandle(mapping);
                return Err(error);
            }
            Ok(Self {
                mapping,
                view,
                sequence: 0,
            })
        }
    }

    fn shared(&self) -> &Shared {
        // SAFETY: the aligned, live mapping contains only valid zero-initialized atomics.
        unsafe { &*self.view.Value.cast::<Shared>() }
    }

    /// One writer per client, on the shell thread. Atomic payloads avoid Rust data races.
    pub fn write(&mut self, request: &Request) {
        let words: [u32; WORDS] = bytemuck::cast(Wire::from(request));
        let shared = self.shared();
        let base = shared.sequence.load(Ordering::Relaxed) | 1;
        shared.sequence.store(base, Ordering::SeqCst);
        for (slot, value) in shared.words.iter().zip(words) {
            slot.store(value, Ordering::Relaxed);
        }
        shared
            .sequence
            .store(base.wrapping_add(1), Ordering::Release);
    }

    /// Bursts overwrite the snapshot and leave at most one wake outstanding.
    pub fn publish(&mut self, engine: HWND, request: &Request) -> bool {
        self.write(request);
        if self.shared().wake.swap(true, Ordering::AcqRel) {
            return true;
        }
        // SAFETY: the message contains only the client HWND, never a process-local pointer.
        let sent = unsafe {
            PostMessageW(
                Some(engine),
                WM_PLACEMENT,
                WPARAM(request.client().0 as usize),
                LPARAM(0),
            )
        }
        .is_ok();
        if !sent {
            self.shared().wake.store(false, Ordering::Release);
        }
        sent
    }

    /// Never waits for a writer. Clear the wake before reading so a racing write wakes again.
    pub fn take(&mut self) -> Option<Request> {
        self.shared().wake.store(false, Ordering::Release);
        for _ in 0..8 {
            let shared = self.shared();
            let sequence = shared.sequence.load(Ordering::Acquire);
            if sequence & 1 != 0 {
                continue;
            }
            if sequence == self.sequence {
                return None;
            }
            let words: [u32; WORDS] =
                std::array::from_fn(|i| shared.words[i].load(Ordering::Relaxed));
            fence(Ordering::Acquire);
            if sequence != shared.sequence.load(Ordering::Acquire) {
                continue;
            }
            let request = bytemuck::cast::<_, Wire>(words).decode();
            self.sequence = sequence;
            return request;
        }
        None
    }
}

impl Drop for Channel {
    fn drop(&mut self) {
        // SAFETY: this owner holds both resources and no references outlive it.
        unsafe {
            let _ = UnmapViewOfFile(self.view);
            let _ = CloseHandle(self.mapping);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Layer, Material};
    use super::*;
    use mypaper_desktop::Rect;

    #[test]
    fn a_burst_reads_only_the_latest_complete_placement() {
        let client = HWND((0x7000_0000usize + std::process::id() as usize) as *mut _);
        let mut writer = Channel::open(client, true).unwrap();
        let mut reader = Channel::open(client, false).unwrap();
        let mut latest = Request::Close { client };
        for x in 0..100 {
            latest = Request::Open {
                client,
                layer: Layer::Popup,
                rect: Rect {
                    x,
                    y: -20,
                    width: 300 + x,
                    height: 100,
                },
                radius: x as f32,
                material: Material {
                    blur: x as f32,
                    ..Material::CLEAR
                },
            };
            writer.write(&latest);
        }
        assert_eq!(reader.take(), Some(latest));
        assert_eq!(reader.take(), None);
        writer.write(&Request::Close { client });
        assert_eq!(reader.take(), Some(Request::Close { client }));
    }
}
