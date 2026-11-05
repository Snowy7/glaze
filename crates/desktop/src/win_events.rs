//! One shell-owned WinEvent source. Subscribers run on its UI thread; other threads forward
//! notifications to their own queues. Location hooks follow only Explorer and the foreground app.
use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use windows::Win32::{
    Foundation::HWND,
    UI::{
        Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent},
        WindowsAndMessaging::*,
    },
};

#[derive(Clone, Copy)]
pub struct Event {
    pub event: u32,
    pub window: isize,
}

type Callback = Arc<dyn Fn(Event) + Send + Sync>;
type Listeners = Arc<Vec<(u64, Callback)>>;
fn listeners() -> &'static Mutex<Listeners> {
    static LISTENERS: OnceLock<Mutex<Listeners>> = OnceLock::new();
    LISTENERS.get_or_init(|| Mutex::new(Arc::new(Vec::new())))
}
static NEXT: AtomicU64 = AtomicU64::new(1);
static SCOPES_DIRTY: AtomicBool = AtomicBool::new(true);
static DELIVERED: AtomicU64 = AtomicU64::new(0);

/// Dropping a subscription removes it without touching another thread's native hooks.
pub struct Subscription(u64);
impl Subscription {
    pub fn new(callback: impl Fn(Event) + Send + Sync + 'static) -> Self {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let mut listeners = listeners().lock().unwrap();
        Arc::make_mut(&mut listeners).push((id, Arc::new(callback)));
        Self(id)
    }
}
impl Drop for Subscription {
    fn drop(&mut self) {
        let mut listeners = listeners().lock().unwrap();
        Arc::make_mut(&mut listeners).retain(|(id, _)| *id != self.0);
    }
}

/// Install once on the shell UI thread, pump that thread and refresh after dispatching messages.
pub struct WinEvents {
    hooks: Vec<HWINEVENTHOOK>,
    locations: Vec<HWINEVENTHOOK>,
    processes: Vec<u32>,
}
impl WinEvents {
    /// Active native hooks, callbacks delivered and current subscribers (live diagnostics).
    pub fn counts(&self) -> (usize, u64, usize) {
        (
            self.hooks.len() + self.locations.len(),
            DELIVERED.load(Ordering::Relaxed),
            listeners().lock().unwrap().len(),
        )
    }
    pub fn install() -> Self {
        let hooks = [
            (EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND),
            (EVENT_SYSTEM_MINIMIZESTART, EVENT_SYSTEM_MINIMIZEEND),
            (EVENT_OBJECT_CREATE, EVENT_OBJECT_HIDE),
            (EVENT_OBJECT_NAMECHANGE, EVENT_OBJECT_NAMECHANGE),
            (EVENT_OBJECT_CLOAKED, EVENT_OBJECT_UNCLOAKED),
        ]
        .into_iter()
        .filter_map(|(first, last)| hook(first, last, 0))
        .collect();
        let mut source = Self {
            hooks,
            locations: Vec::new(),
            processes: Vec::new(),
        };
        SCOPES_DIRTY.store(true, Ordering::Relaxed);
        source.refresh();
        source
    }

    pub fn refresh(&mut self) {
        if !SCOPES_DIRTY.swap(false, Ordering::Relaxed) {
            return;
        }
        let (mut front, mut explorer) = (0, 0);
        // SAFETY: read-only queries; vanished windows leave process IDs at zero.
        unsafe {
            GetWindowThreadProcessId(GetForegroundWindow(), Some(&mut front));
            GetWindowThreadProcessId(GetShellWindow(), Some(&mut explorer));
        }
        let mut processes: Vec<_> = [front, explorer]
            .into_iter()
            .filter(|&pid| pid != 0 && pid != std::process::id())
            .collect();
        processes.sort_unstable();
        processes.dedup();
        if processes != self.processes {
            unhook(&mut self.locations);
            self.locations = processes
                .iter()
                .filter_map(|&pid| {
                    hook(
                        EVENT_OBJECT_LOCATIONCHANGE,
                        EVENT_OBJECT_LOCATIONCHANGE,
                        pid,
                    )
                })
                .collect();
            self.processes = processes;
        }
    }
}
impl Drop for WinEvents {
    fn drop(&mut self) {
        unhook(&mut self.hooks);
        unhook(&mut self.locations);
    }
}
fn hook(first: u32, last: u32, pid: u32) -> Option<HWINEVENTHOOK> {
    // SAFETY: static callback; hooks are owned and dropped on the installing UI thread.
    let hook = unsafe {
        SetWinEventHook(
            first,
            last,
            None,
            Some(on_event),
            pid,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        )
    };
    (!hook.is_invalid()).then_some(hook)
}
fn unhook(hooks: &mut Vec<HWINEVENTHOOK>) {
    for hook in hooks.drain(..) {
        // SAFETY: owned hook on its installing thread.
        let _ = unsafe { UnhookWinEvent(hook) };
    }
}
unsafe extern "system" fn on_event(
    _: HWINEVENTHOOK,
    event: u32,
    window: HWND,
    object: i32,
    child: i32,
    _: u32,
    _: u32,
) {
    if object != OBJID_WINDOW.0 || child != CHILDID_SELF as i32 {
        return;
    }
    DELIVERED.fetch_add(1, Ordering::Relaxed);
    if matches!(
        event,
        EVENT_SYSTEM_FOREGROUND | EVENT_OBJECT_CREATE | EVENT_OBJECT_DESTROY
    ) {
        SCOPES_DIRTY.store(true, Ordering::Relaxed);
    }
    // Release the lock before callbacks: a subscriber may disappear on another thread.
    let listeners = listeners().lock().unwrap().clone();
    let event = Event {
        event,
        window: window.0 as isize,
    };
    for (_, callback) in listeners.iter() {
        callback(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_event_fans_out_and_dropping_a_subscriber_stops_its_deliveries() {
        let first = Arc::new(AtomicU64::new(0));
        let second = Arc::new(AtomicU64::new(0));
        let a = first.clone();
        let b = second.clone();
        let subscription = Subscription::new(move |_| {
            a.fetch_add(1, Ordering::Relaxed);
        });
        let _other = Subscription::new(move |_| {
            b.fetch_add(1, Ordering::Relaxed);
        });
        let notify = || {
            // SAFETY: a synthetic notification to our callback; it never changes a window.
            unsafe {
                on_event(
                    HWINEVENTHOOK::default(),
                    EVENT_SYSTEM_FOREGROUND,
                    HWND::default(),
                    OBJID_WINDOW.0,
                    CHILDID_SELF as i32,
                    0,
                    0,
                )
            };
        };
        notify();
        drop(subscription);
        notify();
        assert_eq!(first.load(Ordering::Relaxed), 1);
        assert_eq!(second.load(Ordering::Relaxed), 2);
    }
}
