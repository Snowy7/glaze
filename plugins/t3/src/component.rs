//! The plugin loop: finds the local T3 Code (`server-runtime.json`), gets the background key
//! (`auth.rs`), then keeps one websocket with the shell stream (`orchestration.subscribeShell`),
//! the config stream (`subscribeServerConfig`: limits, models) and one-shot requests (usage,
//! approvals, new threads) multiplexed on it by request id.
use std::collections::BTreeMap;

use mypaper_plugin_sdk::SizeClass;
use mypaper_plugin_sdk::guest::{self, Event, FaceOpen, Plugin, files, http, launch, timers, ui, websocket};
use serde_json::{Value, json};

use crate::auth::{Auth, Outcome};
use crate::t3::{self, ShellEnvelope, ShellItem};
use crate::t3_client::Metadata;
use crate::{Availability, Data, Snapshot, faces};

/// Reconnect after a drop (one timer, not a poll).
const RETRY_MS: u32 = 30_000;

#[derive(Default)]
struct T3 {
    /// Placed faces: instance → (face, size).
    faces: BTreeMap<String, (String, SizeClass)>,
    picks: BTreeMap<String, faces::Pick>,
    /// The instance settings' `threadLinks` (thread id → link), merged across faces.
    links: BTreeMap<String, String>,
    availability: Option<Availability>,
    state: t3::Snapshot,
    metadata: Metadata,
    baseline: bool,
    auth: Auth,
    port: u16,
    token: Option<String>,
    socket: Option<u64>,
    session_check: Option<u64>,
    /// Refused once already: the next refusal is final until a retry.
    reissued: bool,
    next: u64,
    /// One-shot request ids → the 30 UTC days asked for (usage) or `None` (commands).
    requests: BTreeMap<String, Option<Vec<String>>>,
    last_transition: Option<(String, t3::Status)>,
    notice: Option<String>,
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

impl T3 {
    fn snapshot(&self) -> Snapshot {
        Snapshot { availability: self.availability.clone().unwrap_or(Availability::Connecting), data: self.baseline.then(|| Data::T3(self.state.clone())) }
    }

    fn draw(&self, instance: &str, snapshot: &Snapshot) {
        let Some((face, size)) = self.faces.get(instance) else { return };
        let pick = self.picks.get(instance).cloned().unwrap_or_default();
        let mut view = faces::face(face, *size, snapshot, &pick, now_ms());
        if let Some(notice) = &self.notice {
            view.footer = Some(notice.clone());
        }
        ui::set_view(instance, view);
    }

    fn draw_all(&self) {
        let snapshot = self.snapshot();
        for instance in self.faces.keys() {
            self.draw(instance, &snapshot);
        }
    }

    fn fail(&mut self, why: String) {
        guest::log(&format!("T3: {why}"));
        self.availability = Some(if self.baseline { Availability::Stale(why) } else { Availability::Unavailable(why) });
        if let Some(socket) = self.socket.take() {
            websocket::close(socket);
        }
        timers::after("retry", RETRY_MS);
        self.draw_all();
    }

    /// Finds the server, then gets the key; `connect` follows once it's there.
    fn start(&mut self, force: bool) {
        if self.socket.is_some() || self.auth.busy() {
            return;
        }
        #[derive(serde::Deserialize)]
        struct Runtime {
            port: u16,
        }
        let runtime = files::read("~/.t3/userdata/server-runtime.json").ok().flatten().and_then(|b| serde_json::from_slice::<Runtime>(&b).ok());
        let Some(runtime) = runtime else { return self.fail("T3 Code is not running".into()) };
        self.port = runtime.port;
        match self.auth.ensure(force) {
            Outcome::Ready(token) => self.connect(token),
            Outcome::Failed(why) => self.fail(why),
            Outcome::Pending | Outcome::Other => {}
        }
    }

    fn connect(&mut self, token: String) {
        let bearer = format!("Bearer {token}");
        self.token = Some(token);
        let header = |value: &str| http::Header { name: "Authorization".into(), value: value.into() };
        // The client's own grant (its scopes); a 401 here means the key was revoked.
        let check = http::Request { method: http::Method::Get, url: format!("http://127.0.0.1:{}/api/auth/session", self.port), headers: vec![header(&bearer)], body: None, timeout_ms: Some(5000) };
        self.session_check = http::fetch(&check).ok();
        let request = websocket::ConnectRequest { url: format!("ws://127.0.0.1:{}/ws?orchestrationProtocol=2", self.port), headers: vec![header(&bearer)], protocols: Vec::new() };
        match websocket::connect_with(&request) {
            Ok(id) => self.socket = Some(id),
            Err(e) => self.fail(format!("T3 Code at port {} isn't reachable ({e:?})", self.port)),
        }
    }

    fn send(&mut self, value: Value) {
        if let Some(socket) = self.socket
            && websocket::send(socket, &websocket::Message::Text(value.to_string())).is_err()
        {
            self.fail("T3 disconnected".into());
        }
    }

    fn request(&mut self, tag: &str, payload: Value, days: Option<Vec<String>>) {
        self.next += 1;
        let id = format!("r{}", self.next);
        self.send(json!({"_tag":"Request","id":id,"tag":tag,"payload":payload,"headers":[]}));
        self.requests.insert(id, days);
    }

    fn opened(&mut self) {
        self.baseline = false;
        self.state = t3::Snapshot { permissions: std::mem::take(&mut self.state.permissions), ..Default::default() };
        self.send(json!({"_tag":"Request","id":"metadata","tag":"server.getConfig","payload":{},"headers":[]}));
        self.send(json!({"_tag":"Request","id":"config","tag":"subscribeServerConfig","payload":{"usageLimitSources":true},"headers":[]}));
    }

    /// The shell stream, after the config told us which omissions the server supports.
    fn subscribe_shell(&mut self) {
        let mut payload = json!({"requestCompletionMarker":true});
        if self.state.omit_older_settled {
            payload["omitOlderSettled"] = json!(true);
        }
        if self.state.omit_idle_subagents {
            payload["omitIdleSubagents"] = json!(true);
        }
        self.send(json!({"_tag":"Request","id":"1","tag":"orchestration.subscribeShell","payload":payload,"headers":[]}));
        self.refresh_usage();
    }

    fn refresh_usage(&mut self) {
        if !self.state.permissions.iter().any(|p| p == "diagnostics:read") {
            self.state.usage_notice = Some("Usage is unavailable. Refresh to reconnect.".into());
            return;
        }
        let today = chrono::Utc::now().date_naive();
        let days: Vec<String> = (0..30).rev().map(|back| (today - chrono::Days::new(back)).format("%Y-%m-%d").to_string()).collect();
        let payload = json!({"sinceDay":days[0],"untilDay":days[29],"timeZone":"UTC","resolution":"day"});
        self.request("server.getUsageSummary", payload, Some(days));
    }

    fn message(&mut self, bytes: &[u8]) -> Result<(), String> {
        let frames: Vec<ShellEnvelope> = if bytes.first() == Some(&b'[') { serde_json::from_slice(bytes) } else { serde_json::from_slice(bytes).map(|f| vec![f]) }.map_err(|e| format!("unreadable T3 frame: {e}"))?;
        let mut changed = false;
        for frame in frames {
            let id = frame.request_id.as_deref().unwrap_or_default().to_owned();
            match (frame.tag.as_str(), id.as_str()) {
                ("Chunk", "config") => {
                    for item in frame.values {
                        changed |= self.metadata.apply(serde_json::from_str(item.get()).map_err(|e| e.to_string())?);
                    }
                    self.state.configure(&self.metadata);
                    self.send(json!({"_tag":"Ack","requestId":"config"}));
                }
                ("Chunk", "1") => {
                    for item in frame.values {
                        let item: ShellItem = serde_json::from_str(item.get()).map_err(|e| e.to_string())?;
                        if !self.baseline && item.kind != "snapshot" {
                            return Err("T3 delta arrived before its baseline".into());
                        }
                        let item = serde_json::to_value(item).map_err(|e| e.to_string())?;
                        self.state.apply(&item).map_err(|e| e.to_string())?;
                        self.baseline = true;
                    }
                    let transition = self.state.transitions.last().filter(|t| !t.attention).map(|t| (t.run_id.clone(), t.status));
                    if transition.is_some() && transition != self.last_transition {
                        self.last_transition = transition;
                        self.refresh_usage();
                    }
                    self.send(json!({"_tag":"Ack","requestId":"1"}));
                    changed = true;
                }
                ("Exit", "metadata") => {
                    let exit: Value = frame.exit.and_then(|e| serde_json::from_str(e.get()).ok()).unwrap_or_default();
                    if let Ok(metadata) = serde_json::from_value::<Metadata>(exit["value"].clone()) {
                        self.metadata = Metadata { usage_limit_sources: std::mem::take(&mut self.metadata.usage_limit_sources), ..metadata };
                        self.state.configure(&self.metadata);
                    }
                    self.subscribe_shell();
                }
                ("Exit", "1" | "config") => return Err("T3 subscription ended or was rejected".into()),
                ("Exit", _) => {
                    let exit: Value = frame.exit.and_then(|e| serde_json::from_str(e.get()).ok()).unwrap_or_default();
                    let ok = exit["_tag"] == "Success";
                    match self.requests.remove(&id) {
                        Some(Some(days)) if ok => {
                            let today = days[29].clone();
                            self.state.history(&exit["value"], days, &today);
                            self.state.usage_day = Some(today);
                        }
                        Some(Some(_)) => self.state.usage_notice = Some("Usage is unavailable. Refresh to try again.".into()),
                        Some(None) if !ok => self.notice = Some("T3 couldn't complete that. Retry in T3 Code.".into()),
                        _ => {}
                    }
                    changed = true;
                }
                ("Defect", _) => return Err("T3 could not complete a request".into()),
                _ => {}
            }
        }
        if changed && self.baseline {
            self.availability = Some(Availability::Live);
            self.draw_all();
        }
        Ok(())
    }

    fn open(&mut self, open: &FaceOpen) {
        let settings = guest::settings(open);
        if let Some(links) = settings["threadLinks"].as_object() {
            self.links.extend(links.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_owned()))));
        }
        if self.faces.is_empty() {
            // Countdowns in Limits move without server events: redraw each minute.
            let _ = timers::every("minute", 60_000);
        }
        let size = match open.size {
            guest::SizeClass::Small => SizeClass::Small,
            guest::SizeClass::Medium => SizeClass::Medium,
            guest::SizeClass::Large => SizeClass::Large,
        };
        self.faces.insert(open.instance.clone(), (open.face.clone(), size));
        self.start(false);
        self.draw(&open.instance, &self.snapshot());
    }

    fn action(&mut self, instance: &str, action: &str, value: Option<String>) {
        match action.split_once(':') {
            Some((verb @ ("approve" | "deny"), target)) => {
                if let Some((thread, request)) = target.split_once(':') {
                    let payload = json!({"type":"runtime-request.respond","commandId":self.command_id(),"threadId":thread,"requestId":request,
                        "decision":if verb == "approve" {"accept"} else {"decline"}});
                    self.request("orchestration.dispatchCommand", payload, None);
                }
            }
            Some(("thread", thread)) => {
                let link = self.links.get(thread).cloned().or_else(|| self.state.thread_link(thread));
                let opened = link.ok_or_else(|| "Refresh to load the T3 thread link.".to_owned()).and_then(|link| launch::open(&link).map_err(|e| format!("{e:?}")));
                self.notice = opened.err().map(|e| format!("Couldn't open the thread · {e}"));
            }
            None if action == "project" || action == "model" => {
                let pick = self.picks.entry(instance.to_owned()).or_default();
                if action == "project" {
                    pick.project = value;
                } else {
                    pick.model = value;
                }
            }
            None if action == "send" => {
                let pick = self.picks.get(instance).cloned().unwrap_or_default();
                let (project, model) = pick.resolve(&self.state);
                let (project, model) = (project.cloned(), model.cloned());
                match (value.filter(|p| !p.trim().is_empty() && p.len() <= 32 * 1024), project, model) {
                    (Some(prompt), Some(project), Some(model)) => {
                        let payload = json!({"commandId":self.command_id(),"projectId":project,"title":prompt.trim().chars().take(80).collect::<String>(),
                            "generateTitle":true,"modelSelection":{"instanceId":model.provider,"model":model.model},
                            "runtimeMode":"approval-required","interactionMode":"default","workspaceStrategy":{"type":"root"},
                            "initialMessage":{"text":prompt,"attachments":[]}});
                        self.request("orchestration.launchThread", payload, None);
                    }
                    _ => self.notice = Some("Write a prompt of up to 32 KiB, in a project.".into()),
                }
            }
            None if action == "retry" || action == "refresh" => {
                self.notice = None;
                self.reissued = false;
                if self.socket.is_some() {
                    self.refresh_usage();
                } else {
                    timers::cancel("retry");
                    self.start(false);
                }
            }
            _ => {}
        }
        self.draw_all();
    }

    fn command_id(&mut self) -> String {
        self.next += 1;
        format!("mypaper-{}-{}", now_ms(), self.next)
    }

    /// A `process.run` result for `auth`, or the session check.
    fn http(&mut self, id: u64, outcome: Result<http::Response, guest::Error>) {
        if self.session_check == Some(id) {
            self.session_check = None;
            match outcome {
                Ok(r) if matches!(r.status, 401 | 403) => self.refused(),
                Ok(r) if r.status == 200 => {
                    let body: Value = serde_json::from_slice(&r.body).unwrap_or_default();
                    let scopes = body.get("permissions").unwrap_or(&body["scopes"]);
                    self.state.permissions = scopes.as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect();
                    if self.baseline {
                        self.draw_all();
                    }
                }
                _ => {}
            }
            return;
        }
        let outcome = outcome.map(|r| (r.status, r.body)).map_err(|e| format!("{e:?}"));
        match self.auth.reply(id, &outcome) {
            Outcome::Ready(token) => self.connect(token),
            Outcome::Failed(why) => self.fail(why),
            Outcome::Pending | Outcome::Other => {}
        }
    }

    /// The server dropped our grant (revoked or expired early): re-issue once.
    fn refused(&mut self) {
        if let Some(socket) = self.socket.take() {
            websocket::close(socket);
        }
        if std::mem::replace(&mut self.reissued, true) {
            return self.fail("T3 Code no longer accepts mypaper's access".into());
        }
        self.start(true);
    }
}

impl Plugin for T3 {
    fn on_event(&mut self, event: Event) {
        match event {
            Event::Open(open) | Event::Resize(open) => self.open(&open),
            Event::Close(instance) => {
                self.faces.remove(&instance);
                self.picks.remove(&instance);
                if self.faces.is_empty() {
                    timers::cancel("minute");
                }
            }
            Event::Action(action) => self.action(&action.instance, &action.action, action.value),
            Event::Timer(id) if id == "minute" => self.draw_all(),
            Event::Timer(id) if id == "retry" => self.start(false),
            Event::Http(reply) => self.http(reply.id, reply.outcome),
            Event::Socket(websocket::SocketEvent::Opened(id)) if self.socket == Some(id) => self.opened(),
            Event::Socket(websocket::SocketEvent::Message((id, message))) if self.socket == Some(id) => {
                let result = match &message {
                    websocket::Message::Text(text) => self.message(text.as_bytes()),
                    websocket::Message::Binary(bytes) => self.message(bytes),
                };
                if let Err(why) = result {
                    self.fail(why);
                }
            }
            Event::Socket(websocket::SocketEvent::Closed((id, reason))) if self.socket == Some(id) => {
                self.socket = None;
                self.fail(reason.map_or_else(|| "T3 disconnected".into(), |r| format!("T3 disconnected · {r}")));
            }
            _ => {}
        }
    }
}

mypaper_plugin_sdk::export!(T3);
