//! Reviewed settings may supply a loopback port or a list of HTTPS webhook origins.
//! Expansion is validated and happens once when the instance loads.
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

/// Expand HTTPS hosts from a reviewed list setting (an array, or its JSON text form).
/// All other declarations use the same resolver as fixed origins and loopback ports.
pub fn resolve_all(permissions: &[String], settings: &Map<String, Value>) -> Result<Vec<String>> {
    let mut resolved = Vec::new();
    for permission in permissions {
        let list = permission.strip_prefix("network:https://{settings.").and_then(|s| s.strip_suffix("[].url}"));
        let Some(key) = list else { resolved.push(resolve(permission, settings)?); continue; };
        ensure!(!key.is_empty() && key.bytes().all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c)), "invalid network URL-list setting");
        let value = settings.get(key).ok_or_else(|| anyhow::anyhow!("missing URL-list setting {key:?}"))?;
        let value = match value { Value::String(text) => serde_json::from_str(text)?, value => value.clone() };
        let list = value.as_array().ok_or_else(|| anyhow::anyhow!("network URL-list setting must be an array"))?;
        ensure!(list.len() <= 32, "at most 32 network URLs");
        for item in list {
            let url = url::Url::parse(item["url"].as_str().ok_or_else(|| anyhow::anyhow!("network URL-list item needs a URL"))?)?;
            ensure!(url.scheme() == "https" && url.host_str().is_some_and(|h| !h.contains('*')) && url.username().is_empty() && url.password().is_none() && url.query().is_none() && url.fragment().is_none(), "network URLs must be HTTPS without credentials, query or fragment");
            let origin = format!("network:{}", url.origin().ascii_serialization());
            if !resolved.contains(&origin) { resolved.push(origin); }
        }
    }
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn reviewed_webhook_urls_grant_only_their_exact_https_origins() {
        let permission = ["network:https://{settings.routines[].url}".into()];
        let settings = json!({"routines":"[{\"url\":\"https://example.test/run\"},{\"url\":\"https://other.test:9443/run\"},{\"url\":\"https://example.test/again\"}]"});
        assert_eq!(resolve_all(&permission, settings.as_object().unwrap()).unwrap(), ["network:https://example.test", "network:https://other.test:9443"]);
        assert!(resolve_all(&permission,json!({"routines":[]}).as_object().unwrap()).unwrap().is_empty());
        for url in ["http://example.test/run", "https://user@example.test/run", "https://example.test/run?key=x", "https://*.test/run"] {
            assert!(resolve_all(&permission,json!({"routines":[{"url":url}]}).as_object().unwrap()).is_err());
        }
    }
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
