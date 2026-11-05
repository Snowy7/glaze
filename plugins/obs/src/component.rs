use std::collections::BTreeMap;
use mypaper_plugin_sdk::guest::{self, Event, Plugin, secrets, timers, ui, websocket};
use mypaper_plugin_sdk::SizeClass;
use serde_json::Value;
use crate::{Availability, Data, Snapshot, faces, obs};

#[derive(Default)]
struct Obs {
    faces: BTreeMap<String, SizeClass>,
    connection: obs::Connection,
    socket: Option<u64>,
    port: u16,
    snapshot: Snapshot,
    suspended: bool,
    needs_password: bool,
}
fn now() -> i64 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as i64 }
impl Obs {
    fn stop(&mut self) {
        if let Some(socket) = self.socket.take() { websocket::close(socket); }
        for timer in ["stats", "clock", "handshake", "retry"] { timers::cancel(timer); }
        self.connection = obs::Connection::default();
    }
    fn connect(&mut self) {
        if self.suspended || self.faces.is_empty() || self.socket.is_some() { return; }
        self.snapshot = Snapshot::default();
        self.needs_password = false;
        let request = websocket::ConnectRequest { url: format!("ws://127.0.0.1:{}", self.port), headers: vec![], protocols: vec!["obswebsocket.json".into()] };
        match websocket::connect_with(&request) {
            Ok(id) => { self.socket = Some(id); timers::after("handshake", 15_000); }
            Err(guest::Error::Denied(_)) => self.fail("Allow the configured OBS WebSocket port in plugin permissions"),
            Err(_) => self.fail("OBS isn't running"),
        }
    }
    fn fail(&mut self, reason: &str) {
        self.stop();
        self.snapshot = Snapshot { availability: Availability::Unavailable(reason.into()), data: None, observed_at_ms: now() };
        if !self.suspended && !self.faces.is_empty() { timers::after("retry", 30_000); }
    }
    fn send(&mut self, messages: Vec<Value>) {
        for message in messages {
            if let Some(id) = self.socket && websocket::send(id, &websocket::Message::Text(message.to_string())).is_err() {
                self.fail("OBS isn't running"); break;
            }
        }
    }
    fn draw(&self) {
        for (id, size) in &self.faces {
            let mut face = faces::face(*size, &self.snapshot, now());
            if self.needs_password {
                face.body = mypaper_plugin_sdk::View::stack([
                    mypaper_plugin_sdk::View::empty("video", "OBS password needed", "Enter the WebSocket password to connect."),
                    mypaper_plugin_sdk::View::Field { placeholder: "Save OBS password…".into(), action: "password".into() },
                ]);
            }
            if let Some(note) = &self.connection.notice { face.footer = Some(note.clone()); }
            ui::set_view(id, face);
        }
    }
    fn sync(&mut self) {
        if self.connection.live() {
            timers::cancel("handshake");
            self.snapshot = Snapshot { availability: Availability::Live, data: Some(Data::Obs(self.connection.state.clone())), observed_at_ms: now() };
            let _ = timers::every("stats", 5000);
            if self.connection.state.record_clock.advancing || self.connection.state.stream_clock.advancing { let _ = timers::every("clock", 1000); }
            else { timers::cancel("clock"); }
        }
    }
}
impl Plugin for Obs {
    fn init(&mut self, info: &guest::InitInfo, _state: Option<&[u8]>) -> Result<(), String> {
        let settings: Value = serde_json::from_str(&info.settings).map_err(|e| e.to_string())?;
        self.port = settings["port"].as_u64().unwrap_or(4455).try_into().ok().filter(|p| *p > 0).ok_or("OBS port must be between 1 and 65535")?;
        Ok(())
    }
    fn on_event(&mut self, event: Event) {
        match event {
            Event::Open(open) | Event::Resize(open) => {
                let size = match open.size { guest::SizeClass::Small => SizeClass::Small, guest::SizeClass::Medium => SizeClass::Medium, guest::SizeClass::Large => SizeClass::Large };
                self.faces.insert(open.instance, size); self.connect();
            }
            Event::Close(id) => { self.faces.remove(&id); if self.faces.is_empty() { self.stop(); } }
            Event::Suspend => { self.suspended = true; self.stop(); }
            Event::Resume => { self.suspended = false; self.connect(); }
            Event::Timer(id) => match id.as_str() {
                "retry" => self.connect(),
                "handshake" => self.fail("OBS WebSocket didn't finish connecting"),
                "stats" => match self.connection.sample(now()) { Ok(messages) => self.send(messages), Err(reason) => self.fail(&reason) },
                "clock" => {},
                _ => return,
            },
            Event::Action(a) if self.faces.contains_key(&a.instance) => {
                if a.action == "password" {
                    if let Some(password) = a.value.filter(|p| !p.is_empty()) {
                        if secrets::set("obs", &password).is_ok() { self.stop(); self.connect(); }
                        else { self.fail("Couldn't save the OBS password"); }
                    }
                } else if a.action == "refresh" { self.stop(); self.connect(); }
                else if let Some(action) = faces::action(&a.action, self.snapshot.data.as_ref()) && let Some(message) = self.connection.action(action, now()) { self.send(vec![message]); }
            }
            Event::Socket(websocket::SocketEvent::Message((id, websocket::Message::Text(text)))) if self.socket == Some(id) => {
                let result = serde_json::from_str::<Value>(&text).map_err(|_| "Invalid OBS message".into()).and_then(|frame| {
                    let password = if frame["op"] == 0 { secrets::get("obs").ok().flatten() } else { None };
                    self.connection.receive(&frame, password.as_deref(), now())
                });
                match result { Ok(messages) => { self.send(messages); self.sync(); }, Err(reason) => { self.fail(&reason); self.needs_password = reason.contains("password"); } }
            }
            Event::Socket(websocket::SocketEvent::Closed((id, reason))) if self.socket == Some(id) => {
                let auth = reason.as_deref().is_some_and(|r| r.contains("4005") || r.contains("4009") || r.to_ascii_lowercase().contains("auth"));
                self.fail(if auth { "Check the OBS WebSocket password" } else { "OBS isn't running" });
                self.needs_password = auth;
            }
            _ => return,
        }
        self.draw();
    }
}
mypaper_plugin_sdk::export!(Obs);
