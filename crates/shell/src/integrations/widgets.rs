//! Visible built-in widgets lease the same providers as bars and panels.
use crate::widgets::model::{Model as WidgetModel, Widget};
use mypaper_integrations::{
    Action, Availability, Data, Lease, Provider, Snapshot, launcher,
};
use mypaper_ui::HitId;

pub const CONNECT: HitId = 6000;
pub const REFRESH: HitId = 6001;
pub const PRIMARY: HitId = 6002;
pub const SECONDARY: HitId = 6003;
pub const ITEM: HitId = 6400;

pub struct Model {
    pub t3: super::t3_widgets::State,
    pub calendar: super::calendar::Model,
    pub discord: super::discord::Model,
    pub custom: super::custom::Model,
    pub(super) lease: Option<Lease>,
    pub(super) generation: Option<u64>,
    pub snapshot: Snapshot,
    pub(super) task: Option<super::Task<()>>,
    pub notice: Option<String>,
    pub thread_links: std::collections::BTreeMap<String, String>,
    pub observed_at_ms: i64,
}
impl Default for Model {
    fn default() -> Self {
        Self {
            t3: Default::default(),
            calendar: Default::default(),
            discord: Default::default(),
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
            thread_links: Default::default(),
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
        Widget::Discord => Some(Provider::Discord),
        Widget::Calendar => Some(Provider::Calendar),
        Widget::T3 => Some(Provider::T3),
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
        state.discord.notifications = None;
        state.lease = None;
        state.generation = None;
        return;
    }
    let generation = super::generation();
    if state.generation != Some(generation) {
        state.discord.notifications = None;
        state.calendar.invalidate();
        state.lease = None;
        state.generation = Some(generation);
        let config = super::config();
        state.thread_links = config.t3.thread_links.clone();
        if let Some(provider) = provider(widget.kind) {
            state.snapshot = match super::acquire(provider) {
                Ok(lease) => {
                    state.lease = Some(lease);
                    Snapshot::default()
                }
                // T3 switches itself on: its CLI can grant access without a prompt.
                Err(_) if widget.kind == Widget::T3 && mypaper_integrations::t3_auth::available() => {
                    super::t3_widgets::connect(state, None);
                    Snapshot::default()
                }
                Err(_) => Snapshot {
                    availability: Availability::Unavailable(
                        if widget.kind == Widget::T3 {
                            "T3 Code is not running"
                        } else {
                            "Connect this integration in Settings"
                        }
                        .into(),
                    ),
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
        if widget.kind == Widget::T3
            && result.is_ok()
            && let Some(lease) = &state.lease
            && !matches!(state.snapshot.data, Some(Data::T3(_)))
        {
            // Access was just granted; reconnect now rather than on the next refresh.
            lease.retry();
        }
        if widget.kind == Widget::T3 && state.t3.submitted {
            if result.is_ok() {
                state.t3.draft.clear();
                if let Some(field) = &widget.field {let _ = field.set_text("");}
            }
            state.t3.submitted = false;
        }
        state.notice = result.err();
        state.task = None;
        widget.dirty = true;
    }
    if widget.kind == Widget::Calendar {
        super::calendar::sync(widget);
    } else if widget.kind == Widget::Discord {
        super::discord::sync(widget);
    }
    let state = &mut widget.integration;
}

pub(super) fn open(target: &str) -> anyhow::Result<()> {
    launcher::open(&launcher::Recent {
        title: String::new(),
        target: target.into(),
        arguments: String::new(),
    })
}

/// Explicit hits only; verification never invokes vendor actions.
pub fn action(widget: &mut WidgetModel, hit: HitId) -> bool {
    if widget.kind == Widget::T3 && hit != REFRESH {return super::t3_widgets::action(widget, hit);}
    if widget.kind == Widget::Custom { return super::custom::action(widget, hit); }
    if widget.kind == Widget::Calendar { return super::calendar::action(widget, hit); }
    if widget.kind == Widget::Discord { return super::discord::action(widget, hit); }
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
    let result = match (&state.snapshot.data, widget.kind) {
        (_, Widget::Discord) if hit == PRIMARY || (ITEM..ITEM + 512).contains(&hit) => {
            open("https://discord.com/channels/@me")
        }
        _ => Ok(()),
    };
    state.notice = result.err().map(|error| error.to_string());
    true
}
