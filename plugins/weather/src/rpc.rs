//! JSON-RPC 2.0 over stdio, one JSON object per line (docs/widgets.md, "Plugins").

use std::io::{self, Write};
use std::sync::Mutex;

use serde_json::{Value, json};

/// A JSON-RPC error. `data_code` carries one of mypaper's error names (`NotFound`, ...) so the
/// host can pass it on to the widget; without it the host maps by `code`.
#[derive(Debug, Clone, PartialEq)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    pub data_code: Option<&'static str>,
}

impl RpcError {
    fn new(code: i64, message: impl Into<String>, data_code: Option<&'static str>) -> Self {
        Self {
            code,
            message: message.into(),
            data_code,
        }
    }

    pub fn parse_error(message: impl Into<String>) -> Self {
        Self::new(-32700, message, None)
    }
    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::new(-32600, message, None)
    }
    pub fn method_not_found(method: &str) -> Self {
        Self::new(
            -32601,
            format!("unknown method \"{method}\""),
            Some("NotFound"),
        )
    }
    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self::new(-32602, message, Some("InvalidParams"))
    }
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(-32000, message, Some("NotFound"))
    }
    /// The upstream service can't be reached.
    pub fn unavailable(message: impl Into<String>) -> Self {
        Self::new(-32001, message, Some("PluginUnavailable"))
    }
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(-32603, message, Some("Internal"))
    }
}

/// A message from the host.
#[derive(Debug, PartialEq)]
pub enum Incoming {
    /// Has an `id`: needs a response.
    Request {
        id: Value,
        method: String,
        params: Value,
    },
    /// No `id`: `suspend`, `resume`.
    Notification { method: String },
}

/// Parses one line. On failure returns the id to answer (`null` when unknown) and the error.
pub fn parse_line(line: &str) -> Result<Incoming, (Value, RpcError)> {
    let value: Value = serde_json::from_str(line)
        .map_err(|e| (Value::Null, RpcError::parse_error(e.to_string())))?;
    let id = value.get("id").cloned();
    let Some(method) = value.get("method").and_then(Value::as_str) else {
        return Err((
            id.unwrap_or(Value::Null),
            RpcError::invalid_request("missing \"method\""),
        ));
    };
    let method = method.to_owned();
    Ok(match id {
        Some(id) => Incoming::Request {
            id,
            method,
            params: value.get("params").cloned().unwrap_or(Value::Null),
        },
        None => Incoming::Notification { method },
    })
}

pub fn response(id: &Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

pub fn error_response(id: &Value, err: &RpcError) -> Value {
    let mut error = json!({ "code": err.code, "message": err.message });
    if let Some(code) = err.data_code {
        error["data"] = json!({ "code": code });
    }
    json!({ "jsonrpc": "2.0", "id": id, "error": error })
}

/// A notification the host forwards to widgets subscribed with `onPlugin`.
pub fn event(name: &str, data: Value) -> Value {
    json!({ "jsonrpc": "2.0", "method": "event", "params": { "event": name, "data": data } })
}

/// Writes messages as single lines. Shared between threads; each message is written and flushed
/// under one lock so lines never interleave.
pub struct Out<W: Write>(Mutex<W>);

impl<W: Write> Out<W> {
    pub fn new(writer: W) -> Self {
        Self(Mutex::new(writer))
    }

    pub fn send(&self, message: &Value) -> io::Result<()> {
        // serde_json's compact form escapes newlines inside strings, so this is one line.
        let mut line = message.to_string();
        line.push('\n');
        let mut writer = self.0.lock().unwrap_or_else(|e| e.into_inner());
        writer.write_all(line.as_bytes())?;
        writer.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_requests_and_notifications() {
        let request = r#"{"jsonrpc":"2.0","id":7,"method":"current","params":{"city":"Oslo"}}"#;
        assert_eq!(
            parse_line(request),
            Ok(Incoming::Request {
                id: json!(7),
                method: "current".into(),
                params: json!({ "city": "Oslo" }),
            })
        );
        // Handshake uses id 0, which is still a request.
        let init = r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocol":1}}"#;
        assert!(matches!(parse_line(init), Ok(Incoming::Request { id, .. }) if id == json!(0)));
        // A request without params gets null.
        let shutdown = r#"{"jsonrpc":"2.0","id":3,"method":"shutdown"}"#;
        assert!(matches!(
            parse_line(shutdown),
            Ok(Incoming::Request {
                params: Value::Null,
                ..
            })
        ));
        assert_eq!(
            parse_line(r#"{"jsonrpc":"2.0","method":"suspend"}"#),
            Ok(Incoming::Notification {
                method: "suspend".into()
            })
        );
    }

    #[test]
    fn rejects_bad_lines_with_the_right_codes() {
        let (id, err) = parse_line("{not json").unwrap_err();
        assert_eq!((id, err.code), (Value::Null, -32700));

        let (id, err) = parse_line(r#"{"jsonrpc":"2.0","id":5}"#).unwrap_err();
        assert_eq!((id, err.code), (json!(5), -32600));
    }

    #[test]
    fn builds_responses_errors_and_events() {
        assert_eq!(
            response(&json!(1), json!({ "ok": true })),
            json!({ "jsonrpc": "2.0", "id": 1, "result": { "ok": true } })
        );
        assert_eq!(
            error_response(&json!(2), &RpcError::not_found("no such city")),
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "error": { "code": -32000, "message": "no such city", "data": { "code": "NotFound" } }
            })
        );
        // Protocol-level errors carry no data code.
        assert!(
            error_response(&json!(3), &RpcError::parse_error("x"))["error"]
                .get("data")
                .is_none()
        );
        assert_eq!(
            event("updated", json!({ "a": 1 })),
            json!({ "jsonrpc": "2.0", "method": "event", "params": { "event": "updated", "data": { "a": 1 } } })
        );
    }

    #[test]
    fn writes_one_line_per_message_even_with_newlines_in_strings() {
        let out = Out::new(Vec::new());
        out.send(&json!({ "text": "a\nb" })).unwrap();
        out.send(&json!({ "n": 2 })).unwrap();
        let written = String::from_utf8(out.0.into_inner().unwrap()).unwrap();
        let lines: Vec<&str> = written.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(written.ends_with('\n'));
        assert_eq!(
            serde_json::from_str::<Value>(lines[0]).unwrap()["text"],
            "a\nb"
        );
    }
}
