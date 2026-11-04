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
    HGDIOBJ, SelectObject,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GWL_EXSTYLE, GWLP_USERDATA,
    GetForegroundWindow, GetWindowLongPtrW, GetWindowThreadProcessId, HICON, IDC_ARROW,
    IsWindowVisible, LoadCursorW, MA_NOACTIVATE, RegisterClassExW, SW_HIDE, SW_SHOWNOACTIVATE,
    SetForegroundWindow, SetWindowLongPtrW, ShowWindow, ULW_ALPHA, UPDATELAYEREDWINDOWINFO,
    UpdateLayeredWindowIndirect, WA_INACTIVE, WINDOW_EX_STYLE, WM_ACTIVATE,
    WM_GETOBJECT, WM_KEYDOWN, WM_KILLFOCUS, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN,
    WM_MBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_NCDESTROY, WM_RBUTTONDOWN, WM_RBUTTONUP,
    WM_SETFOCUS, WNDCLASSEXW, WS_CHILD, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::core::{HSTRING, PCWSTR, w};

use crate::a11y::{A11y, WM_A11Y_ACTION};
use crate::geom::{PixelRect, Point};
use crate::render::{Canvas, Ui};
use crate::scene::{DisplayList, HitId};

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
    /// A layered child of the desktop host, for widgets. Layered child windows need the
    /// Windows 8+ compatibility manifest, which the shell gains with the widgets (N4).
    DesktopChild { parent: HWND },
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
    Move(Pointer),
    Press(Pointer, Button),
    Release(Pointer, Button),
    /// A key went down (a virtual-key code).
    Key(u16),
    /// The surface lost the activation (another window was clicked or focused).
    Deactivated,
    /// Assistive technology invoked a hit item.
    Invoke(HitId),
    /// Assistive technology moved the focus to a hit item.
    Focus(HitId),
}

thread_local! {
    static EVENTS: RefCell<VecDeque<(SurfaceId, Event)>> = const { RefCell::new(VecDeque::new()) };
}

/// The oldest input not handled yet, for any surface of this thread. Drain it after
/// dispatching window messages.
pub fn next_event() -> Option<(SurfaceId, Event)> {
    EVENTS.with(|queue| queue.borrow_mut().pop_front())
}

fn push(id: SurfaceId, event: Event) {
    EVENTS.with(|queue| queue.borrow_mut().push_back((id, event)));
}

/// What the window procedure shares with the [`Surface`].
struct Shared {
    id: SurfaceId,
    activate: bool,
    /// The window's top-left on screen, for pointer events.
    origin: Cell<(i32, i32)>,
    a11y: RefCell<Option<A11y>>,
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
    shared: Rc<Shared>,
    pixels: Option<(Canvas, Dib)>,
    shown: DisplayList,
    origin: (i32, i32),
    size: (u32, u32),
    alpha: u8,
}

impl Surface {
    /// A hidden, empty window. `name` and `role` describe it to assistive technology.
    pub fn new(kind: Kind, name: &str, role: Role) -> Result<Self> {
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
            Kind::Bar => (
                WS_POPUP,
                WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                None,
            ),
            Kind::DesktopChild { parent } => (
                WS_CHILD,
                WS_EX_LAYERED | WS_EX_NOACTIVATE,
                Some(parent),
            ),
        };
        let activate = !ex.contains(WS_EX_NOACTIVATE);
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
        let id = SurfaceId(hwnd.0 as isize);
        let shared = Rc::new(Shared {
            id,
            activate,
            origin: Cell::new((0, 0)),
            a11y: RefCell::new(Some(A11y::new(hwnd, role, name.into()))),
        });
        // The window holds a strong reference, released in WM_NCDESTROY.
        let raw = Rc::into_raw(shared.clone());
        // SAFETY: our own window; the pointer stays valid until WM_NCDESTROY takes it back.
        unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, raw as isize) };
        Ok(Self {
            hwnd,
            shared,
            pixels: None,
            shown: DisplayList::default(),
            origin: (0, 0),
            size: (0, 0),
            alpha: 255,
        })
    }

    pub fn id(&self) -> SurfaceId {
        self.shared.id
    }

    pub fn hwnd(&self) -> HWND {
        self.hwnd
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
        let resized = self.pixels.is_none() || size != self.size;
        if resized {
            self.pixels = None; // the old bitmaps go before the new ones come
            self.pixels = Some((Canvas::new(ui, size.0, size.1)?, Dib::new(size.0, size.1)?));
            self.size = size;
        }
        let dirty = if resized {
            Some(PixelRect::full(size.0, size.1))
        } else {
            list.damage(&self.shown)
                .and_then(|rect| rect.pixels(size.0, size.1))
        };
        if let (Some(area), Some((canvas, dib))) = (dirty, &mut self.pixels) {
            canvas.draw(&mut ui.text, &list, area)?;
            canvas.read(area, |row, bytes| dib.put_row(area, row, bytes))?;
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
        // SAFETY: our own window.
        let _ = unsafe { ShowWindow(self.hwnd, SW_SHOWNOACTIVATE) };
    }

    pub fn hide(&self) {
        // SAFETY: our own window.
        let _ = unsafe { ShowWindow(self.hwnd, SW_HIDE) };
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
        self.pixels = None;
        self.shown = DisplayList::default();
        self.size = (0, 0);
    }

    /// Drops input queued for this surface and not handled yet (it was hidden or reused).
    pub fn discard_events(&self) {
        let id = self.id();
        EVENTS.with(|queue| queue.borrow_mut().retain(|(of, _)| *of != id));
    }

    /// Makes this window the foreground window, and says how that went (for logs).
    ///
    /// A click our hook swallowed never reached any window, so Windows may not count us as the
    /// receiver of the last input and refuse a plain `SetForegroundWindow`. Then this attaches
    /// our input queue to the foreground window's thread for the call (a documented way to
    /// share its foreground right).
    pub fn take_focus(&self) -> &'static str {
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

    /// `UpdateLayeredWindowIndirect`: always position and alpha, plus pixels from `source`
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
        let dirty = source.map(|(_, area, _)| RECT {
            left: area.x as i32,
            top: area.y as i32,
            right: (area.x + area.width) as i32,
            bottom: (area.y + area.height) as i32,
        });
        let info = UPDATELAYEREDWINDOWINFO {
            cbSize: size_of::<UPDATELAYEREDWINDOWINFO>() as u32,
            hdcDst: HDC::default(),
            pptDst: &at,
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

    fn set_hits(&self, list: &DisplayList) {
        let events = self
            .shared
            .a11y
            .try_borrow_mut()
            .ok()
            .and_then(|mut a11y| a11y.as_mut()?.update(&list.hits, list.focus));
        if let Some(events) = events {
            events.raise();
        }
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        self.discard_events();
        // SAFETY: our own window, destroyed on its creating thread.
        let _ = unsafe { DestroyWindow(self.hwnd) };
    }
}

fn register_class() -> Result<()> {
    // SAFETY: plain Win32 registration with a 'static window procedure.
    unsafe {
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
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
    let id = shared.id;
    let button = |button| (pointer(shared, lparam), button);
    match msg {
        WM_MOUSEACTIVATE if !shared.activate => return LRESULT(MA_NOACTIVATE as isize),
        WM_MOUSEMOVE => push(id, Event::Move(pointer(shared, lparam))),
        WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN => {
            let (p, b) = button(button_of(msg));
            push(id, Event::Press(p, b));
        }
        WM_LBUTTONUP | WM_RBUTTONUP | WM_MBUTTONUP => {
            let (p, b) = button(button_of(msg));
            push(id, Event::Release(p, b));
        }
        WM_KEYDOWN => push(id, Event::Key(wparam.0 as u16)),
        WM_ACTIVATE if (wparam.0 & 0xFFFF) as u32 == WA_INACTIVE => push(id, Event::Deactivated),
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
        WM_LBUTTONDOWN | WM_LBUTTONUP => Button::Left,
        WM_RBUTTONDOWN | WM_RBUTTONUP => Button::Right,
        _ => Button::Middle,
    }
}
