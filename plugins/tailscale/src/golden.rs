use crate::{State, faces, tailscale};
use glaze_plugin_sdk::SizeClass;
use serde_json::{Map, Value, json};

pub const NOW: i64 = 1_793_875_200_000;
pub fn fixture() -> State {
    let mut data = tailscale::Snapshot::default();
    tailscale::apply(&mut data, &json!({"InitialStatus": {
        "BackendState":"Running", "TailscaleIPs":["100.64.0.24"], "ExitNodeStatus":{"ID":"home"},
        "Peer": {
            "a":{"ID":"mac","HostName":"macbook","OS":"macOS","Online":true,"RxBytes":1073741824_u64,"TxBytes":52428800},
            "b":{"ID":"home","HostName":"home-server","OS":"Linux","Online":true,"ExitNode":true,"ExitNodeOption":true,"RxBytes":104857600,"TxBytes":8388608},
            "c":{"ID":"phone","HostName":"phone","OS":"iOS","Online":true,"RxBytes":1024,"TxBytes":2048},
            "d":{"ID":"pc","HostName":"workstation","OS":"Windows","Online":false,"LastSeen":"2026-11-04T08:00:00Z","RxBytes":4096,"TxBytes":8192}
        }
    }}));
    State { data: Some(data), ..Default::default() }
}

pub fn faces() -> Value {
    let mut result = Map::new();
    for id in ["connection", "peers", "traffic", "disconnected", "unavailable", "empty"] {
        let mut state = fixture();
        if id == "disconnected" { state.data.as_mut().unwrap().state = "Stopped".into(); }
        if id == "unavailable" { state.error = Some("Tailscale service is unavailable".into()); }
        if id == "empty" { state.data = Some(tailscale::Snapshot { state: "Stopped".into(), peers_known: true, ..Default::default() }); }
        let sizes: Map<String, Value> = [("small", SizeClass::Small), ("medium", SizeClass::Medium), ("large", SizeClass::Large)].into_iter().map(|(name, size)| (name.into(), json!(faces::face(id, size, &state, NOW)))).collect();
        result.insert(id.into(), sizes.into());
    }
    result.into()
}
#[test]
fn faces_match_the_view_golden() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("golden/faces.json");
    let actual = serde_json::to_string_pretty(&faces()).unwrap() + "\n";
    if std::env::var_os("GLAZE_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &actual).unwrap();
    }
    assert_eq!(std::fs::read_to_string(path).expect("bless the view golden").replace("\r\n", "\n"), actual);
}
