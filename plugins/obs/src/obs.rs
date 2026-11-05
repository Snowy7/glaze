//! obs-websocket 5 protocol, independent of I/O. The guest sends messages through the host.
use std::collections::BTreeMap;
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub recording: bool,
    pub paused: bool,
    pub streaming: bool,
    pub scene: String,
    pub scenes: Vec<String>,
    pub microphones: Vec<Microphone>,
    pub record_clock: Clock,
    pub stream_clock: Clock,
    pub stats: Stats,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub cpu_tenths: Option<u32>,
    pub dropped_frames: Option<u64>,
    pub bitrate_kbps: Option<u64>,
}
impl Stats {
    pub fn apply(&mut self, result: &Value) {
        self.cpu_tenths = result["cpuUsage"].as_f64().filter(|v| v.is_finite() && *v >= 0.0).map(|v| (v * 10.0).round() as u32);
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Microphone { pub name: String, pub muted: bool }
/// A duration anchored to the last server reply; local ticks never query OBS.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Clock { pub elapsed_ms: u64, pub at_ms: i64, pub advancing: bool }
impl Clock {
    pub fn elapsed(&self, now: i64) -> u64 {
        self.elapsed_ms.saturating_add(if self.advancing { now.saturating_sub(self.at_ms).max(0) as u64 } else { 0 })
    }
    fn seed(&mut self, value: &Value, advancing: bool, now: i64) {
        *self = Self { elapsed_ms: value["outputDuration"].as_u64().unwrap_or(0), at_ms: now, advancing };
    }
    fn freeze(&mut self, now: i64) { self.elapsed_ms = self.elapsed(now); self.at_ms = now; self.advancing = false; }
}
#[derive(Clone, Debug)]
pub enum Action {
    StartRecording, StopRecording, PauseRecording, ResumeRecording, StartStreaming, StopStreaming,
    SwitchScene(String), MuteInput { name: String, muted: bool },
}
pub fn authentication(password: &str, salt: &str, challenge: &str) -> String {
    let secret = STANDARD.encode(Sha256::digest(format!("{password}{salt}")));
    STANDARD.encode(Sha256::digest(format!("{secret}{challenge}")))
}
#[derive(Default, PartialEq, Eq)]
enum Phase { #[default] Hello, Identify, Ready }
struct Pending { kind: String, input: Option<String>, at: i64 }
#[derive(Default)]
pub struct Connection {
    pub state: Snapshot,
    phase: Phase,
    next: u64,
    seeded: [bool; 5],
    pending: BTreeMap<String, Pending>,
    stream_sample: Option<(u64, i64)>,
    pub notice: Option<String>,
}
impl Connection {
    pub fn live(&self) -> bool { self.phase == Phase::Ready && self.seeded.iter().all(|v| *v) }
    fn request(&mut self, kind: &str, data: Value, input: Option<String>, now: i64) -> Value {
        self.next += 1;
        let id = self.next.to_string();
        self.pending.insert(id.clone(), Pending { kind: kind.into(), input, at: now });
        json!({"op":6,"d":{"requestType":kind,"requestId":id,"requestData":data}})
    }
    /// Bound outstanding requests on a slow server. The guest samples only while visible.
    pub fn sample(&mut self, now: i64) -> Result<Vec<Value>, String> {
        if self.phase != Phase::Ready { return Ok(vec![]); }
        if self.pending.values().any(|p| now.saturating_sub(p.at) > 15_000) { return Err("OBS stopped responding".into()); }
        let mut out = vec![];
        for kind in ["GetStats", "GetStreamStatus"] {
            if !self.pending.values().any(|p| p.kind == kind) { out.push(self.request(kind, json!({}), None, now)); }
        }
        Ok(out)
    }
    pub fn action(&mut self, action: Action, now: i64) -> Option<Value> {
        if !self.live() || self.pending.len() >= 64 { return None; }
        let (kind, data) = match action {
            Action::StartRecording => ("StartRecord", json!({})),
            Action::StopRecording => ("StopRecord", json!({})),
            Action::PauseRecording => ("PauseRecord", json!({})),
            Action::ResumeRecording => ("ResumeRecord", json!({})),
            Action::StartStreaming => ("StartStream", json!({})),
            Action::StopStreaming => ("StopStream", json!({})),
            Action::SwitchScene(name) if self.state.scenes.contains(&name) => ("SetCurrentProgramScene", json!({"sceneName": name})),
            Action::MuteInput { name, muted } if self.state.microphones.iter().any(|m| m.name == name) => ("SetInputMute", json!({"inputName":name,"inputMuted":muted})),
            _ => return None,
        };
        self.notice = None;
        Some(self.request(kind, data, None, now))
    }
    pub fn receive(&mut self, frame: &Value, password: Option<&str>, now: i64) -> Result<Vec<Value>, String> {
        let d = &frame["d"];
        match frame["op"].as_u64() {
            Some(0) if self.phase == Phase::Hello => {
                if !d["rpcVersion"].as_u64().is_some_and(|v| v >= 1) { return Err("Unsupported OBS WebSocket version".into()); }
                let mut identify = json!({"op":1,"d":{"rpcVersion":1,"eventSubscriptions":76}});
                if let Some(auth) = d.get("authentication") {
                    let password = password.ok_or("Save the OBS WebSocket password below")?;
                    let salt = auth["salt"].as_str().ok_or("Missing OBS authentication salt")?;
                    let challenge = auth["challenge"].as_str().ok_or("Missing OBS authentication challenge")?;
                    identify["d"]["authentication"] = authentication(password, salt, challenge).into();
                }
                self.phase = Phase::Identify;
                Ok(vec![identify])
            }
            Some(2) if self.phase == Phase::Identify => {
                self.phase = Phase::Ready;
                Ok(["GetRecordStatus", "GetStreamStatus", "GetCurrentProgramScene", "GetSceneList", "GetInputList", "GetStats"]
                    .into_iter().map(|kind| self.request(kind, json!({}), None, now)).collect())
            }
            Some(7) if self.phase == Phase::Ready => {
                let Some(pending) = d["requestId"].as_str().and_then(|id| self.pending.remove(id)) else { return Ok(vec![]); };
                if d["requestType"].as_str() != Some(pending.kind.as_str()) { return Err("Unexpected OBS response".into()); }
                if d["requestStatus"]["result"].as_bool() != Some(true) {
                    if pending.kind == "GetStats" { self.state.stats.cpu_tenths = None; return Ok(vec![]); }
                    let why = format!("OBS: {}", d["requestStatus"]["comment"].as_str().unwrap_or("request rejected"));
                    if !self.live() { return Err(why); }
                    self.notice = Some(why);
                    return Ok(vec![]);
                }
                let r = &d["responseData"];
                let mut out = vec![];
                match pending.kind.as_str() {
                    "GetRecordStatus" => {
                        self.state.recording = r["outputActive"].as_bool().unwrap_or(false);
                        self.state.paused = r["outputPaused"].as_bool().unwrap_or(false);
                        self.state.record_clock.seed(r, self.state.recording && !self.state.paused, now);
                        self.seeded[0] = true;
                    }
                    "GetStreamStatus" => {
                        self.state.streaming = r["outputActive"].as_bool().unwrap_or(false);
                        self.state.stream_clock.seed(r, self.state.streaming, now);
                        self.state.stats.dropped_frames = r["outputSkippedFrames"].as_u64();
                        self.state.stats.bitrate_kbps = if self.state.streaming {
                            r["outputBytes"].as_u64().and_then(|bytes| {
                                let previous = self.stream_sample.replace((bytes, now));
                                previous.filter(|(before, at)| bytes >= *before && now - at >= 1000)
                                    .map(|(before, at)| ((bytes - before) as u128 * 8 / (now - at) as u128).min(u64::MAX as u128) as u64)
                            })
                        } else { self.stream_sample = None; Some(0) };
                        self.seeded[1] = true;
                    }
                    "GetCurrentProgramScene" => { self.state.scene = r["currentProgramSceneName"].as_str().unwrap_or_default().into(); self.seeded[2] = true; }
                    "GetSceneList" => {
                        self.state.scenes = r["scenes"].as_array().into_iter().flatten().filter_map(|v| v["sceneName"].as_str()).take(128).map(str::to_owned).collect();
                        if let Some(name) = r["currentProgramSceneName"].as_str() { self.state.scene = name.into(); }
                        self.seeded[3] = true;
                    }
                    "GetInputList" => {
                        self.pending.retain(|_, p| p.kind != "GetInputMute");
                        self.state.microphones = r["inputs"].as_array().into_iter().flatten()
                            .filter(|v| matches!(v["inputKind"].as_str(), Some("wasapi_input_capture" | "pulse_input_capture" | "coreaudio_input_capture")))
                            .filter_map(|v| v["inputName"].as_str()).take(16).map(|name| Microphone { name: name.into(), muted: false }).collect();
                        let names: Vec<_> = self.state.microphones.iter().map(|m| m.name.clone()).collect();
                        for name in names { out.push(self.request("GetInputMute", json!({"inputName":name}), Some(name), now)); }
                        self.seeded[4] = self.state.microphones.is_empty();
                    }
                    "GetInputMute" => {
                        if let Some(input) = self.state.microphones.iter_mut().find(|m| Some(&m.name) == pending.input.as_ref()) { input.muted = r["inputMuted"].as_bool().unwrap_or(false); }
                        self.seeded[4] = !self.pending.values().any(|p| p.kind == "GetInputMute");
                    }
                    "GetStats" => self.state.stats.apply(r),
                    "StartRecord" | "StopRecord" | "PauseRecord" | "ResumeRecord" => out.push(self.request("GetRecordStatus", json!({}), None, now)),
                    "StartStream" | "StopStream" => out.push(self.request("GetStreamStatus", json!({}), None, now)),
                    "SetCurrentProgramScene" => out.push(self.request("GetCurrentProgramScene", json!({}), None, now)),
                    "SetInputMute" => out.push(self.request("GetInputList", json!({}), None, now)),
                    _ => {}
                }
                Ok(out)
            }
            Some(5) if self.phase == Phase::Ready => {
                let e = &d["eventData"];
                let refresh = match d["eventType"].as_str() {
                    Some("ExitStarted") => return Err("OBS isn't running".into()),
                    Some("RecordStateChanged") => {
                        self.state.record_clock.freeze(now);
                        self.state.recording = e["outputActive"].as_bool().unwrap_or(false);
                        self.state.paused = e["outputState"].as_str() == Some("OBS_WEBSOCKET_OUTPUT_PAUSED");
                        Some("GetRecordStatus")
                    }
                    Some("StreamStateChanged") => { self.state.stream_clock.freeze(now); self.state.streaming = e["outputActive"].as_bool().unwrap_or(false); Some("GetStreamStatus") }
                    Some("CurrentProgramSceneChanged") => { self.state.scene = e["sceneName"].as_str().unwrap_or_default().into(); None }
                    Some("SceneListChanged" | "SceneNameChanged") => Some("GetSceneList"),
                    Some("InputCreated" | "InputRemoved" | "InputNameChanged") => Some("GetInputList"),
                    Some("InputMuteStateChanged") => {
                        if let Some(input) = self.state.microphones.iter_mut().find(|m| Some(m.name.as_str()) == e["inputName"].as_str()) { input.muted = e["inputMuted"].as_bool().unwrap_or(false); }
                        None
                    }
                    _ => None,
                };
                Ok(refresh.filter(|kind| !self.pending.values().any(|p| p.kind == *kind)).map(|kind| self.request(kind, json!({}), None, now)).into_iter().collect())
            }
            _ => Ok(vec![]),
        }
    }
}
