//! Calendar data hookup: imported events join local events without entering local persistence.
use crate::widgets::model::Model as WidgetModel;
use mypaper_integrations::{
    Data,
    calendar_oauth::{self, Service},
};
use mypaper_ui::HitId;

pub const SOURCES: HitId = 7100;
pub const GOOGLE: HitId = 7101;
pub const MICROSOFT: HitId = 7102;
pub const REFRESH: HitId = 7103;
pub const WINDOWS: HitId = 7104;
pub const DISCONNECT_GOOGLE: HitId = 7105;
pub const DISCONNECT_MICROSOFT: HitId = 7106;

#[derive(Default)]
pub struct Model {
    revision: Option<u64>,
    task: Option<super::Task<()>>,
    pub show_sources: bool,
    pub notice: Option<String>,
}
impl Model {
    pub fn busy(&self) -> bool {
        self.task.is_some()
    }
    pub(super) fn invalidate(&mut self) {
        self.revision = None;
    }
}

pub fn sync(widget: &mut WidgetModel) {
    let state = &mut widget.integration;
    if state.calendar.revision != Some(state.snapshot.revision) {
        state.calendar.revision = Some(state.snapshot.revision);
        if let Some(Data::Calendar(snapshot)) = &state.snapshot.data {
            widget.events.retain(|event| event["source"] != "calendar");
            widget
                .events
                .extend(mypaper_integrations::calendar::view_events(snapshot));
            widget.dirty = true;
        }
    }
    if let Some(result) = state.calendar.task.as_ref().and_then(super::Task::take) {
        state.calendar.notice = result.err();
        state.calendar.task = None;
        if let Some(lease) = &state.lease {
            lease.retry();
        } else {
            state.generation = None;
        }
        widget.dirty = true;
    }
}

/// Keep ordinary month/day/editor actions with the calendar; consume imported edit attempts.
pub fn action(widget: &mut WidgetModel, hit: HitId) -> bool {
    use crate::widgets::views::{DELETE_TASK, EDIT_TASK};
    if hit < SOURCES || hit > DISCONNECT_MICROSOFT {
        let index = if (EDIT_TASK..EDIT_TASK + 2000).contains(&hit) {
            Some((hit - EDIT_TASK) as usize)
        } else if (DELETE_TASK..EDIT_TASK).contains(&hit) {
            Some((hit - DELETE_TASK) as usize)
        } else {
            None
        };
        if index
            .and_then(|i| widget.events.get(i))
            .is_some_and(|event| event["readOnly"] == true)
        {
            widget.notice =
                Some("This calendar is read-only. Edit the event in its calendar app.".into());
            return true;
        }
        return false;
    }
    let state = &mut widget.integration;
    state.calendar.notice = None;
    match hit {
        SOURCES => state.calendar.show_sources = !state.calendar.show_sources,
        REFRESH => {
            if let Some(lease) = &state.lease {
                lease.retry();
            } else {
                state.generation = None;
            }
        }
        GOOGLE | MICROSOFT if !state.calendar.busy() => {
            let service = if hit == GOOGLE {
                Service::Google
            } else {
                Service::Microsoft
            };
            let config = super::config();
            let id = if service == Service::Google {
                config.calendar.google_client_id
            } else {
                config.calendar.microsoft_client_id
            };
            state.calendar.task = Some(super::Task::spawn(move || {
                let Some(id)=mypaper_integrations::calendar_setup::configure(service,&id)? else{return Ok(());};
                let mut config=super::load()?;
                if service==Service::Google{config.calendar.google_client_id=id.clone();}else{config.calendar.microsoft_client_id=id.clone();}
                mypaper_widgets::files::write_atomic(&super::config_path(), &serde_json::to_vec_pretty(&config)?)?;
                calendar_oauth::connect(service, &id)?;
                let mut config=super::load()?;config.calendar.enabled=true;
                mypaper_widgets::files::write_atomic(&super::config_path(), &serde_json::to_vec_pretty(&config)?)
            }));
        }
        DISCONNECT_GOOGLE | DISCONNECT_MICROSOFT if !state.calendar.busy() => {
            let service = if hit == DISCONNECT_GOOGLE {
                Service::Google
            } else {
                Service::Microsoft
            };
            state.calendar.task = Some(super::Task::spawn(move || {
                calendar_oauth::disconnect(service)
            }));
        }
        WINDOWS if !state.calendar.busy() => {
            state.calendar.task = Some(super::Task::spawn(|| {
                let mut config = super::load()?;
                config.calendar.enabled = true;
                config.calendar.windows = true;
                mypaper_widgets::files::write_atomic(
                    &super::config_path(),
                    &serde_json::to_vec_pretty(&config)?,
                )
            }));
        }
        _ => {}
    }
    true
}
