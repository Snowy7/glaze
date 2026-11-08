//! Visible built-in widgets lease the same providers as bars and panels.
use crate::widgets::model::{Model as WidgetModel, Widget};
use glaze_integrations::{
    Availability, Lease, Provider, Snapshot,
};
use glaze_ui::HitId;

pub const CONNECT: HitId = 6000;
pub const REFRESH: HitId = 6001;
pub const PRIMARY: HitId = 6002;
pub const SECONDARY: HitId = 6003;
pub const ITEM: HitId = 6400;

pub struct Model {
    pub calendar: super::calendar::Model,
    pub custom: super::custom::Model,
    pub(super) lease: Option<Lease>,
    pub(super) generation: Option<u64>,
    pub snapshot: Snapshot,
    pub(super) task: Option<super::Task<()>>,
    pub notice: Option<String>,
    pub observed_at_ms: i64,
}
impl Default for Model {
    fn default() -> Self {
        Self {
            calendar: Default::default(),
            custom: Default::default(),
            lease: None,
            generation: None,
            snapshot: Snapshot {
                availability: Availability::Unavailable(
                    "Connect this integration in Settings".into(),
                ),
                ..Default::default()
            },
            task: None,
            notice: None,
            observed_at_ms: 0,
        }
    }
}
impl Model {
    pub fn busy(&self) -> bool {
        self.task.is_some()
    }
    pub fn actionable(&self) -> bool {
        matches!(
            self.snapshot.availability,
            Availability::Live | Availability::Cached
        )
    }
}

pub fn provider(kind: Widget) -> Option<Provider> {
    match kind {
        Widget::Calendar => Some(Provider::Calendar),
        _ => None,
    }
}

/// Runs on provider/UI wakes, never installs a deadline. Hidden/held instances release demand.
pub fn sync(widget: &mut WidgetModel, active: bool) {
    if widget.kind == Widget::Custom {
        super::custom::sync(widget, active);
        return;
    }
    if provider(widget.kind).is_none() {
        return;
    }
    let state = &mut widget.integration;
    if !active {
        state.lease = None;
        state.generation = None;
        return;
    }
    let generation = super::generation();
    if state.generation != Some(generation) {
        state.calendar.invalidate();
        state.lease = None;
        state.generation = Some(generation);
        if let Some(provider) = provider(widget.kind) {
            state.snapshot = match super::acquire(provider) {
                Ok(lease) => {
                    state.lease = Some(lease);
                    Snapshot::default()
                }
                Err(_) => Snapshot {
                    availability: Availability::Unavailable("Connect this integration in Settings".into()),
                    ..Default::default()
                },
            };
        }
        widget.dirty = true;
    }
    if let Some(lease) = &state.lease {
        let snapshot = lease.snapshot();
        if snapshot != state.snapshot {
            state.observed_at_ms = crate::widgets::time::epoch_ms();
            state.snapshot = snapshot;
            widget.dirty = true;
        }
    }
    if let Some(result) = state.task.as_ref().and_then(super::Task::take) {
        state.notice = result.err();
        state.task = None;
        widget.dirty = true;
    }
    if widget.kind == Widget::Calendar {
        super::calendar::sync(widget);

    }
}

/// Explicit hits only; verification never invokes vendor actions.
pub fn action(widget: &mut WidgetModel, hit: HitId) -> bool {
    if widget.kind == Widget::Custom { return super::custom::action(widget, hit); }
    if widget.kind == Widget::Calendar { return super::calendar::action(widget, hit); }
    if provider(widget.kind).is_none() {
        return false;
    }
    let state = &mut widget.integration;
    state.notice = None;
    if hit == CONNECT {
        // Studio owns credential entry and consent. A widget never places a token in settings.
        crate::apps::open_studio();
        return true;
    }
    if hit == REFRESH {
        if let Some(lease) = &state.lease {
            lease.retry();
        } else {
            state.generation = None;
        }
        return true;
    }
    false
}
