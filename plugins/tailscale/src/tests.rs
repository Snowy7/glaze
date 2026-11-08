use super::*;
use serde_json::json;
use glaze_plugin_sdk::SizeClass;

#[test]
fn disconnected_or_unreachable_service_never_claims_a_green_connection() {
    for (backend, error) in [("Stopped", None), ("NeedsLogin", None), ("Running", Some("pipe closed".into()))] {
        let state = State { data: Some(tailscale::Snapshot { state: backend.into(), ..Default::default() }), error, ..Default::default() };
        for size in [SizeClass::Small, SizeClass::Medium, SizeClass::Large] {
            for id in ["connection", "peers", "traffic", "status"] {
                let wire = serde_json::to_string(&faces::face(id, size, &state, golden::NOW)).unwrap();
                assert!(!wire.contains("Connected"));
                assert!(!wire.contains("\"positive\""));
            }
        }
    }
}

#[test]
fn peer_metadata_unknowns_and_preferences_are_preserved() {
    let state = golden::fixture().data.unwrap();
    assert_eq!(state.peers.len(), 4);
    assert_eq!(state.exit_node.as_deref(), Some("home-server"));
    assert_eq!(state.peers.iter().find(|p| p.id == "pc").unwrap().os.as_deref(), Some("Windows"));
    assert!(state.rx_bytes.unwrap() > 1 << 30);
    assert_eq!(tailscale::prefs(&Action::Connected(false)).unwrap(), json!({"WantRunning":false,"WantRunningSet":true}));
    let clear = tailscale::prefs(&Action::ExitNode(String::new())).unwrap();
    assert_eq!(clear["ExitNodeID"], "");
    assert_eq!(clear["ExitNodeIPSet"], true);
    assert!(tailscale::prefs(&Action::Refresh).is_none());
    assert_eq!(faces::bytes(None), "Unavailable");
    assert_eq!(faces::bytes(Some(0)), "0 B");
    assert!(allowed_exit(&golden::fixture(), "home"));
    assert!(allowed_exit(&golden::fixture(), ""));
    assert!(!allowed_exit(&golden::fixture(), "pc"));
    assert!(!allowed_exit(&golden::fixture(), "untrusted"));
}
