//! Engine-drawn glass for the previews (D3). Studio has no GPU device; it asks the engine to draw
//! the wallpaper with the preview's glass plates into shared memory, once per change, and draws
//! the shell's own views above the answer. While the engine is off (or the wallpaper draws
//! itself, a web page or an app), the stage shows the still with a flat tint instead.
//!
//! The protocol is `crates/wallpaper/src/preview.rs`'s (keep the two in step): Studio owns the
//! section `Local\mypaper.preview` and the auto-reset event `Local\mypaper.preview.done`; the
//! engine owns `Local\mypaper.preview.request`. Everything here is released when Studio closes.

use std::sync::Arc;

use bytemuck_free::{Header, Plate};
use mypaper_ui::{Image, Rect, Rgba};
use windows::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows::Win32::System::Memory::{
    CreateFileMappingW, FILE_MAP_ALL_ACCESS, MEMORY_MAPPED_VIEW_ADDRESS, MapViewOfFile,
    PAGE_READWRITE, UnmapViewOfFile,
};
use windows::Win32::System::Threading::{CreateEventW, EVENT_MODIFY_STATE, OpenEventW, SetEvent};
use windows::core::w;

const MAGIC: u32 = 0x5650_504d;
const VERSION: u32 = 1;
const MAX_PLATES: usize = 32;
const MAX_SIZE: (u32, u32) = (2048, 1280);
const PIXELS: usize = 4096;

/// The engine's layout, without a bytemuck dependency: plain `repr(C)` with the same sizes
/// (asserted below and in the engine's test).
mod bytemuck_free {
    #[repr(C)]
    #[derive(Debug, Clone, Copy, Default)]
    pub struct Plate {
        pub rect: [f32; 4],
        pub radius: f32,
        pub clarity: f32,
        pub blur: f32,
        pub refraction: f32,
        pub highlight: f32,
        pub _pad: [f32; 3],
        pub tint: [f32; 4],
    }

    #[repr(C)]
    pub struct Header {
        pub magic: u32,
        pub version: u32,
        pub request: u64,
        pub done: u64,
        pub width: u32,
        pub height: u32,
        pub screen: u32,
        pub status: u32,
        pub count: u32,
        pub _pad: u32,
        pub plates: [Plate; super::MAX_PLATES],
    }
}

const _: () = assert!(size_of::<Plate>() == 64);
const _: () = assert!(size_of::<Header>() == 48 + 64 * MAX_PLATES);

/// One glass shape for the preview, in preview pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shape {
    pub rect: Rect,
    pub radius: f32,
    pub clarity: f32,
    pub blur: f32,
    pub refraction: f32,
    pub highlight: f32,
    pub tint: Rgba,
}

/// What a preview of `size` with `shapes` looks like, keyed so a frame asks only once.
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub size: (u32, u32),
    pub shapes: Vec<Shape>,
}

struct Shared {
    mapping: HANDLE,
    view: *mut u8,
    done: HANDLE,
}

#[derive(Default)]
pub struct Glass {
    shared: Option<Shared>,
    /// What was last asked, and whether its answer is in.
    asked: Option<(Request, u64)>,
    /// The last answer: the wallpaper with glass, for that request.
    pub answer: Option<(Request, Arc<Image>)>,
    /// The engine said it has no frame to draw from (or isn't running).
    pub unavailable: bool,
    sequence: u64,
    /// Asked for while another request was out.
    pending: Option<Request>,
}

impl Glass {
    /// The event to wait on while a request is out.
    pub fn handle(&self) -> Option<HANDLE> {
        self.asked.as_ref()?;
        self.shared.as_ref().map(|s| s.done)
    }

    /// The image for `request` if it's in; otherwise asks for it (once) and returns the last
    /// answer, so a resize keeps the previous glass visible until the new one lands.
    pub fn image(&mut self, request: &Request) -> Option<Arc<Image>> {
        if let Some((r, image)) = &self.answer
            && r == request
        {
            return Some(image.clone());
        }
        let in_flight = self.asked.as_ref().is_some_and(|(r, _)| r == request);
        if !in_flight && self.asked.is_none() {
            self.ask(request);
        } else if !in_flight {
            // A request is out: ask again when it's answered (`arrived` picks up the newest).
            self.pending = Some(request.clone());
        }
        self.answer
            .as_ref()
            .map(|(_, i)| i.clone())
    }

    fn ask(&mut self, request: &Request) {
        let (w, h) = request.size;
        if w == 0 || h == 0 || w > MAX_SIZE.0 || h > MAX_SIZE.1 {
            return;
        }
        // SAFETY: the engine's event, opened and closed here.
        let event = unsafe { OpenEventW(EVENT_MODIFY_STATE, false, w!("Local\\mypaper.preview.request")) };
        let Ok(event) = event else {
            self.unavailable = true;
            return;
        };
        self.sequence += 1;
        let sequence = self.sequence;
        let Some(shared) = self.shared() else {
            // SAFETY: closing the handle opened above.
            let _ = unsafe { CloseHandle(event) };
            return;
        };
        // SAFETY: our mapping is sized for MAX_SIZE; the engine reads it only after the event.
        unsafe {
            let header = &mut *(shared.view as *mut Header);
            header.magic = MAGIC;
            header.version = VERSION;
            header.width = w;
            header.height = h;
            header.screen = 0;
            header.status = 0;
            let count = request.shapes.len().min(MAX_PLATES);
            header.count = count as u32;
            for (slot, s) in header.plates.iter_mut().zip(&request.shapes) {
                *slot = Plate {
                    rect: [s.rect.left, s.rect.top, s.rect.width(), s.rect.height()],
                    radius: s.radius,
                    clarity: s.clarity,
                    blur: s.blur,
                    refraction: s.refraction,
                    highlight: s.highlight,
                    _pad: [0.0; 3],
                    tint: [s.tint.r, s.tint.g, s.tint.b, s.tint.a],
                };
            }
            std::sync::atomic::fence(std::sync::atomic::Ordering::Release);
            header.request = sequence;
            let _ = SetEvent(event);
            let _ = CloseHandle(event);
        }
        self.asked = Some((request.clone(), sequence));
        self.unavailable = false;
    }

    /// The done event fired: read the answer. Returns whether the stage should redraw.
    pub fn arrived(&mut self) -> bool {
        let (Some(shared), Some((request, sequence))) = (&self.shared, self.asked.take()) else {
            return false;
        };
        // SAFETY: the engine finished writing before setting the event (it writes `done` last).
        let image = unsafe {
            let header = &*(shared.view as *const Header);
            std::sync::atomic::fence(std::sync::atomic::Ordering::Acquire);
            if header.done != sequence {
                None
            } else if header.status != 0 {
                self.unavailable = true;
                None
            } else {
                let (w, h) = (header.width, header.height);
                let bytes = std::slice::from_raw_parts(shared.view.add(PIXELS), w as usize * h as usize * 4);
                Some(Image {
                    width: w,
                    height: h,
                    pixels: bytes.to_vec(),
                })
            }
        };
        if let Some(image) = image {
            self.answer = Some((request, Arc::new(image)));
        }
        if let Some(next) = self.pending.take() {
            self.ask(&next);
        }
        true
    }

    fn shared(&mut self) -> Option<&Shared> {
        if self.shared.is_none() {
            let size = PIXELS + MAX_SIZE.0 as usize * MAX_SIZE.1 as usize * 4;
            // SAFETY: a pagefile-backed section and auto-reset event, owned until drop.
            unsafe {
                let mapping = CreateFileMappingW(
                    INVALID_HANDLE_VALUE,
                    None,
                    PAGE_READWRITE,
                    (size >> 32) as u32,
                    size as u32,
                    w!("Local\\mypaper.preview"),
                )
                .ok()?;
                let view = MapViewOfFile(mapping, FILE_MAP_ALL_ACCESS, 0, 0, 0);
                if view.Value.is_null() {
                    let _ = CloseHandle(mapping);
                    return None;
                }
                let Ok(done) = CreateEventW(None, false, false, w!("Local\\mypaper.preview.done")) else {
                    let _ = UnmapViewOfFile(view);
                    let _ = CloseHandle(mapping);
                    return None;
                };
                self.shared = Some(Shared {
                    mapping,
                    view: view.Value.cast(),
                    done,
                });
            }
        }
        self.shared.as_ref()
    }
}

impl Drop for Glass {
    fn drop(&mut self) {
        if let Some(s) = self.shared.take() {
            // SAFETY: our own view and handles, released once.
            unsafe {
                let _ = UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS { Value: s.view.cast() });
                let _ = CloseHandle(s.mapping);
                let _ = CloseHandle(s.done);
            }
        }
    }
}
