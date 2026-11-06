//! Read-only measurements of this Studio process, never another window or desktop component.
#[derive(Debug, Clone, Copy)]
pub struct Memory {
    pub working_mb: f64,
    pub private_mb: f64,
}

/// Temporary picture factories; bitmap caches never retain widget models.
#[derive(Default)]
pub struct Raster(pub std::cell::RefCell<Option<mypaper_ui::Ui>>);
impl std::fmt::Debug for Raster {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Raster")
    }
}

pub fn raster_ui(app: &crate::app::App) -> Option<std::cell::RefMut<'_, mypaper_ui::Ui>> {
    let mut raster = app.panel.as_ref()?.raster.0.borrow_mut();
    if raster.is_none() {
        let mut ui = mypaper_ui::Ui::isolated().ok()?;
        ui.text.set_capacity(24);
        *raster = Some(ui);
    }
    Some(std::cell::RefMut::map(raster, |raster| {
        raster.as_mut().unwrap()
    }))
}

/// The bitmap is complete: retain pixels and release temporary native font resources.
pub fn finish_raster(app: &crate::app::App) {
    app.panel.as_ref().unwrap().raster.0.borrow_mut().take();
    optimize_heap();
}

/// Lay out at widget size, then paint those operations once at the displayed pixel size.
/// This preserves text fitting without allocating a native intermediate bitmap.
pub fn scale_picture(ops: &mut [mypaper_ui::Op], scale: f32) {
    use mypaper_ui::{Op, Point, Rect};
    let rect = |r: &mut Rect| {
        r.left *= scale;
        r.top *= scale;
        r.right *= scale;
        r.bottom *= scale;
    };
    let point = |p: &mut Point| {
        p.x *= scale;
        p.y *= scale;
    };
    for op in ops {
        match op {
            Op::Image { rect: r, .. } | Op::Clip { rect: r } | Op::TextField { rect: r, .. } => {
                rect(r)
            }
            Op::RoundedImage {
                rect: r, radius, ..
            }
            | Op::Fill {
                rect: r, radius, ..
            } => {
                rect(r);
                *radius *= scale;
            }
            Op::Stroke {
                rect: r,
                radius,
                width,
                ..
            } => {
                rect(r);
                *radius *= scale;
                *width *= scale;
            }
            Op::Icon {
                rect: r, weight, ..
            } => {
                rect(r);
                *weight *= scale;
            }
            Op::Text { rect: r, style, .. } | Op::Emphasis { rect: r, style, .. } => {
                rect(r);
                style.size *= scale;
                style.tracking *= scale;
            }
            Op::Line {
                from, to, width, ..
            } => {
                point(from);
                point(to);
                *width *= scale;
            }
            Op::Arc {
                center,
                radius,
                width,
                ..
            } => {
                point(center);
                *radius *= scale;
                *width *= scale;
            }
            Op::Shadow {
                rect: r,
                radius,
                spread,
                drop,
                ..
            } => {
                rect(r);
                *radius *= scale;
                *spread *= scale;
                *drop *= scale;
            }
            Op::Clipped { rect: r, ops } => {
                rect(r);
                scale_picture(std::sync::Arc::make_mut(ops), scale);
            }
            Op::Unclip => {}
        }
    }
}

/// Temporary software canvases can finish after a page's async wallpaper decode.
/// Reclaim their released allocations after the resulting frame has been presented too.
pub fn raster_finished(app: &crate::app::App) {
    if !app.fixture {
        optimize_heap();
        app.panel.as_ref().unwrap().reclaim_heap.set(true);
    }
}

/// A page owns its decoded thumbnails, preview artwork and shaped text.
/// Keep the image worker connected while dropping its ready pixels, so the next page stays async.
pub fn leave_page(app: &crate::app::App, text: &mut mypaper_ui::Text) {
    // Snapshot rasters live only while their History rows are on screen.
    app.panel
        .as_ref()
        .unwrap()
        .history
        .borrow()
        .pictures
        .borrow_mut()
        .clear();
    app.panel.as_ref().unwrap().raster.0.borrow_mut().take();
    if app.fixture {
        return;
    }
    app.panel.as_ref().unwrap().reclaim_heap.set(true);
    let mut images = app.images.borrow_mut();
    images.clear_page();
    drop(images);
    app.stage.borrow_mut().clear_pictures();
    text.clear_cache();
    // Native thumbnail/icon providers are page-owned too. COM unloads only libraries
    // whose objects and calls have all been released; active UI factories stay live.
    unsafe { windows::Win32::System::Com::CoFreeUnusedLibrariesEx(0, None); }
    optimize_heap();
}

/// Release the previous canvas and retained list before constructing the next page's pictures.
pub fn prepare_frame(app: &crate::app::App, surface: &mut mypaper_ui::Surface) {
    let Some(state) = &app.panel else { return };
    let page =
        if matches!(state.page, super::Page::Top | super::Page::Dock) && app.nav.module.is_some() {
            super::Page::Module
        } else {
            state.page
        };
    if state.specimen_page.get() != Some(page) {
        surface.release();
    }
}

/// Ask the allocator to decommit freed pages, once per navigation rather than on hover.
pub fn reclaim(app: &crate::app::App) {
    if !app
        .panel
        .as_ref()
        .is_some_and(|state| state.reclaim_heap.replace(false))
    {
        return;
    }
    optimize_heap();
    if let Some(window) = app.window {
        // Resize/paint and retired accessibility providers can release buffers after present.
        // Coalesce one idle pass per navigation; hover never schedules this timer.
        unsafe {
            windows::Win32::UI::WindowsAndMessaging::SetTimer(
                Some(window),
                RECLAIM_TIMER,
                250,
                Some(reclaim_idle),
            );
        }
    }
}

const RECLAIM_TIMER: usize = 0x6A34;

unsafe extern "system" fn reclaim_idle(
    window: windows::Win32::Foundation::HWND,
    _: u32,
    id: usize,
    _: u32,
) {
    // SAFETY: the callback belongs to this panel's UI thread and is one-shot.
    let _ = unsafe { windows::Win32::UI::WindowsAndMessaging::KillTimer(Some(window), id) };
    optimize_heap();
}

fn optimize_heap() {
    #[repr(C)]
    struct HeapOptimizeResourcesInformation {
        version: u32,
        flags: u32,
    }
    let information = HeapOptimizeResourcesInformation {
        version: 1,
        flags: 0,
    };
    // SAFETY: a versioned input buffer for the documented current-process heap operation.
    // Null selects this process's heaps; this releases unused committed pages, not live data.
    let _ = unsafe {
        windows::Win32::System::Memory::HeapSetInformation(
            None,
            windows::Win32::System::Memory::HeapOptimizeResources,
            Some((&raw const information).cast()),
            size_of::<HeapOptimizeResourcesInformation>(),
        )
    };
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

/// The selected display's current refresh rate, read from Windows without changing its mode.
pub fn refresh_rate(app: &crate::app::App) -> Option<u32> {
    if app.fixture {
        return Some(144);
    }
    use windows::Win32::Graphics::Gdi::{DEVMODEW, ENUM_CURRENT_SETTINGS, EnumDisplaySettingsW};
    let monitor = app.display()?;
    let device = windows::core::HSTRING::from(&monitor.device);
    let mut mode = DEVMODEW {
        dmSize: size_of::<DEVMODEW>() as u16,
        ..Default::default()
    };
    // SAFETY: a read-only query with a correctly sized writable DEVMODEW.
    unsafe { EnumDisplaySettingsW(&device, ENUM_CURRENT_SETTINGS, &mut mode) }
        .as_bool()
        .then_some(mode.dmDisplayFrequency)
        .filter(|hz| (1..=360).contains(hz))
}
