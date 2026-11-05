//! Compatibility at the native registries: both existing surfaces lease the SAME SMTC provider.
use mypaper_integrations::{Action, Data, Lease, Provider, media::Action as Control};
use mypaper_widgets::bridge::{ErrorCode, MediaAction, Out, Target, WidgetError};
use serde_json::{Value, json};
use std::{
    cell::RefCell,
    collections::BTreeSet,
    rc::{Rc, Weak},
};

thread_local! {
    static BARS: RefCell<Vec<Weak<RefCell<BarState>>>> = const { RefCell::new(Vec::new()) };
    static WIDGETS: RefCell<Vec<Weak<RefCell<WidgetState>>>> = const { RefCell::new(Vec::new()) };
}
struct BarState {
    lease: Option<Lease>,
    waker: mypaper_bars::wake::Waker,
    revision: u64,
    active: bool,
}
pub struct BarMedia(Rc<RefCell<BarState>>);
impl BarMedia {
    pub fn new(waker: mypaper_bars::wake::Waker) -> Self {
        let state = Rc::new(RefCell::new(BarState {
            lease: None,
            waker,
            revision: u64::MAX,
            active: false,
        }));
        BARS.with_borrow_mut(|bars| bars.push(Rc::downgrade(&state)));
        Self(state)
    }
    pub fn set_active(&mut self, active: bool) {
        let mut state = self.0.borrow_mut();
        state.active = active;
        if active {
            if state.lease.is_none() {
                state.lease = super::acquire(Provider::Media).ok();
                state.revision = u64::MAX;
            }
        } else {
            state.lease = None;
            state.waker.send(mypaper_bars::wake::Msg::Media(None));
        }
        bar_changed(&mut state);
    }
    pub fn control(&self, action: mypaper_bars::actions::MediaAction) {
        if let Some(lease) = &self.0.borrow().lease {
            let _ = lease.action(Action::Media(match action {
                mypaper_bars::actions::MediaAction::Toggle => Control::Toggle,
                mypaper_bars::actions::MediaAction::Next => Control::Next,
                mypaper_bars::actions::MediaAction::Previous => Control::Previous,
            }));
        }
    }
}
fn bar_changed(state: &mut BarState) {
    let Some(snapshot) = state.lease.as_ref().map(Lease::snapshot) else {
        if state.revision != 0 {
            state.revision = 0;
            state.waker.send(mypaper_bars::wake::Msg::Media(None));
        }
        return;
    };
    if snapshot.revision == state.revision {
        return;
    }
    state.revision = snapshot.revision;
    let media = match snapshot.data {
        Some(Data::Media(s)) if !s.title.is_empty() || !s.artist.is_empty() => {
            Some(mypaper_bars::media::MediaInfo {
                title: s.title,
                artist: s.artist,
                app: s.app_id,
                playing: s.playing,
            })
        }
        _ => None,
    };
    state.waker.send(mypaper_bars::wake::Msg::Media(media));
}

struct WidgetState {
    lease: Option<Lease>,
    waker: mypaper_widgets::wake::Waker,
    revision: u64,
    subs: BTreeSet<(String, String)>,
    held: bool,
}
pub struct WidgetMedia(Rc<RefCell<WidgetState>>);
impl WidgetMedia {
    pub fn new(waker: mypaper_widgets::wake::Waker) -> Self {
        let state = Rc::new(RefCell::new(WidgetState {
            lease: None,
            waker,
            revision: u64::MAX,
            subs: BTreeSet::new(),
            held: false,
        }));
        WIDGETS.with_borrow_mut(|items| items.push(Rc::downgrade(&state)));
        Self(state)
    }
    pub fn subscribe(&mut self, instance: &str, sub: &str) {
        let mut state = self.0.borrow_mut();
        state.subs.insert((instance.into(), sub.into()));
        widget_lease(&mut state);
    }
    pub fn forget(&mut self, instance: &str) {
        let mut state = self.0.borrow_mut();
        state.subs.retain(|(id, _)| id != instance);
        widget_lease(&mut state);
    }
    pub fn set_held(&mut self, held: bool) {
        let mut state = self.0.borrow_mut();
        state.held = held;
        widget_lease(&mut state);
    }
    pub fn current(&mut self, target: Target) {
        let state = self.0.borrow();
        state.waker.send(mypaper_widgets::wake::Msg::Answered(
            target,
            Ok(widget_value(state.lease.as_ref())),
        ));
    }
    pub fn control(&mut self, target: Target, action: MediaAction) {
        let state = self.0.borrow();
        let result = state
            .lease
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("media integration disabled"))
            .and_then(|lease| {
                let control = match action {
                    MediaAction::Next => Control::Next,
                    MediaAction::Previous => Control::Previous,
                    MediaAction::Toggle => Control::Toggle,
                    MediaAction::Play => Control::Play,
                    MediaAction::Pause => Control::Pause,
                };
                lease.action(Action::Media(control))
            })
            .map(|_| widget_value(state.lease.as_ref()))
            .map_err(|e| WidgetError::new(ErrorCode::Internal, e.to_string()));
        state
            .waker
            .send(mypaper_widgets::wake::Msg::Answered(target, result));
    }
    pub fn on_result(&mut self, _: Option<u64>, result: Result<Value, String>) -> Vec<Out> {
        let state = self.0.borrow();
        match result {
            Ok(value) if !state.held => state
                .subs
                .iter()
                .map(|(instance, sub)| Out::Event {
                    instance: instance.clone(),
                    event: "sub",
                    data: json!({"sub":sub,"value":value}),
                })
                .collect(),
            _ => Vec::new(),
        }
    }
}
fn widget_value(lease: Option<&Lease>) -> Value {
    match lease.map(Lease::snapshot).and_then(|s| s.data) {
        Some(Data::Media(s)) if !s.title.is_empty() || !s.artist.is_empty() => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as i64;
            widget_data(s, now)
        }
        _ => Value::Null,
    }
}
fn widget_data(s: mypaper_integrations::media::Snapshot, now: i64) -> Value {
    let advance = if s.playing {
        s.position_at_ms
            .map_or(0, |at| now.saturating_sub(at).max(0))
    } else {
        0
    };
    let position = s.position_ms.map(|position| {
        position
            .saturating_add(advance)
            .min(s.duration_ms.unwrap_or(i64::MAX)) as f64
            / 1000.0
    });
    let duration = s.duration_ms.map(|duration| duration as f64 / 1000.0);
    json!({"title":s.title,"artist":s.artist,"album":s.album,"app":s.app_id,"playing":s.playing,"artwork":s.artwork,
        "position":position,"duration":duration,"_at":now})
}

#[cfg(test)]
#[test]
fn widget_timeline_interpolates_playback_and_preserves_paused_and_live_states() {
    let mut snapshot = mypaper_integrations::media::Snapshot {
        position_ms: Some(12_000),
        duration_ms: Some(60_000),
        position_at_ms: Some(100_000),
        playing: true,
        ..Default::default()
    };
    assert_eq!(
        widget_data(snapshot.clone(), 103_000)["position"],
        json!(15.0)
    );
    assert_eq!(
        widget_data(snapshot.clone(), 200_000)["position"],
        json!(60.0)
    );
    snapshot.playing = false;
    assert_eq!(widget_data(snapshot, 103_000)["position"], json!(12.0));
    let live = widget_data(Default::default(), 103_000);
    assert!(live["duration"].is_null() && live["position"].is_null());
}
fn widget_lease(state: &mut WidgetState) {
    if state.held || state.subs.is_empty() {
        state.lease = None;
    } else if state.lease.is_none() {
        state.lease = super::acquire(Provider::Media).ok();
        state.revision = u64::MAX;
    }
    widget_changed(state);
}
fn widget_changed(state: &mut WidgetState) {
    let Some(snapshot) = state.lease.as_ref().map(Lease::snapshot) else {
        if state.revision != 0 {
            state.revision = 0;
            state.waker.send(mypaper_widgets::wake::Msg::Media {
                token: None,
                result: Ok(Value::Null),
            });
        }
        return;
    };
    if state.revision == snapshot.revision {
        return;
    }
    state.revision = snapshot.revision;
    state.waker.send(mypaper_widgets::wake::Msg::Media {
        token: None,
        result: Ok(widget_value(state.lease.as_ref())),
    });
}
pub fn changed(reload: bool) {
    BARS.with_borrow_mut(|items| {
        items.retain(|weak| {
            let Some(item) = weak.upgrade() else {
                return false;
            };
            let mut state = item.borrow_mut();
            if reload {
                state.lease = None;
                if state.active {
                    state.lease = super::acquire(Provider::Media).ok();
                }
                state.revision = u64::MAX;
            }
            bar_changed(&mut state);
            true
        })
    });
    WIDGETS.with_borrow_mut(|items| {
        items.retain(|weak| {
            let Some(item) = weak.upgrade() else {
                return false;
            };
            let mut state = item.borrow_mut();
            if reload {
                state.lease = None;
                widget_lease(&mut state);
            }
            widget_changed(&mut state);
            true
        })
    });
}
