use glaze_integrations::{config::Config, launcher};

#[test]
fn configuration_rejects_secrets_remote_endpoints_and_ambiguous_actions() {
    assert!(Config::parse(r#"{"grok":{"routines":[{"id":"a","title":"A","url":"https://grok.bot/routine?key=secret"}]}}"#).is_err());
    assert!(Config::parse(r#"{"docker":{"pipe":"\\\\remote\\pipe\\docker"}}"#).is_err());
    assert_eq!(launcher::encode("a/b?c"), "a%2Fb%3Fc");
}

#[test]
fn legacy_t3_settings_are_ignored_without_relaxing_other_config_fields() {
    let old = r#"{"t3":{"enabled":true,"endpoint":"ws://127.0.0.1:3773/ws","protocolVersion":2,"liveVerified":true,"threadLinks":{"thread":"https://example.test/thread"}}}"#;
    let config = Config::parse(old).unwrap();
    assert_eq!(config, Config::default());
    assert!(serde_json::to_value(config).unwrap().get("t3").is_none());
    assert!(Config::parse(r#"{"t3":{"futureField":true}}"#).is_ok());
    assert!(Config::parse(r#"{"typo":{}}"#).is_err());
}
