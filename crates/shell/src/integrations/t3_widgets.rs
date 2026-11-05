//! T3 widget state/actions. Input is captured only by a mypaper RichEdit field.
use super::widgets::{Model, CONNECT};
use crate::widgets::model::{Model as WidgetModel, Widget};
use mypaper_integrations::{Data, pairing, t3_client};
use mypaper_ui::HitId;

pub const PROJECT: HitId = 7000;
pub const MODEL: HitId = 7001;
pub const SEND: HitId = 7002;
pub const APPROVE: HitId = 100_000;
pub const DENY: HitId = 120_000;
pub const THREAD: HitId = 200_000;

#[derive(Default)]
pub struct State {
    pub project: usize,
    pub model: usize,
    pub draft: String,
    pub submitted: bool,
}

pub fn connect(state: &mut Model) {
    if state.busy() {return;}
    state.task = Some(super::Task::spawn(|| {
        let config = super::load()?;
        if !pairing::connect_t3_blocking(&config.t3.endpoint)? {return Ok(())}
        let mut config = super::load()?;
        config.t3.enabled = true;
        config.t3.live_verified = true;
        if config.t3.endpoint.is_empty() {config.t3.endpoint = t3_client::DEFAULT_ENDPOINT.into();}
        mypaper_widgets::files::write_atomic(&super::config_path(), &serde_json::to_vec_pretty(&config)?)
    }));
}

pub fn submit(widget: &mut WidgetModel, prompt: &str) -> bool {
    if widget.kind != Widget::T3 {return false;}
    let state = &mut widget.integration;
    if state.busy() {return true;}
    let Some(Data::T3(snapshot)) = &state.snapshot.data else {return true;};
    if !state.actionable() || !snapshot.can_operate() {
        state.notice = Some("Allow thread controls in T3 Code, then reconnect.".into());
        return true;
    }
    let Some(project) = snapshot.projects.keys().nth(state.t3.project).cloned() else {
        state.notice=Some("Choose a project in T3 Code first.".into());return true;
    };
    let Some(model) = snapshot.models.get(state.t3.model).cloned() else {return true;};
    let prompt = prompt.to_owned();
    let address = super::config().t3.endpoint;
    state.t3.draft = prompt.clone();
    state.t3.submitted = true;
    state.task = Some(super::Task::spawn(move || t3_client::launch_blocking(&address, &project, &model, &prompt)));
    true
}

pub fn action(widget: &mut WidgetModel, hit: HitId) -> bool {
    if widget.kind != Widget::T3 {return false;}
    if hit == SEND {
        let text = widget.field.as_ref().and_then(|f| f.text().ok()).unwrap_or_else(|| widget.integration.t3.draft.clone());
        return submit(widget, &text);
    }
    let state = &mut widget.integration;
    if hit == CONNECT {connect(state);return true;}
    let Some(Data::T3(snapshot)) = &state.snapshot.data else {return true;};
    if hit == PROJECT {
        state.t3.project = (state.t3.project + 1) % snapshot.projects.len().max(1);
    } else if hit == MODEL {
        state.t3.model = (state.t3.model + 1) % snapshot.models.len().max(1);
    } else if (APPROVE..APPROVE+16_384).contains(&hit) || (DENY..DENY+16_384).contains(&hit) {
        if state.busy() || !state.actionable() || !snapshot.can_operate() {return true;}
        let approve = hit < DENY;
        if let Some(thread) = snapshot.threads.get((hit - if approve {APPROVE} else {DENY}) as usize)
            && let Some(request) = &thread.pending_runtime_request
            && request.kind != "user_input" && request.kind != "auth_refresh" && request.kind != "dynamic_tool_call"
        {
            let (thread, request) = (thread.id.clone(), request.id.clone());
            let address = super::config().t3.endpoint;
            state.task = Some(super::Task::spawn(move || t3_client::respond_blocking(&address, &thread, &request, approve)));
        }
    } else if let Some(thread) = hit.checked_sub(THREAD).and_then(|i|snapshot.threads.get(i as usize)) {
        let target=state.thread_links.get(&thread.id).cloned().or_else(||snapshot.thread_link(&thread.id));
        state.notice=target.ok_or_else(||anyhow::anyhow!("Refresh to load the T3 thread link.")).and_then(|target| super::widgets::open(&target)).err().map(|e|e.to_string());
    }
    true
}
