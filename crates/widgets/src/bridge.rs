//! Typed service replies for the native shell and JSON-RPC plugins. No UI transport.
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ErrorCode {
    PermissionDenied,
    NotFound,
    InvalidParams,
    PluginUnavailable,
    Internal,
}

/// The `error` of a failed widget call.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WidgetError {
    pub code: ErrorCode,
    pub message: String,
}

impl WidgetError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

pub type Reply = Result<Value, WidgetError>;

/// Who waits for the reply to a widget call.
#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    pub instance: String,
    /// The bridge's call id, echoed back.
    pub id: Value,
}

/// What a background service (plugins, media) hands back to the host.
#[derive(Debug)]
pub enum Out {
    Reply(Target, Reply),
    /// A widget event for one instance.
    Event {
        instance: String,
        event: &'static str,
        data: Value,
    },
}

impl ErrorCode {
    /// The code a plugin picked by name in `error.data.code`.
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "PermissionDenied" => Self::PermissionDenied,
            "NotFound" => Self::NotFound,
            "InvalidParams" => Self::InvalidParams,
            "PluginUnavailable" => Self::PluginUnavailable,
            "Internal" => Self::Internal,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MediaAction {
    Play,
    Pause,
    Toggle,
    Next,
    Previous,
}
