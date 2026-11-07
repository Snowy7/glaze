//! Coalescing keys for background input retained during games. Completion IDs and connection
//! lifecycle events stay distinct, so a reply cannot replace another outstanding request.
use crate::host::Input;
use crate::runtime::mypaper::plugin::{pipes::PipeEvent, websocket::SocketEvent};
use mypaper_plugin_api::wire::{HostMsg, SystemEvent};

#[derive(PartialEq, Eq)]
pub(crate) struct Key(String, &'static str, String);

pub(crate) fn key(input: &Input) -> Option<Key> {
    let (plugin, kind, id) = match input {
        Input::Host(HostMsg::System(event)) => return system(event),
        Input::Host(HostMsg::Open { instance, .. }) => ("", "face-open", instance.clone()),
        Input::Host(HostMsg::Close { instance }) => ("", "face-close", instance.clone()),
        Input::Host(HostMsg::Options { instance, .. }) => ("", "face-options", instance.clone()),
        Input::Host(HostMsg::Held(_)) => ("", "held", String::new()),
        // Explicit user actions keep their order rather than silently losing commands.
        Input::Host(_) | Input::Toast { .. } => return None,
        Input::Http { plugin, id, .. } | Input::PipeHttp { plugin, id, .. } => {
            (plugin.as_str(), "http", id.to_string())
        }
        Input::Native { plugin, id, .. } => (plugin.as_str(), "native-reply", id.to_string()),
        Input::NativeEvent(event) => (
            event.plugin.as_str(),
            "native-event",
            format!("{}:{}", event.module, event.name),
        ),
        Input::Socket { plugin, event } => match event {
            SocketEvent::Opened(id) => (plugin.as_str(), "socket-open", id.to_string()),
            // A socket can multiplex request/reply traffic: every message keeps its read
            // credit and arrival order, just like pipe chunks.
            SocketEvent::Message(_) => return None,
            SocketEvent::Closed((id, _)) => (plugin.as_str(), "socket-close", id.to_string()),
        },
        Input::Pipe { plugin, event } => match event {
            // A pipe is a byte stream, not state snapshots: dropping chunks corrupts its
            // framing. The transport's 64-event credit bounds this ordered backlog.
            PipeEvent::Data(_) => return None,
            PipeEvent::Closed(id) => (plugin.as_str(), "pipe-close", id.to_string()),
        },
        Input::Removed(plugin) => (plugin.as_str(), "removed", String::new()),
        Input::Eof => ("", "eof", String::new()),
    };
    Some(Key(plugin.into(), kind, id))
}

fn system(event: &SystemEvent) -> Option<Key> {
    let (kind, id) = match event {
        SystemEvent::Window { id, .. } => ("window", id.to_string()),
        SystemEvent::Monitors => ("monitors", String::new()),
        SystemEvent::Notification(_) => ("notification", String::new()),
        SystemEvent::Media(_) => ("media", String::new()),
        SystemEvent::Power => ("power", String::new()),
        SystemEvent::Theme => ("theme", String::new()),
    };
    Some(Key(String::new(), kind, id))
}

pub(crate) fn credit(input: &Input) -> Option<(&str, u64)> {
    match input {
        Input::Socket {
            plugin,
            event: SocketEvent::Message((id, _)),
        }
        | Input::Pipe {
            plugin,
            event: PipeEvent::Data((id, _)),
        } => Some((plugin, *id)),
        _ => None,
    }
}

pub(crate) fn belongs_to(input: &Input, plugin: &str, instances: &[String]) -> bool {
    match input {
        Input::Host(
            HostMsg::Open { plugin: owner, .. } | HostMsg::Command { plugin: owner, .. },
        )
        | Input::Http { plugin: owner, .. }
        | Input::PipeHttp { plugin: owner, .. }
        | Input::Native { plugin: owner, .. }
        | Input::Socket { plugin: owner, .. }
        | Input::Pipe { plugin: owner, .. }
        | Input::Toast { plugin: owner, .. } => owner == plugin,
        Input::NativeEvent(event) => event.plugin == plugin,
        Input::Host(
            HostMsg::Close { instance }
            | HostMsg::Options { instance, .. }
            | HostMsg::Action { instance, .. },
        ) => instances.contains(instance),
        _ => false,
    }
}
