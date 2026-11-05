//! T3 widget state/actions. Input is captured only by a mypaper RichEdit field.
use super::widgets::{CONNECT, Model};
use crate::widgets::model::{Model as WidgetModel, Widget};
use mypaper_integrations::{Data, pairing, t3_auth, t3_client};
use mypaper_ui::HitId;

pub const PROJECT: HitId = 7000;
pub const MODEL: HitId = 7001;
pub const SEND: HitId = 7002;
pub const APPROVE: HitId = 100_000;
pub const DENY: HitId = 120_000;
pub const THREAD: HitId = 200_000;
pub const PROJECT_CHOICE: HitId = 300_000;
pub const MODEL_CHOICE: HitId = 400_000;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Picker {
    Project,
    Model,
}

#[derive(Clone)]
pub struct Target {
    pub thread_id: String,
    pub request_id: Option<String>,
}

#[derive(Default)]
pub struct State {
    /// Bind rendered controls to exact identities across provider updates before the next paint.
    pub targets: std::cell::RefCell<std::collections::BTreeMap<HitId, Target>>,
    pub project: usize,
    pub model: usize,
    pub draft: String,
    pub submitted: bool,
    pub picker: Option<Picker>,
}

pub(crate) fn resolve_approval(
    state: &State,
    snapshot: &mypaper_integrations::t3::Snapshot,
    hit: HitId,
) -> Option<(String, String)> {
    let target = state.targets.borrow().get(&hit)?.clone();
    let thread = snapshot
        .threads
        .iter()
        .find(|thread| thread.id == target.thread_id)?;
    let request = thread.pending_runtime_request.as_ref()?;
    (target.request_id.as_deref() == Some(request.id.as_str()) && request.approvable())
        .then(|| (thread.id.clone(), request.id.clone()))
}

/// Connects in the background: T3's CLI issues mypaper's token, or, when a pairing code was
/// pasted into the tile, that code is exchanged once. Then T3 is switched on and live.
pub fn connect(state: &mut Model, code: Option<String>) {
    if state.busy() {
        return;
    }
    state.task = Some(super::Task::spawn(move || {
        let config = super::load()?;
        match code {
            Some(code) => pairing::t3_blocking(&config.t3.endpoint, &code)?,
            None => {
                t3_auth::ensure(false)?;
            }
        }
        let mut config = super::load()?;
        config.t3.enabled = true;
        config.t3.live_verified = true;
        if config.t3.endpoint.is_empty() {
            config.t3.endpoint = t3_client::DEFAULT_ENDPOINT.into();
        }
        mypaper_widgets::files::write_atomic(
            &super::config_path(),
            &serde_json::to_vec_pretty(&config)?,
        )
    }));
}

pub fn submit(widget: &mut WidgetModel, prompt: &str) -> bool {
    if widget.kind != Widget::T3 {
        return false;
    }
    let state = &mut widget.integration;
    if state.busy() {
        return true;
    }
    let Some(Data::T3(snapshot)) = &state.snapshot.data else {
        // Before any data the tile's field holds a pasted pairing code, never a prompt.
        let code = prompt.trim();
        if !code.is_empty() {
            state.t3.submitted = true;
            connect(state, Some(code.to_owned()));
        }
        return true;
    };
    if !state.actionable() || !snapshot.can_operate() {
        state.notice = Some("Thread controls are unavailable. Refresh to reconnect.".into());
        return true;
    }
    let Some(project) = snapshot.projects.keys().nth(state.t3.project).cloned() else {
        state.notice = Some("Choose a project in T3 Code first.".into());
        return true;
    };
    let Some(model) = snapshot.models.get(state.t3.model).cloned() else {
        return true;
    };
    let prompt = prompt.to_owned();
    let address = super::config().t3.endpoint;
    state.t3.draft = prompt.clone();
    state.t3.submitted = true;
    state.task = Some(super::Task::spawn(move || {
        t3_client::launch_blocking(&address, &project, &model, &prompt)
    }));
    true
}

pub fn action(widget: &mut WidgetModel, hit: HitId) -> bool {
    if widget.kind != Widget::T3 {
        return false;
    }
    if hit == SEND || hit == CONNECT {
        let text = widget
            .field
            .as_ref()
            .and_then(|f| f.text().ok())
            .unwrap_or_else(|| widget.integration.t3.draft.clone());
        if hit == CONNECT
            && (text.trim().is_empty()
                || matches!(widget.integration.snapshot.data, Some(Data::T3(_))))
        {
            // Connect with an empty field retries the background path.
            connect(&mut widget.integration, None);
            return true;
        }
        return submit(widget, &text);
    }
    let state = &mut widget.integration;
    let Some(Data::T3(snapshot)) = &state.snapshot.data else {
        return true;
    };
    if hit == PROJECT {
        state.t3.picker = if state.t3.picker == Some(Picker::Project) {
            None
        } else {
            Some(Picker::Project)
        };
        widget.scroll = 0.0;
    } else if hit == MODEL {
        state.t3.picker = if state.t3.picker == Some(Picker::Model) {
            None
        } else {
            Some(Picker::Model)
        };
        widget.scroll = 0.0;
    } else if (PROJECT_CHOICE..PROJECT_CHOICE + 16_384).contains(&hit) {
        let index = (hit - PROJECT_CHOICE) as usize;
        if index < snapshot.projects.len() {
            state.t3.project = index;
            state.t3.picker = None;
            widget.scroll = 0.0;
        }
    } else if (MODEL_CHOICE..MODEL_CHOICE + 16_384).contains(&hit) {
        let index = (hit - MODEL_CHOICE) as usize;
        if index < snapshot.models.len() {
            state.t3.model = index;
            state.t3.picker = None;
            widget.scroll = 0.0;
        }
    } else if (APPROVE..APPROVE + 16_384).contains(&hit) || (DENY..DENY + 16_384).contains(&hit) {
        if state.busy() || !state.actionable() || !snapshot.can_operate() {
            return true;
        }
        let approve = hit < DENY;
        if let Some((thread, request)) = resolve_approval(&state.t3, snapshot, hit) {
            let address = super::config().t3.endpoint;
            state.task = Some(super::Task::spawn(move || {
                t3_client::respond_blocking(&address, &thread, &request, approve)
            }));
        }
    } else if let Some(target) = state.t3.targets.borrow().get(&hit).cloned()
        && let Some(thread) = snapshot
            .threads
            .iter()
            .find(|thread| thread.id == target.thread_id)
    {
        let target = state
            .thread_links
            .get(&thread.id)
            .cloned()
            .or_else(|| snapshot.thread_link(&thread.id));
        state.notice = target
            .ok_or_else(|| anyhow::anyhow!("Refresh to load the T3 thread link."))
            .and_then(|target| super::widgets::open(&target))
            .err()
            .map(|e| e.to_string());
    }
    true
}
