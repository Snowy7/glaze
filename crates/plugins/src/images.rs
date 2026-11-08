//! `ui.put-image` (docs/plugins.md › "Images"): each image is a named, read-only-to-the-shell
//! file mapping `Local\glaze-image-<helper pid>-<id>` holding a 16-byte header (`MPI1`, then
//! width, height and stride as `u32` LE) followed by BGRA8 premultiplied rows. Ids are unique in
//! the helper process. The mapping lives until the plugin drops the image or unloads; the shell
//! opens it by name when a canvas names the id and keeps its own handle while it draws it.
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};

use windows::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows::Win32::System::Memory::{CreateFileMappingW, FILE_MAP_WRITE, MapViewOfFile, PAGE_READWRITE, UnmapViewOfFile};
use windows::core::HSTRING;

pub const MAGIC: [u8; 4] = *b"MPI1";
pub const HEADER: usize = 16;
pub const MAX_SIDE: u32 = 4096;
/// Pixel bytes one plugin may hold in images at once.
pub const MAX_BYTES: usize = 16 << 20;
pub const MAX_IMAGES: usize = 64;

/// The mapping name the shell opens for image `id` of the helper `pid`.
pub fn name(pid: u32, id: u32) -> String {
    format!(r"Local\glaze-image-{pid}-{id}")
}

struct Mapping(HANDLE);
// SAFETY: a section handle is a kernel object usable from any thread.
unsafe impl Send for Mapping {}
impl Drop for Mapping {
    fn drop(&mut self) {
        // SAFETY: owned handle, closed once.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

/// One plugin's images; dropping it (unload, trap) releases them all.
#[derive(Default)]
pub struct Images {
    live: HashMap<u32, (Mapping, usize)>,
    bytes: usize,
}

impl Images {
    pub fn put(&mut self, width: u32, height: u32, pixels: &[u8]) -> Result<u32, String> {
        if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
            return Err(format!("images are 1-{MAX_SIDE} px a side"));
        }
        let len = width as usize * height as usize * 4;
        if pixels.len() != len {
            return Err(format!("{width}x{height} BGRA8 is {len} bytes, not {}", pixels.len()));
        }
        if self.live.len() >= MAX_IMAGES || self.bytes + len > MAX_BYTES {
            return Err("too many image bytes; drop some first".into());
        }
        static NEXT: AtomicU32 = AtomicU32::new(1);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let size = HEADER + len;
        let name = HSTRING::from(name(std::process::id(), id));
        // SAFETY: a pagefile-backed section of `size` bytes; the view is written within bounds and
        // unmapped before returning; the section handle is owned by `Mapping`.
        unsafe {
            let section = CreateFileMappingW(INVALID_HANDLE_VALUE, None, PAGE_READWRITE, 0, size as u32, &name).map_err(|e| e.message())?;
            let mapping = Mapping(section);
            let view = MapViewOfFile(mapping.0, FILE_MAP_WRITE, 0, 0, size);
            if view.Value.is_null() {
                return Err(windows::core::Error::from_thread().message());
            }
            let out = std::slice::from_raw_parts_mut(view.Value.cast::<u8>(), size);
            out[..4].copy_from_slice(&MAGIC);
            out[4..8].copy_from_slice(&width.to_le_bytes());
            out[8..12].copy_from_slice(&height.to_le_bytes());
            out[12..16].copy_from_slice(&(width * 4).to_le_bytes());
            out[HEADER..].copy_from_slice(pixels);
            let _ = UnmapViewOfFile(view);
            self.live.insert(id, (mapping, len));
        }
        self.bytes += len;
        Ok(id)
    }

    pub fn remove(&mut self, id: u32) {
        if let Some((_, len)) = self.live.remove(&id) {
            self.bytes -= len;
        }
    }
}
