//! Windowless RichEdit. The surface forwards messages synchronously (IME messages contain
//! borrowed pointers); app callbacks still run only from the event queue. RichEdit draws into
//! the same software D2D target as the rest of the surface.
#![allow(non_snake_case)]

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::rc::Rc;
use std::sync::OnceLock;

use crate::{Rect, Rgba};
use anyhow::{Context, Result};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct2D::ID2D1RenderTarget;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::UI::Controls::RichEdit::*;
use windows::Win32::UI::Input::Ime::*;
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, SetFocus};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{BOOL, BSTR, GUID, HRESULT, HSTRING, IUnknown, Interface, implement, s, w};

pub(crate) const WM_TEXT_INVALIDATED: u32 = WM_APP + 120;
const TIMER_BASE: usize = 0x50000;

// The SDK metadata omits textserv.h's UUIDs (windows-rs emits IID=0). Keep its complete ABI,
// with the real host UUID and base UUID for QueryInterface.
windows::core::imp::define_interface!(TextHost, HostVTable, 0x13e670f5_1a5a_11cf_abeb_00aa00b65ea1);
impl windows::core::RuntimeName for TextHost {}
#[repr(C)]
pub struct HostVTable {
    base: ITextHost2_Vtbl,
}
impl HostVTable {
    const fn new<T: ITextHost2_Impl, const OFFSET: isize>() -> Self {
        Self {
            base: ITextHost2_Vtbl::new::<T, OFFSET>(),
        }
    }
    fn matches(iid: &GUID) -> bool {
        *iid == TextHost::IID || *iid == GUID::from_u128(0x13e670f4_1a5a_11cf_abeb_00aa00b65ea1)
    }
}

struct State {
    window: HWND,
    rect: Cell<RECT>,
    revision: Cell<u64>,
    posted: Cell<bool>,
    drawing: Cell<bool>,
    multiline: bool,
    format: CHARFORMATW,
    paragraph: PARAFORMAT,
    timers: RefCell<Vec<usize>>,
    /// The caret RichEdit asks for (left, top, width, height) and whether it shows: a layered
    /// window never shows the system caret, so views draw it from [`TextField::caret`].
    caret: Cell<(i32, i32, i32, i32)>,
    caret_shown: Cell<bool>,
}
impl State {
    fn invalidate(&self) {
        // RichEdit can notify while painting; that paint already contains the current state.
        if self.drawing.get() {
            return;
        }
        self.revision.set(self.revision.get().wrapping_add(1));
        if !self.posted.replace(true) {
            // SAFETY: a window notification, carrying no borrowed pointer.
            let _ = unsafe {
                PostMessageW(Some(self.window), WM_TEXT_INVALIDATED, WPARAM(0), LPARAM(0))
            };
        }
    }
}

#[implement(TextHost)]
struct Host {
    state: Rc<State>,
}

/// A field belongs to one surface/UI thread. Clones share its text services and undo stack.
#[derive(Clone)]
pub struct TextField {
    inner: Rc<Inner>,
}
struct Inner {
    services: ITextServices2,
    state: Rc<State>,
    _host: TextHost,
}
impl std::fmt::Debug for TextField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextField").finish_non_exhaustive()
    }
}
impl PartialEq for TextField {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

impl TextField {
    pub fn new(window: HWND, rect: Rect, multiline: bool, size: f32, color: Rgba) -> Result<Self> {
        let mut format = CHARFORMATW {
            cbSize: size_of::<CHARFORMATW>() as u32,
            dwMask: CFM_FACE | CFM_SIZE | CFM_COLOR,
            yHeight: (size * 15.0) as i32,
            crTextColor: COLORREF(
                (color.r * 255.0) as u32
                    | (((color.g * 255.0) as u32) << 8)
                    | (((color.b * 255.0) as u32) << 16),
            ),
            ..Default::default()
        };
        for (slot, ch) in format.szFaceName.iter_mut().zip("Segoe UI".encode_utf16()) {
            *slot = ch;
        }
        let state = Rc::new(State {
            window,
            rect: Cell::new(winrect(rect)),
            revision: Cell::new(0),
            posted: Cell::new(false),
            drawing: Cell::new(false),
            multiline,
            format,
            paragraph: PARAFORMAT {
                cbSize: size_of::<PARAFORMAT>() as u32,
                ..Default::default()
            },
            timers: RefCell::new(Vec::new()),
            caret: Cell::new((0, 0, 1, 0)),
            caret_shown: Cell::new(false),
        });
        let host: TextHost = Host {
            state: state.clone(),
        }
        .into();
        let dll = library()?;
        // SAFETY: Msftedit's documented exports, with the ABI from textserv.h. The DLL stays
        // loaded for the process; host and services stay alive together on this UI thread.
        let services = unsafe {
            type Create =
                unsafe extern "system" fn(*mut c_void, *mut c_void, *mut *mut c_void) -> HRESULT;
            let create: Create = std::mem::transmute(
                GetProcAddress(dll, s!("CreateTextServices")).context("CreateTextServices")?,
            );
            let mut raw = std::ptr::null_mut();
            create(std::ptr::null_mut(), host.as_raw(), &mut raw).ok()?;
            let unknown = IUnknown::from_raw(raw);
            let iid = GetProcAddress(dll, s!("IID_ITextServices2")).context("IID_ITextServices2")?
                as *const GUID;
            let mut raw = std::ptr::null_mut();
            unknown.query(iid, &mut raw).ok()?;
            ITextServices2::from_raw(raw)
        };
        let field = Self {
            inner: Rc::new(Inner {
                services,
                state,
                _host: host,
            }),
        };
        field.send(
            EM_SETEVENTMASK,
            WPARAM(0),
            LPARAM((ENM_CHANGE | ENM_UPDATE | ENM_SELCHANGE) as isize),
        );
        field.send(
            EM_EXLIMITTEXT,
            WPARAM(0),
            LPARAM(if multiline { 500_000 } else { 4096 }),
        );
        Ok(field)
    }
    /// Where the caret is, in the surface's pixels, while it shows (steady: no blink timer).
    pub fn caret(&self) -> Option<Rect> {
        let state = &self.inner.state;
        let (x, y, w, h) = state.caret.get();
        (state.caret_shown.get() && h > 0)
            .then(|| Rect::new(x as f32, y as f32, w.max(1) as f32, h as f32))
    }
    pub fn revision(&self) -> u64 {
        self.inner.state.revision.get()
    }
    pub fn set_rect(&self, rect: Rect) {
        let rect = winrect(rect);
        if self.inner.state.rect.replace(rect) != rect {
            // SAFETY: services are alive on the creating UI thread.
            let _ = unsafe {
                self.inner
                    .services
                    .OnTxPropertyBitsChange(TXTBIT_CLIENTRECTCHANGE, TXTBIT_CLIENTRECTCHANGE)
            };
            self.inner.state.invalidate();
        }
    }
    pub fn set_text(&self, text: &str) -> Result<()> {
        // SAFETY: valid, terminated string for the synchronous call.
        unsafe {
            self.inner.services.TxSetText(&HSTRING::from(text))?;
        }
        Ok(())
    }
    pub fn text(&self) -> Result<String> {
        let mut value = BSTR::new();
        // SAFETY: BSTR out parameter is owned by `value` after the call.
        unsafe {
            self.inner.services.TxGetText(&mut value)?;
        }
        Ok(value.to_string())
    }
    pub fn focus(&self) {
        // SAFETY: the field's surface lives throughout the field's use.
        unsafe {
            let _ = SetForegroundWindow(self.inner.state.window);
            let _ = SetFocus(Some(self.inner.state.window));
            let _ = self
                .inner
                .services
                .OnTxInPlaceActivate(&mut self.inner.state.rect.get());
            let _ = self.inner.services.OnTxUIActivate();
        }
        self.send(WM_SETFOCUS, WPARAM(0), LPARAM(0));
    }
    pub fn blur(&self) {
        self.send(WM_KILLFOCUS, WPARAM(0), LPARAM(0));
        // SAFETY: paired with activation; kill every field timer as it stops receiving input.
        unsafe {
            let _ = self.inner.services.OnTxUIDeactivate();
            let _ = self.inner.services.OnTxInPlaceDeactivate();
        }
        self.kill_timers();
    }
    pub(crate) fn invalidated(&self) {
        self.inner.state.posted.set(false);
    }
    pub(crate) fn multiline(&self) -> bool {
        self.inner.state.multiline
    }
    pub(crate) fn contains(&self, x: i32, y: i32) -> bool {
        let r = self.inner.state.rect.get();
        x >= r.left && x < r.right && y >= r.top && y < r.bottom
    }
    /// Raw messages are forwarded only on the owning UI thread. `lparam` must obey the
    /// corresponding Windows message's pointer/lifetime contract.
    pub(crate) fn send(&self, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        let mut result = LRESULT(0);
        // SAFETY: called synchronously from the original window procedure or with owned data.
        let _ = unsafe {
            self.inner
                .services
                .TxSendMessage(msg, wparam, lparam, &mut result)
        };
        result
    }
    pub(crate) fn timer(&self, id: usize) -> bool {
        self.inner.state.timers.borrow().contains(&id)
    }
    pub(crate) fn draw(&self, target: &ID2D1RenderTarget, rect: Rect) -> Result<()> {
        let mut bounds = RECTL {
            left: rect.left as i32,
            top: rect.top as i32,
            right: rect.right as i32,
            bottom: rect.bottom as i32,
        };
        self.inner.state.drawing.set(true);
        // SAFETY: called between BeginDraw/EndDraw, software target and bounds valid throughout.
        let result = unsafe {
            self.inner.services.TxDrawD2D(
                target,
                &mut bounds,
                std::ptr::null_mut(),
                TXTVIEW_ACTIVE.0,
            )
        };
        self.inner.state.drawing.set(false);
        result?;
        Ok(())
    }
    fn kill_timers(&self) {
        for timer in self.inner.state.timers.borrow_mut().drain(..) {
            // SAFETY: timers are registered only on our own window.
            let _ = unsafe { KillTimer(Some(self.inner.state.window), timer) };
        }
    }
}
impl Drop for Inner {
    fn drop(&mut self) {
        // SAFETY: field timers must not outlive their state.
        for id in self.state.timers.borrow_mut().drain(..) {
            // SAFETY: this field registered this timer on its own surface.
            let _ = unsafe { KillTimer(Some(self.state.window), id) };
        }
    }
}
fn library() -> Result<HMODULE> {
    static DLL: OnceLock<isize> = OnceLock::new();
    let raw = *DLL.get_or_init(|| {
        // SAFETY: Windows' RichEdit DLL, retained because its COM vtables refer into it.
        unsafe { LoadLibraryW(w!("Msftedit.dll")).map_or(0, |m| m.0 as isize) }
    });
    anyhow::ensure!(raw != 0, "Msftedit.dll unavailable");
    Ok(HMODULE(raw as *mut c_void))
}
fn winrect(rect: Rect) -> RECT {
    RECT {
        left: rect.left as i32,
        top: rect.top as i32,
        right: rect.right as i32,
        bottom: rect.bottom as i32,
    }
}

impl ITextHost_Impl for Host_Impl {
    fn TxGetDC(&self) -> HDC {
        // SAFETY: synchronous host callback on the UI thread; the surface owns this HWND.
        unsafe { GetDC(Some(self.state.window)) }
    }
    fn TxReleaseDC(&self, dc: HDC) -> i32 {
        // SAFETY: synchronous host callback on the UI thread; the surface owns this HWND.
        unsafe { ReleaseDC(Some(self.state.window), dc) }
    }
    fn TxShowScrollBar(&self, _: i32, _: BOOL) -> BOOL {
        true.into()
    }
    fn TxEnableScrollBar(&self, _: SCROLLBAR_CONSTANTS, _: i32) -> BOOL {
        true.into()
    }
    fn TxSetScrollRange(&self, _: i32, _: i32, _: i32, _: BOOL) -> BOOL {
        true.into()
    }
    fn TxSetScrollPos(&self, _: i32, _: i32, _: BOOL) -> BOOL {
        true.into()
    }
    fn TxInvalidateRect(&self, _: *mut RECT, _: BOOL) {
        self.state.invalidate();
    }
    fn TxViewChange(&self, _: BOOL) {
        self.state.invalidate();
    }
    fn TxCreateCaret(&self, bitmap: HBITMAP, width: i32, height: i32) -> BOOL {
        let (x, y, _, _) = self.state.caret.get();
        self.state.caret.set((x, y, width, height));
        // SAFETY: synchronous host callback on the UI thread; the surface owns this HWND.
        unsafe {
            CreateCaret(self.state.window, Some(bitmap), width, height)
                .is_ok()
                .into()
        }
    }
    fn TxShowCaret(&self, show: BOOL) -> BOOL {
        if self.state.caret_shown.replace(show.as_bool()) != show.as_bool() {
            self.state.invalidate();
        }
        // SAFETY: synchronous host callback on the UI thread; the surface owns this HWND.
        unsafe {
            if show.as_bool() {
                ShowCaret(Some(self.state.window))
            } else {
                HideCaret(Some(self.state.window))
            }
            .is_ok()
            .into()
        }
    }
    fn TxSetCaretPos(&self, x: i32, y: i32) -> BOOL {
        let (_, _, w, h) = self.state.caret.get();
        if self.state.caret.replace((x, y, w, h)) != (x, y, w, h) {
            self.state.invalidate();
        }
        // SAFETY: synchronous host callback on the UI thread; the surface owns this HWND.
        unsafe { SetCaretPos(x, y).is_ok().into() }
    }
    fn TxSetTimer(&self, id: u32, timeout: u32) -> BOOL {
        let id = TIMER_BASE + id as usize;
        if !self.state.timers.borrow().contains(&id) {
            self.state.timers.borrow_mut().push(id);
        }
        // SAFETY: synchronous host callback on the UI thread; the surface owns this HWND.
        unsafe { (SetTimer(Some(self.state.window), id, timeout, None) != 0).into() }
    }
    fn TxKillTimer(&self, id: u32) {
        let id = TIMER_BASE + id as usize;
        self.state.timers.borrow_mut().retain(|t| *t != id);
        // SAFETY: synchronous host callback on the UI thread; the surface owns this HWND.
        unsafe {
            let _ = KillTimer(Some(self.state.window), id);
        }
    }
    fn TxScrollWindowEx(
        &self,
        _: i32,
        _: i32,
        _: *mut RECT,
        _: *mut RECT,
        _: HRGN,
        _: *mut RECT,
        _: SCROLL_WINDOW_FLAGS,
    ) {
        self.state.invalidate();
    }
    fn TxSetCapture(&self, capture: BOOL) {
        // SAFETY: synchronous host callback on the UI thread; the surface owns this HWND.
        unsafe {
            if capture.as_bool() {
                SetCapture(self.state.window);
            } else {
                let _ = ReleaseCapture();
            }
        }
    }
    fn TxSetFocus(&self) {
        // SAFETY: synchronous host callback on the UI thread; the surface owns this HWND.
        unsafe {
            let _ = SetFocus(Some(self.state.window));
        }
    }
    fn TxSetCursor(&self, cursor: HCURSOR, _: BOOL) {
        // SAFETY: synchronous host callback on the UI thread; the surface owns this HWND.
        unsafe {
            SetCursor(Some(cursor));
        }
    }
    fn TxScreenToClient(&self, p: *mut POINT) -> BOOL {
        // SAFETY: RichEdit supplies a valid in/out POINT for this synchronous callback.
        unsafe { ScreenToClient(self.state.window, p) }
    }
    fn TxClientToScreen(&self, p: *mut POINT) -> BOOL {
        // SAFETY: RichEdit supplies a valid in/out POINT for this synchronous callback.
        unsafe { ClientToScreen(self.state.window, p) }
    }
    fn TxActivate(&self, old: *mut i32) -> windows::core::Result<()> {
        // SAFETY: RichEdit supplies valid output pointers for the duration of this callback.
        unsafe {
            if !old.is_null() {
                *old = 0;
            }
        }
        Ok(())
    }
    fn TxDeactivate(&self, _: i32) -> windows::core::Result<()> {
        Ok(())
    }
    fn TxGetClientRect(&self, out: *mut RECT) -> windows::core::Result<()> {
        // SAFETY: RichEdit supplies valid output pointers for the duration of this callback.
        unsafe {
            *out = self.state.rect.get();
        }
        Ok(())
    }
    fn TxGetViewInset(&self, out: *mut RECT) -> windows::core::Result<()> {
        // SAFETY: RichEdit supplies valid output pointers for the duration of this callback.
        unsafe {
            *out = RECT::default();
        }
        Ok(())
    }
    fn TxGetCharFormat(&self, out: *const *const CHARFORMATW) -> windows::core::Result<()> {
        // SAFETY: RichEdit supplies valid output pointers for the duration of this callback.
        unsafe {
            *(out as *mut *const CHARFORMATW) = &self.state.format;
        }
        Ok(())
    }
    fn TxGetParaFormat(&self, out: *const *const PARAFORMAT) -> windows::core::Result<()> {
        // SAFETY: RichEdit supplies valid output pointers for the duration of this callback.
        unsafe {
            *(out as *mut *const PARAFORMAT) = &self.state.paragraph;
        }
        Ok(())
    }
    fn TxGetSysColor(&self, index: SYS_COLOR_INDEX) -> COLORREF {
        // SAFETY: a read-only query of a system colour index supplied by RichEdit.
        COLORREF(unsafe { GetSysColor(index) })
    }
    fn TxGetBackStyle(&self, out: *mut TXTBACKSTYLE) -> windows::core::Result<()> {
        // SAFETY: RichEdit supplies valid output pointers for the duration of this callback.
        unsafe {
            *out = TXTBACK_TRANSPARENT;
        }
        Ok(())
    }
    fn TxGetMaxLength(&self, out: *mut u32) -> windows::core::Result<()> {
        // SAFETY: RichEdit supplies valid output pointers for the duration of this callback.
        unsafe {
            *out = if self.state.multiline { 500_000 } else { 4096 };
        }
        Ok(())
    }
    fn TxGetScrollBars(&self, out: *mut u32) -> windows::core::Result<()> {
        // SAFETY: RichEdit supplies valid output pointers for the duration of this callback.
        unsafe {
            *out = if self.state.multiline {
                WS_VSCROLL.0
            } else {
                0
            };
        }
        Ok(())
    }
    fn TxGetPasswordChar(&self) -> windows::core::Result<i8> {
        Ok(0)
    }
    fn TxGetAcceleratorPos(&self, out: *mut i32) -> windows::core::Result<()> {
        // SAFETY: RichEdit supplies valid output pointers for the duration of this callback.
        unsafe {
            *out = -1;
        }
        Ok(())
    }
    fn TxGetExtent(&self, out: *mut SIZE) -> windows::core::Result<()> {
        let r = self.state.rect.get();
        // SAFETY: RichEdit supplies valid output pointers for the duration of this callback.
        unsafe {
            *out = SIZE {
                cx: (r.right - r.left) * 2540 / 96,
                cy: (r.bottom - r.top) * 2540 / 96,
            };
        }
        Ok(())
    }
    fn OnTxCharFormatChange(&self, _: *const CHARFORMATW) -> windows::core::Result<()> {
        self.state.invalidate();
        Ok(())
    }
    fn OnTxParaFormatChange(&self, _: *const PARAFORMAT) -> windows::core::Result<()> {
        self.state.invalidate();
        Ok(())
    }
    fn TxGetPropertyBits(&self, mask: u32, out: *mut u32) -> windows::core::Result<()> {
        // SAFETY: RichEdit supplies valid output pointers for the duration of this callback.
        unsafe {
            *out = mask
                & (TXTBIT_D2DDWRITE
                    | TXTBIT_WORDWRAP
                    | TXTBIT_ALLOWBEEP
                    | if self.state.multiline {
                        TXTBIT_MULTILINE
                    } else {
                        0
                    });
        }
        Ok(())
    }
    fn TxNotify(&self, code: u32, _: *mut c_void) -> windows::core::Result<()> {
        if code == EN_CHANGE || code == EN_SELCHANGE {
            self.state.invalidate();
        }
        Ok(())
    }
    fn TxImmGetContext(&self) -> HIMC {
        // SAFETY: synchronous host callback on the UI thread; the surface owns this HWND.
        unsafe { ImmGetContext(self.state.window) }
    }
    fn TxImmReleaseContext(&self, context: HIMC) {
        // SAFETY: synchronous host callback on the UI thread; the surface owns this HWND.
        unsafe {
            let _ = ImmReleaseContext(self.state.window, context);
        }
    }
    fn TxGetSelectionBarWidth(&self, out: *mut i32) -> windows::core::Result<()> {
        // SAFETY: RichEdit supplies valid output pointers for the duration of this callback.
        unsafe {
            *out = 0;
        }
        Ok(())
    }
}

impl ITextHost2_Impl for Host_Impl {
    fn TxIsDoubleClickPending(&self) -> BOOL {
        false.into()
    }
    fn TxGetWindow(&self, out: *mut HWND) -> windows::core::Result<()> {
        // SAFETY: RichEdit supplies valid output pointers for the duration of this callback.
        unsafe {
            *out = self.state.window;
        }
        Ok(())
    }
    fn TxSetForegroundWindow(&self) -> windows::core::Result<()> {
        // SAFETY: synchronous host callback on the UI thread; the surface owns this HWND.
        unsafe {
            let _ = SetForegroundWindow(GetAncestor(self.state.window, GA_ROOT));
        }
        Ok(())
    }
    fn TxGetPalette(&self) -> HPALETTE {
        HPALETTE::default()
    }
    fn TxGetEastAsianFlags(&self, out: *mut i32) -> windows::core::Result<()> {
        // SAFETY: RichEdit supplies valid output pointers for the duration of this callback.
        unsafe {
            *out = 0;
        }
        Ok(())
    }
    fn TxSetCursor2(&self, cursor: HCURSOR, _: BOOL) -> HCURSOR {
        // SAFETY: synchronous host callback on the UI thread; the surface owns this HWND.
        unsafe { SetCursor(Some(cursor)) }
    }
    fn TxFreeTextServicesNotification(&self) {}
    fn TxGetEditStyle(&self, _: u32, out: *mut u32) -> windows::core::Result<()> {
        // SAFETY: RichEdit supplies valid output pointers for the duration of this callback.
        unsafe {
            *out = SES_USECTF | SES_CTFALLOWEMBED;
        }
        Ok(())
    }
    fn TxGetWindowStyles(&self, style: *mut u32, ex: *mut u32) -> windows::core::Result<()> {
        // SAFETY: RichEdit supplies valid output pointers for the duration of this callback.
        unsafe {
            *style = if self.state.multiline {
                ES_MULTILINE as u32 | ES_AUTOVSCROLL as u32
            } else {
                ES_AUTOHSCROLL as u32
            };
            *ex = 0;
        }
        Ok(())
    }
    fn TxShowDropCaret(&self, _: BOOL, _: HDC, _: *mut RECT) -> windows::core::Result<()> {
        Ok(())
    }
    fn TxDestroyCaret(&self) -> windows::core::Result<()> {
        // SAFETY: synchronous host callback on the UI thread; the surface owns this HWND.
        unsafe { DestroyCaret() }
    }
    fn TxGetHorzExtent(&self, out: *mut i32) -> windows::core::Result<()> {
        // SAFETY: RichEdit supplies valid output pointers for the duration of this callback.
        unsafe {
            *out = 0;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DisplayList, Kind, Op, Role, Surface, Ui, render};
    use windows::Win32::UI::Controls::{EM_SETSEL, EM_UNDO};

    #[test]
    fn rich_edit_draws_bidi_cjk_selection_and_undo_on_the_software_target() {
        let mut ui = Ui::new().unwrap();
        let surface = Surface::new(
            Kind::Popup { activate: true },
            "Text field test",
            Role::TextInput,
        )
        .unwrap();
        let rect = Rect::new(8.0, 8.0, 300.0, 140.0);
        let field = TextField::new(
            surface.hwnd(),
            rect,
            true,
            18.0,
            Rgba::new(1.0, 1.0, 1.0, 1.0),
        )
        .unwrap();
        field.set_text("مرحبا بالعالم\n日本語 中文 한국어").unwrap();
        assert!(field.text().unwrap().contains("مرحبا بالعالم"));
        assert!(field.text().unwrap().contains("日本語 中文 한국어"));
        field.focus();
        field.send(EM_SETSEL, WPARAM(0), LPARAM(0));
        field.send(WM_CHAR, WPARAM('X' as usize), LPARAM(1));
        assert!(field.text().unwrap().starts_with('X'));
        field.send(EM_UNDO, WPARAM(0), LPARAM(0));
        assert!(
            !field.text().unwrap().starts_with('X'),
            "native RichEdit undo"
        );
        field.send(EM_SETSEL, WPARAM(0), LPARAM(5));
        let list = DisplayList {
            ops: vec![
                Op::Fill {
                    rect: Rect::new(0.0, 0.0, 320.0, 160.0),
                    radius: 0.0,
                    color: Rgba::HIT,
                },
                Op::TextField {
                    rect,
                    field: field.clone(),
                    revision: field.revision(),
                },
            ],
            ..Default::default()
        };
        let revision = field.revision();
        let image = render(&mut ui, &list, 320, 160).unwrap();
        assert_eq!(
            field.revision(),
            revision,
            "painting must not schedule itself again"
        );
        assert!(
            image
                .pixels
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|px| px[3] > 1)
                .count()
                > 100,
            "TxDrawD2D paints real glyphs/selection"
        );
        field.blur();
    }
}
