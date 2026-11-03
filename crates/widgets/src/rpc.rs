//! JSON-RPC 2.0 over newline-delimited stdio: framing and request/response
//! correlation (docs/widgets.md, "Plugins"). Pure; the process side lives in
//! `plugins.rs`.

use std::collections::HashMap;
use std::time::Instant;

use serde_json::{Value, json};

/// The id of the `initialize` request; calls count up from 1.
pub const INIT_ID: u64 = 0;

/// A request line, `\n` included. serde_json escapes newlines inside
/// strings, so one message is always exactly one line.
pub fn request(id: u64, method: &str, params: Option<&Value>) -> String {
    let mut message = json!({ "jsonrpc": "2.0", "id": id, "method": method });
    if let Some(params) = params {
        message["params"] = params.clone();
    }
    message.to_string() + "\n"
}

/// A notification line (no id, no reply expected).
pub fn notification(method: &str) -> String {
    json!({ "jsonrpc": "2.0", "method": method }).to_string() + "\n"
}

/// The error response to a request the host doesn't serve.
pub fn method_not_found(id: &Value) -> String {
    let error = json!({ "code": -32601, "message": "the host serves no methods" });
    json!({ "jsonrpc": "2.0", "id": id, "error": error }).to_string() + "\n"
}

#[derive(Debug, Clone, PartialEq)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    /// `error.data.code`: the mypaper error name the plugin chose, if any.
    pub name: Option<String>,
}

/// One line from a plugin.
#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    /// The reply to our request `id`.
    Response {
        id: u64,
        outcome: Result<Value, RpcError>,
    },
    /// `{"method":"event","params":{"event":…,"data":…}}`.
    Event { event: String, data: Value },
    /// A request from the plugin; answered with [`method_not_found`].
    Request { id: Value },
    /// Anything else (logged and ignored).
    Invalid(String),
}

pub fn parse(line: &str) -> Incoming {
    let invalid = |why: &str| Incoming::Invalid(format!("{why}: {line}"));
    let Ok(Value::Object(message)) = serde_json::from_str::<Value>(line) else {
        return invalid("not a JSON object");
    };
    if message.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return invalid("not JSON-RPC 2.0");
    }
    match (
        message.get("id"),
        message.get("method").and_then(Value::as_str),
    ) {
        (Some(id), Some(_)) => Incoming::Request { id: id.clone() },
        (None, Some("event")) => {
            let params = message.get("params");
            match params.and_then(|p| p.get("event")).and_then(Value::as_str) {
                Some(event) => Incoming::Event {
                    event: event.to_owned(),
                    data: params
                        .and_then(|p| p.get("data"))
                        .cloned()
                        .unwrap_or(Value::Null),
                },
                None => invalid("event notification without params.event"),
            }
        }
        (None, Some(_)) => invalid("unknown notification"),
        (Some(id), None) => {
            let Some(id) = id.as_u64() else {
                return invalid("response id isn't one of ours");
            };
            let outcome = match (message.get("result"), message.get("error")) {
                (_, Some(error)) => Err(RpcError {
                    code: error.get("code").and_then(Value::as_i64).unwrap_or(-32603),
                    message: error
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("plugin error")
                        .to_owned(),
                    name: error
                        .pointer("/data/code")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                }),
                (Some(result), None) => Ok(result.clone()),
                (None, None) => Ok(Value::Null),
            };
            Incoming::Response { id, outcome }
        }
        (None, None) => invalid("neither a request, a response nor a notification"),
    }
}

/// Requests in flight: each id maps to whoever waits for the reply, with a
/// deadline.
#[derive(Debug)]
pub struct Pending<T> {
    next_id: u64,
    waiting: HashMap<u64, (T, Instant)>,
}

impl<T> Default for Pending<T> {
    fn default() -> Self {
        Self {
            next_id: INIT_ID + 1,
            waiting: HashMap::new(),
        }
    }
}

impl<T> Pending<T> {
    /// Registers a request and returns the id to send it with.
    pub fn insert(&mut self, waiter: T, deadline: Instant) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.waiting.insert(id, (waiter, deadline));
        id
    }

    /// An id for a request whose reply nobody waits for (`shutdown`).
    pub fn reserve(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id - 1
    }

    /// The waiter for a reply, if the id is still pending.
    pub fn take(&mut self, id: u64) -> Option<T> {
        self.waiting.remove(&id).map(|(waiter, _)| waiter)
    }

    /// Removes and returns every request whose deadline has passed.
    pub fn expire(&mut self, now: Instant) -> Vec<T> {
        let late: Vec<u64> = self
            .waiting
            .iter()
            .filter(|(_, (_, deadline))| *deadline <= now)
            .map(|(id, _)| *id)
            .collect();
        late.into_iter().filter_map(|id| self.take(id)).collect()
    }

    pub fn deadline(&self) -> Option<Instant> {
        self.waiting.values().map(|(_, deadline)| *deadline).min()
    }

    pub fn len(&self) -> usize {
        self.waiting.len()
    }

    /// Removes every pending request (the plugin is gone).
    pub fn drain(&mut self) -> Vec<T> {
        self.waiting
            .drain()
            .map(|(_, (waiter, _))| waiter)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn requests_are_single_lines() {
        let params = json!({ "city": "Oslo\nNorway" });
        let line = request(7, "current", Some(&params));
        assert!(line.ends_with('\n') && line.matches('\n').count() == 1);
        let parsed: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(
            parsed,
            json!({ "jsonrpc": "2.0", "id": 7, "method": "current", "params": params })
        );
        assert!(!request(1, "x", None).contains("params"));
        assert_eq!(
            notification("suspend"),
            "{\"jsonrpc\":\"2.0\",\"method\":\"suspend\"}\n"
        );
    }

    #[test]
    fn parses_responses_events_and_junk() {
        assert_eq!(
            parse(r#"{"jsonrpc":"2.0","id":0,"result":{"protocol":1}}"#),
            Incoming::Response {
                id: 0,
                outcome: Ok(json!({ "protocol": 1 }))
            }
        );
        assert_eq!(
            parse(r#"{"jsonrpc":"2.0","id":3,"error":{"code":-32601,"message":"nope"}}"#),
            Incoming::Response {
                id: 3,
                outcome: Err(RpcError {
                    code: -32601,
                    message: "nope".into(),
                    name: None,
                })
            }
        );
        let named = r#"{"jsonrpc":"2.0","id":4,"error":{"code":-32000,"message":"no such city","data":{"code":"NotFound"}}}"#;
        assert!(matches!(
            parse(named),
            Incoming::Response { outcome: Err(RpcError { name: Some(name), .. }), .. } if name == "NotFound"
        ));
        assert_eq!(
            parse(
                r#"{"jsonrpc":"2.0","method":"event","params":{"event":"updated","data":{"t":1}}}"#
            ),
            Incoming::Event {
                event: "updated".into(),
                data: json!({ "t": 1 })
            }
        );
        assert_eq!(
            parse(r#"{"jsonrpc":"2.0","id":"a","method":"log"}"#),
            Incoming::Request { id: json!("a") }
        );
        for junk in [
            "",
            "[]",
            "not json",
            r#"{"id":1,"result":1}"#,
            r#"{"jsonrpc":"2.0","method":"event"}"#,
        ] {
            assert!(matches!(parse(junk), Incoming::Invalid(_)), "{junk}");
        }
    }

    #[test]
    fn correlates_and_expires() {
        let now = Instant::now();
        let mut pending = Pending::default();
        let a = pending.insert("a", now + Duration::from_secs(1));
        let b = pending.insert("b", now + Duration::from_secs(5));
        assert_eq!((a, b), (1, 2));
        assert_eq!(pending.deadline(), Some(now + Duration::from_secs(1)));
        assert_eq!(pending.take(b), Some("b"));
        assert_eq!(pending.take(b), None, "a reply is delivered once");
        assert_eq!(pending.expire(now), Vec::<&str>::new());
        assert_eq!(pending.expire(now + Duration::from_secs(2)), ["a"]);
        assert_eq!(pending.len(), 0);
        pending.insert("c", now);
        assert_eq!(pending.drain(), ["c"]);
    }
}
