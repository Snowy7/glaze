use mypaper_integrations::{config::Config, docker, launcher, obs, t3, tailscale};
use serde_json::json;

fn thread(status: &str, run: &str, pending: serde_json::Value) -> serde_json::Value {
    json!({"id":"a","projectId":"p","title":"A thread","providerInstanceId":"codex","status":status,"latestRunId":run,"activeRunId":null,"pendingRuntimeRequest":pending,"updatedAt":"2026-11-05T00:00:00Z"})
}
#[test]
fn t3_baselines_enrichment_sequences_and_distinct_run_completions() {
    let mut state = t3::Snapshot::default();
    state.apply(&json!({"kind":"snapshot","snapshot":{"schemaVersion":1,"snapshotSequence":10,"projects":[{"id":"p","title":"mypaper"}],"threads":[thread("completed","r1",serde_json::Value::Null)]}})).unwrap();
    assert!(state.transitions.is_empty());
    state.apply(&json!({"kind":"snapshot","resolvedRepositoryIdentityRoots":["root"],"snapshot":{"schemaVersion":1,"snapshotSequence":10,"projects":[],"threads":[]}})).unwrap();
    assert_eq!(state.threads.len(), 1);
    state.apply(&json!({"kind":"thread.updated","location":"active","sequence":15,"thread":thread("waiting","r2",serde_json::Value::Null)})).unwrap();
    assert!(!state.threads[0].attention());
    assert!(state.threads[0].running());
    state.apply(&json!({"kind":"thread.updated","location":"active","sequence":16,"thread":thread("waiting","r2",json!({"id":"q","kind":"user-input"}))})).unwrap();
    assert!(state.threads[0].attention());
    assert_eq!(state.transitions.len(), 1);
    let done = json!({"kind":"thread.updated","location":"active","sequence":17,"thread":thread("failed","r2",serde_json::Value::Null)});
    state.apply(&done).unwrap();
    state.apply(&done).unwrap();
    assert_eq!(state.transitions.len(), 2);
    assert_eq!(state.transitions[1].status, t3::Status::Failed);
    state.apply(&json!({"kind":"thread.updated","location":"active","sequence":22,"thread":thread("cancelled","r3",serde_json::Value::Null)})).unwrap();
    assert_eq!(state.transitions.len(), 3);
    assert_eq!(state.transitions[2].run_id, "r3");
    state
        .apply(&json!({"kind":"thread.removed","location":"active","sequence":23,"threadId":"a"}))
        .unwrap();
    assert!(state.threads.is_empty());
}
#[test]
fn obs_events_do_not_use_duration_or_meters() {
    let mut state = obs::Snapshot::default();
    obs::apply_event(
        &mut state,
        &json!({"eventType":"RecordStateChanged","eventData":{"outputActive":true,"outputState":"OBS_WEBSOCKET_OUTPUT_PAUSED"}}),
    );
    obs::apply_event(
        &mut state,
        &json!({"eventType":"StreamStateChanged","eventData":{"outputActive":true}}),
    );
    assert!(state.recording && state.paused && state.streaming);
    let old = state.clone();
    obs::apply_event(
        &mut state,
        &json!({"eventType":"InputVolumeMeters","eventData":{"level":999}}),
    );
    assert_eq!(old, state);
    assert_eq!(
        obs::authentication("password", "salt", "challenge"),
        "zTM5ki6L2vVvBQiTG9ckH1Lh64AbnCf6XZ226UmnkIA="
    );
}
#[test]
fn docker_failures_are_not_every_stopped_container() {
    let state = docker::snapshot(&json!([
        {"Id":"ok","Names":["/web"],"State":"exited","Status":"Exited (0) 1 minute ago"},
        {"Id":"bad","Names":["/worker"],"State":"exited","Status":"Exited (137) 1 minute ago"},
        {"Id":"sick","Names":["/db"],"State":"running","Status":"Up (unhealthy)"}
    ]))
    .unwrap();
    assert!(!state.containers[0].failed);
    assert!(state.containers[1].failed);
    assert!(state.containers[2].failed);
}

#[test]
fn widget_clocks_metrics_and_peer_snapshots_preserve_unknowns() {
    let clock = obs::Clock {
        elapsed_ms: 12_000,
        at_ms: 100_000,
        advancing: true,
    };
    assert_eq!(clock.elapsed(103_500), 15_500);
    assert_eq!(clock.elapsed(99_000), 12_000);
    assert_eq!(
        obs::Clock {
            advancing: false,
            ..clock
        }
        .elapsed(110_000),
        12_000
    );
    let stats = docker::usage(
        &json!({"cpu_stats":{"cpu_usage":{"total_usage":500},"system_cpu_usage":2000,"online_cpus":4},"precpu_stats":{"cpu_usage":{"total_usage":400},"system_cpu_usage":1000},"memory_stats":{"usage":4096,"limit":8192,"stats":{"inactive_file":1024}}}),
    );
    assert_eq!(stats.cpu_hundredths, Some(4000));
    assert_eq!(stats.memory_bytes, 3072);
    assert_eq!(docker::usage(&json!({})).cpu_hundredths, None);
    let mut vpn = tailscale::Snapshot::default();
    tailscale::apply(
        &mut vpn,
        &json!({"InitialStatus":{"BackendState":"Running","Peer":{"a":{"DNSName":"macbook.tailnet.","Online":true},"b":{"HostName":"home","Online":false,"ExitNode":true}}}}),
    );
    assert_eq!(vpn.peers.len(), 2);
    assert_eq!(vpn.peers[0].name, "macbook.tailnet");
    assert!(vpn.peers[0].online);
    assert_eq!(vpn.exit_node.as_deref(), Some("home"));
}
#[test]
fn tailscale_seeds_and_applies_prefs_without_engine_telemetry() {
    assert_eq!(tailscale::WATCH_MASK & 1, 0);
    let mut state = tailscale::Snapshot::default();
    tailscale::apply(
        &mut state,
        &json!({"State":"Running","InitialStatus":{"BackendState":"Running","TailscaleIPs":["100.64.0.1"],"ExitNodeStatus":{"ID":"exit1"}}}),
    );
    assert_eq!(state.addresses, ["100.64.0.1"]);
    assert_eq!(state.exit_node.as_deref(), Some("exit1"));
    tailscale::apply(
        &mut state,
        &json!({"Prefs":{"ExitNodeID":""},"State":"Stopped"}),
    );
    assert!(state.exit_node.is_none());
    assert_eq!(state.state, "Stopped");
}
#[test]
fn configuration_rejects_secrets_remote_endpoints_and_ambiguous_actions() {
    assert!(Config::parse(r#"{"t3":{"endpoint":"ws://example.com/ws"}}"#).is_err());
    assert!(Config::parse(r#"{"t3":{"token":"secret"}}"#).is_err());
    assert!(Config::parse(r#"{"grok":{"routines":[{"id":"a","title":"A","url":"https://grok.bot/routine?key=secret"}]}}"#).is_err());
    assert!(Config::parse(r#"{"docker":{"pipe":"\\\\remote\\pipe\\docker"}}"#).is_err());
    assert_eq!(launcher::encode("a/b?c"), "a%2Fb%3Fc");
    assert!(
        Config::parse(
            r#"{"t3":{"enabled":true,"endpoint":"ws://127.0.0.1:3773/ws","protocolVersion":2}}"#
        )
        .is_ok()
    );
}
