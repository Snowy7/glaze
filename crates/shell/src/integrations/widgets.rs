//! Visible widgets lease the same providers as bars and panels. Only OBS's active clocks tick.
use crate::widgets::model::{Model as WidgetModel, Widget};
use mypaper_integrations::{
    Action, Availability, Data, Lease, Provider, Snapshot, config, launcher, obs,
};
use mypaper_ui::HitId;

pub const CONNECT: HitId = 6000;
pub const REFRESH: HitId = 6001;
pub const PRIMARY: HitId = 6002;
pub const SECONDARY: HitId = 6003;
pub const MIC: HitId = 6100;
pub const SCENE: HitId = 6200;
pub const ITEM: HitId = 6400;
pub const RUN: HitId = 6800;

#[derive(Default)]
pub struct Model {
    lease: Option<Lease>,
    generation: Option<u64>,
    pub snapshot: Snapshot,
    pub routines: Vec<config::Routine>,
    pub runs: Vec<launcher::RoutineRun>,
    task: Option<super::Task<()>>,
    pub notice: Option<String>,
    pub thread_links: std::collections::BTreeMap<String, String>,
    pub observed_at_ms: i64,
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
    pub fn clock_running(&self) -> bool {
        self.snapshot.availability == Availability::Live
            && matches!(&self.snapshot.data, Some(Data::Obs(state)) if state.recording && !state.paused || state.streaming)
    }
}

pub fn provider(kind: Widget) -> Option<Provider> {
    match kind {
        Widget::Discord => Some(Provider::Notifications),
        Widget::T3 => Some(Provider::T3),
        Widget::Obs => Some(Provider::Obs),
        Widget::Docker => Some(Provider::Docker),
        Widget::Tailscale => Some(Provider::Tailscale),
        _ => None,
    }
}

/// Runs on provider/UI wakes, never installs a deadline. Hidden/held instances release demand.
pub fn sync(widget: &mut WidgetModel, active: bool) {
    if provider(widget.kind).is_none() && widget.kind != Widget::Grok {
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
        state.lease = None;
        state.generation = Some(generation);
        let config = super::config();
        state.thread_links = config.t3.thread_links.clone();
        if widget.kind == Widget::Grok {
            state.routines = if config.grok.enabled {
                config.grok.routines
            } else {
                Vec::new()
            };
            state.snapshot.availability = if state.routines.is_empty() {
                Availability::Unavailable("Add a saved routine in Settings".into())
            } else {
                Availability::Live
            };
        } else if let Some(provider) = provider(widget.kind) {
            state.snapshot = match super::acquire(provider) {
                Ok(lease) => {
                    state.lease = Some(lease);
                    Snapshot::default()
                }
                Err(_) => Snapshot {
                    availability: Availability::Unavailable(
                        if widget.kind == Widget::T3 {
                            "Read-only pairing required"
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
        state.notice = result.err();
        state.task = None;
        widget.dirty = true;
    }
    if widget.kind == Widget::Grok {
        let runs = launcher::routine_runs();
        if state.runs != runs {
            state.runs = runs;
            widget.dirty = true;
        }
    }
}

fn open(target: &str) -> anyhow::Result<()> {
    launcher::open(&launcher::Recent {
        title: String::new(),
        target: target.into(),
        arguments: String::new(),
    })
}

/// Explicit hits only; verification never invokes vendor actions.
pub fn action(widget: &mut WidgetModel, hit: HitId) -> bool {
    if provider(widget.kind).is_none() && widget.kind != Widget::Grok {
        return false;
    }
    let state = &mut widget.integration;
    state.notice = None;
    if hit == CONNECT {
        if widget.kind == Widget::T3 {
            if state.task.is_none() {
                state.task = Some(super::Task::spawn(|| {
                    let Some(code) = mypaper_integrations::credentials::prompt_t3_pairing()? else {
                        return Ok(());
                    };
                    let mut config = super::load()?;
                    let endpoint = if config.t3.endpoint.is_empty() {
                        "ws://127.0.0.1:3773/ws"
                    } else {
                        &config.t3.endpoint
                    };
                    mypaper_integrations::pairing::t3_blocking(endpoint, code.text()?)?;
                    // Re-read after the dialog/exchange so a concurrent Settings edit survives.
                    config = super::load()?;
                    config.t3.enabled = true;
                    if config.t3.endpoint.is_empty() {
                        config.t3.endpoint = "ws://127.0.0.1:3773/ws".into();
                    }
                    mypaper_widgets::files::write_atomic(
                        &super::config_path(),
                        &serde_json::to_vec_pretty(&config)?,
                    )
                }));
            }
            return true;
        }
        // Studio owns credential entry and consent. A widget never places a token in settings.
        crate::apps::open_studio();
        return true;
    }
    if hit == REFRESH {
        if let Some(lease) = &state.lease {
            if widget.kind == Widget::Docker && state.actionable() {
                state.notice = lease
                    .action(Action::DockerMetrics)
                    .err()
                    .map(|error| error.to_string());
            } else {
                lease.retry();
            }
        } else {
            state.generation = None;
        }
        return true;
    }
    let result = match (&state.snapshot.data, widget.kind) {
        (_, Widget::Discord) if hit == PRIMARY || (ITEM..ITEM + 512).contains(&hit) => {
            open("https://discord.com/channels/@me")
        }
        (Some(Data::T3(snapshot)), Widget::T3) => {
            if let Some(thread) = hit
                .checked_sub(ITEM)
                .and_then(|index| snapshot.threads.get(index as usize))
            {
                state
                    .thread_links
                    .get(&thread.id)
                    .ok_or_else(|| anyhow::anyhow!("Add this thread's copied link in Settings"))
                    .and_then(|link| open(link))
            } else {
                Ok(())
            }
        }
        (Some(Data::Obs(snapshot)), Widget::Obs) if state.actionable() => {
            let command = match hit {
                PRIMARY => Some(if snapshot.recording {
                    obs::Action::StopRecording
                } else {
                    obs::Action::StartRecording
                }),
                SECONDARY => Some(if snapshot.streaming {
                    obs::Action::StopStreaming
                } else {
                    obs::Action::StartStreaming
                }),
                _ => hit
                    .checked_sub(MIC)
                    .and_then(|index| snapshot.microphones.get(index as usize))
                    .map(|input| obs::Action::MuteInput {
                        name: input.name.clone(),
                        muted: !input.muted,
                    })
                    .or_else(|| {
                        hit.checked_sub(SCENE)
                            .and_then(|index| snapshot.scenes.get(index as usize))
                            .map(|name| obs::Action::SwitchScene(name.clone()))
                    }),
            };
            command.map_or(Ok(()), |command| {
                state
                    .lease
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("OBS is disconnected"))
                    .and_then(|lease| lease.action(Action::Obs(command)))
            })
        }
        (Some(Data::Docker(snapshot)), Widget::Docker) if state.actionable() => {
            if let Some(container) = hit
                .checked_sub(ITEM)
                .and_then(|index| snapshot.containers.get(index as usize))
            {
                state
                    .lease
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("Docker is disconnected"))
                    .and_then(|lease| {
                        lease.action(Action::Docker {
                            id: container.id.clone(),
                            start: container.state != "running",
                        })
                    })
            } else {
                Ok(())
            }
        }
        (Some(Data::Tailscale(snapshot)), Widget::Tailscale)
            if hit == PRIMARY && state.actionable() =>
        {
            state
                .lease
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Tailscale is disconnected"))
                .and_then(|lease| {
                    lease.action(Action::Tailscale {
                        connected: snapshot.state != "Running",
                    })
                })
        }
        (_, Widget::Grok) => {
            if state.task.is_none()
                && let Some(routine) = hit
                    .checked_sub(RUN)
                    .and_then(|index| state.routines.get(index as usize))
                    .cloned()
            {
                state.task = Some(super::Task::spawn(move || {
                    launcher::start_routine(&routine)
                }));
            }
            Ok(())
        }
        _ => Ok(()),
    };
    state.notice = result.err().map(|error| error.to_string());
    true
}
