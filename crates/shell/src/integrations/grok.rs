//! Grok discovery and native prompt handoff. Routines still use its supported webhook contract.
use crate::widgets::model::{Model as WidgetModel, Widget};
use mypaper_integrations::{Data, Lease, Provider, grok};
use mypaper_ui::{FileWatch, HitId};
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

pub const OPEN: HitId = 7300;
pub const PROMPT: HitId = 7301;

#[derive(Default)]
pub struct Model {
    pub discovery: grok::Discovery,
    pub cards: Vec<mypaper_integrations::notifications::Card>,
    notifications: Option<Lease>,
    watcher: Option<FileWatch>,
    changed: Arc<AtomicBool>,
}

pub fn sync(widget: &mut WidgetModel, active: bool) {
    let state = &mut widget.integration.grok;
    if !active {state.notifications=None;state.watcher=None;return;}
    if state.watcher.is_none() {
        state.discovery=grok::discover();
        let changed=state.changed.clone();
        // The marker's directory changes on sign-in/out and process lifecycle, never on a timer.
        let thread=unsafe {windows::Win32::System::Threading::GetCurrentThreadId()};
        state.watcher=FileWatch::new(&grok::status_path(),move||{changed.store(true,Ordering::Release);super::wake(thread);}).ok();
    }
    if state.changed.swap(false,Ordering::AcqRel) {
        let discovery=grok::discover();
        widget.dirty |= state.discovery != discovery;
        state.discovery=discovery;
    }
    if state.notifications.is_none() && super::config().notifications.enabled {
        state.notifications=super::acquire(Provider::Notifications).ok();
    }
    if let Some(lease)=&state.notifications {
        let snapshot=lease.snapshot();
        if let Some(Data::Notifications(snapshot))=snapshot.data {
            let cards:Vec<_>=snapshot.cards.into_iter().filter(|card|card.app.eq_ignore_ascii_case("Grok Bot")).take(16).collect();
            widget.dirty |= cards != state.cards;
            state.cards=cards;
        }
    }
}

pub fn submit(widget: &mut WidgetModel, prompt: &str) -> bool {
    if widget.kind != Widget::Grok {return false;}
    let prompt=prompt.trim();
    let target=format!("grokbot://app/v1/prompt?text={}",mypaper_integrations::launcher::encode(prompt));
    widget.integration.notice=if prompt.is_empty() || prompt.chars().count()>1000 || target.len()>2048 {
        Some("Use a shorter prompt (up to 1,000 characters).".into())
    } else {
        super::widgets::open(&target).err().map(|_|"Grok Bot could not open. Start it, then retry.".into())
    };
    true
}

pub fn action(widget: &mut WidgetModel, hit: HitId) -> bool {
    if widget.kind!=Widget::Grok {return false;}
    if hit==super::widgets::CONNECT {
        let discovery=grok::discover();
        if !discovery.installed {widget.integration.notice=Some("Install Grok Bot, then Connect again.".into());return true;}
        if !widget.integration.busy() {
            widget.integration.task=Some(super::Task::spawn(move||{
                let mut config=super::load()?;
                config.grok.enabled=true;
                mypaper_widgets::files::write_atomic(&super::config_path(),&serde_json::to_vec_pretty(&config)?)?;
                if !discovery.signed_in {super::widgets::open("grokbot://app/v1/open")?;}
                Ok(())
            }));
        }
        return true;
    }
    if hit==OPEN {
        widget.integration.notice=super::widgets::open("grokbot://app/v1/open").err().map(|_|"Install or open Grok Bot, then retry.".into());
        return true;
    }
    if hit==PROMPT {
        let prompt=widget.field.as_ref().and_then(|field|field.text().ok()).unwrap_or_default();
        return submit(widget,&prompt);
    }
    false
}
