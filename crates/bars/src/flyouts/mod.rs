//! On-demand Windows providers, exposed as plain Rust calls for native flyouts.
pub mod clipboard;
pub mod index;
pub mod language;
pub mod mixer;
pub mod notifications;
pub mod quick;
pub mod wifi;
pub mod winkey;
use crate::actions::Topic;
use std::sync::mpsc::{self, Sender};
use std::time::{Duration, Instant};
pub const IDLE: Duration = Duration::from_secs(60);
pub enum Done {
    WinKey(Instant, winkey::Target),
    Changed(Watched),
    NotificationCount(Option<usize>),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Watched {
    Core(Topic),
    Apps,
    Mixer,
    Wifi,
}

/// A unit of work for a lane.
type Job<S> = Box<dyn FnOnce(&mut S) + Send>;

/// One provider's thread: jobs go in; results come back through the waker. Dropping it ends the
/// thread (and drops its state there, in its own COM apartment).
pub struct Lane<S> {
    tx: Sender<Job<S>>,
    pub used: Instant,
}

impl<S: 'static> Lane<S> {
    /// Starts a lane; `init` runs on its thread and makes its state.
    pub fn spawn(name: &str, init: impl FnOnce() -> S + Send + 'static) -> Option<Self> {
        let (tx, rx) = mpsc::channel::<Job<S>>();
        let spawned = std::thread::Builder::new()
            .name(name.to_owned())
            .spawn(move || {
                let mut state = init();
                for job in rx {
                    job(&mut state);
                }
            });
        spawned
            .inspect_err(|e| log::warn!("no {name} thread: {e}"))
            .ok()
            .map(|_| Self {
                tx,
                used: Instant::now(),
            })
    }

    pub fn run(&mut self, job: impl FnOnce(&mut S) + Send + 'static) {
        self.used = Instant::now();
        let _ = self.tx.send(Box::new(job));
    }

    pub fn idle(&self, now: Instant) -> bool {
        now.duration_since(self.used) >= IDLE
    }
}

/// Runs `job` on the lane in `slot`, starting it if needed.
pub fn on_lane<S: 'static>(
    slot: &mut Option<Lane<S>>,
    name: &str,
    init: impl FnOnce() -> S + Send + 'static,
    job: impl FnOnce(&mut S) + Send + 'static,
) {
    if slot.is_none() {
        *slot = Lane::spawn(name, init);
    }
    if let Some(lane) = slot {
        lane.run(job);
    }
}

/// Drops `slot`'s lane when it's been idle long enough. A `kept` one (something it feeds is
/// watched, or it's busy) counts as used now, so it goes [`IDLE`] after the last watch ends, and
/// its release time (the loop's deadline) never lies in the past.
pub fn release_idle<S: 'static>(slot: &mut Option<Lane<S>>, now: Instant, kept: bool) {
    if kept {
        if let Some(lane) = slot {
            lane.used = now;
        }
    } else if slot.as_ref().is_some_and(|lane| lane.idle(now)) {
        *slot = None;
    }
}
