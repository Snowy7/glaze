use crate::{Data, faces, golden, obs};
use mypaper_plugin_sdk::SizeClass;
use serde_json::json;

#[test]
fn clocks_freeze_when_paused_and_disconnected() {
    let mut snapshot = golden::fixture();
    let Some(Data::Obs(state)) = &mut snapshot.data else { unreachable!() };
    state.record_clock.advancing = false;
    assert_eq!(state.record_clock.elapsed(golden::NOW + 100_000), 3_965_000);
    snapshot.availability = crate::Availability::Stale("offline".into());
    assert_eq!(faces::face(SizeClass::Large, &snapshot, golden::NOW), faces::face(SizeClass::Large, &snapshot, golden::NOW + 100_000));
}
#[test]
fn controls_scene_and_mic_actions_use_current_state() {
    let fixture = golden::fixture();
    assert!(matches!(faces::action("record.stop", fixture.data.as_ref()), Some(obs::Action::StopRecording)));
    assert!(matches!(faces::action("record.pause", fixture.data.as_ref()), Some(obs::Action::PauseRecording)));
    assert!(matches!(faces::action("stream.stop", fixture.data.as_ref()), Some(obs::Action::StopStreaming)));
    assert!(matches!(faces::action("scene:1", fixture.data.as_ref()), Some(obs::Action::SwitchScene(name)) if name == "Camera"));
    assert!(matches!(faces::action("mic:0", fixture.data.as_ref()), Some(obs::Action::MuteInput { muted: true, .. })));
    assert!(faces::action("scene:128", fixture.data.as_ref()).is_none());
    assert!(faces::action("record.start", fixture.data.as_ref()).is_none());
}
#[test]
fn obs_auth_events_and_optional_stats() {
    assert_eq!(obs::authentication("password", "salt", "challenge"), "zTM5ki6L2vVvBQiTG9ckH1Lh64AbnCf6XZ226UmnkIA=");
    let mut state = obs::Snapshot::default();
    state.stats.apply(&json!({"cpuUsage":2.4}));
    assert_eq!(state.stats.cpu_tenths, Some(24));
    state.stats.apply(&json!({}));
    assert_eq!(state.stats.cpu_tenths, None);
}

fn connected() -> (obs::Connection, Vec<serde_json::Value>) {
    let mut connection = obs::Connection::default();
    let identify = connection.receive(&json!({"op":0,"d":{"rpcVersion":1}}), None, 0).unwrap();
    assert_eq!(identify[0]["op"], 1);
    let requests = connection.receive(&json!({"op":2,"d":{"negotiatedRpcVersion":1}}), None, 0).unwrap();
    (connection, requests)
}
fn reply(connection: &mut obs::Connection, request: &serde_json::Value, data: serde_json::Value, now: i64) -> Vec<serde_json::Value> {
    connection.receive(&json!({"op":7,"d":{"requestId":request["d"]["requestId"],"requestType":request["d"]["requestType"],"requestStatus":{"result":true},"responseData":data}}), None, now).unwrap()
}
#[test]
fn handshake_seed_mic_correlation_actions_and_stats() {
    let (mut connection, requests) = connected();
    for request in &requests {
        let data = match request["d"]["requestType"].as_str().unwrap() {
            "GetRecordStatus" => json!({"outputActive":true,"outputDuration":1000}),
            "GetStreamStatus" => json!({"outputActive":true,"outputDuration":2000,"outputBytes":1000,"outputSkippedFrames":2}),
            "GetCurrentProgramScene" => json!({"currentProgramSceneName":"Desktop"}),
            "GetSceneList" => json!({"scenes":[{"sceneName":"Desktop"},{"sceneName":"Camera"}]}),
            "GetInputList" => json!({"inputs":[{"inputKind":"wasapi_input_capture","inputName":"Desk mic"}]}),
            "GetStats" => json!({"cpuUsage":2.4}),
            _ => unreachable!(),
        };
        let more = reply(&mut connection, request, data, 0);
        for request in more { reply(&mut connection, &request, json!({"inputMuted":true}), 0); }
    }
    assert!(connection.live());
    assert!(connection.state.microphones[0].muted);
    assert_eq!(connection.state.stats.cpu_tenths, Some(24));
    assert_eq!(connection.state.stats.dropped_frames, Some(2));
    let switch = connection.action(obs::Action::SwitchScene("Camera".into()), 1000).unwrap();
    assert_eq!(switch["d"]["requestData"]["sceneName"], "Camera");
    let mute = connection.action(obs::Action::MuteInput { name: "Desk mic".into(), muted: false }, 1000).unwrap();
    assert_eq!(mute["d"]["requestData"]["inputMuted"], false);
    connection.receive(&json!({"op":5,"d":{"eventType":"CurrentProgramSceneChanged","eventData":{"sceneName":"Camera"}}}), None, 1000).unwrap();
    assert_eq!(connection.state.scene, "Camera");
    let sample = connection.sample(5000).unwrap();
    assert_eq!(sample.len(), 2);
    assert!(connection.sample(5001).unwrap().is_empty());
    for request in sample {
        if request["d"]["requestType"] == "GetStreamStatus" {
            reply(&mut connection, &request, json!({"outputActive":true,"outputBytes":3_751_000,"outputSkippedFrames":3}), 5000);
        }
    }
    assert_eq!(connection.state.stats.bitrate_kbps, Some(6000));
    assert!(connection.sample(30_000).is_err());
}
#[test]
fn authentication_is_required_only_when_obs_challenges() {
    let frame = json!({"op":0,"d":{"rpcVersion":1,"authentication":{"salt":"salt","challenge":"challenge"}}});
    let mut connection = obs::Connection::default();
    assert!(connection.receive(&frame, None, 0).unwrap_err().contains("password"));
    let messages = connection.receive(&frame, Some("password"), 0).unwrap();
    assert_eq!(messages[0]["d"]["authentication"], obs::authentication("password", "salt", "challenge"));
}
