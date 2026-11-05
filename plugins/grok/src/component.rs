use std::collections::BTreeMap;

use mypaper_plugin_sdk::{Face, Role, SizeClass, Tone, View};
use mypaper_plugin_sdk::guest::{self, Event, FaceOpen, Plugin, http, launch, secrets, ui};
use serde::{Deserialize, Serialize};

use crate::{config::Grok as Config, discovery::Discovery, faces, routines::{self, RoutineRun, RoutineState}};

#[derive(Default, Serialize, Deserialize)]
struct Saved { connected: bool, runs: Vec<RoutineRun> }

struct OpenFace { id: String, size: SizeClass, config: Config, advanced: bool }

#[derive(Default)]
struct Grok {
    faces: BTreeMap<String, OpenFace>,
    discovery: Discovery,
    saved: Saved,
    pending: BTreeMap<u64, String>,
    notice: Option<String>,
}

impl Grok {
    fn discover(&mut self) -> Result<(), String> {
        let status = launch::inspect("Grok Bot").map_err(|_| "Could not inspect Grok Bot")?;
        self.discovery = Discovery::from_marker(status.installed, status.running, status.marker.as_deref());
        Ok(())
    }

    fn open(&mut self, open: FaceOpen) {
        let config = match Config::parse(&guest::settings(&open)) {
            Ok(config) => config,
            Err(error) => { self.notice = Some(error); Config::default() }
        };
        let size = match open.size { guest::SizeClass::Small => SizeClass::Small, guest::SizeClass::Medium => SizeClass::Medium, guest::SizeClass::Large => SizeClass::Large };
        let advanced = self.faces.get(&open.instance).is_some_and(|f| f.advanced);
        self.faces.insert(open.instance, OpenFace { id: open.face, size, config, advanced });
        if let Err(error) = self.discover() { self.notice = Some(error); }
    }

    fn draw(&self) {
        for (instance, open) in &self.faces {
            let mut face = if open.advanced { self.advanced(open) } else {
                faces::face(&open.id, open.size, &self.discovery, &open.config.routines, &self.saved.runs)
            };
            if let Some(notice) = &self.notice { face.footer = Some(notice.clone()); }
            ui::set_view(instance, face);
        }
    }

    fn advanced(&self, open: &OpenFace) -> Face {
        let mut children = vec![View::tinted("Webhook keys", Role::Headline, Tone::Secondary)];
        if open.config.routines.is_empty() {
            children.push(View::tinted("Add routines in this widget’s Advanced settings.", Role::Callout, Tone::Secondary));
        } else {
            children.extend(open.config.routines.iter().map(|r| View::Field { placeholder: format!("Key for {}", r.title), action: format!("key:{}", r.id) }));
        }
        children.push(View::Button { label: "Back".into(), action: "advanced".into(), icon: None, primary: false });
        Face { body: View::stack(children), footer: Some("Keys stay in Windows Credential Manager".into()), refresh: false }
    }

    fn action(&mut self, instance: &str, action: &str, value: Option<String>) -> Result<(), String> {
        if !self.faces.contains_key(instance) { return Ok(()); }
        match action {
            "refresh" => self.discover()?,
            "connect" => {
                self.discover()?;
                if !self.discovery.installed { return Err("Install Grok Bot, then Connect again.".into()); }
                if !self.discovery.signed_in { launch::open("grokbot://app/v1/open").map_err(|_| "Open Grok Bot, then retry")?; }
                self.saved.connected = true;
                self.notice = Some(if self.discovery.signed_in { "Connected · existing Grok Bot sign-in detected" } else { "Sign in once in Grok Bot, then refresh" }.into());
            }
            "open" => launch::open("grokbot://app/v1/open").map_err(|_| "Install or open Grok Bot, then retry")?,
            "prompt" => launch::open(&routines::prompt_target(value.as_deref().unwrap_or_default())?).map_err(|_| "Grok Bot could not open. Start it, then retry.")?,
            "advanced" => if let Some(face) = self.faces.get_mut(instance) { face.advanced = !face.advanced; },
            _ => {
                if let Some(id) = action.strip_prefix("run:") { self.run(instance, id)?; }
                else if let Some(id) = action.strip_prefix("key:") {
                    let open = &self.faces[instance];
                    if !open.advanced || !open.config.routines.iter().any(|r| r.id == id) { return Err("Unknown routine".into()); }
                    let key = value.as_deref().unwrap_or_default().trim();
                    if key.is_empty() || key.contains(['\r', '\n']) { return Err("Paste a valid webhook key".into()); }
                    secrets::set(&format!("grok/{id}"), key).map_err(|_| "Could not save the webhook key")?;
                    self.notice = Some("Webhook key saved".into());
                }
            }
        }
        Ok(())
    }

    fn run(&mut self, instance: &str, id: &str) -> Result<(), String> {
        if self.pending.values().any(|pending| pending == id) { return Err("Routine request is already sending".into()); }
        if self.pending.len() >= 8 { return Err("Wait for the pending requests to finish".into()); }
        let routine = self.faces[instance].config.routines.iter().find(|r| r.id == id).ok_or("Unknown routine")?;
        let url = routines::routine_url(&routine.url)?;
        let mut key = secrets::get(&format!("grok/{id}")).map_err(|_| "Could not read the webhook key")?.ok_or("Add the routine key under Advanced")?;
        if key.is_empty() || key.contains(['\r', '\n']) { return Err("Invalid webhook key".into()); }
        let body = serde_json::to_vec(&routine.input).map_err(|_| "Invalid routine input")?;
        if body.len() > 64 * 1024 { return Err("Routine input exceeds 64 KiB".into()); }
        let request = http::Request {
            method: http::Method::Post, url: url.into(),
            headers: vec![http::Header { name: "Authorization".into(), value: format!("Bearer {key}") }, http::Header { name: "Content-Type".into(), value: "application/json".into() }],
            body: Some(body), timeout_ms: Some(10_000),
        };
        key.clear();
        let request = http::fetch(&request).map_err(|e| match e {
            guest::Error::Denied(_) => "Allow this routine’s HTTPS host in Advanced network settings",
            _ => "Could not send the routine request",
        })?;
        self.pending.insert(request, id.into());
        // Keep in-flight receipts, and bound completed receipts without retaining response bodies.
        while self.saved.runs.len() >= 16 {
            if let Some(index) = self.saved.runs.iter().rposition(|r| r.state != RoutineState::Sending) { self.saved.runs.remove(index); } else { break; }
        }
        self.saved.runs.insert(0, RoutineRun { id: id.into(), title: routine.title.chars().take(128).collect(), state: RoutineState::Sending });
        Ok(())
    }
}

impl Plugin for Grok {
    fn init(&mut self, _info: &guest::InitInfo, state: Option<&[u8]>) -> Result<(), String> {
        self.saved = state.and_then(|bytes| serde_json::from_slice(bytes).ok()).unwrap_or_default();
        self.saved.runs.truncate(16);
        // A saved receipt never claims an interrupted request is still running.
        for run in &mut self.saved.runs { if run.state == RoutineState::Sending { run.state = RoutineState::Failed; } }
        Ok(())
    }

    fn on_event(&mut self, event: Event) {
        match event {
            Event::Open(open) | Event::Resize(open) => self.open(open),
            Event::Close(instance) => { self.faces.remove(&instance); return; }
            Event::Action(action) => {
                self.notice = None;
                if let Err(error) = self.action(&action.instance, &action.action, action.value) { self.notice = Some(error); }
            }
            Event::Http(reply) => {
                let Some(id) = self.pending.remove(&reply.id) else { return; };
                let accepted = matches!(reply.outcome, Ok(ref response) if response.status == 200);
                if let Some(run) = self.saved.runs.iter_mut().find(|r| r.id == id && r.state == RoutineState::Sending) {
                    run.state = if accepted { RoutineState::Accepted } else { RoutineState::Failed };
                }
                self.notice = Some(if accepted { "Accepted · open Grok for results" } else { "Request failed · retry routine" }.into());
            }
            Event::Resume => if let Err(error) = self.discover() { self.notice = Some(error); },
            _ => return,
        }
        self.draw();
    }

    fn save(&mut self) -> Vec<u8> { serde_json::to_vec(&self.saved).unwrap_or_default() }
}

mypaper_plugin_sdk::export!(Grok);
