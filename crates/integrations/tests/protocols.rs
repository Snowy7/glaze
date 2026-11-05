use mypaper_integrations::{config::Config, launcher, t3};
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
