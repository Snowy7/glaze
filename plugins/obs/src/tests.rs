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
    obs::apply_event(&mut state, &json!({"eventType":"CurrentProgramSceneChanged","eventData":{"sceneName":"Camera"}}));
    assert_eq!(state.scene, "Camera");
    state.stats.apply(&json!({"cpuUsage":2.4}));
    assert_eq!(state.stats.cpu_tenths, Some(24));
    state.stats.apply(&json!({}));
    assert_eq!(state.stats.cpu_tenths, None);
}
