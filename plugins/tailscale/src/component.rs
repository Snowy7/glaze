//! Async request bookkeeping: old status replies cannot overwrite a completed action.
use std::{collections::BTreeMap, time::{SystemTime, UNIX_EPOCH}};

use glaze_plugin_sdk::guest::{self, Event, Plugin, SizeClass, http, pipes, timers, ui};
use crate::{Action, State, allowed_exit, faces, tailscale};

const PIPE: &str = r"ProtectedPrefix\Administrators\Tailscale\tailscaled";

#[derive(Default)]
struct Tailscale {
    state: State,
    faces: BTreeMap<String, (String, SizeClass)>,
    status: Option<u64>,
    patch: Option<u64>,
    suspended: bool,
}

impl Tailscale {
    fn request(method: http::Method, path: &str, body: Option<Vec<u8>>) -> Result<u64, String> {
        pipes::http_request(PIPE, &http::Request {
            method, url: format!("http://local-tailscaled.sock/localapi/v0/{path}"),
            headers: vec![http::Header { name: "Content-Type".into(), value: "application/json".into() }],
            body, timeout_ms: Some(5000),
        }).map_err(|e| format!("{e:?}"))
    }

    fn refresh(&mut self) {
        if self.suspended || self.faces.is_empty() || self.status.is_some() || self.patch.is_some() { return; }
        match Self::request(http::Method::Get, "status", None) {
            Ok(id) => self.status = Some(id),
            Err(error) => self.state.error = Some(error),
        }
    }

    fn action(&mut self, action: Action) {
        if self.suspended || self.patch.is_some() { return; }
        let Some(patch) = tailscale::prefs(&action) else { return; };
        // A status started before this action is no longer authoritative.
        self.status = None;
        match Self::request(http::Method::Patch, "prefs", Some(patch.to_string().into_bytes())) {
            Ok(id) => { self.patch = Some(id); self.state.action_error = Some("Applying change…".into()); }
            Err(error) => self.state.action_error = Some(format!("Action failed · {error}")),
        }
    }

    fn redraw(&self) {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as i64;
        for (instance, (face, size)) in &self.faces {
            let size = match size {
                SizeClass::Small => glaze_plugin_sdk::SizeClass::Small,
                SizeClass::Medium => glaze_plugin_sdk::SizeClass::Medium,
                SizeClass::Large => glaze_plugin_sdk::SizeClass::Large,
            };
            ui::set_view(instance, faces::face(face, size, &self.state, now));
        }
    }

    fn reply(&mut self, reply: http::Reply) {
        let result = match reply.outcome {
            Ok(response) if (200..300).contains(&response.status) => Ok(response.body),
            Ok(response) => Err(format!("Tailscale returned HTTP {}", response.status)),
            Err(error) => Err(format!("{error:?}")),
        };
        if self.patch == Some(reply.id) {
            self.patch = None;
            self.state.action_error = result.err().map(|e| format!("Action failed · {e}"));
            self.refresh();
        } else if self.status == Some(reply.id) {
            self.status = None;
            match result.and_then(|body| serde_json::from_slice::<serde_json::Value>(&body).map_err(|e| e.to_string())) {
                Ok(status) if status["BackendState"].as_str().is_some_and(|s| !s.is_empty()) => {
                    let mut data = tailscale::Snapshot::default();
                    tailscale::apply(&mut data, &serde_json::json!({"InitialStatus":status}));
                    self.state.data = Some(data);
                    self.state.error = None;
                }
                Ok(_) => self.state.error = Some("Incompatible Tailscale status".into()),
                Err(error) => self.state.error = Some(error),
            }
        }
    }
}

impl Plugin for Tailscale {
    fn init(&mut self, _info: &guest::InitInfo, saved: Option<&[u8]>) -> Result<(), String> {
        self.state.data = saved.and_then(|bytes| serde_json::from_slice(bytes).ok());
        if self.state.data.is_some() { self.state.error = Some("Refreshing saved status…".into()); }
        Ok(())
    }

    fn on_event(&mut self, event: Event) {
        match event {
            Event::Open(open) | Event::Resize(open) => {
                let first = self.faces.is_empty();
                self.faces.insert(open.instance, (open.face, open.size));
                if first { let _ = timers::every("last-seen", 60_000); self.refresh(); }
            }
            Event::Close(instance) => {
                self.faces.remove(&instance);
                if self.faces.is_empty() { timers::cancel("last-seen"); }
            }
            Event::Action(action) if self.faces.contains_key(&action.instance) => {
                match action.action.as_str() {
                    "refresh" => { self.state.action_error = None; self.refresh(); }
                    "connect" => self.action(Action::Connected(true)),
                    "disconnect" => self.action(Action::Connected(false)),
                    "exit:none" => self.action(Action::ExitNode(String::new())),
                    "exit" => if let Some(id) = action.value.filter(|id| allowed_exit(&self.state, id)) {
                        self.action(Action::ExitNode(id));
                    },
                    other => if let Some(id) = other.strip_prefix("exit:").filter(|id| allowed_exit(&self.state, id)) {
                        self.action(Action::ExitNode(id.into()));
                    },
                }
            }
            Event::Http(reply) => self.reply(reply),
            Event::Suspend => { self.suspended = true; timers::cancel("last-seen"); }
            Event::Resume => {
                self.suspended = false;
                if !self.faces.is_empty() { let _ = timers::every("last-seen", 60_000); self.refresh(); }
            }
            Event::Timer(_) => {},
            _ => return,
        }
        self.redraw();
    }

    fn save(&mut self) -> Vec<u8> {
        self.state.data.as_ref().and_then(|data| serde_json::to_vec(data).ok()).unwrap_or_default()
    }
}

glaze_plugin_sdk::export!(Tailscale);
