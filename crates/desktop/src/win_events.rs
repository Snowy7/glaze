//! One shell-owned WinEvent source. Its hooks live on a thread of their own (`win-events`) that
//! takes events in batches: location hooks follow Explorer and the foreground app, and Windows
//! counts every cursor move over that app as one of their events (about 300 a second from a
//! moving mouse), so on the shell's UI thread they woke it that often for nothing. Subscribers
//! run on the hook thread and record what they need; one that recorded something calls
//! [`notify`], and the thread that installed the source then gets one `WM_NULL` per batch.
use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering},
};
use std::time::Duration;
use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    System::Threading::{GetCurrentThreadId, INFINITE},
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
static HOOKS: AtomicUsize = AtomicUsize::new(0);
/// A subscriber recorded something during the current batch.
static NOTIFIED: AtomicBool = AtomicBool::new(false);
/// The thread that installed the source (0: none), woken after a batch that [`notify`]d.
static CONSUMER: AtomicU32 = AtomicU32::new(0);
static SOURCE: AtomicU32 = AtomicU32::new(0);
static TILING: Mutex<Option<Vec<u32>>> = Mutex::new(None);

/// Optional move/size and managed-process location hooks on the existing source thread.
/// Dropping the lease removes only these hooks; disabled tiling adds no native hook.
pub struct TilingHooks;
impl TilingHooks {
    pub fn enable() -> Self { Self::processes(Vec::new()); Self }
    pub fn processes(processes: Vec<u32>) {
        *TILING.lock().unwrap() = Some(processes);
        refresh_hooks();
    }
}
impl Drop for TilingHooks {
    fn drop(&mut self) { *TILING.lock().unwrap() = None; refresh_hooks(); }
}
fn refresh_hooks() {
    SCOPES_DIRTY.store(true, Ordering::Relaxed);
    let thread = SOURCE.load(Ordering::Relaxed);
    if thread != 0 {
        // SAFETY: an existing source queue; this only requests hook reconciliation.
        let _ = unsafe { PostThreadMessageW(thread, WM_NULL, WPARAM(0), LPARAM(0)) };
    }
}

/// How long the hook thread lets events gather after taking a batch: at most this much later
/// the consumer hears of a window change, and a moving mouse wakes the hook thread at most
/// 1 s / `BATCH` times a second.
const BATCH: Duration = Duration::from_millis(25);

/// Subscribers call this when an event changed something the installing thread must act on
/// (it is woken once, after the batch).
pub fn notify() {
    NOTIFIED.store(true, Ordering::Relaxed);
}

/// Dropping a subscription removes it without touching another thread's native hooks.
pub struct Subscription(u64);
impl Subscription {
    /// `callback` runs on the hook thread, once per whole-window event.
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

/// Install once, on the thread that should hear about changes (it must pump messages: it gets
/// `WM_NULL` after a batch in which a subscriber called [`notify`]). Unhooks on drop.
pub struct WinEvents {
    thread: Option<std::thread::JoinHandle<()>>,
    thread_id: u32,
}
impl WinEvents {
    /// Active native hooks, callbacks delivered and current subscribers (live diagnostics).
    pub fn counts(&self) -> (usize, u64, usize) {
        (
            HOOKS.load(Ordering::Relaxed),
            DELIVERED.load(Ordering::Relaxed),
            listeners().lock().unwrap().len(),
        )
    }
    pub fn install() -> Self {
        // SAFETY: a read of the calling thread's id.
        CONSUMER.store(unsafe { GetCurrentThreadId() }, Ordering::Relaxed);
        SCOPES_DIRTY.store(true, Ordering::Relaxed);
        let (started, thread_id) = std::sync::mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("win-events".into())
            .stack_size(256 * 1024)
            .spawn(move || hook_thread(&started))
            .inspect_err(|e| log::warn!("window events unavailable: {e}"))
            .ok();
        Self {
            thread_id: thread.as_ref().and_then(|_| thread_id.recv().ok()).unwrap_or(0),
            thread,
        }
    }
}
impl Drop for WinEvents {
    fn drop(&mut self) {
        if let Some(thread) = self.thread.take() {
            // SAFETY: the hook thread's queue exists (made before it sent its id).
            let _ = unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0)) };
            let _ = thread.join();
        }
        CONSUMER.store(0, Ordering::Relaxed);
    }
}

/// The hook thread: hooks, sends its id once its queue exists, then takes events in batches
/// (re-scoping the location hooks when the foreground changed) until `WM_QUIT`, and unhooks.
fn hook_thread(started: &std::sync::mpsc::Sender<u32>) {
    let mut hooks: Vec<HWINEVENTHOOK> = [
        (EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND),
        (EVENT_SYSTEM_MINIMIZESTART, EVENT_SYSTEM_MINIMIZEEND),
        (EVENT_OBJECT_CREATE, EVENT_OBJECT_HIDE),
        (EVENT_OBJECT_NAMECHANGE, EVENT_OBJECT_NAMECHANGE),
        (EVENT_OBJECT_CLOAKED, EVENT_OBJECT_UNCLOAKED),
    ]
    .into_iter()
    .filter_map(|(first, last)| hook(first, last, 0))
    .collect();
    let mut scopes = Scopes::default();
    scopes.refresh();
    HOOKS.store(hooks.len() + scopes.locations.len()+scopes.moves.len(), Ordering::Relaxed);
    let mut msg = MSG::default();
    // SAFETY: messages of this thread's own queue; WM_QUIT ends the loop before unhooking.
    unsafe {
        let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);
        SOURCE.store(GetCurrentThreadId(), Ordering::Relaxed);
        let _ = started.send(GetCurrentThreadId());
        'run: loop {
            MsgWaitForMultipleObjectsEx(None, INFINITE, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_QUIT {
                    break 'run;
                }
                DispatchMessageW(&msg);
            }
            scopes.refresh();
            HOOKS.store(hooks.len() + scopes.locations.len()+scopes.moves.len(), Ordering::Relaxed);
            if NOTIFIED.swap(false, Ordering::Relaxed) {
                let consumer = CONSUMER.load(Ordering::Relaxed);
                if consumer != 0 {
                    let _ = PostThreadMessageW(consumer, WM_NULL, WPARAM(0), LPARAM(0));
                }
            }
            std::thread::sleep(BATCH);
        }
    }
    unhook(&mut hooks);
    unhook(&mut scopes.locations);
    unhook(&mut scopes.moves);
    SOURCE.store(0, Ordering::Relaxed);
}

/// The location hooks, scoped to the foreground app's and Explorer's processes.
#[derive(Default)]
struct Scopes {
    locations: Vec<HWINEVENTHOOK>,
    processes: Vec<u32>,
    moves: Vec<HWINEVENTHOOK>,
}
impl Scopes {
    fn refresh(&mut self) {
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
        let tiling = TILING.lock().unwrap().clone();
        if let Some(extra) = &tiling { processes.extend(extra); }
        if tiling.is_some() && self.moves.is_empty() {
            self.moves.extend(hook(EVENT_SYSTEM_MOVESIZESTART, EVENT_SYSTEM_MOVESIZEEND, 0));
        } else if tiling.is_none() { unhook(&mut self.moves); }
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
fn hook(first: u32, last: u32, pid: u32) -> Option<HWINEVENTHOOK> {
    // SAFETY: static callback; hooks are owned and dropped on the hook thread.
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
