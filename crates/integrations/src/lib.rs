//! One connection per provider, reference counted across every visible shell surface.
//! Workers sleep in IO/completion ports; disconnections await a real retry, never a timer.
pub mod config;
pub mod calendar;
pub mod calendar_http;
pub mod calendar_oauth;
pub mod calendar_setup;
pub mod discord;
pub mod credentials;
pub mod launcher;
pub mod media;
pub mod notifications;
pub mod pairing;
pub mod t3;
pub mod t3_auth;
pub mod t3_client;
mod transport;

use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicU64, Ordering},
    },
    thread::JoinHandle,
};
use tokio::sync::{mpsc, watch};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Provider {
    Notifications,
    Media,
    T3,
    Calendar,
    Discord,
}
impl Provider {
    pub const ALL: [Self; 5] = [
        Self::Notifications,
        Self::Media,
        Self::T3,
        Self::Calendar,
        Self::Discord,
    ];
    pub fn enabled(self, c: &config::Config) -> bool {
        match self {
            Self::Notifications => c.notifications.enabled,
            Self::Media => c.media.enabled,
            Self::T3 => c.t3.enabled,
            Self::Calendar => c.calendar.enabled,
            Self::Discord => c.discord.enabled,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Availability {
    Connecting,
    Live,
    Cached,
    Stale(String),
    Unavailable(String),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Data {
    Notifications(notifications::Snapshot),
    Media(media::Snapshot),
    T3(t3::Snapshot),
    Calendar(calendar::Snapshot),
    Discord(discord::Snapshot),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub revision: u64,
    pub availability: Availability,
    pub data: Option<Data>,
    pub action_error: Option<String>,
}
impl Default for Snapshot {
    fn default() -> Self {
        Self {
            revision: 0,
            availability: Availability::Connecting,
            data: None,
            action_error: None,
        }
    }
}

#[derive(Clone, Debug)]
pub enum Action {
    DiscordVoice { muted: Option<bool>, deafened: Option<bool> },
    Dismiss(Vec<u32>),
    /// Refresh a cached toast snapshot while its center is visible.
    RefreshNotifications,
    Media(media::Action),
}

#[derive(Default)]
struct Counters {
    attempts: AtomicU64,
    deliveries: AtomicU64,
}
#[derive(Clone, Copy, Debug)]
pub struct Activity {
    pub attempts: u64,
    pub deliveries: u64,
}
struct Slot {
    users: usize,
    control: watch::Sender<u64>,
    actions: mpsc::Sender<Action>,
    worker: JoinHandle<()>,
    snapshot: Arc<Mutex<Snapshot>>,
    counters: Arc<Counters>,
}
struct Inner {
    slots: Mutex<BTreeMap<Provider, Slot>>,
    wake: Arc<dyn Fn(Provider) + Send + Sync>,
}
#[derive(Clone)]
pub struct Hub(Arc<Inner>);

/// Cloning a lease adds a consumer; dropping the last consumer cancels and joins the lane.
pub struct Lease {
    hub: Weak<Inner>,
    provider: Provider,
    snapshot: Arc<Mutex<Snapshot>>,
    counters: Arc<Counters>,
}

#[derive(Clone)]
pub(crate) struct Publish {
    provider: Provider,
    snapshot: Arc<Mutex<Snapshot>>,
    wake: Arc<dyn Fn(Provider) + Send + Sync>,
    counters: Arc<Counters>,
}
impl Publish {
    pub fn delivered(&self) {
        self.counters.deliveries.fetch_add(1, Ordering::Relaxed);
    }
    pub fn set(&self, availability: Availability, data: Option<Data>) {
        let mut current = self.snapshot.lock().unwrap();
        if current.availability == availability && current.data == data {
            return;
        }
        current.availability = availability;
        current.data = data;
        current.revision += 1;
        drop(current);
        (self.wake)(self.provider);
    }
    pub fn error(&self, error: &str) {
        let data = self.snapshot.lock().unwrap().data.clone();
        self.set(
            if data.is_some() {
                Availability::Stale(error.into())
            } else {
                Availability::Unavailable(error.into())
            },
            data,
        );
    }
    pub fn action_result(&self, result: anyhow::Result<()>) {
        let error = result.err().map(|e| e.to_string());
        let mut state = self.snapshot.lock().unwrap();
        if state.action_error == error {
            return;
        }
        state.action_error = error;
        state.revision += 1;
        drop(state);
        (self.wake)(self.provider);
    }
}

impl Hub {
    pub fn new(wake: impl Fn(Provider) + Send + Sync + 'static) -> Self {
        Self(Arc::new(Inner {
            slots: Mutex::new(BTreeMap::new()),
            wake: Arc::new(wake),
        }))
    }
    /// Immediately release every subscription on suspend or a settings change. Old leases become inert.
    pub fn stop_all(&self) {
        let slots = std::mem::take(&mut *self.0.slots.lock().unwrap());
        for (provider, slot) in slots {
            let data = slot.snapshot.lock().unwrap().data.clone();
            let mut snapshot = slot.snapshot.lock().unwrap();
            snapshot.availability = Availability::Unavailable("subscription stopped".into());
            snapshot.data = data;
            snapshot.revision += 1;
            drop(snapshot);
            (self.0.wake)(provider);
            let _ = slot.control.send(u64::MAX);
            let _ = slot.worker.join();
        }
    }
    pub fn acquire(&self, provider: Provider, config: &config::Config) -> anyhow::Result<Lease> {
        anyhow::ensure!(provider.enabled(config), "integration disabled");
        let mut slots = self.0.slots.lock().unwrap();
        if let Some(slot) = slots.get_mut(&provider) {
            slot.users += 1;
            return Ok(Lease {
                hub: Arc::downgrade(&self.0),
                provider,
                snapshot: slot.snapshot.clone(),
                counters: slot.counters.clone(),
            });
        }
        let snapshot = Arc::new(Mutex::new(Snapshot::default()));
        let counters = Arc::new(Counters::default());
        let publish = Publish {
            provider,
            snapshot: snapshot.clone(),
            wake: self.0.wake.clone(),
            counters: counters.clone(),
        };
        let (control, mut signal) = watch::channel(0_u64);
        let (actions, mut commands) = mpsc::channel(16);
        let config = config.clone();
        let worker = std::thread::Builder::new().name(format!("integration-{provider:?}")).stack_size(512 * 1024).spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                Ok(runtime) => runtime, Err(e) => { publish.error(&e.to_string()); return; }
            };
            runtime.block_on(async {
                loop {
                    while commands.try_recv().is_ok() {}
                    let retained = publish.snapshot.lock().unwrap().data.clone();
                    publish.set(Availability::Connecting,retained);
                    publish.counters.attempts.fetch_add(1,Ordering::Relaxed);
                    let work = async {
                        match provider {
                            Provider::Notifications => notifications::run(&config.notifications, &publish, &mut commands).await,
                            Provider::Media => media::run(&publish, &mut commands).await,
                            Provider::T3 => t3::run(&config.t3, &publish).await,
                            Provider::Calendar => calendar::run(&config.calendar, &publish, &mut commands).await,
                            Provider::Discord => discord::run(&config.discord, &publish, &mut commands).await,
                        }
                    };
                    tokio::select! {
                        biased;
                        _ = signal.changed() => { if *signal.borrow_and_update() == u64::MAX { break; } continue; }
                        result = work => {
                            if let Err(e) = result { publish.error(&e.to_string()); }
                        }
                    }
                    if signal.changed().await.is_err() || *signal.borrow_and_update() == u64::MAX { break; }
                }
            });
        })?;
        slots.insert(
            provider,
            Slot {
                users: 1,
                control,
                actions,
                worker,
                snapshot: snapshot.clone(),
                counters: counters.clone(),
            },
        );
        Ok(Lease {
            hub: Arc::downgrade(&self.0),
            provider,
            snapshot,
            counters,
        })
    }
}
impl Lease {
    pub fn activity(&self) -> Activity {
        Activity {
            attempts: self.counters.attempts.load(Ordering::Relaxed),
            deliveries: self.counters.deliveries.load(Ordering::Relaxed),
        }
    }
    pub fn snapshot(&self) -> Snapshot {
        self.snapshot.lock().unwrap().clone()
    }
    pub fn retry(&self) {
        if let Some(hub) = self.hub.upgrade()
            && let Some(slot) = hub.slots.lock().unwrap().get(&self.provider)
            && Arc::ptr_eq(&slot.snapshot, &self.snapshot)
        {
            slot.control.send_modify(|v| *v = v.wrapping_add(1));
        }
    }
    pub fn action(&self, action: Action) -> anyhow::Result<()> {
        anyhow::ensure!(
            matches!(
                (self.provider, &action),
                (
                    Provider::Notifications,
                    Action::Dismiss(_) | Action::RefreshNotifications
                ) | (Provider::Media, Action::Media(_))
                    | (Provider::Discord, Action::DiscordVoice { .. })
            ),
            "action does not belong to this provider"
        );
        let hub = self
            .hub
            .upgrade()
            .ok_or_else(|| anyhow::anyhow!("integration stopped"))?;
        let slots = hub.slots.lock().unwrap();
        let slot = slots
            .get(&self.provider)
            .ok_or_else(|| anyhow::anyhow!("integration stopped"))?;
        anyhow::ensure!(
            Arc::ptr_eq(&slot.snapshot, &self.snapshot),
            "integration settings changed; reopen the panel"
        );
        anyhow::ensure!(
            matches!(
                self.snapshot().availability,
                Availability::Live | Availability::Cached
            ),
            "integration is not connected"
        );
        slot.actions
            .try_send(action)
            .map_err(|_| anyhow::anyhow!("integration busy or disconnected"))
    }
}
impl Clone for Lease {
    fn clone(&self) -> Self {
        if let Some(hub) = self.hub.upgrade()
            && let Some(slot) = hub.slots.lock().unwrap().get_mut(&self.provider)
            && Arc::ptr_eq(&slot.snapshot, &self.snapshot)
        {
            slot.users += 1;
        }
        Self {
            hub: self.hub.clone(),
            provider: self.provider,
            snapshot: self.snapshot.clone(),
            counters: self.counters.clone(),
        }
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        let Some(hub) = self.hub.upgrade() else {
            return;
        };
        let slot = {
            let mut slots = hub.slots.lock().unwrap();
            let Some(slot) = slots.get_mut(&self.provider) else {
                return;
            };
            if !Arc::ptr_eq(&slot.snapshot, &self.snapshot) {
                return;
            }
            slot.users -= 1;
            if slot.users != 0 {
                return;
            }
            slots.remove(&self.provider).unwrap()
        };
        let _ = slot.control.send(u64::MAX);
        let _ = slot.worker.join();
    }
}
impl Drop for Inner {
    fn drop(&mut self) {
        for (_, slot) in std::mem::take(self.slots.get_mut().unwrap()) {
            let _ = slot.control.send(u64::MAX);
            let _ = slot.worker.join();
        }
    }
}
