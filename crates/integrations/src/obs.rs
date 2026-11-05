//! obs-websocket 5: output/scene events only; no meters, stats, previews or idle pings.
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
}
#[derive(Clone, Debug)]
pub enum Action {
    StartRecording,
    StopRecording,
    PauseRecording,
    ResumeRecording,
    StartStreaming,
    StopStreaming,
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
    let mut identify = json!({"op":1,"d":{"rpcVersion":1,"eventSubscriptions":68}}); // Outputs | Scenes, no high volume flags.
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
    ] {
        transport::send(&mut socket, request(name, next_id)).await?;
        next_id += 1;
    }
    let mut state = Snapshot::default();
    let mut seeded = [false; 3];
    loop {
        tokio::select! {
            Some(action) = commands.recv() => {
                if let crate::Action::Obs(action) = action {
                    transport::send(&mut socket, request(action.request(), next_id)).await?;
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
                        if !ok {
                            publish.action_result(Err(anyhow::anyhow!("OBS request rejected ({})", data["requestStatus"]["code"])));
                            ensure!(data["requestId"].as_str().and_then(|s| s.parse::<u64>().ok()).is_none_or(|id| id > 3), "OBS initial state request failed");
                            continue;
                        }
                        let result = &data["responseData"];
                        match data["requestType"].as_str() {
                            Some("GetRecordStatus") => { state.recording = result["outputActive"].as_bool().unwrap_or(false); state.paused = result["outputPaused"].as_bool().unwrap_or(false); seeded[0] = true; }
                            Some("GetStreamStatus") => { state.streaming = result["outputActive"].as_bool().unwrap_or(false); seeded[1] = true; }
                            Some("GetCurrentProgramScene") => { state.scene = result["currentProgramSceneName"].as_str().unwrap_or_default().into(); seeded[2] = true; }
                            _ => publish.action_result(Ok(())),
                        }
                    }
                    Some(5) => apply_event(&mut state, data),
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
        _ => {}
    }
}
