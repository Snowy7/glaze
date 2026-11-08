use crate::{Availability, Data, Snapshot, obs};
use glaze_plugin_sdk::SizeClass;
use serde_json::json;
pub const NOW: i64 = 1_793_875_200_000;
pub fn fixture() -> Snapshot {
    Snapshot { availability: Availability::Live, observed_at_ms: NOW, data: Some(Data::Obs(obs::Snapshot {
        recording: true, streaming: true, scene: "Desktop + camera".into(),
        scenes: ["Desktop + camera", "Camera", "Be right back"].map(str::to_owned).into(),
        microphones: vec![obs::Microphone { name: "Desk microphone".into(), muted: false }],
        record_clock: obs::Clock { elapsed_ms: 3_965_000, at_ms: NOW, advancing: true },
        stream_clock: obs::Clock { elapsed_ms: 825_000, at_ms: NOW, advancing: true },
        stats: obs::Stats { cpu_tenths: Some(24), dropped_frames: Some(0), bitrate_kbps: Some(6000) },
        ..Default::default()
    })) }
}
#[test]
fn faces_match_the_view_golden() {
    let snapshot = fixture();
    let sizes: serde_json::Map<String, serde_json::Value> = [("small", SizeClass::Small), ("medium", SizeClass::Medium), ("large", SizeClass::Large)]
        .into_iter().map(|(name, size)| (name.into(), json!(crate::faces::face(size, &snapshot, NOW)))).collect();
    let offline = Snapshot { availability: Availability::Unavailable("OBS isn't running".into()), data: None, observed_at_ms: NOW };
    let offline_sizes: serde_json::Map<String, serde_json::Value> = [("small", SizeClass::Small), ("medium", SizeClass::Medium), ("large", SizeClass::Large)]
        .into_iter().map(|(name, size)| (name.into(), json!(crate::faces::face(size, &offline, NOW)))).collect();
    let actual = serde_json::to_string_pretty(&json!({"controls": sizes, "offline": offline_sizes})).unwrap() + "\n";
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("golden/faces.json");
    if std::env::var_os("GLAZE_BLESS").is_some() { std::fs::write(&path, &actual).unwrap(); }
    assert_eq!(std::fs::read_to_string(path).unwrap().replace("\r\n", "\n"), actual);
}
