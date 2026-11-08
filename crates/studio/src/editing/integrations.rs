//! Built-in connection catalog, credentials and connection actions.

use glaze_ui::controls::Glyph;
use serde_json::{Value, json};

use crate::actions::{Action, Page};
use crate::app::App;
use crate::docs::{DocId, Op};
use crate::theme::glyph;

/// One option of an integration, in `integrations.json`.
pub enum Field {
    Switch {
        path: &'static str,
        label: &'static str,
        hint: &'static str,
    },
}

pub struct Integration {
    pub name: &'static str,
    pub adds: &'static str,
    pub glyph: Glyph,
    /// Its section in `integrations.json` (`None`: a plugin with no settings).
    pub section: Option<&'static str>,
    /// Shipped and on unless turned off.
    pub builtin: bool,
    /// Its top-bar module, if it has one.
    pub module: Option<&'static str>,
    /// Its Credential Manager key and what the secret is called.
    pub secret: Option<(&'static str, &'static str)>,
    pub fields: &'static [Field],
    pub setup: &'static str,
    pub privacy: &'static str,
}

pub static LIST: &[Integration] = &[
    Integration {
        name: "Media",
        adds: "Any player, Spotify included",
        glyph: glyph::MUSIC,
        section: Some("media"),
        builtin: true,
        module: Some("media"),
        secret: None,
        fields: &[],
        setup: "Windows' media controls cover every player that publishes to them.",
        privacy: "Reads what Windows' media controls already show. Nothing leaves this PC.",
    },
    Integration {
        name: "Notifications",
        adds: "Badges for Discord, Grok and chat apps",
        glyph: glyph::BELL,
        section: Some("notifications"),
        builtin: true,
        module: Some("notifications"),
        secret: None,
        fields: &[
            Field::Switch {
                path: "/notifications/showText",
                label: "Show what they say",
                hint: "Off shows only which app, on a plain card",
            },
            Field::Switch {
                path: "/notifications/liveVerified",
                label: "Live badges",
                hint: "Only once delivery is qualified; otherwise read on open",
            },
        ],
        setup: "One listener covers chat apps without a client per app. Windows asks once for access.",
        privacy: "Reads Windows' own notifications, with your permission. Nothing leaves this PC.",
    },
    Integration {
        name: "App actions",
        adds: "Recent items and app actions in Start",
        glyph: glyph::LAUNCH,
        section: Some("launcher"),
        builtin: true,
        module: None,
        secret: None,
        fields: &[],
        setup: "A dock app's Recent items and integration shortcuts in Start.",
        privacy: "Reads Windows' Recent lists. Nothing leaves this PC.",
    },
    Integration {
        name: "Weather and RSS",
        adds: "Data for their widgets",
        glyph: glyph::CLOUD,
        section: None,
        builtin: true,
        module: None,
        secret: None,
        fields: &[],
        setup: "Plugins that sleep between fetches.",
        privacy: "Weather comes from Open-Meteo (no key); feeds from the addresses you add.",
    },
];

pub(crate) fn enabled(app: &App, i: &Integration) -> bool {
    match i.section {
        Some(s) => app
            .get(DocId::Integrations, &format!("/{s}/enabled"))
            .and_then(Value::as_bool)
            .unwrap_or(i.builtin && s != "notifications"),
        None => true,
    }
}

/// Whether the secret is stored (one Credential Manager read; the value is never kept).
fn has_secret(app: &App, i: &Integration) -> Option<bool> {
    let (k, _) = i.secret?;
    if app.fixture {
        return Some(false);
    }
    Some(glaze_integrations::credentials::read(k).is_ok())
}

/// Where its module is in the top bar, if anywhere.
fn module_shown(app: &App, module: &str) -> bool {
    ["left", "center", "right"].iter().any(|l| {
        app.get(DocId::Bars, &format!("/top/{l}"))
            .and_then(Value::as_array)
            .is_some_and(|a| a.iter().any(|m| m.as_str() == Some(module)))
    })
}

/// Integration actions (they arrive as Studio pseudo-paths through `pages::intercept`).
pub fn intercept(app: &mut App, path: &str) -> bool {
    let n = |prefix: &str| {
        path.strip_prefix(prefix)
            .and_then(|n| n.parse::<usize>().ok())
    };
    if let Some(i) = n("/integration_on/") {
        connect(app, i, true);
    } else if let Some(i) = n("/integration_off/") {
        connect(app, i, false);
    } else if let Some(i) = n("/integration_module/") {
        let Some(module) = LIST.get(i).and_then(|item| item.module) else {
            return true;
        };
        let shown = module_shown(app, module);
        app.apply("Shows in", module_ops(app, module, !shown));
    } else if let Some(i) = n("/integration_secret/") {
        let Some((k, name)) = LIST.get(i).and_then(|item| item.secret) else {
            return true;
        };
        paste_secret(app, k, name);
    } else if let Some(i) = n("/integration_forget/") {
        let Some((k, name)) = LIST.get(i).and_then(|item| item.secret) else {
            return true;
        };
        match glaze_integrations::credentials::remove(k) {
            Ok(()) => app.toast(format!("Forgot the {}", name.to_lowercase()), None),
            Err(e) => app.alert(format!("{e:#}")),
        }
    } else if path == "/integration_access" {
        grant_access(app);
    } else {
        return false;
    }
    true
}

/// The bars.json ops that show or hide `module` in the top bar (at the end).
fn module_ops(app: &App, module: &str, show: bool) -> Vec<Op> {
    let mut ops = Vec::new();
    for list in ["left", "center", "right"] {
        let path = format!("/top/{list}");
        let mut items = app
            .get(DocId::Bars, &path)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let had = items.iter().any(|m| m.as_str() == Some(module));
        if !show && had {
            items.retain(|m| m.as_str() != Some(module));
            ops.push(Op {
                doc: DocId::Bars,
                path,
                value: Some(Value::Array(items)),
            });
        } else if show && list == "right" && !module_shown(app, module) {
            // Before the clock, when there is one.
            let at = items
                .iter()
                .position(|m| m.as_str() == Some("clock"))
                .unwrap_or(items.len());
            items.insert(at, json!(module));
            ops.push(Op {
                doc: DocId::Bars,
                path,
                value: Some(Value::Array(items)),
            });
        }
    }
    ops
}

/// Connect (on, and its module shown) or Disconnect (off, and its module gone): one step.
fn connect(app: &mut App, i: usize, on: bool) {
    let Some(item) = LIST.get(i) else { return };
    let Some(section) = item.section else { return };
    let mut ops = vec![Op {
        doc: DocId::Integrations,
        path: format!("/{section}/enabled"),
        value: Some(json!(on)),
    }];
    if let Some(module) = item.module
        && !item.builtin
    {
        ops.extend(module_ops(app, module, on));
    }
    let label = if on {
        format!("Connect {}", item.name)
    } else {
        format!("Disconnect {}", item.name)
    };
    app.apply(&label, ops);
    if on && has_secret(app, item) == Some(false) {
        app.toast(
            format!(
                "{} is on: it needs its {}",
                item.name,
                item.secret.map_or("setup", |s| s.1).to_lowercase()
            ),
            None,
        );
    } else if on {
        app.toast(
            format!("{} is on", item.name),
            item.module
                .map(|_| ("Show me".to_owned(), Action::Nav(Page::Bars))),
        );
    } else {
        app.toast_undo(format!("{} is off", item.name));
    }
}

/// Stores the clipboard's text as a secret, then clears the clipboard. The value is never shown.
fn paste_secret(app: &mut App, credential: &str, name: &str) {
    let Some(text) = clipboard_text() else {
        app.alert(format!(
            "Copy the {} first, then Paste",
            name.to_lowercase()
        ));
        return;
    };
    let text = text.trim().to_owned();
    if text.is_empty() || text.len() > 2560 {
        app.alert(format!("That doesn't look like a {}", name.to_lowercase()));
        return;
    }
    match glaze_integrations::credentials::write(credential, &text) {
        Ok(()) => {
            crate::system::copy_text("");
            app.toast(format!("{name} stored in Windows Credential Manager"), None);
        }
        Err(e) => app.alert(format!("{e:#}")),
    }
}

fn clipboard_text() -> Option<String> {
    use windows::Win32::System::DataExchange::{CloseClipboard, GetClipboardData, OpenClipboard};
    use windows::Win32::System::Memory::{GlobalLock, GlobalUnlock};
    // SAFETY: the clipboard's own memory, read while it's open and locked.
    unsafe {
        OpenClipboard(None).ok()?;
        let text = GetClipboardData(13).ok().and_then(|h| {
            let mem = windows::Win32::Foundation::HGLOBAL(h.0);
            let ptr = GlobalLock(mem) as *const u16;
            if ptr.is_null() {
                return None;
            }
            let len = (0..).take_while(|&i| *ptr.add(i) != 0).count();
            let text = String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len));
            let _ = GlobalUnlock(mem);
            Some(text)
        });
        let _ = CloseClipboard();
        text
    }
}

/// Asks Windows for notification access (only on this click, on the UI thread).
fn grant_access(app: &mut App) {
    match glaze_integrations::notifications::request_access() {
        Ok(operation) => match operation.join() {
            Ok(status) => {
                use windows::UI::Notifications::Management::UserNotificationListenerAccessStatus as Status;
                let text = match status {
                    Status::Allowed => "Notifications: access granted",
                    Status::Denied => {
                        "Notifications: access denied (Settings › Privacy › Notifications)"
                    }
                    _ => "Notifications: Windows didn't decide",
                };
                app.toast(text, None);
            }
            Err(e) => app.alert(format!("{e}")),
        },
        Err(e) => app.alert(format!("Notification access isn't available here: {e}")),
    }
}
