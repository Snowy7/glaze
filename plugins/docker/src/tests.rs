use crate::*;
use glaze_plugin_sdk::SizeClass;
use serde_json::json;

#[test]
fn actions_require_a_live_snapshot_and_correct_state() {
    let mut snapshot = golden::fixture();
    let path = |action| action_path(&snapshot, false, action);
    assert_eq!(path("stop:web").as_deref(), Some("/containers/web/stop"));
    assert_eq!(
        path("start:worker").as_deref(),
        Some("/containers/worker/start")
    );
    assert!(path("start:web").is_none());
    assert!(path("stop:missing").is_none());
    assert!(path("remove:web").is_none());
    assert!(action_path(&snapshot, true, "stop:web").is_none());
    snapshot.availability = Availability::Stale("offline".into());
    assert!(action_path(&snapshot, false, "stop:web").is_none());
    assert_eq!(encode("a/b?x=1"), "a%2Fb%3Fx%3D1");
    assert!(docker::snapshot(&json!(vec![json!({}); 129])).is_err());
}

#[test]
fn container_failures_and_samples_preserve_native_meaning() {
    let state = docker::snapshot(&json!([
        {"Id":"ok","Names":["/web"],"State":"exited","Status":"Exited (0) 1 minute ago"},
        {"Id":"bad","Names":["/worker"],"State":"exited","Status":"Exited (137) 1 minute ago"},
        {"Id":"sick","Names":["/db"],"State":"running","Status":"Up (unhealthy)"}
    ]))
    .unwrap();
    assert_eq!(
        state
            .containers
            .iter()
            .map(|c| c.failed)
            .collect::<Vec<_>>(),
        [false, true, true]
    );
    assert_eq!(state.containers[0].name, "web");
    assert!(docker::snapshot(&json!([{"State":"running"}])).is_err());
    let stats = docker::usage(
        &json!({"cpu_stats":{"cpu_usage":{"total_usage":500},"system_cpu_usage":2000,"online_cpus":4},"precpu_stats":{"cpu_usage":{"total_usage":400},"system_cpu_usage":1000},"memory_stats":{"usage":4096,"limit":8192,"stats":{"inactive_file":1024}}}),
    );
    assert_eq!(stats.cpu_hundredths, Some(4000));
    assert_eq!(stats.memory_bytes, 3072);
    assert_eq!(docker::usage(&json!({})).cpu_hundredths, None);
}

#[test]
fn faces_target_stable_container_ids_and_disable_stale_controls() {
    let mut snapshot = golden::fixture();
    let wire = serde_json::to_string(&faces::face(SizeClass::Large, &snapshot)).unwrap();
    assert!(wire.contains("stop:web") && wire.contains("start:worker"));
    assert!(wire.contains("19%") && wire.contains("576"));
    snapshot.availability = Availability::Stale("disconnected".into());
    let wire = serde_json::to_value(faces::face(SizeClass::Large, &snapshot)).unwrap();
    let rows = wire["body"]["children"][1]["children"].as_array().unwrap();
    assert!(
        rows.iter()
            .all(|row| row["children"][1]["enabled"] == false)
    );
    let manifest: serde_json::Value = serde_json::from_str(include_str!("../plugin.json")).unwrap();
    assert_eq!(manifest["faces"].as_array().unwrap().len(), 1);
    assert_eq!(manifest["permissions"], json!(["pipe:docker_engine"]));
}
