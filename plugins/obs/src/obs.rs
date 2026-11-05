//! Ported obs-websocket 5 provider. Stats sample every five seconds while a face is open.
use crate::{Availability, Data, Publish, credentials, transport};
use anyhow::{Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::sync::mpsc;

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
pub struct Microphone {
    pub name: String,
    pub muted: bool,
}
/// A server duration anchored once; the visible widget advances it without querying OBS.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Clock {
    pub elapsed_ms: u64,
    pub at_ms: i64,
    pub advancing: bool,
}
impl Clock {
    pub fn elapsed(&self, now_ms: i64) -> u64 {
        self.elapsed_ms.saturating_add(if self.advancing {
            now_ms.saturating_sub(self.at_ms).max(0) as u64
        } else {
            0
        })
    }
    fn seed(&mut self, data: &Value, advancing: bool) {
        self.elapsed_ms = data["outputDuration"].as_u64().unwrap_or(0);
        self.at_ms = epoch_ms();
        self.advancing = advancing;
    }
}
fn epoch_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}
#[derive(Clone, Debug)]
pub enum Action {
    StartRecording,
    StopRecording,
    PauseRecording,
    ResumeRecording,
    StartStreaming,
    StopStreaming,
    SwitchScene(String),
    MuteInput { name: String, muted: bool },
}
impl Action {
    fn request(&self) -> &'static str {
        match self {
            Self::StartRecording => "StartRecord",
            Self::StopRecording => "StopRecord",
            Self::PauseRecording => "PauseRecord",
            Self::ResumeRecording => "ResumeRecord",
            Self::StartStreaming => "StartStream",
            Self::StopStreaming => "StopStream",
            Self::SwitchScene(_) => "SetCurrentProgramScene",
            Self::MuteInput { .. } => "SetInputMute",
        }
    }
}
pub fn authentication(password: &str, salt: &str, challenge: &str) -> String {
    let secret = STANDARD.encode(Sha256::digest(format!("{password}{salt}")));
    STANDARD.encode(Sha256::digest(format!("{secret}{challenge}")))
}
fn request(name: &str, id: u64) -> Value {
    json!({"op":6,"d":{"requestType":name,"requestId":id.to_string()}})
}

pub(crate) async fn run(
    config: &crate::config::Obs,
    publish: &Publish,
    commands: &mut mpsc::Receiver<crate::Action>,
) -> Result<()> {
    let mut socket = transport::socket(&config.endpoint, None, None).await?;
    let hello = transport::receive(&mut socket).await?;
    ensure!(
        hello["op"] == 0 && hello["d"]["rpcVersion"].as_u64().is_some_and(|v| v >= 1),
        "unsupported OBS protocol"
    );
    let mut identify = json!({"op":1,"d":{"rpcVersion":1,"eventSubscriptions":76}}); // Outputs | Scenes | Inputs, no meters.
    if let Some(auth) = hello["d"].get("authentication") {
        let password = credentials::read("obs")?;
        identify["d"]["authentication"] = authentication(
            password.text()?,
            auth["salt"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("missing OBS salt"))?,
            auth["challenge"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("missing OBS challenge"))?,
        )
        .into();
    }
    transport::send(&mut socket, identify).await?;
    ensure!(
        transport::receive(&mut socket).await?["op"] == 2,
        "OBS identification failed"
    );
    let mut next_id = 1;
    for name in [
        "GetRecordStatus",
        "GetStreamStatus",
        "GetCurrentProgramScene",
        "GetSceneList",
        "GetInputList",
    ] {
        transport::send(&mut socket, request(name, next_id)).await?;
        next_id += 1;
    }
    let mut state = Snapshot::default();
    let mut seeded = [false; 5];
    let mut input_requests = std::collections::BTreeMap::new();
    let mut sample = tokio::time::interval(std::time::Duration::from_secs(5));
    sample.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut stream_bytes: Option<(u64, std::time::Instant)> = None;
    let mut pending_stats = std::collections::BTreeSet::new();
    loop {
        tokio::select! {
            _ = sample.tick() => {
                // Keep at most one outstanding sample of each kind on a slow server.
                for name in ["GetStats", "GetStreamStatus"] {
                    if pending_stats.insert(name) {
                        transport::send(&mut socket, request(name, next_id)).await?;
                        next_id += 1;
                    }
                }
            }
            Some(action) = commands.recv() => {
                {
                    let crate::Action::Obs(action) = action;
                    let mut message = request(action.request(), next_id);
                    match &action {
                        Action::SwitchScene(name) => {
                            if !state.scenes.contains(name) { publish.action_result(Err(anyhow::anyhow!("scene is no longer available"))); continue; }
                            message["d"]["requestData"] = json!({"sceneName":name});
                        }
                        Action::MuteInput { name, muted } => {
                            if !state.microphones.iter().any(|input| &input.name == name) { publish.action_result(Err(anyhow::anyhow!("microphone is no longer available"))); continue; }
                            message["d"]["requestData"] = json!({"inputName":name,"inputMuted":muted});
                        }
                        _ => {}
                    }
                    transport::send(&mut socket, message).await?;
                    next_id += 1;
                }
            }
            frame = transport::receive(&mut socket) => {
                let frame = frame?;
                publish.delivered();
                let data = &frame["d"];
                match frame["op"].as_u64() {
                    Some(7) => {
                        let ok = data["requestStatus"]["result"].as_bool().unwrap_or(false);
                        if let Some(name @ ("GetStats" | "GetStreamStatus")) = data["requestType"].as_str() {
                            pending_stats.remove(name);
                        }
                        if !ok {
                            if data["requestType"] == "GetStats" { continue; }
                            publish.action_result(Err(anyhow::anyhow!("OBS request rejected ({})", data["requestStatus"]["code"])));
                            ensure!(data["requestId"].as_str().and_then(|s| s.parse::<u64>().ok()).is_none_or(|id| id > 5), "OBS initial state request failed");
                            continue;
                        }
                        let result = &data["responseData"];
                        match data["requestType"].as_str() {
                            Some("GetRecordStatus") => { state.recording = result["outputActive"].as_bool().unwrap_or(false); state.paused = result["outputPaused"].as_bool().unwrap_or(false); state.record_clock.seed(result, state.recording && !state.paused); seeded[0] = true; }
                            Some("GetStats") => state.stats.apply(result),
                            Some("GetStreamStatus") => {
                                state.streaming = result["outputActive"].as_bool().unwrap_or(false);
                                state.stream_clock.seed(result, state.streaming);
                                state.stats.dropped_frames = result["outputSkippedFrames"].as_u64();
                                if let Some(bytes) = result["outputBytes"].as_u64() {
                                    let now = std::time::Instant::now();
                                    state.stats.bitrate_kbps = if !state.streaming { Some(0) } else {
                                        stream_bytes.filter(|(before, _)| bytes >= *before).and_then(|(before, at)| {
                                            let elapsed = now.duration_since(at).as_millis();
                                            (elapsed >= 1000).then(|| ((bytes - before) as u128 * 8 / elapsed) as u64)
                                        }).or(state.stats.bitrate_kbps)
                                    };
                                    stream_bytes = Some((bytes, now));
                                }
                                seeded[1] = true;
                            }
                            Some("GetCurrentProgramScene") => { state.scene = result["currentProgramSceneName"].as_str().unwrap_or_default().into(); seeded[2] = true; }
                            Some("GetSceneList") => { state.scenes = result["scenes"].as_array().into_iter().flatten().filter_map(|scene| scene["sceneName"].as_str()).take(128).map(str::to_owned).collect(); seeded[3] = true; }
                            Some("GetInputList") => {
                                input_requests.clear();
                                state.microphones = result["inputs"].as_array().into_iter().flatten().filter(|input| matches!(input["inputKind"].as_str(), Some("wasapi_input_capture" | "pulse_input_capture" | "coreaudio_input_capture"))).filter_map(|input| input["inputName"].as_str()).take(16).map(|name| Microphone { name: name.into(), muted: false }).collect();
                                for input in &state.microphones {
                                    let mut message = request("GetInputMute", next_id);
                                    message["d"]["requestData"] = json!({"inputName":input.name});
                                    input_requests.insert(next_id, input.name.clone());
                                    transport::send(&mut socket, message).await?;
                                    next_id += 1;
                                }
                                seeded[4] = state.microphones.is_empty();
                            }
                            Some("GetInputMute") => {
                                // OBS echoes neither the request data nor name: retain the request id mapping below.
                                if let Some(id) = data["requestId"].as_str().and_then(|id| id.parse::<u64>().ok())
                                    && let Some(name) = input_requests.remove(&id)
                                    && let Some(input) = state.microphones.iter_mut().find(|input| input.name == name) {
                                    input.muted = result["inputMuted"].as_bool().unwrap_or(false);
                                }
                                seeded[4] = input_requests.is_empty();
                            }
                            _ => publish.action_result(Ok(())),
                        }
                    }
                    Some(5) => {
                        apply_event(&mut state, data);
                        let refresh = match data["eventType"].as_str() {
                            Some("RecordStateChanged") => Some("GetRecordStatus"),
                            Some("StreamStateChanged") => Some("GetStreamStatus"),
                            Some("SceneListChanged") => Some("GetSceneList"),
                            Some("InputCreated" | "InputRemoved" | "InputNameChanged") => Some("GetInputList"),
                            _ => None,
                        };
                        if let Some(name) = refresh { transport::send(&mut socket, request(name, next_id)).await?; next_id += 1; }
                    }
                    _ => {}
                }
                if seeded.iter().all(|v| *v) { publish.set(Availability::Live, Some(Data::Obs(state.clone()))); }
            }
        }
    }
}
pub fn apply_event(state: &mut Snapshot, data: &Value) {
    let event = &data["eventData"];
    match data["eventType"].as_str() {
        Some("RecordStateChanged") => {
            state.recording = event["outputActive"].as_bool().unwrap_or(false);
            state.paused = event["outputState"].as_str() == Some("OBS_WEBSOCKET_OUTPUT_PAUSED");
        }
        Some("StreamStateChanged") => {
            state.streaming = event["outputActive"].as_bool().unwrap_or(false)
        }
        Some("CurrentProgramSceneChanged") => {
            state.scene = event["sceneName"].as_str().unwrap_or_default().into()
        }
        Some("InputMuteStateChanged") => {
            if let Some(input) = state
                .microphones
                .iter_mut()
                .find(|input| Some(input.name.as_str()) == event["inputName"].as_str())
            {
                input.muted = event["inputMuted"].as_bool().unwrap_or(false);
            }
        }
        _ => {}
    }
}
