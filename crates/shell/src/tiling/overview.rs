//! Event-driven workspace overview. DWM owns thumbnail liveness; a stationary overview has no timer.
use super::platform;
use anyhow::Result;
use glaze_tiling::{Layout, Rect, WindowId};
use std::{
    cell::RefCell,
    collections::{HashMap, VecDeque},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::Instant,
};
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::{Dwm::*, Gdi::*},
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::*,
    },
    core::w,
};

#[derive(Clone)]
struct Slot {
    monitor: String,
    index: usize,
    rect: Rect,
    insert: bool,
}
#[derive(Clone, Copy)]
struct Tile {
    window: WindowId,
    rect: Rect,
    from: Rect,
}
#[derive(Clone)]
pub enum Event {
    Select(WindowId),
    Drop {
        window: WindowId,
        monitor: String,
        index: usize,
        insert: bool,
    },
    Close,
}
struct State {
    tiles: Vec<Tile>,
    slots: Vec<Slot>,
    events: RefCell<VecDeque<Event>>,
    drag: RefCell<Option<(WindowId, (f64, f64), (f64, f64))>>,
    thumbnails: RefCell<HashMap<WindowId, isize>>,
    started: Instant,
    cancel: Arc<AtomicBool>,
}
pub struct Overview {
    window: HWND,
    state: Box<State>,
    thumbnails: Vec<isize>,
    worker: Option<JoinHandle<()>>,
    signature: Vec<glaze_tiling::Target>,
}
impl Overview {
    pub fn open(layout: &Layout, scoped: bool) -> Result<Self> {
        let targets = layout.targets();
        let monitors: Vec<_> = layout
            .monitors
            .iter()
            .filter(|m| {
                !scoped
                    || m.workspaces
                        .iter()
                        .any(|w| targets.iter().any(|t| t.workspace == w.id))
            })
            .collect();
        anyhow::ensure!(!monitors.is_empty(), "no overview output");
        let left = monitors
            .iter()
            .map(|m| m.area.x)
            .fold(f64::INFINITY, f64::min);
        let top = monitors
            .iter()
            .map(|m| m.area.y)
            .fold(f64::INFINITY, f64::min);
        let right = monitors
            .iter()
            .map(|m| m.area.x + m.area.w)
            .fold(f64::NEG_INFINITY, f64::max);
        let bottom = monitors
            .iter()
            .map(|m| m.area.y + m.area.h)
            .fold(f64::NEG_INFINITY, f64::max);
        let mut slots = Vec::new();
        let mut tiles = Vec::new();
        for m in &monitors {
            let gap = 24.0 * m.scale;
            let row_height = ((m.area.h - gap) / (m.workspaces.len() as f64)).min(m.area.h * 0.5);
            for (index, workspace) in m.workspaces.iter().enumerate() {
                let rect = Rect {
                    x: m.area.x - left + gap,
                    y: m.area.y - top + gap + index as f64 * row_height,
                    w: (m.area.w - gap * 2.0).max(1.0),
                    h: (row_height - gap).max(32.0),
                };
                slots.push(Slot {
                    monitor: m.id.clone(),
                    index,
                    rect,
                    insert: false,
                });
                slots.push(Slot {
                    monitor: m.id.clone(),
                    index,
                    rect: Rect {
                        y: rect.y - gap,
                        h: gap,
                        ..rect
                    },
                    insert: true,
                });
                let windows: Vec<_> = targets
                    .iter()
                    .filter(|t| t.workspace == workspace.id)
                    .collect();
                // Preserve the strip's actual column widths and stack geometry while zooming.
                let low_x = windows.iter().map(|t| t.rect.x).fold(m.area.x, f64::min);
                let low_y = windows.iter().map(|t| t.rect.y).fold(m.area.y, f64::min);
                let high_x = windows
                    .iter()
                    .map(|t| t.rect.x + t.rect.w)
                    .fold(m.area.x + m.area.w, f64::max);
                let high_y = windows
                    .iter()
                    .map(|t| t.rect.y + t.rect.h)
                    .fold(m.area.y + m.area.h, f64::max);
                let zoom = ((rect.w - 16.0) / (high_x - low_x).max(1.0))
                    .min((rect.h - 16.0) / (high_y - low_y).max(1.0))
                    .max(0.01);
                for t in windows {
                    tiles.push(Tile {
                        window: t.window,
                        from: Rect {
                            x: t.rect.x - left,
                            y: t.rect.y - top,
                            ..t.rect
                        },
                        rect: Rect {
                            x: rect.x + 8.0 + (t.rect.x - low_x) * zoom,
                            y: rect.y + 8.0 + (t.rect.y - low_y) * zoom,
                            w: (t.rect.w * zoom).max(1.0),
                            h: (t.rect.h * zoom).max(1.0),
                        },
                    });
                }
            }
        }
        // SAFETY: static procedure, ordinary no-activate popup owned by our UI thread.
        let window = unsafe {
            let class = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(procedure),
                hInstance: GetModuleHandleW(None)?.into(),
                lpszClassName: w!("mypaper.tiling.overview"),
                ..Default::default()
            };
            RegisterClassExW(&class);
            CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                w!("mypaper.tiling.overview"),
                w!("Workspaces"),
                WS_POPUP,
                left as i32,
                top as i32,
                (right - left) as i32,
                (bottom - top) as i32,
                None,
                None,
                None,
                None,
            )?
        };
        let state = Box::new(State {
            tiles,
            slots,
            events: RefCell::default(),
            drag: RefCell::new(None),
            thumbnails: RefCell::new(HashMap::new()),
            started: Instant::now(),
            cancel: Arc::new(AtomicBool::new(false)),
        });
        let mut overview = Self {
            window,
            state,
            thumbnails: Vec::new(),
            worker: None,
            signature: targets,
        };
        // SAFETY: stable heap allocation outlives its window; union region owned by USER on success.
        unsafe {
            SetWindowLongPtrW(
                window,
                GWLP_USERDATA,
                &*overview.state as *const State as isize,
            );
            let region = CreateRectRgn(0, 0, 0, 0);
            for m in &monitors {
                let part = CreateRectRgn(
                    (m.area.x - left) as i32,
                    (m.area.y - top) as i32,
                    (m.area.x + m.area.w - left) as i32,
                    (m.area.y + m.area.h - top) as i32,
                );
                CombineRgn(Some(region), Some(region), Some(part), RGN_OR);
                let _ = DeleteObject(HGDIOBJ(part.0));
            }
            if SetWindowRgn(window, Some(region), true) == 0 {
                let _ = DeleteObject(HGDIOBJ(region.0));
            }
            let _ = SetWindowPos(
                window,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
            let _ = ShowWindow(window, SW_SHOWNOACTIVATE);
            let _ = UpdateWindow(window);
        }
        for tile in &overview.state.tiles {
            // SAFETY: top-level source and shell-owned top-level destination; unregistered on drop.
            if let Ok(thumbnail) =
                unsafe { DwmRegisterThumbnail(window, platform::hwnd(tile.window)) }
            {
                overview.thumbnails.push(thumbnail);
                overview
                    .state
                    .thumbnails
                    .borrow_mut()
                    .insert(tile.window, thumbnail);
                let r = tile.rect;
                let properties = DWM_THUMBNAIL_PROPERTIES {
                    dwFlags: DWM_TNP_RECTDESTINATION | DWM_TNP_VISIBLE | DWM_TNP_OPACITY,
                    rcDestination: RECT {
                        left: r.x as i32,
                        top: r.y as i32,
                        right: (r.x + r.w) as i32,
                        bottom: (r.y + r.h) as i32,
                    },
                    opacity: 255,
                    fVisible: true.into(),
                    ..Default::default()
                };
                let _ = unsafe { DwmUpdateThumbnailProperties(thumbnail, &properties) };
            }
        }
        let motion: Vec<_> = overview
            .state
            .tiles
            .iter()
            .filter_map(|t| {
                overview
                    .state
                    .thumbnails
                    .borrow()
                    .get(&t.window)
                    .map(|thumbnail| (*thumbnail, *t))
            })
            .collect();
        let cancel = overview.state.cancel.clone();
        let started = overview.state.started;
        overview.worker = std::thread::Builder::new()
            .name("tiling-overview-zoom".into())
            .stack_size(256 * 1024)
            .spawn(move || {
                loop {
                    if cancel.load(Ordering::Acquire) {
                        break;
                    }
                    let t = super::animation::progress(started.elapsed().as_secs_f64());
                    for (thumbnail, tile) in &motion {
                        let r = interpolate(tile.from, tile.rect, t);
                        let properties = DWM_THUMBNAIL_PROPERTIES {
                            dwFlags: DWM_TNP_RECTDESTINATION,
                            rcDestination: native(r),
                            ..Default::default()
                        };
                        // SAFETY: drop joins this worker before unregistering its thumbnails.
                        let _ = unsafe { DwmUpdateThumbnailProperties(*thumbnail, &properties) };
                    }
                    // SAFETY: DWM pacing only during this finite transition.
                    if unsafe { DwmFlush() }.is_err() || t > 0.999 {
                        break;
                    }
                }
            })
            .ok();
        Ok(overview)
    }
    pub fn matches(&self, layout: &Layout) -> bool {
        self.signature == layout.targets()
    }
    pub fn events(&self) -> VecDeque<Event> {
        std::mem::take(&mut *self.state.events.borrow_mut())
    }
}
impl Drop for Overview {
    fn drop(&mut self) {
        self.state.cancel.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        /* SAFETY: owned UI resources; clear userdata before releasing the backing allocation. */
        unsafe {
            SetWindowLongPtrW(self.window, GWLP_USERDATA, 0);
            for thumbnail in self.thumbnails.drain(..) {
                let _ = DwmUnregisterThumbnail(thumbnail);
            }
            let _ = DestroyWindow(self.window);
        }
    }
}
fn interpolate(a: Rect, b: Rect, t: f64) -> Rect {
    Rect {
        x: a.x + (b.x - a.x) * t,
        y: a.y + (b.y - a.y) * t,
        w: a.w + (b.w - a.w) * t,
        h: a.h + (b.h - a.h) * t,
    }
}
fn native(r: Rect) -> RECT {
    RECT {
        left: r.x.round() as i32,
        top: r.y.round() as i32,
        right: (r.x + r.w).round() as i32,
        bottom: (r.y + r.h).round() as i32,
    }
}
fn point(l: LPARAM) -> (f64, f64) {
    (f64::from(l.0 as i16), f64::from((l.0 >> 16) as i16))
}
unsafe extern "system" fn procedure(h: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    // SAFETY: set to an owned Box after creation; cleared before destruction.
    let state = unsafe { (GetWindowLongPtrW(h, GWLP_USERDATA) as *const State).as_ref() };
    if let Some(state) = state {
        match msg {
            WM_MOUSEACTIVATE => return LRESULT(MA_NOACTIVATE as isize),
            WM_LBUTTONDOWN => {
                let (x, y) = point(l);
                let progress = super::animation::progress(state.started.elapsed().as_secs_f64());
                *state.drag.borrow_mut() = state
                    .tiles
                    .iter()
                    .find(|t| interpolate(t.from, t.rect, progress).contains(x, y))
                    .map(|t| (t.window, (x, y), (x - t.rect.x, y - t.rect.y))); /* SAFETY: capture our own pointer drag until release. */
                unsafe {
                    windows::Win32::UI::Input::KeyboardAndMouse::SetCapture(h);
                }
                return LRESULT(0);
            }
            WM_MOUSEMOVE => {
                let (x, y) = point(l);
                if let Some((window, _, offset)) = *state.drag.borrow() {
                    state.cancel.store(true, Ordering::Release);
                    if let Some(tile) = state.tiles.iter().find(|t| t.window == window)
                        && let Some(thumbnail) = state.thumbnails.borrow().get(&window)
                    {
                        let r = Rect {
                            x: x - offset.0,
                            y: y - offset.1,
                            ..tile.rect
                        };
                        let properties = DWM_THUMBNAIL_PROPERTIES {
                            dwFlags: DWM_TNP_RECTDESTINATION,
                            rcDestination: native(r),
                            ..Default::default()
                        }; /* SAFETY: this thumbnail is owned by the overview until its window is destroyed. */
                        let _ = unsafe { DwmUpdateThumbnailProperties(*thumbnail, &properties) };
                    }
                }
                return LRESULT(0);
            }
            WM_LBUTTONUP => {
                let (x, y) = point(l);
                if let Some((window, start, _)) = state.drag.borrow_mut().take() {
                    if (x - start.0).hypot(y - start.1) < 6.0 {
                        state.events.borrow_mut().push_back(Event::Select(window));
                    } else if let Some(slot) = state.slots.iter().find(|s| s.rect.contains(x, y)) {
                        state.events.borrow_mut().push_back(Event::Drop {
                            window,
                            monitor: slot.monitor.clone(),
                            index: slot.index,
                            insert: slot.insert,
                        });
                    }
                } else {
                    state.events.borrow_mut().push_back(Event::Close);
                }
                // SAFETY: releases only the capture our window took.
                unsafe {
                    let _ = windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture();
                }
                crate::inbox::post(crate::inbox::WM_CONFIG + 22, 0);
                return LRESULT(0);
            }
            WM_PAINT => {
                // SAFETY: one WM_PAINT bracket with local GDI brushes.
                unsafe {
                    let mut ps = PAINTSTRUCT::default();
                    let dc = BeginPaint(h, &mut ps);
                    let background =
                        CreateSolidBrush(windows::Win32::Foundation::COLORREF(0x00251c17));
                    FillRect(dc, &ps.rcPaint, background);
                    let _ = DeleteObject(HGDIOBJ(background.0));
                    let card = CreateSolidBrush(windows::Win32::Foundation::COLORREF(0x004a3930));
                    for slot in state.slots.iter().filter(|s| !s.insert) {
                        let r = slot.rect;
                        let native = RECT {
                            left: r.x as i32,
                            top: r.y as i32,
                            right: (r.x + r.w) as i32,
                            bottom: (r.y + r.h) as i32,
                        };
                        FillRect(dc, &native, card);
                    }
                    let _ = DeleteObject(HGDIOBJ(card.0));
                    let _ = EndPaint(h, &ps);
                }
                return LRESULT(0);
            }
            _ => {}
        }
    }
    // SAFETY: default handling of the original window message.
    unsafe { DefWindowProcW(h, msg, w, l) }
}
