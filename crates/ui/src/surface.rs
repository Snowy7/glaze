//! A surface: one per-pixel-alpha layered window, updated with `UpdateLayeredWindowIndirect`
//! over the dirty rectangle of each new display list.
//!
//! Input never runs app code inside a window procedure: the procedure turns messages into
//! [`Event`]s on this thread's queue ([`next_event`]), and the app's loop handles them after
//! dispatching. So app state is never borrowed twice, even when Windows sends a message from
//! inside one of our own calls (showing, focusing, a COM call to Explorer).
//!
//! Hit-testing follows alpha: pixels at alpha 0 let clicks through, anything else is ours. Views
//! fill a surface's whole shape with at least [`Rgba::HIT`](crate::Rgba::HIT).

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

use accesskit::Role;
use anyhow::{Context, Result};
use windows::Win32::Foundation::{
    COLORREF, ERROR_CLASS_ALREADY_EXISTS, GetLastError, HWND, LPARAM, LRESULT, POINT, RECT, SIZE,
    WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION,
    CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, HBITMAP, HDC,
    HGDIOBJ, ScreenToClient, SelectObject,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::WindowsAndMessaging::{
    CS_DBLCLKS, CreateWindowExW, DefWindowProcW, DestroyWindow, GWL_EXSTYLE, GWLP_USERDATA,
    GetForegroundWindow, GetWindowLongPtrW, GetWindowThreadProcessId, HICON, IDC_ARROW,
    IsWindowVisible, LoadCursorW, MA_ACTIVATE, MA_NOACTIVATE, RegisterClassExW, SW_HIDE,
    SW_SHOWNOACTIVATE, SetForegroundWindow, SetWindowLongPtrW, ShowWindow, ULW_ALPHA,
    UPDATELAYEREDWINDOWINFO, UpdateLayeredWindowIndirect, WA_INACTIVE, WINDOW_EX_STYLE,
    WM_ACTIVATE, WM_CAPTURECHANGED, WM_GETOBJECT, WM_KEYDOWN, WM_KILLFOCUS, WM_LBUTTONDBLCLK,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDBLCLK, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEACTIVATE,
    WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCDESTROY, WM_RBUTTONDBLCLK, WM_RBUTTONDOWN, WM_RBUTTONUP,
    WM_SETFOCUS, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::core::{HSTRING, PCWSTR, w};

use crate::a11y::{A11y, WM_A11Y_ACTION};
use crate::geom::{PixelRect, Point};
use crate::render::{Canvas, Ui};
use crate::scene::{DisplayList, HitId};

const WM_MOUSELEAVE: u32 = 0x02A3;
const CLASS: PCWSTR = w!("mypaper.ui");

/// What kind of window a surface is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A topmost popup: menus, flyouts. With `activate` it takes the keyboard focus when
    /// asked ([`Surface::take_focus`]); without, clicking it never activates it (submenus).
    Popup { activate: bool },
    /// A floating bar: topmost and never activated. Its app-bar reservation
    /// (`SHAppBarMessage`) belongs to the bar (N2).
    Bar,
    /// Desktop-layer surface. The name is retained for callers, but this is a top-level
    /// layered tool window anchored immediately above `parent` (Progman), never a WS_CHILD.
    /// Raised desktops do not display ULW children, and WGC cannot exclude child windows.
    DesktopChild { parent: HWND },
    /// A normal app window (Studio): resizable, activated, in the taskbar, drawn opaque.
    /// `min` is its smallest client size in design pixels. See [`crate::app_window`].
    App { min: (u32, u32) },
}

/// Identifies a surface in [`Event`]s.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SurfaceId(isize);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
    Middle,
}

/// Where the pointer is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pointer {
    /// In the surface's pixels.
    pub at: Point,
    /// In screen pixels.
    pub screen: (i32, i32),
}

/// Input for a surface, taken from [`next_event`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Event {
    /// A temporary animation requested another frame; no timer remains once it settles.
    Frame,
    Move(Pointer),
    Leave,
    /// Positive delta scrolls up; position uses the same coordinates as other pointer events.
    Wheel(Pointer, i16),
    CaptureLost,
    Press(Pointer, Button),
    Release(Pointer, Button),
    DoubleClick(Pointer, Button),
    /// A key went down (a virtual-key code).
    Key(u16),
    /// A desktop surface activated; Explorer may finish reordering its band afterwards.
    Activated,
    /// The surface lost the activation (another window was clicked or focused).
    Deactivated,
    /// Assistive technology invoked a hit item.
    Invoke(HitId),
    /// Assistive technology moved the focus to a hit item.
    Focus(HitId),
    DpiChanged(u32),
    VisibilityChanged,
    Cancelled,
    /// Windowless RichEdit changed text, selection or caret.
    TextChanged,
    /// Enter in a single-line windowless field.
    TextSubmit,
    /// An app window's client area is now this many pixels.
    Resized(u32, u32),
    /// The user asked an app window to close (its close button, Alt+F4, the taskbar).
    Close,
    /// Windows' settings or theme changed (light or dark, contrast, text size).
    SettingChanged,
}

/// Stops a temporary frame producer when its surface's animation ends or closes.
pub struct AnimationFrames(std::sync::Arc<std::sync::atomic::AtomicBool>);
impl Drop for AnimationFrames {
    fn drop(&mut self) {
        self.0.store(false, std::sync::atomic::Ordering::Relaxed);
    }
}
const WM_FRAME: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 30;

thread_local! {
    static EVENTS: RefCell<VecDeque<(SurfaceId, Event)>> = const { RefCell::new(VecDeque::new()) };
}

/// The oldest input not handled yet, for any surface of this thread. Drain it after
/// dispatching window messages.
pub fn next_event() -> Option<(SurfaceId, Event)> {
    EVENTS.with(|queue| queue.borrow_mut().pop_front())
}

/// Queues semantic input from an app-owned IPC window on this UI thread. It bypasses
/// native keyboard/text dispatch; the caller must own `hwnd` on the calling thread.
pub fn queue_event(hwnd: HWND, event: Event) {
    push(SurfaceId(hwnd.0 as isize), event);
}

pub(crate) fn push(id: SurfaceId, event: Event) {
    EVENTS.with(|queue| queue.borrow_mut().push_back((id, event)));
}

/// What the window procedure shares with the [`Surface`].
pub(crate) struct Shared {
    id: SurfaceId,
    /// App windows only.
    pub(crate) app: Option<crate::app_window::AppShared>,
    activate: bool,
    no_activate: Cell<bool>,
    normally_no_activate: bool,
    desktop: Option<HWND>,
    restacking: Cell<bool>,
    /// Where a left click activates a never-activated window ([`Surface::set_click_activates`]).
    activates: RefCell<Vec<crate::geom::Rect>>,
    field: RefCell<Option<crate::TextField>>,
    /// The window's top-left on screen, for pointer events.
    origin: Cell<(i32, i32)>,
    a11y: RefCell<Option<A11y>>,
    semantics: RefCell<Vec<(HitId, crate::Semantics)>>,
}

/// The bitmap `UpdateLayeredWindowIndirect` reads: a DIB section selected into a memory DC.
struct Dib {
    dc: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
    bits: *mut u8,
    width: u32,
}

impl Dib {
    fn new(width: u32, height: u32) -> Result<Self> {
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width as i32,
                biHeight: -(height as i32), // top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        // SAFETY: GDI objects created here and released in `Drop`; `bits` is the section's memory.
        unsafe {
            let dc = CreateCompatibleDC(None);
            let mut bits = std::ptr::null_mut();
            let bitmap = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0)
                .inspect_err(|_| {
                    let _ = DeleteDC(dc);
                })
                .context("DIB section")?;
            let previous = SelectObject(dc, bitmap.into());
            Ok(Self {
                dc,
                bitmap,
                previous,
                bits: bits.cast(),
                width,
            })
        }
    }

    /// Copies one row of `area` (premultiplied BGRA) into place.
    fn put_row(&mut self, area: PixelRect, row: u32, bytes: &[u8]) {
        let stride = self.width as usize * 4;
        let at = (area.y + row) as usize * stride + area.x as usize * 4;
        // SAFETY: `area` lies inside the bitmap, so `at..at + bytes.len()` is inside its memory.
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), self.bits.add(at), bytes.len()) };
    }
}

impl Drop for Dib {
    fn drop(&mut self) {
        // SAFETY: our own GDI objects, released once.
        unsafe {
            SelectObject(self.dc, self.previous);
            let _ = DeleteObject(self.bitmap.into());
            let _ = DeleteDC(self.dc);
        }
    }
}

/// One layered window and what it shows.
pub struct Surface {
    hwnd: HWND,
    pub(crate) shared: Rc<Shared>,
    pixels: Option<(Canvas, Dib)>,
    shown: DisplayList,
    origin: (i32, i32),
    size: (u32, u32),
    alpha: u8,
    /// The engine glass wanted under the body, and whether it is open (only while shown).
    glass: Cell<Option<crate::glass::Glass>>,
    glass_open: Cell<bool>,
    glass_style: Cell<crate::glass::GlassStyle>,
}

impl Surface {
    /// Posts display-rate frames from a high-resolution waitable timer for this animation only.
    pub fn animation_frames(&self, duration: std::time::Duration) -> AnimationFrames {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };
        let active = Arc::new(AtomicBool::new(true));
        let running = active.clone();
        let target = self.hwnd.0 as isize;
        let _ = std::thread::Builder::new()
            .name("ui-motion".into())
            .spawn(move || {
                let Ok(mut ticker) = crate::Ticker::new() else {
                    return;
                };
                let started = std::time::Instant::now();
                while running.load(Ordering::Relaxed) && started.elapsed() <= duration {
                    ticker.schedule(true);
                    // SAFETY: this worker owns its timer; the bounded wait ends with the next frame.
                    unsafe {
                        windows::Win32::System::Threading::WaitForSingleObject(
                            ticker.handle(),
                            100,
                        );
                    }
                    if !running.load(Ordering::Relaxed) {
                        break;
                    }
                    // SAFETY: posting is safe across threads and a destroyed window simply refuses.
                    let _ = unsafe {
                        windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                            Some(HWND(target as _)),
                            WM_FRAME,
                            WPARAM(0),
                            LPARAM(0),
                        )
                    };
                }
            });
        AnimationFrames(active)
    }
    /// A hidden, empty window. `name` and `role` describe it to assistive technology.
    pub fn new(kind: Kind, name: &str, role: Role) -> Result<Self> {
        Self::create(kind, name, role, false)
    }

    /// A startup surface whose UI Automation adapter is created on its first WM_GETOBJECT.
    /// Its latest hit snapshot is retained from the first present, including before activation.
    pub fn new_deferred_accessibility(kind: Kind, name: &str, role: Role) -> Result<Self> {
        Self::create(kind, name, role, true)
    }

    fn create(kind: Kind, name: &str, role: Role, deferred: bool) -> Result<Self> {
        if let Kind::App { min } = kind {
            let hwnd = crate::app_window::create(name, Some(wndproc))?;
            let app = crate::app_window::AppShared::new(min);
            return Ok(Self::adopt(hwnd, kind, name, role, deferred, Some(app)));
        }
        register_class()?;
        let (style, ex, parent) = match kind {
            Kind::Popup { activate } => {
                let ex = WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW;
                (
                    WS_POPUP,
                    if activate { ex } else { ex | WS_EX_NOACTIVATE },
                    None,
                )
            }
            // No WS_EX_NOACTIVATE: `WM_MOUSEACTIVATE` decides per click (`set_click_activates`).
            Kind::Bar => (
                WS_POPUP,
                WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
                None,
            ),
            Kind::DesktopChild { .. } => (
                WS_POPUP,
                // Receive normal click activation for keyboard editing. WM_WINDOWPOSCHANGING
                // below prevents activation from raising the surface out of its desktop band.
                WS_EX_LAYERED | WS_EX_TOOLWINDOW,
                None,
            ),
            Kind::App { .. } => unreachable!("created above"),
        };
        // SAFETY: class registered above; position and size come with the first update.
        let hwnd = unsafe {
            CreateWindowExW(
                ex,
                CLASS,
                &HSTRING::from(name),
                style,
                0,
                0,
                1,
                1,
                parent,
                None,
                None,
                None,
            )
        }
        .context("creating a surface window")?;
        Ok(Self::adopt(hwnd, kind, name, role, deferred, None))
    }

    /// Ties a new window to its shared state (the window procedure's view of the surface).
    fn adopt(
        hwnd: HWND,
        kind: Kind,
        name: &str,
        role: Role,
        deferred: bool,
        app: Option<crate::app_window::AppShared>,
    ) -> Self {
        let activate = match kind {
            Kind::Popup { activate } => activate,
            Kind::App { .. } | Kind::DesktopChild { .. } => true,
            Kind::Bar => false,
        };
        let id = SurfaceId(hwnd.0 as isize);
        let is_app = app.is_some();
        let shared = Rc::new(Shared {
            id,
            app,
            activate,
            no_activate: Cell::new(false),
            normally_no_activate: matches!(kind, Kind::Popup { activate: false }),
            desktop: match kind {
                Kind::DesktopChild { parent } => Some(parent),
                _ => None,
            },
            restacking: Cell::new(false),
            activates: RefCell::new(Vec::new()),
            field: RefCell::new(None),
            origin: Cell::new((0, 0)),
            semantics: RefCell::default(),
            a11y: RefCell::new(Some(if deferred {
                A11y::deferred(hwnd, role, name.into())
            } else {
                A11y::new(hwnd, role, name.into())
            })),
        });
        // The window holds a strong reference, released in WM_NCDESTROY.
        let raw = Rc::into_raw(shared.clone());
        // SAFETY: our own window; the pointer stays valid until WM_NCDESTROY takes it back.
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, raw as isize);
            if shared.desktop.is_some() {
                let _ = windows::Win32::UI::WindowsAndMessaging::SetPropW(
                    hwnd,
                    w!("mypaper.ui.desktop"),
                    Some(windows::Win32::Foundation::HANDLE(std::ptr::dangling_mut())),
                );
            }
        };
        if is_app {
            crate::app_window::created(hwnd);
        }
        Self {
            hwnd,
            shared,
            pixels: None,
            shown: DisplayList::default(),
            origin: (0, 0),
            size: (0, 0),
            alpha: 255,
            glass: Cell::new(None),
            glass_open: Cell::new(false),
            glass_style: Cell::new(crate::glass::GlassStyle::default()),
        }
    }

    pub fn id(&self) -> SurfaceId {
        self.shared.id
    }

    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }

    /// Attach a windowless field to receive raw keyboard, pointer and IME messages.
    /// Drop/blur it before hiding or destroying its surface.
    pub fn set_text_field(&self, field: Option<crate::TextField>) {
        if let Some(old) = self.shared.field.replace(field) {
            old.blur();
        }
    }

    /// Puts engine glass under `glass.body` (or takes it away, `None`). While the window shows,
    /// the glass follows it: opened on [`show`](Self::show), placed before every move, reshaped
    /// here, closed on [`hide`](Self::hide) and drop (crate::glass).
    pub fn set_glass(&self, glass: Option<crate::glass::Glass>) {
        let old = self.glass.replace(glass);
        if !self.glass_open.get() {
            if glass.is_some() && self.is_visible() {
                self.open_glass();
            }
            return;
        }
        let (Some(new), Some(backdrop)) = (glass, crate::glass::backdrop()) else {
            self.close_glass();
            return;
        };
        if old.is_none_or(|old| (old.body, old.radius) != (new.body, new.radius)) {
            backdrop.place(self.hwnd, new.shape(self.origin), new.radius);
        }
        if old.is_none_or(|old| old.material != new.material) {
            backdrop.material(self.hwnd, &new.material);
        }
    }

    /// Sets cached optics before showing a surface; changing them updates an open backdrop.
    pub fn set_glass_style(&self, style: crate::glass::GlassStyle) {
        if self.glass_style.replace(style) != style
            && self.glass_open.get()
            && let Some(backdrop) = crate::glass::backdrop()
        {
            backdrop.style(self.hwnd, &style);
        }
    }

    /// Whether engine glass is open under the surface.
    pub fn has_glass(&self) -> bool {
        self.glass_open.get()
    }

    fn open_glass(&self) {
        if let (Some(glass), Some(backdrop)) = (self.glass.get(), crate::glass::backdrop()) {
            let shape = glass.shape(self.origin);
            let opened = backdrop.open_styled(
                self.hwnd,
                shape,
                glass.radius,
                &glass.material,
                &self.glass_style.get(),
            );
            self.glass_open.set(opened);
        }
    }

    fn close_glass(&self) {
        if self.glass_open.replace(false)
            && let Some(backdrop) = crate::glass::backdrop()
        {
            backdrop.close(self.hwnd);
        }
    }

    /// The window's top-left on screen.
    pub fn origin(&self) -> (i32, i32) {
        self.origin
    }

    /// What is on screen.
    pub fn shown(&self) -> &DisplayList {
        &self.shown
    }

    /// Shows `list` in a `size` window at `origin` (screen pixels). Draws only what differs
    /// from the list on screen; returns whether anything was drawn.
    pub fn present(
        &mut self,
        ui: &mut Ui,
        origin: (i32, i32),
        size: (u32, u32),
        list: DisplayList,
    ) -> Result<bool> {
        self.present_damage(ui, origin, size, list, false)
    }

    /// Paint disjoint changed regions (widget meters) with one layered-window present.
    pub fn present_sparse(
        &mut self,
        ui: &mut Ui,
        origin: (i32, i32),
        size: (u32, u32),
        list: DisplayList,
    ) -> Result<bool> {
        self.present_damage(ui, origin, size, list, true)
    }

    fn present_damage(
        &mut self,
        ui: &mut Ui,
        origin: (i32, i32),
        size: (u32, u32),
        mut list: DisplayList,
        sparse: bool,
    ) -> Result<bool> {
        // A clipped or removed control must never leave an invalid UI Automation focus node.
        list.focus = list
            .focus
            .filter(|id| list.hits.iter().any(|hit| hit.id == *id));
        if self.shared.app.is_some() {
            return self.present_app(ui, size, list);
        }
        let resized = self.pixels.is_none() || size != self.size;
        if resized {
            self.pixels = None; // the old bitmaps go before the new ones come
            self.pixels = Some((Canvas::new(ui, size.0, size.1)?, Dib::new(size.0, size.1)?));
            self.size = size;
        }
        let regions = if resized {
            vec![PixelRect::full(size.0, size.1)]
        } else if sparse {
            list.damage_regions(&self.shown)
                .into_iter()
                .filter_map(|rect| rect.pixels(size.0, size.1))
                .collect::<Vec<_>>()
        } else {
            list.damage(&self.shown)
                .and_then(|rect| rect.pixels(size.0, size.1))
                .into_iter()
                .collect()
        };
        let dirty = regions
            .iter()
            .map(|r| r.to_rect())
            .reduce(|a, b| a.union(b))
            .and_then(|r| r.pixels(size.0, size.1));
        if let (Some(_), Some((canvas, dib))) = (dirty, &mut self.pixels) {
            canvas.draw_regions(&mut ui.text, &list, &regions)?;
            canvas.read_regions(&regions, |area, row, bytes| dib.put_row(area, row, bytes))?;
        }
        if dirty.is_some() || origin != self.origin {
            let source = dirty.map(|area| (self.dib_dc(), area, resized));
            self.update(origin, source)?;
        }
        self.set_hits(&list);
        self.shown = list;
        Ok(dirty.is_some())
    }

    /// Moves the window without redrawing (slides).
    pub fn move_to(&mut self, origin: (i32, i32)) -> Result<()> {
        if origin != self.origin {
            self.update(origin, None)?;
        }
        Ok(())
    }

    /// Fades the whole window without redrawing.
    pub fn set_alpha(&mut self, alpha: u8) -> Result<()> {
        if alpha != self.alpha {
            self.alpha = alpha;
            self.update(self.origin, None)?;
        }
        Ok(())
    }

    /// Shows the window without activating it.
    pub fn show(&self) {
        // SAFETY: read-only window queries and a show of our own window, without activation.
        unsafe {
            if !IsWindowVisible(self.hwnd).as_bool()
                || windows::Win32::UI::WindowsAndMessaging::IsIconic(self.hwnd).as_bool()
            {
                let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
            }
        }
        if self.shared.desktop.is_some() {
            let _ = self.keep_above_desktop();
        }
        // After the content is on screen: the request may wait on the engine's loop.
        if !self.glass_open.get() {
            self.open_glass();
        }
    }

    /// Keep a desktop surface and its directly-underlying engine glass in the bottom band.
    /// Returns true only when a z-order repair was necessary. Coordinates never change here.
    pub fn keep_above_desktop(&self) -> Result<bool> {
        use windows::Win32::UI::WindowsAndMessaging::{
            GW_HWNDNEXT, GW_HWNDPREV, GetWindow, HWND_TOP, IsWindow, SWP_NOACTIVATE, SWP_NOMOVE,
            SWP_NOSIZE, SetWindowPos,
        };
        let Some(anchor) = self.shared.desktop else {
            return Ok(false);
        };
        // SAFETY: handle queries only. A destroyed Explorer anchor is resolved by the host's
        // DesktopEvents before it creates replacement surfaces.
        unsafe {
            if !IsWindow(Some(anchor)).as_bool() {
                return Ok(false);
            }
            let next = GetWindow(self.hwnd, GW_HWNDNEXT).ok();
            let mut below = next;
            while let Some(window) = below {
                if window == anchor {
                    return Ok(false);
                }
                if !desktop_member(window) {
                    break;
                }
                below = GetWindow(window, GW_HWNDNEXT).ok();
            }
            self.shared.restacking.set(true);
            let result = (|| -> windows::core::Result<()> {
                let glass = next.filter(|&window| is_desktop_glass(window));
                let last = GetWindow(anchor, GW_HWNDPREV)
                    .ok()
                    .filter(|&w| w != self.hwnd)
                    .unwrap_or(HWND_TOP);
                let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE;
                if let Some(glass) = glass {
                    SetWindowPos(glass, Some(last), 0, 0, 0, 0, flags)?;
                    let before = GetWindow(glass, GW_HWNDPREV)
                        .ok()
                        .filter(|&w| w != self.hwnd)
                        .unwrap_or(HWND_TOP);
                    SetWindowPos(self.hwnd, Some(before), 0, 0, 0, 0, flags)?;
                } else {
                    SetWindowPos(self.hwnd, Some(last), 0, 0, 0, 0, flags)?;
                }
                Ok(())
            })();
            self.shared.restacking.set(false);
            result.context("restacking a desktop surface")?;
        }
        Ok(true)
    }

    /// Read-only z-order probe for live harnesses. Desktop windows must be in the contiguous
    /// widget/glass band over their Explorer anchor, below ordinary applications.
    pub fn desktop_order_valid(&self) -> bool {
        use windows::Win32::UI::WindowsAndMessaging::{GW_HWNDNEXT, GetWindow};
        let Some(anchor) = self.shared.desktop else {
            return true;
        };
        // SAFETY: read-only z-order walk. GetWindow ends the walk when a handle disappears.
        unsafe {
            let mut next = GetWindow(self.hwnd, GW_HWNDNEXT).ok();
            while let Some(window) = next {
                if window == anchor {
                    return true;
                }
                if !desktop_member(window) {
                    return false;
                }
                next = GetWindow(window, GW_HWNDNEXT).ok();
            }
        }
        false
    }

    /// Put edit chrome above the other desktop surfaces, while leaving every app above it.
    pub fn raise_in_desktop_band(&self) -> Result<()> {
        use windows::Win32::UI::WindowsAndMessaging::{
            GW_HWNDPREV, GetWindow, HWND_TOP, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetWindowPos,
        };
        let Some(anchor) = self.shared.desktop else {
            return Ok(());
        };
        // SAFETY: z-order queries and moves on our own window. The guard permits this explicit
        // repair through WM_WINDOWPOSCHANGING, which otherwise refuses automatic raises.
        unsafe {
            let mut top = anchor;
            while let Ok(previous) = GetWindow(top, GW_HWNDPREV) {
                if !desktop_member(previous) {
                    break;
                }
                top = previous;
            }
            if top == self.hwnd {
                return Ok(());
            }
            let before = GetWindow(top, GW_HWNDPREV).unwrap_or(HWND_TOP);
            self.shared.restacking.set(true);
            let result = SetWindowPos(
                self.hwnd,
                Some(before),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
            self.shared.restacking.set(false);
            result?;
        }
        Ok(())
    }

    pub fn hide(&self) {
        // SAFETY: our own window.
        let _ = unsafe { ShowWindow(self.hwnd, SW_HIDE) };
        self.close_glass();
    }

    pub fn is_visible(&self) -> bool {
        // SAFETY: a query on our own window.
        unsafe { IsWindowVisible(self.hwnd) }.as_bool()
    }

    /// Lets clicks through the whole window (`true`), or only through alpha 0 (`false`).
    pub fn set_click_through(&self, through: bool) {
        // SAFETY: our own window's extended style.
        unsafe {
            let ex = WINDOW_EX_STYLE(GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE) as u32);
            let ex = if through {
                ex | WS_EX_TRANSPARENT
            } else {
                ex & !WS_EX_TRANSPARENT
            };
            SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, ex.0 as isize);
        }
    }

    /// Frees the bitmaps (a hidden surface keeps none); the next [`present`](Self::present)
    /// draws everything again.
    pub fn release(&mut self) {
        if let Some(app) = &self.shared.app {
            app.release_canvas();
        }
        self.pixels = None;
        self.shown = DisplayList::default();
        self.size = (0, 0);
    }

    /// Drops input queued for this surface and not handled yet (it was hidden or reused).
    pub fn discard_events(&self) {
        let id = self.id();
        EVENTS.with(|queue| queue.borrow_mut().retain(|(of, _)| *of != id));
    }

    /// Where a left click activates this window (surface pixels), for a window that is
    /// otherwise never activated (a bar). Windows never refuses an activation by click, so a
    /// bar item that opens a flyout or brings an app forward claims the foreground this way,
    /// even from a shell window that holds the foreground lock (Search, Start, a toast).
    /// Clicks anywhere else leave the app in front alone.
    pub fn set_click_activates(&self, rects: Vec<crate::geom::Rect>) {
        *self.shared.activates.borrow_mut() = rects;
    }

    /// Temporarily keeps keyboard focus in another app (Studio's desktop-pick session).
    /// Both the style and WM_MOUSEACTIVATE policy are needed for layered desktop popups.
    pub fn set_no_activate(&self, disabled: bool) {
        if self.shared.no_activate.replace(disabled) == disabled {
            return;
        }
        // SAFETY: changes only this owned window's activation style.
        unsafe {
            let ex = WINDOW_EX_STYLE(GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE) as u32);
            let normally_disabled = self.shared.normally_no_activate;
            let ex = if disabled || normally_disabled {
                ex | WS_EX_NOACTIVATE
            } else {
                ex & !WS_EX_NOACTIVATE
            };
            SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, ex.0 as isize);
        }
    }

    /// Makes this window the foreground window, and says how that went (for logs).
    ///
    /// A click our hook swallowed never reached any window, so Windows may not count us as the
    /// receiver of the last input and refuse a plain `SetForegroundWindow`. Then this attaches
    /// our input queue to the foreground window's thread for the call (a documented way to
    /// share its foreground right). Neither helps against a shell window holding the
    /// foreground lock: there the click itself must activate us ([`Self::set_click_activates`]).
    pub fn take_focus(&self) -> &'static str {
        if self.shared.no_activate.get() {
            return "suppressed";
        }
        let hwnd = self.hwnd;
        // SAFETY: plain window and thread queries; the queue attachment is undone at once.
        unsafe {
            let _ = SetForegroundWindow(hwnd);
            if GetForegroundWindow() == hwnd {
                return "plain";
            }
            let current = GetCurrentThreadId();
            let owner = GetWindowThreadProcessId(GetForegroundWindow(), None);
            if owner == 0 || owner == current {
                return "refused";
            }
            let attached = AttachThreadInput(current, owner, true).as_bool();
            let _ = SetForegroundWindow(hwnd);
            if attached {
                let _ = AttachThreadInput(current, owner, false);
            }
            if GetForegroundWindow() == hwnd {
                "attached"
            } else {
                "refused"
            }
        }
    }

    fn dib_dc(&self) -> HDC {
        self.pixels
            .as_ref()
            .map_or(HDC::default(), |(_, dib)| dib.dc)
    }

    /// `UpdateLayeredWindowIndirect`: changed position and alpha, plus pixels from `source`
    /// (the DC, its dirty area, and whether the size changed) when there are new ones.
    fn update(&mut self, origin: (i32, i32), source: Option<(HDC, PixelRect, bool)>) -> Result<()> {
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: self.alpha,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let at = POINT {
            x: origin.0,
            y: origin.1,
        };
        let size = SIZE {
            cx: self.size.0 as i32,
            cy: self.size.1 as i32,
        };
        let zero = POINT::default();
        // The glass moves first (inside the engine's own handling of the request), so both
        // windows land in the same composition frame.
        if origin != self.origin
            && self.glass_open.get()
            && let (Some(glass), Some(backdrop)) = (self.glass.get(), crate::glass::backdrop())
        {
            backdrop.place(self.hwnd, glass.shape(origin), glass.radius);
        }
        let dirty = source.map(|(_, area, _)| RECT {
            left: area.x as i32,
            top: area.y as i32,
            right: (area.x + area.width) as i32,
            bottom: (area.y + area.height) as i32,
        });
        let info = UPDATELAYEREDWINDOWINFO {
            cbSize: size_of::<UPDATELAYEREDWINDOWINFO>() as u32,
            hdcDst: HDC::default(),
            pptDst: if origin != self.origin {
                &at
            } else {
                std::ptr::null()
            },
            // Always with new pixels, resized or not: without a size, Windows accepts the call
            // and keeps showing the old pixels (measured live: every hover and press on the
            // bars and widgets stayed invisible).
            psize: if source.is_some() {
                &size
            } else {
                std::ptr::null()
            },
            hdcSrc: source.map_or(HDC::default(), |(dc, _, _)| dc),
            pptSrc: if source.is_some() {
                &zero
            } else {
                std::ptr::null()
            },
            crKey: COLORREF(0),
            pblend: &blend,
            dwFlags: ULW_ALPHA,
            // A new size needs the whole bitmap.
            prcDirty: match (&dirty, source) {
                (Some(rect), Some((_, _, false))) => rect,
                _ => std::ptr::null(),
            },
        };
        // SAFETY: every pointer in `info` refers to a local that outlives the call.
        unsafe { UpdateLayeredWindowIndirect(self.hwnd, &info) }
            .ok()
            .context("UpdateLayeredWindowIndirect")?;
        self.origin = origin;
        self.shared.origin.set(origin);
        Ok(())
    }

    /// Records `list` as on screen (hits included) without drawing it.
    pub(crate) fn set_shown(&mut self, list: DisplayList) {
        self.set_hits(&list);
        self.shown = list;
    }

    fn set_hits(&self, list: &DisplayList) {
        if let Some(app) = &self.shared.app {
            let mut hits = app.hits.borrow_mut();
            hits.clear();
            hits.extend(list.hits.iter().map(|h| h.rect));
        }
        let events = self.shared.a11y.try_borrow_mut().ok().and_then(|mut a11y| {
            a11y.as_mut()?
                .update_full(&list.hits, list.focus, &self.shared.semantics.borrow())
        });
        if let Some(events) = events {
            events.raise();
        }
    }

    /// Optional metadata for the next presented frame; callers that omit it retain their existing tree.
    pub fn set_accessibility_semantics(&self, semantics: Vec<(HitId, crate::Semantics)>) {
        *self.shared.semantics.borrow_mut() = semantics;
    }

    /// Consume a rich UIA request when handling its corresponding `Event::Invoke`.
    pub fn take_accessibility_action(&self, hit: HitId) -> Option<crate::AccessibilityAction> {
        self.shared.a11y.borrow().as_ref()?.take_action(hit)
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        self.close_glass();
        self.discard_events();
        // SAFETY: our own window, destroyed on its creating thread.
        let _ = unsafe { DestroyWindow(self.hwnd) };
    }
}

fn desktop_member(window: HWND) -> bool {
    // SAFETY: read-only query of a live or recently destroyed window handle.
    unsafe {
        windows::Win32::UI::WindowsAndMessaging::GetPropW(window, w!("mypaper.ui.desktop")).0
            as usize
            == 1
            || is_desktop_glass(window)
    }
}
fn is_desktop_glass(window: HWND) -> bool {
    let mut class = [0u16; 64];
    // SAFETY: bounded local buffer for a read-only class-name query.
    let len = unsafe { windows::Win32::UI::WindowsAndMessaging::GetClassNameW(window, &mut class) }
        .max(0) as usize;
    class[..len]
        .iter()
        .copied()
        .eq("mypaper.wallpaper.glass".encode_utf16())
}

fn register_class() -> Result<()> {
    // SAFETY: plain Win32 registration with a 'static window procedure.
    unsafe {
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            style: CS_DBLCLKS,
            hInstance: GetModuleHandleW(None)?.into(),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            hIcon: HICON::default(),
            lpszClassName: CLASS,
            ..Default::default()
        };
        if RegisterClassExW(&class) == 0 && GetLastError() != ERROR_CLASS_ALREADY_EXISTS {
            return Err(windows::core::Error::from_thread()).context("RegisterClassExW");
        }
    }
    Ok(())
}

fn pointer(shared: &Shared, lparam: LPARAM) -> Pointer {
    let x = i32::from(lparam.0 as i16);
    let y = i32::from((lparam.0 >> 16) as i16);
    let (ox, oy) = shared.origin.get();
    Pointer {
        at: Point::new(x as f32, y as f32),
        screen: (ox + x, oy + y),
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: GWLP_USERDATA is 0 or the pointer `Surface::new` stored, valid until WM_NCDESTROY.
    let raw = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const Shared;
    if raw.is_null() {
        // SAFETY: default handling of a message we were called with.
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    }
    if msg == WM_NCDESTROY {
        // SAFETY: takes back the reference the window held; nothing reads it after this.
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            drop(Rc::from_raw(raw));
            return DefWindowProcW(hwnd, msg, wparam, lparam);
        }
    }
    // SAFETY: the window still holds its reference (see above).
    let shared = unsafe { &*raw };
    if msg == windows::Win32::UI::WindowsAndMessaging::WM_WINDOWPOSCHANGING
        && shared.desktop.is_some()
        && !shared.restacking.get()
        && lparam.0 != 0
    {
        // SAFETY: WM_WINDOWPOSCHANGING provides a live WINDOWPOS. Explicit desktop repairs
        // use the guard above; activation/ULW may move/resize but cannot raise a widget.
        unsafe {
            (*(lparam.0 as *mut windows::Win32::UI::WindowsAndMessaging::WINDOWPOS)).flags |=
                windows::Win32::UI::WindowsAndMessaging::SWP_NOZORDER;
        }
    }
    let id = shared.id;
    if let Some(app) = &shared.app
        && let Some(result) = crate::app_window::message(id, app, hwnd, msg, wparam, lparam)
    {
        return result;
    }
    if let Some(field) = shared.field.borrow().clone() {
        use windows::Win32::UI::Input::KeyboardAndMouse::{GetCapture, GetFocus};
        use windows::Win32::UI::WindowsAndMessaging::{
            WM_CHAR, WM_IME_COMPOSITION, WM_IME_ENDCOMPOSITION, WM_IME_KEYUP, WM_IME_SETCONTEXT,
            WM_IME_STARTCOMPOSITION, WM_INPUTLANGCHANGE, WM_INPUTLANGCHANGEREQUEST, WM_KEYFIRST,
            WM_KEYLAST, WM_MOUSEWHEEL, WM_TIMER,
        };
        if matches!(msg, WM_INPUTLANGCHANGE | WM_INPUTLANGCHANGEREQUEST) {
            field.send(msg, wparam, lparam);
            // SAFETY: preserve the normal HWND language/IME-context update after notifying
            // the windowless services. The original message's data is still valid.
            return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
        }
        if msg == crate::text_field::WM_TEXT_INVALIDATED {
            field.invalidated();
            push(id, Event::TextChanged);
            return LRESULT(0);
        }
        // SAFETY: read-only queries of this thread's focus and capture windows.
        let focused = unsafe { GetFocus() } == hwnd;
        // SAFETY: same read-only query, with no borrowed pointer.
        let captured = unsafe { GetCapture() } == hwnd;
        let mouse = (WM_MOUSEMOVE..=WM_MOUSEWHEEL).contains(&msg);
        let keyboard = (WM_KEYFIRST..=WM_KEYLAST).contains(&msg)
            || matches!(
                msg,
                WM_IME_STARTCOMPOSITION | WM_IME_ENDCOMPOSITION | WM_IME_COMPOSITION
            )
            || (WM_IME_SETCONTEXT..=WM_IME_KEYUP).contains(&msg);
        let mut field_point = POINT {
            x: i32::from(lparam.0 as i16),
            y: i32::from((lparam.0 >> 16) as i16),
        };
        if msg == WM_MOUSEWHEEL {
            // SAFETY: wheel messages carry screen pixels; RichEdit's hit region is client-local.
            let _ = unsafe { ScreenToClient(hwnd, &mut field_point) };
        }
        let in_field = field.contains(field_point.x, field_point.y);
        if msg == WM_LBUTTONDOWN && in_field && !focused {
            field.focus();
        }
        if msg == WM_KEYDOWN && focused && wparam.0 == 13 && !field.multiline() {
            push(id, Event::TextSubmit);
            return LRESULT(0);
        }
        // A one-line field has no use for Tab, Up, Down, Page Up or Page Down: they stay the
        // app's (moving focus, or through a list of results under a search field).
        if msg == WM_KEYDOWN
            && focused
            && !field.multiline()
            && matches!(wparam.0, 0x09 | 0x21 | 0x22 | 0x26 | 0x28)
        {
            push(id, Event::Key(wparam.0 as u16));
            return LRESULT(0);
        }
        if msg == WM_CHAR && focused && wparam.0 == 13 && !field.multiline() {
            return LRESULT(0);
        }
        // A single line has no use for Up, Down, Tab and the page keys: they move the
        // selection of whatever the field filters (the launcher's results).
        const NAVIGATION: [usize; 5] = [0x26, 0x28, 0x09, 0x21, 0x22];
        if focused && !field.multiline() {
            if msg == WM_KEYDOWN
                && (NAVIGATION.contains(&wparam.0)
                    || ([0x25, 0x27].contains(&wparam.0)
                        && field.text().is_ok_and(|text| text.is_empty())))
            {
                push(id, Event::Key(wparam.0 as u16));
                return LRESULT(0);
            }
            if msg == WM_CHAR && wparam.0 == 9 {
                return LRESULT(0);
            }
        }
        if msg == WM_TIMER && field.timer(wparam.0) {
            return field.send(msg, WPARAM(wparam.0 - 0x50000), lparam);
        }
        if (mouse && (in_field || captured))
            || (keyboard && focused && wparam.0 != 27)
            || msg == WM_SETFOCUS
            || msg == WM_KILLFOCUS
        {
            let result = field.send(msg, wparam, lparam);
            if msg == WM_KILLFOCUS {
                push(id, Event::Deactivated);
            }
            return result;
        }
    }
    let button = |button| (pointer(shared, lparam), button);
    match msg {
        WM_FRAME => push(id, Event::Frame),
        WM_MOUSEACTIVATE if shared.no_activate.get() => return LRESULT(MA_NOACTIVATE as isize),
        WM_MOUSEACTIVATE if !shared.activate => {
            // SAFETY: reads the position of the message being handled.
            let at = unsafe { windows::Win32::UI::WindowsAndMessaging::GetMessagePos() };
            let (origin_x, origin_y) = shared.origin.get();
            let at = Point::new(
                (i32::from(at as i16) - origin_x) as f32,
                (i32::from((at >> 16) as i16) - origin_y) as f32,
            );
            let activates = shared.activates.borrow().iter().any(|r| r.contains(at));
            return LRESULT(if activates {
                MA_ACTIVATE
            } else {
                MA_NOACTIVATE
            } as isize);
        }
        WM_MOUSEMOVE => {
            use windows::Win32::UI::Input::KeyboardAndMouse::{
                TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent,
            };
            // SAFETY: tracks this owned window; no callback or borrowed state escapes.
            unsafe {
                let _ = TrackMouseEvent(&mut TRACKMOUSEEVENT {
                    cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                });
            }
            push(id, Event::Move(pointer(shared, lparam)));
        }
        WM_MOUSELEAVE => push(id, Event::Leave),
        WM_CAPTURECHANGED => push(id, Event::CaptureLost),
        WM_MOUSEWHEEL => {
            let mut at = POINT {
                x: i32::from(lparam.0 as i16),
                y: i32::from((lparam.0 >> 16) as i16),
            };
            let screen = (at.x, at.y);
            // SAFETY: owned window and writable local point.
            let _ = unsafe { ScreenToClient(hwnd, &mut at) };
            push(
                id,
                Event::Wheel(
                    Pointer {
                        at: Point::new(at.x as f32, at.y as f32),
                        screen,
                    },
                    (wparam.0 >> 16) as i16,
                ),
            );
        }
        WM_LBUTTONDBLCLK | WM_RBUTTONDBLCLK | WM_MBUTTONDBLCLK => {
            let (p, b) = button(button_of(msg));
            push(id, Event::Press(p, b));
            push(id, Event::DoubleClick(p, b));
        }
        WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN => {
            let (p, b) = button(button_of(msg));
            push(id, Event::Press(p, b));
        }
        WM_LBUTTONUP | WM_RBUTTONUP | WM_MBUTTONUP => {
            let (p, b) = button(button_of(msg));
            push(id, Event::Release(p, b));
        }
        WM_KEYDOWN => push(id, Event::Key(wparam.0 as u16)),
        windows::Win32::UI::WindowsAndMessaging::WM_DPICHANGED => {
            push(id, Event::DpiChanged((wparam.0 & 0xffff) as u32))
        }
        windows::Win32::UI::WindowsAndMessaging::WM_SHOWWINDOW => {
            push(id, Event::VisibilityChanged)
        }
        windows::Win32::UI::WindowsAndMessaging::WM_WINDOWPOSCHANGED
            if shared.desktop.is_some() && !shared.restacking.get() =>
        {
            push(id, Event::VisibilityChanged)
        }
        windows::Win32::UI::WindowsAndMessaging::WM_CANCELMODE => push(id, Event::Cancelled),
        WM_ACTIVATE if (wparam.0 & 0xFFFF) as u32 == WA_INACTIVE => push(id, Event::Deactivated),
        WM_ACTIVATE if shared.desktop.is_some() => push(id, Event::Activated),
        WM_A11Y_ACTION => {
            let hit = lparam.0 as HitId;
            push(
                id,
                if wparam.0 == 0 {
                    Event::Invoke(hit)
                } else {
                    Event::Focus(hit)
                },
            );
            return LRESULT(0);
        }
        WM_SETFOCUS | WM_KILLFOCUS => {
            let events = shared
                .a11y
                .try_borrow_mut()
                .ok()
                .and_then(|mut a| a.as_mut()?.focus_changed(msg == WM_SETFOCUS));
            if let Some(events) = events {
                events.raise();
            }
        }
        WM_GETOBJECT => {
            let answer = shared
                .a11y
                .try_borrow_mut()
                .ok()
                .and_then(|mut a| a.as_mut()?.get_object(wparam, lparam));
            if let Some(answer) = answer {
                return answer.into();
            }
        }
        _ => {}
    }
    // SAFETY: default handling of a message we were called with.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

fn button_of(msg: u32) -> Button {
    match msg {
        WM_LBUTTONDOWN | WM_LBUTTONUP | WM_LBUTTONDBLCLK => Button::Left,
        WM_RBUTTONDOWN | WM_RBUTTONUP | WM_RBUTTONDBLCLK => Button::Right,
        _ => Button::Middle,
    }
}
