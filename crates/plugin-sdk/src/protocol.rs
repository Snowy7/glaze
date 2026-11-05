//! The wire format (docs/plugins.md, "Protocol"): JSON-RPC 2.0, one object per line. Pure; the
//! loop in `lib.rs` and the shell's host both use these.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::view::{Face, SizeClass};

/// The protocol the SDK speaks; weather/rss speak 1, which this extends.
pub const PROTOCOL: u32 = 2;
/// JSON-RPC error code for a request the plugin's manifest doesn't permit.
pub const PERMISSION_DENIED: i64 = -32003;

/// What the host tells a plugin.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum HostMessage {
    #[serde(rename = "face.open")]
    Open {
        instance: String,
        face: String,
        #[serde(default)]
        size: SizeClass,
        #[serde(default)]
        settings: Value,
    },
    #[serde(rename = "face.resize")]
    Resize { instance: String, size: SizeClass },
    #[serde(rename = "face.close")]
    Close { instance: String },
    #[serde(rename = "action")]
    Action {
        instance: String,
        action: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        value: Option<String>,
    },
    #[serde(rename = "suspend")]
    Suspend,
    #[serde(rename = "resume")]
    Resume,
}

/// A request a plugin makes of the host.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum HostRequest {
    #[serde(rename = "secret.get")]
    GetSecret { key: String },
    #[serde(rename = "secret.set")]
    SetSecret { key: String, value: Option<String> },
    #[serde(rename = "launch")]
    Launch { target: String },
    #[serde(rename = "notify")]
    Notify { title: String, body: String },
}

impl HostRequest {
    /// The permission this request needs.
    pub fn permission(&self) -> crate::Permission {
        match self {
            Self::GetSecret { .. } | Self::SetSecret { .. } => crate::Permission::Secrets,
            Self::Launch { .. } => crate::Permission::Launch,
            Self::Notify { .. } => crate::Permission::Notifications,
        }
    }
}

/// `{"method":"view","params":{"instance":…,"tree":…}}`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ViewUpdate {
    pub instance: String,
    pub tree: Face,
}

fn line(message: Value) -> String {
    message.to_string() + "\n"
}

/// A host → plugin notification line.
pub fn host_message(message: &HostMessage) -> String {
    let mut value = serde_json::to_value(message).unwrap_or_default();
    value["jsonrpc"] = "2.0".into();
    line(value)
}

/// A plugin → host request line.
pub fn host_request(id: u64, request: &HostRequest) -> String {
    let mut value = serde_json::to_value(request).unwrap_or_default();
    value["jsonrpc"] = "2.0".into();
    value["id"] = id.into();
    line(value)
}

/// The plugin's `view` notification line.
pub fn view(update: &ViewUpdate) -> String {
    line(json!({ "jsonrpc": "2.0", "method": "view", "params": update }))
}

pub fn result(id: &Value, result: Value) -> String {
    line(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

pub fn error(id: &Value, code: i64, message: &str, name: &str) -> String {
    let error = json!({ "code": code, "message": message, "data": { "code": name } });
    line(json!({ "jsonrpc": "2.0", "id": id, "error": error }))
}

/// One line the plugin read from the host.
#[derive(Debug, Clone, PartialEq)]
pub enum FromHost {
    Message(HostMessage),
    /// A request with an id: `initialize`, `shutdown`, or a protocol-1 data method.
    Call {
        id: Value,
        method: String,
        params: Value,
    },
    /// The reply to one of our [`HostRequest`]s.
    Reply {
        id: u64,
        outcome: Result<Value, String>,
    },
    Invalid,
}

pub fn parse_from_host(text: &str) -> FromHost {
    let Ok(Value::Object(mut message)) = serde_json::from_str::<Value>(text) else {
        return FromHost::Invalid;
    };
    let id = message.remove("id");
    match (id, message.get("method").and_then(Value::as_str)) {
        (Some(id), Some(method)) => FromHost::Call {
            method: method.into(),
            params: message.remove("params").unwrap_or(Value::Null),
            id,
        },
        (None, Some(_)) => serde_json::from_value(Value::Object(message))
            .map(FromHost::Message)
            .unwrap_or(FromHost::Invalid),
        (Some(id), None) => {
            let Some(id) = id.as_u64() else {
                return FromHost::Invalid;
            };
            let outcome = match message.remove("error") {
                Some(error) => Err(error["message"].as_str().unwrap_or("error").to_owned()),
                None => Ok(message.remove("result").unwrap_or(Value::Null)),
            };
            FromHost::Reply { id, outcome }
        }
        (None, None) => FromHost::Invalid,
    }
}

/// One line the host read from a protocol-2 plugin, beyond what protocol 1 already parses.
#[derive(Debug, Clone, PartialEq)]
pub enum FromPlugin {
    View(ViewUpdate),
    Request { id: Value, request: HostRequest },
    /// A request the host doesn't serve.
    Unknown { id: Value },
    /// Responses and `event`s: handled by the protocol-1 path.
    Other,
}

pub fn parse_from_plugin(text: &str) -> FromPlugin {
    let Ok(Value::Object(mut message)) = serde_json::from_str::<Value>(text) else {
        return FromPlugin::Other;
    };
    let id = message.remove("id");
    let method = message.get("method").and_then(Value::as_str).map(str::to_owned);
    match (id, method.as_deref()) {
        (None, Some("view")) => message
            .remove("params")
            .and_then(|p| serde_json::from_value(p).ok())
            .map_or(FromPlugin::Other, FromPlugin::View),
        (Some(id), Some(_)) => match serde_json::from_value(Value::Object(message)) {
            Ok(request) => FromPlugin::Request { id, request },
            Err(_) => FromPlugin::Unknown { id },
        },
        _ => FromPlugin::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::{Role, View};

    #[test]
    fn host_messages_round_trip() {
        let messages = [
            HostMessage::Open {
                instance: "t3-1".into(),
                face: "limits".into(),
                size: SizeClass::Large,
                settings: json!({ "a": 1 }),
            },
            HostMessage::Action {
                instance: "t3-1".into(),
                action: "send".into(),
                value: Some("hi\nthere".into()),
            },
            HostMessage::Close { instance: "t3-1".into() },
            HostMessage::Suspend,
        ];
        for message in messages {
            let wire = host_message(&message);
            assert_eq!(wire.matches('\n').count(), 1, "one line: {wire}");
            assert_eq!(parse_from_host(wire.trim_end()), FromHost::Message(message));
        }
        // Protocol 1's bare notification shape is the same.
        assert_eq!(
            parse_from_host(r#"{"jsonrpc":"2.0","method":"resume"}"#),
            FromHost::Message(HostMessage::Resume)
        );
    }

    #[test]
    fn calls_and_replies_parse() {
        let init = r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocol":2}}"#;
        assert_eq!(
            parse_from_host(init),
            FromHost::Call { id: json!(0), method: "initialize".into(), params: json!({ "protocol": 2 }) }
        );
        let reply = r#"{"jsonrpc":"2.0","id":7,"result":{"value":"k"}}"#;
        assert_eq!(parse_from_host(reply), FromHost::Reply { id: 7, outcome: Ok(json!({ "value": "k" })) });
        let denied = error(&json!(8), PERMISSION_DENIED, "no", "PermissionDenied");
        assert_eq!(parse_from_host(denied.trim_end()), FromHost::Reply { id: 8, outcome: Err("no".into()) });
    }

    #[test]
    fn plugin_lines_round_trip() {
        let request = HostRequest::SetSecret { key: "token".into(), value: None };
        let wire = host_request(3, &request);
        assert_eq!(parse_from_plugin(&wire), FromPlugin::Request { id: json!(3), request });
        let update = ViewUpdate { instance: "x".into(), tree: View::text("Hi", Role::Body).face() };
        assert_eq!(parse_from_plugin(&view(&update)), FromPlugin::View(update));
        let odd = r#"{"jsonrpc":"2.0","id":4,"method":"fs.read","params":{}}"#;
        assert_eq!(parse_from_plugin(odd), FromPlugin::Unknown { id: json!(4) });
        let event = r#"{"jsonrpc":"2.0","method":"event","params":{"event":"updated"}}"#;
        assert_eq!(parse_from_plugin(event), FromPlugin::Other);
    }
}
