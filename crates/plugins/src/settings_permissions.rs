//! A declared loopback origin may take its port from one numeric plugin setting.
//! Substitution cannot change the scheme or host, and happens once when the instance loads.
use anyhow::{Result, ensure};
use serde_json::{Map, Value};

pub fn settings(schema: &[Value], saved: &str) -> Result<Map<String, Value>> {
    let mut values: Map<String, Value> = serde_json::from_str(saved)?;
    for setting in schema {
        if let (Some(key), Some(default)) = (setting["key"].as_str(), setting.get("default")) {
            values.entry(key).or_insert_with(|| default.clone());
        }
    }
    Ok(values)
}

pub fn resolve(permission: &str, settings: &Map<String, Value>) -> Result<String> {
    if !permission.contains('{') && !permission.contains('}') { return Ok(permission.into()); }
    let key = permission.strip_prefix("network:ws://127.0.0.1:{settings.")
        .and_then(|s| s.strip_suffix('}')).filter(|k| !k.is_empty() && k.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-'));
    let key = key.ok_or_else(|| anyhow::anyhow!("invalid settings-derived network permission"))?;
    let port = settings.get(key).and_then(Value::as_u64).filter(|p| (1..=65535).contains(p));
    ensure!(port.is_some(), "network port setting {key:?} must be an integer between 1 and 65535");
    Ok(format!("network:ws://127.0.0.1:{}", port.unwrap()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn numeric_port_is_scoped_to_the_declared_loopback_host() {
        let schema = [json!({"key":"port","default":4455})];
        let defaults = settings(&schema, "{}").unwrap();
        let configured = settings(&schema, "{\"port\":4466}").unwrap();
        let permission = "network:ws://127.0.0.1:{settings.port}";
        assert_eq!(resolve(permission, &defaults).unwrap(), "network:ws://127.0.0.1:4455");
        assert_eq!(resolve(permission, &configured).unwrap(), "network:ws://127.0.0.1:4466");
        for bad in [json!({"port":0}), json!({"port":65536}), json!({"port":"4455/evil"}), json!({"port":44.55})] {
            assert!(resolve(permission, bad.as_object().unwrap()).is_err());
        }
        assert!(resolve("network:ws://{settings.host}:4455", &defaults).is_err());
    }
}
