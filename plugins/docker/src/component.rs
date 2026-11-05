use super::*;
use std::collections::{BTreeMap, VecDeque};
use mypaper_plugin_sdk::guest::{self, Event, Plugin, SizeClass, http, pipes, timers, ui};

#[derive(Debug)]
enum Pending {
    List { sample: bool },
    Stats(String),
    Action,
}

#[derive(Default)]
struct Docker {
    faces: BTreeMap<String, SizeClass>,
    snapshot: Snapshot,
    pending: BTreeMap<u64, Pending>,
    samples: VecDeque<String>,
    suspended: bool,
}

impl Docker {
    fn request(&mut self, path: &str, method: http::Method, kind: Pending) -> Result<(), String> {
        let id = pipes::http_request("docker_engine", &http::Request {
            method, url: format!("http://localhost{path}"), headers: Vec::new(),
            body: None, timeout_ms: Some(5000),
        }).map_err(|error| format!("{error:?}"))?;
        self.pending.insert(id, kind);
        Ok(())
    }

    fn refresh(&mut self, sample: bool) {
        if self.suspended || self.faces.is_empty() || !self.pending.is_empty() { return; }
        self.snapshot.action_error = None;
        if let Err(error) = self.request("/containers/json?all=1&limit=128", http::Method::Get, Pending::List { sample }) {
            self.unavailable(error);
        }
    }

    fn unavailable(&mut self, why: String) {
        guest::log(&format!("Docker engine unavailable: {why}"));
        self.snapshot.availability = if self.snapshot.data.is_some() {
            Availability::Stale("Docker isn't running".into())
        } else { Availability::Unavailable("Docker isn't running".into()) };
        self.samples.clear();
    }

    fn draw(&self) {
        for (instance, size) in &self.faces { ui::set_view(instance, faces::face(match size { SizeClass::Small => mypaper_plugin_sdk::SizeClass::Small, SizeClass::Medium => mypaper_plugin_sdk::SizeClass::Medium, SizeClass::Large => mypaper_plugin_sdk::SizeClass::Large }, &self.snapshot)); }
    }

    fn next_sample(&mut self) {
        if let Some(id) = self.samples.pop_front() {
            let path = format!("/containers/{}/stats?stream=false&one-shot=true", encode(&id));
            if let Err(error) = self.request(&path, http::Method::Get, Pending::Stats(id)) {
                self.snapshot.action_error = Some(format!("Couldn't sample metrics: {error}"));
                self.samples.clear();
            }
        }
    }

    fn reply(&mut self, reply: http::Reply) {
        let Some(kind) = self.pending.remove(&reply.id) else { return; };
        let response = reply.outcome.map_err(|error| format!("{error:?}"))
            .and_then(|response| if (200..300).contains(&response.status) || matches!(kind, Pending::Action) && response.status == 304 {
                Ok(response)
            } else { Err(format!("Docker returned HTTP {}", response.status)) });
        match kind {
            Pending::List { sample } => match response.and_then(|r| serde_json::from_slice(&r.body).map_err(|e| e.to_string()))
                .and_then(|value| docker::snapshot(&value).map_err(|e| e.to_string())) {
                Ok(mut state) => {
                    if let Some(Data::Docker(previous)) = &mut self.snapshot.data {
                        state.usage = std::mem::take(&mut previous.usage);
                        state.usage.retain(|id, _| state.containers.iter().any(|c| &c.id == id && c.state == "running"));
                    }
                    if sample { self.samples = state.containers.iter().filter(|c| c.state == "running").take(16).map(|c| c.id.clone()).collect(); }
                    self.snapshot.data = Some(Data::Docker(state));
                    self.snapshot.availability = Availability::Live;
                    self.next_sample();
                }
                Err(error) => self.unavailable(error),
            },
            Pending::Stats(id) => {
                match response.and_then(|r| serde_json::from_slice(&r.body).map_err(|e| e.to_string())) {
                    Ok(value) => if let Some(Data::Docker(state)) = &mut self.snapshot.data { state.usage.insert(id, docker::usage(&value)); },
                    Err(error) => { self.snapshot.action_error = Some(format!("Couldn't sample metrics: {error}")); self.samples.clear(); }
                }
                self.next_sample();
            }
            Pending::Action => match response {
                Ok(_) => { self.snapshot.action_error = None; self.refresh(true); }
                Err(error) => self.snapshot.action_error = Some(format!("Container action failed: {error}")),
            },
        }
    }
}

impl Plugin for Docker {
    fn on_event(&mut self, event: Event) {
        match event {
            Event::Open(open) | Event::Resize(open) => {
                let first = self.faces.is_empty();
                self.faces.insert(open.instance, open.size);
                if first { let _ = timers::every("containers", 30_000); self.refresh(true); }
                self.draw();
            }
            Event::Close(instance) => {
                self.faces.remove(&instance);
                if self.faces.is_empty() { timers::cancel("containers"); self.pending.clear(); self.samples.clear(); }
            }
            Event::Action(action) if self.faces.contains_key(&action.instance) => {
                if action.action == "refresh" { self.refresh(true); }
                else if let Some(path) = action_path(&self.snapshot, self.suspended || !self.pending.is_empty(), &action.action) {
                    self.snapshot.action_error = self.request(&path, http::Method::Post, Pending::Action).err();
                }
                self.draw();
            }
            Event::Timer(id) if id == "containers" => { self.refresh(false); self.draw(); }
            Event::Http(reply) => { self.reply(reply); self.draw(); }
            Event::Suspend => { self.suspended = true; timers::cancel("containers"); self.pending.clear(); self.samples.clear(); }
            Event::Resume => {
                self.suspended = false;
                if !self.faces.is_empty() { let _ = timers::every("containers", 30_000); self.refresh(true); self.draw(); }
            }
            _ => {}
        }
    }
}

mypaper_plugin_sdk::export!(Docker);
