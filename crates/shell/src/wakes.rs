//! Counts the shell loop's wake-ups by cause and logs them once a minute, on a wake (it never
//! adds one). Quiet unless the loop's work ran more than 240 times a second on average (a stuck
//! ticker, say); `MYPAPER_WAKES=1`, or a file `%LOCALAPPDATA%\mypaper\wakes.on` (looked for once
//! a minute, so it survives restarts and can be switched live), logs every minute. Counting is a
//! few integer bumps per wake.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use mypaper_shell::cycles::{self, thread as thread_cycles};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{GetClassNameW, MSG, WM_TIMER};

const PERIOD: Duration = Duration::from_secs(60);
const AUTO_REPORT_RATE: f32 = 240.0;

/// Why the loop woke.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum Cause {
    /// A waited handle was signalled (its name).
    Handle(&'static str),
    /// A deadline passed (whose).
    Timeout(&'static str),
    /// A posted message (counted by message and window below).
    Message,
    /// Input was available but no posted message came out: sent messages or hook callbacks,
    /// run inside `PeekMessageW`.
    Sent,
}

pub struct Wakes {
    since: Instant,
    wakes: u32,
    /// Wakes that ran the loop's work (not coalesced).
    worked: u32,
    causes: HashMap<Cause, u32>,
    /// Posted messages by (message, window, timer id).
    messages: HashMap<(u32, isize, usize), u32>,
    /// Time spent in each part of the loop's work (wall clock: a part may block, e.g. on the
    /// engine's glass placement).
    spent: HashMap<&'static str, Duration>,
    /// The UI thread's CPU cycles in each part, while reporting is on.
    cycles: HashMap<&'static str, u64>,
    /// The thread's cycle count at the last [`resume`](Self::resume) or [`spent`](Self::spent).
    mark: u64,
    /// The thread's cycle count when the period began (its total includes the waits).
    period_cycles: u64,
    always: bool,
    /// Reporting every period (`always` or the file switch, as of the last period).
    on: bool,
}

impl Wakes {
    pub fn new() -> Self {
        let always = std::env::var_os("MYPAPER_WAKES").is_some();
        let on = always || switched_on();
        cycles::set_counting(on);
        Self {
            since: Instant::now(),
            wakes: 0,
            worked: 0,
            causes: HashMap::new(),
            messages: HashMap::new(),
            spent: HashMap::new(),
            cycles: HashMap::new(),
            mark: 0,
            period_cycles: if on { thread_cycles() } else { 0 },
            always,
            on,
        }
    }

    /// Starts counting cycles for the next part (after the loop's wait).
    pub fn resume(&mut self) {
        if self.on {
            self.mark = thread_cycles();
        }
    }

    /// Adds the time since `start` to `part`, and returns now (the next part's start).
    pub fn spent(&mut self, part: &'static str, start: Instant) -> Instant {
        let now = Instant::now();
        *self.spent.entry(part).or_default() += now - start;
        if self.on {
            let cycles = thread_cycles();
            *self.cycles.entry(part).or_default() += cycles.saturating_sub(self.mark);
            self.mark = cycles;
        }
        now
    }

    pub fn wake(&mut self, cause: Cause) {
        self.wakes += 1;
        *self.causes.entry(cause).or_default() += 1;
    }

    /// This wake runs the loop's work.
    pub fn worked(&mut self) {
        self.worked += 1;
    }

    pub fn message(&mut self, msg: &MSG) {
        let timer = if msg.message == WM_TIMER { msg.wParam.0 } else { 0 };
        *self
            .messages
            .entry((msg.message, msg.hwnd.0 as isize, timer))
            .or_default() += 1;
    }

    /// Logs and resets once a period has passed; returns whether external counters should reset.
    pub fn report(&mut self, detail: impl FnOnce() -> String) -> bool {
        let elapsed = self.since.elapsed();
        if elapsed < PERIOD {
            return false;
        }
        let was_on = self.on;
        self.on = self.always || switched_on();
        cycles::set_counting(self.on);
        if self.on || self.worked as f32 > AUTO_REPORT_RATE * elapsed.as_secs_f32() {
            let mut causes: Vec<_> = self
                .causes
                .iter()
                .map(|(cause, n)| {
                    let name = match cause {
                        Cause::Handle(h) => format!("handle {h}"),
                        Cause::Timeout(t) => format!("timeout {t}"),
                        Cause::Message => "posted".into(),
                        Cause::Sent => "sent/hook".into(),
                    };
                    (*n, name)
                })
                .collect();
            let mut messages: HashMap<String, u32> = HashMap::new();
            for (&(message, hwnd, timer), &n) in &self.messages {
                let mut key = format!("msg 0x{message:04x} {}", class(hwnd));
                if message == WM_TIMER {
                    key += &format!(" timer {timer}");
                }
                *messages.entry(key).or_default() += n;
            }
            causes.extend(messages.into_iter().map(|(k, n)| (n, k)));
            causes.sort_unstable_by(|a, b| b.0.cmp(&a.0));
            let top: Vec<_> = causes
                .iter()
                .take(16)
                .map(|(n, name)| format!("{name}: {n}"))
                .collect();
            let mut spent: Vec<_> = self.spent.iter().collect();
            spent.sort_unstable_by(|a, b| b.1.cmp(a.1));
            let spent: Vec<_> = spent
                .iter()
                .map(|(part, d)| format!("{part} {:.1} ms", d.as_secs_f64() * 1e3))
                .collect();
            // CPU per part in millions of cycles (about 4 per ms here); the thread's total
            // includes the waits and what runs between the parts.
            let cpu = if was_on {
                let mut cycles: Vec<_> = self.cycles.iter().collect();
                cycles.sort_unstable_by(|a, b| b.1.cmp(a.1));
                let parts: Vec<_> = cycles
                    .iter()
                    .map(|(part, c)| format!("{part} {:.1}", **c as f64 / 1e6))
                    .collect();
                format!(
                    "; cpu Mcycles: thread {:.1}, {}",
                    thread_cycles().saturating_sub(self.period_cycles) as f64 / 1e6,
                    parts.join(", ")
                )
            } else {
                String::new()
            };
            log::info!(
                "wakes in {:.0} s: {}, {} ran the work ({}); work: {}{cpu}; {}",
                elapsed.as_secs_f32(),
                self.wakes,
                self.worked,
                top.join(", "),
                spent.join(", "),
                detail()
            );
        }
        self.since = Instant::now();
        self.wakes = 0;
        self.worked = 0;
        self.causes.clear();
        self.messages.clear();
        self.spent.clear();
        self.cycles.clear();
        if self.on {
            self.period_cycles = thread_cycles();
            self.mark = self.period_cycles;
        }
        true
    }
}

/// Whether `%LOCALAPPDATA%\mypaper\wakes.on` exists.
fn switched_on() -> bool {
    std::env::var_os("LOCALAPPDATA")
        .is_some_and(|dir| std::path::Path::new(&dir).join("mypaper").join("wakes.on").exists())
}

/// A window's class, `thread` for thread messages, `?` once it is gone.
fn class(hwnd: isize) -> String {
    if hwnd == 0 {
        return "thread".into();
    }
    let mut name = [0u16; 64];
    // SAFETY: a stale handle fails harmlessly; the buffer outlives the call.
    let len = unsafe { GetClassNameW(HWND(hwnd as _), &mut name) };
    if len == 0 {
        "?".into()
    } else {
        String::from_utf16_lossy(&name[..len as usize])
    }
}
