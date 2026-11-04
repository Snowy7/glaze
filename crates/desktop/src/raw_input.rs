//! Mouse input for interactive wallpapers (Lively-style).
//!
//! The wallpaper sits *behind* the desktop icons, so it never gets mouse
//! messages. [`RawMouse`] listens through Raw Input instead
//! (`RegisterRawInputDevices` with `RIDEV_INPUTSINK`, a documented way to see
//! the mouse while another window has it): no hook, nothing injected, and
//! nothing swallowed, so the desktop behaves exactly as before.
//!
//! Costs nothing until created, and costs nothing while [`set_active`]`(false)`
//! (the device registration is removed). While active, each raw event is
//! decoded and queued; moves are *coalesced* and handed over at most once per
//! flush interval (the frame interval), button presses and the wheel at once.
//!
//! [`Gate`] decides which events the wallpaper should see (only those that
//! belong to the bare desktop), as pure logic.
//!
//! [`set_active`]: RawMouse::set_active

use std::cell::{Cell, RefCell};

use anyhow::{Context, Result};
use windows::Win32::Foundation::{
    ERROR_CLASS_ALREADY_EXISTS, GetLastError, HWND, LPARAM, LRESULT, POINT, WPARAM,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::{
    GetRawInputData, HRAWINPUT, RAWINPUT, RAWINPUTDEVICE, RAWINPUTDEVICE_FLAGS, RAWINPUTHEADER,
    RID_INPUT, RIDEV_INPUTSINK, RIDEV_REMOVE, RIM_TYPEMOUSE, RegisterRawInputDevices,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GWLP_USERDATA, GetCursorPos, GetWindowLongPtrW,
    HWND_MESSAGE, KillTimer, RegisterClassExW, SetTimer, SetWindowLongPtrW, WM_INPUT, WM_TIMER,
    WNDCLASSEXW,
};
use windows::core::{PCWSTR, w};

const CLASS: PCWSTR = w!("mypaper.rawinput");
const TIMER: usize = 1;

// `usButtonFlags` bits of RAWMOUSE.
const LEFT_DOWN: u16 = 0x0001;
const LEFT_UP: u16 = 0x0002;
const RIGHT_DOWN: u16 = 0x0004;
const RIGHT_UP: u16 = 0x0008;
const MIDDLE_DOWN: u16 = 0x0010;
const MIDDLE_UP: u16 = 0x0020;
const WHEEL: u16 = 0x0400;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
    Middle,
}

impl Button {
    fn index(self) -> usize {
        self as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Move,
    Down(Button),
    Up(Button),
    /// Wheel notches times 120 (`WHEEL_DELTA`); positive is away from the user.
    Wheel(i32),
    /// The pointer left the desktop (only produced by [`Gate`]).
    Leave,
}

/// One mouse event at a screen position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Event {
    pub kind: Kind,
    pub at: (i32, i32),
}

/// Queues events and merges the ones that lose nothing when merged:
/// consecutive moves keep only the last position, consecutive wheel events add up.
#[derive(Debug, Default)]
pub struct Coalescer {
    events: Vec<Event>,
}

impl Coalescer {
    pub fn push(&mut self, event: Event) {
        match (self.events.last_mut(), event.kind) {
            (Some(last), Kind::Move) if last.kind == Kind::Move => last.at = event.at,
            (
                Some(Event {
                    kind: Kind::Wheel(sum),
                    at,
                }),
                Kind::Wheel(delta),
            ) => {
                *sum += delta;
                *at = event.at;
            }
            _ => self.events.push(event),
        }
    }

    /// Is something other than a move waiting? Those are not worth delaying.
    pub fn urgent(&self) -> bool {
        self.events.iter().any(|e| e.kind != Kind::Move)
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    pub fn take(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }
}

/// Which events the wallpaper gets: those on the bare desktop, plus the rest
/// of a drag that started there. Pure; the caller supplies the probes, which
/// are only asked when needed.
#[derive(Debug, Default)]
pub struct Gate {
    held: [bool; 3],
    inside: bool,
}

impl Gate {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// The event to forward, if any. `over_desktop`: the point is on the
    /// desktop layer; `over_icon`: it is on an icon (only asked on a press).
    pub fn filter(
        &mut self,
        event: Event,
        over_desktop: impl FnOnce() -> bool,
        over_icon: impl FnOnce() -> bool,
    ) -> Option<Event> {
        let dragging = self.held.iter().any(|&h| h);
        match event.kind {
            Kind::Move => {
                if dragging || over_desktop() {
                    self.inside = true;
                    Some(event)
                } else if std::mem::take(&mut self.inside) {
                    Some(Event {
                        kind: Kind::Leave,
                        ..event
                    })
                } else {
                    None
                }
            }
            Kind::Down(button) => {
                if over_desktop() && !over_icon() {
                    self.held[button.index()] = true;
                    self.inside = true;
                    Some(event)
                } else {
                    None
                }
            }
            // A release always reaches the page that saw the press, wherever the pointer is.
            Kind::Up(button) => std::mem::take(&mut self.held[button.index()]).then_some(event),
            Kind::Wheel(_) => over_desktop().then_some(event),
            Kind::Leave => None,
        }
    }
}

/// The receiver of coalesced events.
type Handler = Box<dyn FnMut(&[Event])>;

struct Inner {
    queue: RefCell<Coalescer>,
    timer_armed: Cell<bool>,
    /// A handler that pumps messages could re-enter; the second call is dropped.
    handler: RefCell<Handler>,
    interval_ms: u32,
    hwnd: Cell<HWND>,
}

/// A message-only window that receives raw mouse input and calls a handler
/// with coalesced events. Create it, and pump messages, on one thread.
pub struct RawMouse {
    hwnd: HWND,
    inner: Box<Inner>,
    active: bool,
}

impl RawMouse {
    /// `interval_ms` is how often moves may be delivered (16 = 60 per second).
    /// Starts inactive: call [`set_active`](Self::set_active).
    pub fn new(interval_ms: u32, handler: impl FnMut(&[Event]) + 'static) -> Result<Self> {
        register_class()?;
        // SAFETY: class registered above; HWND_MESSAGE makes a message-only window.
        let hwnd = unsafe {
            CreateWindowExW(
                Default::default(),
                CLASS,
                w!("mypaper raw input"),
                Default::default(),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                None,
                None,
            )
        }
        .context("creating the raw input window")?;
        let inner = Box::new(Inner {
            queue: RefCell::default(),
            timer_armed: Cell::new(false),
            handler: RefCell::new(Box::new(handler)),
            interval_ms: interval_ms.max(1),
            hwnd: Cell::new(hwnd),
        });
        // SAFETY: the box outlives the window (destroyed first in `drop`), and its
        // heap address does not move when `RawMouse` does.
        unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, &raw const *inner as isize) };
        Ok(Self {
            hwnd,
            inner,
            active: false,
        })
    }

    /// Starts or stops listening. Stopped costs nothing: Windows no longer
    /// routes mouse input to us.
    pub fn set_active(&mut self, active: bool) {
        if active == self.active {
            return;
        }
        let (flags, target) = if active {
            (RIDEV_INPUTSINK, Some(self.hwnd))
        } else {
            (RIDEV_REMOVE, None)
        };
        if let Err(e) = register(flags, target) {
            log::warn!("raw input {}: {e}", if active { "on" } else { "off" });
            return;
        }
        self.active = active;
        if !active {
            self.inner.queue.borrow_mut().take();
            self.inner.disarm();
        }
    }

    pub fn is_active(&self) -> bool {
        self.active
    }
}

impl Drop for RawMouse {
    fn drop(&mut self) {
        self.set_active(false);
        // SAFETY: our own window, destroyed on its creating thread; clearing the
        // pointer first so a late message cannot reach the freed `Inner`.
        unsafe {
            SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, 0);
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

fn register(flags: RAWINPUTDEVICE_FLAGS, target: Option<HWND>) -> Result<()> {
    let device = RAWINPUTDEVICE {
        usUsagePage: 0x01, // generic desktop
        usUsage: 0x02,     // mouse
        dwFlags: flags,
        hwndTarget: target.unwrap_or_default(),
    };
    // SAFETY: one valid device description; the size is that of the struct.
    unsafe { RegisterRawInputDevices(&[device], size_of::<RAWINPUTDEVICE>() as u32) }
        .context("RegisterRawInputDevices")
}

fn register_class() -> Result<()> {
    // SAFETY: plain Win32 registration with a 'static window procedure.
    unsafe {
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: GetModuleHandleW(None)?.into(),
            lpszClassName: CLASS,
            ..Default::default()
        };
        if RegisterClassExW(&class) == 0 && GetLastError() != ERROR_CLASS_ALREADY_EXISTS {
            return Err(windows::core::Error::from_thread()).context("RegisterClassExW");
        }
    }
    Ok(())
}

impl Inner {
    fn arm(&self) {
        if !self.timer_armed.replace(true) {
            // SAFETY: our window, on its thread.
            unsafe { SetTimer(Some(self.hwnd.get()), TIMER, self.interval_ms, None) };
        }
    }

    fn disarm(&self) {
        if self.timer_armed.replace(false) {
            // SAFETY: as in `arm`.
            let _ = unsafe { KillTimer(Some(self.hwnd.get()), TIMER) };
        }
    }

    fn flush(&self) {
        self.disarm();
        let events = self.queue.borrow_mut().take();
        if events.is_empty() {
            return;
        }
        if let Ok(mut handler) = self.handler.try_borrow_mut() {
            handler(&events);
        }
    }

    /// Decodes one `WM_INPUT` and queues what it carries.
    fn on_input(&self, lparam: LPARAM) {
        let Some(raw) = read_mouse(lparam) else {
            return;
        };
        let mut cursor = POINT::default();
        // SAFETY: `cursor` outlives the call.
        if unsafe { GetCursorPos(&mut cursor) }.is_err() {
            return;
        }
        let at = (cursor.x, cursor.y);
        let urgent = {
            let mut queue = self.queue.borrow_mut();
            if raw.moved {
                queue.push(Event {
                    kind: Kind::Move,
                    at,
                });
            }
            for kind in raw.kinds() {
                queue.push(Event { kind, at });
            }
            queue.urgent()
        };
        if urgent {
            self.flush();
        } else {
            self.arm();
        }
    }
}

/// What one raw mouse report holds.
struct RawReport {
    moved: bool,
    buttons: u16,
    wheel: i16,
}

impl RawReport {
    /// The button and wheel events, in a fixed order (releases last, so a
    /// press and release in one report still reads as a click).
    fn kinds(&self) -> impl Iterator<Item = Kind> + '_ {
        const PRESSES: [(u16, Button); 3] = [
            (LEFT_DOWN, Button::Left),
            (RIGHT_DOWN, Button::Right),
            (MIDDLE_DOWN, Button::Middle),
        ];
        const RELEASES: [(u16, Button); 3] = [
            (LEFT_UP, Button::Left),
            (RIGHT_UP, Button::Right),
            (MIDDLE_UP, Button::Middle),
        ];
        let flags = self.buttons;
        let wheel = (flags & WHEEL != 0).then_some(Kind::Wheel(i32::from(self.wheel)));
        PRESSES
            .into_iter()
            .filter(move |(bit, _)| flags & bit != 0)
            .map(|(_, b)| Kind::Down(b))
            .chain(wheel)
            .chain(
                RELEASES
                    .into_iter()
                    .filter(move |(bit, _)| flags & bit != 0)
                    .map(|(_, b)| Kind::Up(b)),
            )
    }
}

fn read_mouse(lparam: LPARAM) -> Option<RawReport> {
    let mut raw = RAWINPUT::default();
    let mut size = size_of::<RAWINPUT>() as u32;
    // SAFETY: `raw` is a valid buffer of `size` bytes; the header size is that of the struct.
    let copied = unsafe {
        GetRawInputData(
            HRAWINPUT(lparam.0 as _),
            RID_INPUT,
            Some((&raw mut raw).cast()),
            &mut size,
            size_of::<RAWINPUTHEADER>() as u32,
        )
    };
    if copied == u32::MAX || raw.header.dwType != RIM_TYPEMOUSE.0 {
        return None;
    }
    // SAFETY: the header says this report is a mouse report, so `mouse` is the live union field.
    let mouse = unsafe { raw.data.mouse };
    // SAFETY: the button fields are plain integers in an anonymous union.
    let (buttons, wheel) = unsafe {
        let parts = mouse.Anonymous.Anonymous;
        (parts.usButtonFlags, parts.usButtonData as i16)
    };
    Some(RawReport {
        moved: mouse.lLastX != 0 || mouse.lLastY != 0 || mouse.usFlags.0 & 1 != 0,
        buttons,
        wheel,
    })
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: the pointer was set by `RawMouse::new` to a live `Inner`, and is cleared before it dies.
    let inner = unsafe { (GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Inner).as_ref() };
    match (msg, inner) {
        (WM_INPUT, Some(inner)) => inner.on_input(lparam),
        (WM_TIMER, Some(inner)) if wparam.0 == TIMER => inner.flush(),
        _ => {}
    }
    // WM_INPUT needs DefWindowProc for cleanup; the rest is default handling.
    // SAFETY: default handling of a message we were called with.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(kind: Kind, x: i32) -> Event {
        Event { kind, at: (x, x) }
    }

    #[test]
    fn a_burst_of_moves_becomes_one_event_at_the_last_position() {
        let mut queue = Coalescer::default();
        for x in 1..=1000 {
            queue.push(ev(Kind::Move, x));
        }
        assert!(!queue.urgent(), "moves alone can wait for the frame");
        assert_eq!(queue.take(), [ev(Kind::Move, 1000)]);
        assert!(queue.is_empty());
    }

    #[test]
    fn order_around_clicks_is_kept_and_wheel_notches_add_up() {
        let mut queue = Coalescer::default();
        queue.push(ev(Kind::Move, 1));
        queue.push(ev(Kind::Move, 2));
        queue.push(ev(Kind::Down(Button::Left), 2));
        assert!(queue.urgent());
        queue.push(ev(Kind::Move, 3));
        queue.push(ev(Kind::Move, 4));
        queue.push(ev(Kind::Up(Button::Left), 4));
        queue.push(ev(Kind::Wheel(120), 4));
        queue.push(ev(Kind::Wheel(120), 5));
        queue.push(ev(Kind::Wheel(-120), 6));
        assert_eq!(
            queue.take(),
            [
                ev(Kind::Move, 2),
                ev(Kind::Down(Button::Left), 2),
                ev(Kind::Move, 4),
                ev(Kind::Up(Button::Left), 4),
                ev(Kind::Wheel(120), 6),
            ]
        );
    }

    #[test]
    fn a_report_yields_presses_then_wheel_then_releases() {
        let report = RawReport {
            moved: true,
            buttons: LEFT_DOWN | LEFT_UP | RIGHT_DOWN | WHEEL,
            wheel: -120,
        };
        assert_eq!(
            report.kinds().collect::<Vec<_>>(),
            [
                Kind::Down(Button::Left),
                Kind::Down(Button::Right),
                Kind::Wheel(-120),
                Kind::Up(Button::Left),
            ]
        );
        let idle = RawReport {
            moved: true,
            buttons: 0,
            wheel: 0,
        };
        assert_eq!(idle.kinds().count(), 0);
    }

    #[test]
    fn only_the_bare_desktop_gets_events_and_leaving_is_announced_once() {
        let mut gate = Gate::default();
        let desktop = |on| move || on;
        let never = || unreachable!();
        assert_eq!(
            gate.filter(ev(Kind::Move, 1), desktop(true), never),
            Some(ev(Kind::Move, 1))
        );
        assert_eq!(
            gate.filter(ev(Kind::Move, 2), desktop(false), never),
            Some(ev(Kind::Leave, 2))
        );
        assert_eq!(
            gate.filter(ev(Kind::Move, 3), desktop(false), never),
            None,
            "already outside"
        );
        assert_eq!(
            gate.filter(ev(Kind::Wheel(120), 3), desktop(false), never),
            None
        );
        assert_eq!(
            gate.filter(ev(Kind::Wheel(120), 4), desktop(true), never),
            Some(ev(Kind::Wheel(120), 4))
        );
    }

    #[test]
    fn clicks_on_icons_never_reach_the_page() {
        let mut gate = Gate::default();
        let press = ev(Kind::Down(Button::Left), 1);
        assert_eq!(
            gate.filter(press, || true, || true),
            None,
            "an icon is the desktop's"
        );
        assert_eq!(
            gate.filter(
                ev(Kind::Up(Button::Left), 1),
                || unreachable!(),
                || unreachable!()
            ),
            None
        );
        assert_eq!(
            gate.filter(press, || false, || unreachable!()),
            None,
            "an app window"
        );
    }

    #[test]
    fn a_drag_from_the_bare_desktop_follows_the_pointer_and_ends_with_its_release() {
        let mut gate = Gate::default();
        let press = ev(Kind::Down(Button::Left), 1);
        assert_eq!(gate.filter(press, || true, || false), Some(press));
        // Dragged onto an app window: still the page's drag.
        let away = ev(Kind::Move, 50);
        assert_eq!(
            gate.filter(away, || unreachable!(), || unreachable!()),
            Some(away)
        );
        // The release is delivered even over an app, exactly once.
        let release = ev(Kind::Up(Button::Left), 50);
        assert_eq!(
            gate.filter(release, || unreachable!(), || unreachable!()),
            Some(release)
        );
        assert_eq!(
            gate.filter(release, || unreachable!(), || unreachable!()),
            None
        );
        // And the pointer is no longer tracked outside.
        assert_eq!(
            gate.filter(ev(Kind::Move, 60), || false, || unreachable!()),
            Some(ev(Kind::Leave, 60))
        );
        assert_eq!(
            gate.filter(ev(Kind::Move, 61), || false, || unreachable!()),
            None
        );
    }

    #[test]
    fn reset_forgets_a_drag() {
        let mut gate = Gate::default();
        gate.filter(ev(Kind::Down(Button::Right), 1), || true, || false);
        gate.reset();
        assert_eq!(
            gate.filter(
                ev(Kind::Up(Button::Right), 1),
                || unreachable!(),
                || unreachable!()
            ),
            None
        );
    }
}
