//! Integrations (spec 4.6), on the shell's contract (docs/integrations.md): `integrations.json`
//! says which are on and where they connect, `bars.json` which modules show them, and secrets
//! live in Windows Credential Manager (`mypaper_integrations::credentials`), never in a file.
//! A token or password is taken from the clipboard and stored without ever being shown.
//!
//! The list is Connected, Built in and Available; the details say what an integration adds,
//! where it shows (a bar module), its options, and one privacy line. Connect turns it on and
//! adds its module; Disconnect takes both back in one undo step.

use mypaper_ui::controls::Glyph;
use mypaper_ui::controls::{Align, ButtonKind, Font, key};
use mypaper_ui::{Point, Rect, Role};
use serde_json::{Value, json};

use crate::actions::{Action, Edit, FieldId, Page};
use crate::app::App;
use crate::docs::{DocId, Op};
use crate::theme::glyph;
use crate::ui::{B, label, text_field};

thread_local! {static ADVANCED: std::cell::RefCell<std::collections::BTreeSet<usize>> = const {std::cell::RefCell::new(std::collections::BTreeSet::new())};}

/// One option of an integration, in `integrations.json`.
pub enum Field {
    Text { path: &'static str, label: &'static str, placeholder: &'static str },
    Switch { path: &'static str, label: &'static str, hint: &'static str },
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
            Field::Switch { path: "/notifications/showText", label: "Show what they say", hint: "Off shows only which app, on a plain card" },
            Field::Switch { path: "/notifications/liveVerified", label: "Live badges", hint: "Only once delivery is qualified; otherwise read on open" },
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
        setup: "A dock app's Recent items, Discord's direct messages and your Grok routines in Start.",
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
    Integration {
        name: "T3 Code",
        adds: "Running agents, usage, prompts and approvals",
        glyph: glyph::TERMINAL,
        section: Some("t3"),
        builtin: false,
        module: Some("t3"),
        secret: Some(("t3", "Token")),
        fields: &[
            Field::Text { path: "/t3/endpoint", label: "Address", placeholder: "ws://127.0.0.1:3773/ws" },
            Field::Switch { path: "/t3/liveVerified", label: "Live updates", hint: "Off: one snapshot when you open it" },
        ],
        setup: "Connect finds T3 Code on this PC and reuses your pairing. On a first connection, paste one pairing code from T3 Settings > Connections. Allow status, diagnostics for usage, and thread controls to use every widget face.",
        privacy: "Your T3 grant stays in Windows Credential Manager. Threads and approvals run only on your click. No transcripts retained.",
    },
    Integration {
        name: "OBS Studio",
        adds: "REC and LIVE in the top bar, scenes",
        glyph: glyph::PULSE,
        section: Some("obs"),
        builtin: false,
        module: Some("obs"),
        secret: Some(("obs", "Password")),
        fields: &[Field::Text { path: "/obs/endpoint", label: "Address", placeholder: "ws://127.0.0.1:4455" }],
        setup: "In OBS: Tools › WebSocket Server Settings. Copy its password, then Paste password.",
        privacy: "Talks to OBS on this PC. Outputs and scenes only: no meters or previews.",
    },
    Integration {
        name: "Docker",
        adds: "Container failures as a badge",
        glyph: glyph::LAYERS,
        section: Some("docker"),
        builtin: false,
        module: Some("docker"),
        secret: None,
        fields: &[Field::Text { path: "/docker/pipe", label: "Engine pipe", placeholder: "\\\\.\\pipe\\docker_engine" }],
        setup: "The named pipe of your local Docker context. Monitoring never starts Docker Desktop or WSL.",
        privacy: "Reads Docker's local engine. Nothing leaves this PC.",
    },
    Integration {
        name: "Tailscale",
        adds: "Connection and exit node",
        glyph: glyph::GLOBE,
        section: Some("tailscale"),
        builtin: false,
        module: Some("tailscale"),
        secret: None,
        fields: &[
            Field::Text { path: "/tailscale/pipe", label: "Service pipe", placeholder: "\\\\.\\pipe\\ProtectedPrefix\\Administrators\\Tailscale\\tailscaled" },
            Field::Switch { path: "/tailscale/liveVerified", label: "Live updates", hint: "Off: one status when you open it" },
        ],
        setup: "Uses the Tailscale service already on this PC.",
        privacy: "Reads the local Tailscale service's status, with its own access checks.",
    },
    Integration {
        name: "Grok Bot",
        adds: "Quick prompts, routines and recent requests",
        glyph: glyph::STAR,
        section: Some("grok"),
        builtin: false,
        module: None,
        secret: None,
        fields: &[],
        setup: "Connect detects Grok Bot on this PC. Sign in once in Grok Bot. Quick prompt opens a draft there for you to send. Grok does not expose its routine list or run results to other desktop apps; optional webhook routines are under Advanced.",
        privacy: "Uses Grok Bot's existing sign-in. Optional webhook keys stay in Windows Credential Manager; requests run only on your click.",
    },
];

fn enabled(app: &App, i: &Integration) -> bool {
    match i.section {
        Some(s) => app.get(DocId::Integrations, &format!("/{s}/enabled")).and_then(Value::as_bool).unwrap_or(i.builtin && s != "notifications"),
        None => true,
    }
}

/// Whether the secret is stored (one Credential Manager read; the value is never kept).
fn has_secret(app: &App, i: &Integration) -> Option<bool> {
    let (k, _) = i.secret?;
    if app.fixture {
        return Some(false);
    }
    Some(mypaper_integrations::credentials::read(k).is_ok())
}

/// Where its module is in the top bar, if anywhere.
fn module_shown(app: &App, module: &str) -> bool {
    ["left", "center", "right"].iter().any(|l| {
        app.get(DocId::Bars, &format!("/top/{l}"))
            .and_then(Value::as_array)
            .is_some_and(|a| a.iter().any(|m| m.as_str() == Some(module)))
    })
}

fn status(app: &App, i: &Integration) -> &'static str {
    match (enabled(app, i), has_secret(app, i)) {
        (false, _) if i.builtin => "Off",
        (false, _) => "Connect",
        (true, Some(false)) => "Needs attention",
        (true, _) if i.builtin => "On",
        (true, _) => "Connected",
    }
}

pub fn title(app: &App) -> (String, String) {
    let n = LIST.iter().filter(|i| !i.builtin && enabled(app, i)).count();
    ("Integrations".into(), format!("{n} connected"))
}

pub fn bar(app: &App, b: &mut B, bar: Rect, right: f32) -> f32 {
    let y = bar.top + (bar.height() - b.px(32.0)) / 2.0;
    let field = Rect::new(right - b.px(220.0), y, b.px(220.0), b.px(32.0));
    text_field(app, b, field, FieldId::IntegrationSearch, Some(glyph::SEARCH), "Search integrations", "", "Search integrations");
    field.left - b.px(8.0)
}

pub fn view(app: &App, b: &mut B, body: Rect) {
    if app.docs.error(DocId::Integrations).is_some() {
        let r = Rect::new(body.left + b.px(24.0), body.top + b.px(12.0), body.width() - b.px(48.0), 0.0);
        crate::ui::file_banner(app, b, r, DocId::Integrations);
    }
    let list = Rect::new(body.left, body.top, b.px(440.0), body.height());
    let needle = app.text(&FieldId::IntegrationSearch).trim().to_lowercase();
    let x = list.left + b.px(24.0);
    let w = list.width() - b.px(40.0);
    let view = key("integration-list");
    b.scroll(view, list, app.nav.scroll(view), Action::None, |b, top| {
        let mut y = top + b.px(16.0);
        b.group(key("integrations"), |b| {
            for group in ["Connected", "Built in", "Available"] {
                let items: Vec<(usize, &Integration)> = LIST
                    .iter()
                    .enumerate()
                    .filter(|(_, i)| match group {
                        "Connected" => !i.builtin && enabled(app, i),
                        "Built in" => i.builtin,
                        _ => !i.builtin && !enabled(app, i),
                    })
                    .filter(|(_, i)| needle.is_empty() || format!("{} {}", i.name, i.adds).to_lowercase().contains(&needle))
                    .collect();
                if items.is_empty() {
                    continue;
                }
                b.section_header(Rect::new(x + b.px(4.0), y + b.px(2.0), w, b.px(18.0)), group);
                y += b.px(28.0);
                for (i, item) in items {
                    row(app, b, Rect::new(x - b.px(4.0), y, w + b.px(8.0), b.px(52.0)), i, item);
                    y += b.px(54.0);
                }
                y += b.px(10.0);
            }
        });
        y
    });
    details(app, b, Rect::new(list.right + b.px(24.0), body.top + b.px(20.0), body.right - list.right - b.px(48.0), body.height() - b.px(40.0)));
}

fn row(app: &App, b: &mut B, r: Rect, i: usize, item: &Integration) {
    let p = *b.p;
    let k = key(("integration", i));
    let on = app.nav.integration == i;
    if on {
        b.fill(r, b.px(10.0), p.selected);
    } else {
        b.wash(k, r, b.px(10.0));
    }
    let tile = Rect::new(r.left + b.px(10.0), r.top + b.px(10.0), b.px(32.0), b.px(32.0));
    b.fill(tile, b.px(8.0), p.fill);
    b.icon(tile, item.glyph, 15.0, p.ink1);
    let strong = b.font(Font::Strong);
    let small = b.font(Font::Caption);
    b.text_at(tile.right + b.px(12.0), r.top + b.px(8.0), item.name, &strong, p.ink1);
    b.text_in(Rect::new(tile.right + b.px(12.0), r.top + b.px(27.0), r.width() - b.px(170.0), b.px(16.0)), item.adds, &small, p.ink3, Align::Start);
    let state = status(app, item);
    if state == "Connect" {
        let bw = b.button_width("Connect", false, true);
        b.button(key(("connect-row", i)), Rect::new(r.right - bw - b.px(10.0), r.top + b.px(12.0), bw, b.px(28.0)), "Connect", None, ButtonKind::Secondary, true, Action::SelectIntegration(i));
    } else {
        let (sw, _) = b.measure(state, &small);
        let sx = r.right - sw - b.px(10.0);
        if matches!(state, "Connected" | "On") {
            b.dot(Point::new(sx - b.px(10.0), r.top + r.height() / 2.0), true);
        } else if state == "Needs attention" {
            b.icon(Rect::new(sx - b.px(18.0), r.top, b.px(14.0), r.height()), glyph::ALERT, 11.0, p.ink1);
        }
        b.text_in(Rect::new(sx, r.top, sw + b.px(2.0), r.height()), state, &small, p.ink2, Align::Start);
    }
    b.hit_checked(k, Rect::new(r.left, r.top, r.width() - b.px(100.0), r.height()), Role::ListItem, &format!("{}: {state}", item.name), Some(on), Action::SelectIntegration(i), mypaper_ui::controls::Behavior::Click);
    b.focus_ring(k, r, b.px(8.0));
}

fn details(app: &App, b: &mut B, area: Rect) {
    let p = *b.p;
    let Some(item) = LIST.get(app.nav.integration) else { return };
    let i = app.nav.integration;
    let on = enabled(app, item);
    let secret = has_secret(app, item);
    let r = area;
    let view = key(("integration-details", i));
    b.fill(r, b.px(12.0), p.panel);
    let x = r.left + b.px(16.0);
    let w = r.width() - b.px(32.0);
    b.scroll(view, r.inflate(-1.0), app.nav.scroll(view), Action::None, |b, top| {
        let tile = Rect::new(x, top + b.px(16.0), b.px(44.0), b.px(44.0));
        b.fill(tile, b.px(11.0), p.fill);
        b.icon(tile, item.glyph, 20.0, p.ink1);
        let t2 = b.font(Font::Title2);
        let small = b.font(Font::Caption);
        b.text_at(tile.right + b.px(14.0), tile.top + b.px(2.0), item.name, &t2, p.ink1);
        let state = status(app, item);
        let line = match state {
            "Needs attention" => format!("Needs its {}", item.secret.map_or("setup", |s| s.1).to_lowercase()),
            "Connect" => "Not connected".into(),
            s => format!("{s} · this PC"),
        };
        b.dot(Point::new(tile.right + b.px(18.0), tile.top + b.px(32.0)), on);
        b.text_at(tile.right + b.px(28.0), tile.top + b.px(24.0), &line, &small, p.ink3);
        if let Some(section) = item.section {
            let (label, kind, action) = if on {
                (if item.builtin { "Turn off" } else { "Disconnect" }, ButtonKind::Secondary, Action::Set(Edit::new(DocId::Studio, format!("/integration_off/{i}"), "Disconnect"), Value::Null))
            } else {
                (if item.builtin { "Turn on" } else { "Connect" }, ButtonKind::Primary, Action::Set(Edit::new(DocId::Studio, format!("/integration_on/{i}"), "Connect"), Value::Null))
            };
            let bw = b.button_width(label, false, false);
            b.button(key(("connect", section)), Rect::new(r.right - b.px(16.0) - bw, tile.top + b.px(6.0), bw, b.px(32.0)), label, None, kind, true, action);
        }
        let mut y = top + b.px(76.0);
        b.hline(x, x + w, y, p.line);
        y += b.px(14.0);
        let heading = b.font(Font::Heading);
        // Shows in: its top-bar module.
        if let Some(module) = item.module {
            b.text_at(x, y, "Shows in", &heading, p.ink1);
            y += b.px(28.0);
            let shown = module_shown(app, module);
            let row = Rect::new(x, y, w, b.px(40.0));
            label(b, row, "Top bar", "Its module, at the end", None);
            b.switch(key(("shows-in", module)), Point::new(x + w - b.px(36.0), y + b.px(10.0)), shown, "Top bar", Action::Set(Edit::new(DocId::Studio, format!("/integration_module/{i}"), "Shows in"), Value::Null));
            y += b.px(46.0);
            b.hline(x, x + w, y, p.line);
            y += b.px(14.0);
        }
        // Setup and options.
        b.text_at(x, y, if matches!(item.name, "T3 Code" | "Grok Bot") {"Connection"} else {"Options"}, &heading, p.ink1);
        y += b.px(28.0);
        y += b.paragraph(x, y, w, item.setup, &small, b.px(16.0), p.ink2) + b.px(10.0);
        let local = matches!(item.name, "T3 Code" | "Grok Bot");
        let advanced = !local || ADVANCED.with_borrow(|set| set.contains(&i));
        if local {
            let aw = b.button_width(if advanced {"Hide advanced"}else{"Advanced"}, false, true);
            b.button(key(("integration-advanced",i)),Rect::new(x,y,aw,b.px(28.0)),if advanced {"Hide advanced"}else{"Advanced"},None,ButtonKind::Ghost,true,Action::Set(Edit::new(DocId::Studio,format!("/integration_advanced/{i}"),"Advanced"),Value::Null));
            y += b.px(38.0);
        }
        if advanced && let Some((_, name)) = item.secret {
            let row = Rect::new(x, y, w, b.px(40.0));
            let stored = secret == Some(true);
            label(b, row, name, if stored { "Stored in Windows Credential Manager" } else { "Not set" }, None);
            let paste = format!("Paste {}", name.to_lowercase());
            let pw = b.button_width(&paste, true, true);
            b.button(key(("paste-secret", name)), Rect::new(x + w - pw, y + b.px(6.0), pw, b.px(28.0)), &paste, Some(glyph::COPY), ButtonKind::Secondary, true, Action::Set(Edit::new(DocId::Studio, format!("/integration_secret/{i}"), name), Value::Null));
            if stored {
                let fw = b.button_width("Forget", false, true);
                b.button(key(("forget-secret", name)), Rect::new(x + w - pw - fw - b.px(8.0), y + b.px(6.0), fw, b.px(28.0)), "Forget", None, ButtonKind::Ghost, true, Action::Set(Edit::new(DocId::Studio, format!("/integration_forget/{i}"), name), Value::Null));
            }
            y += b.px(46.0);
        }
        if item.name == "Notifications" {
            let row = Rect::new(x, y, w, b.px(40.0));
            label(b, row, "Access", "Windows asks once", None);
            let gw = b.button_width("Grant access", false, true);
            b.button(key("grant-access"), Rect::new(x + w - gw, y + b.px(6.0), gw, b.px(28.0)), "Grant access", None, ButtonKind::Secondary, true, Action::Set(Edit::new(DocId::Studio, "/integration_access", "Access"), Value::Null));
            y += b.px(46.0);
        }
        for field in item.fields.iter().filter(|_| advanced) {
            match field {
                Field::Switch { path, label: l, hint } => {
                    let value = app.get(DocId::Integrations, path).and_then(Value::as_bool).unwrap_or(false);
                    let row = Rect::new(x, y, w, b.px(40.0));
                    label(b, row, l, hint, None);
                    b.switch(key(("int-switch", *path)), Point::new(x + w - b.px(36.0), y + b.px(10.0)), value, l, Action::Toggle(Edit::new(DocId::Integrations, *path, *l), value));
                    y += b.px(46.0);
                }
                Field::Text { path, label: l, placeholder } => {
                    let body = b.font(Font::Body);
                    b.text_at(x, y, l, &body, p.ink1);
                    let value = app.get(DocId::Integrations, path).and_then(Value::as_str).unwrap_or("").to_owned();
                    text_field(app, b, Rect::new(x, y + b.px(22.0), w, b.px(32.0)), FieldId::Text(DocId::Integrations, (*path).into()), None, placeholder, &value, l);
                    y += b.px(62.0);
                }
            }
        }
        if item.name == "Grok Bot" && advanced {
            y = routines(app, b, x, y, w);
        }
        if item.section.is_none() || item.name == "Weather and RSS" {
            let lw = b.button_width("Their widgets", true, true);
            b.button(key("int-widgets"), Rect::new(x, y, lw, b.px(28.0)), "Their widgets", Some(glyph::CHEVRON_RIGHT), ButtonKind::Secondary, true, Action::Nav(Page::Widgets));
            y += b.px(36.0);
        }
        b.hline(x, x + w, y, p.line);
        y += b.px(12.0);
        b.icon(Rect::new(x, y, b.px(16.0), b.px(20.0)), glyph::INFO, 13.0, p.ink3);
        y += b.paragraph(x + b.px(26.0), y + b.px(2.0), w - b.px(26.0), item.privacy, &small, b.px(16.0), p.ink3);
        y + b.px(16.0)
    });
}

/// Grok routines: title and address, each with its own key.
fn routines(app: &App, b: &mut B, x: f32, mut y: f32, w: f32) -> f32 {
    let p = *b.p;
    let list = app.get(DocId::Integrations, "/grok/routines").and_then(Value::as_array).cloned().unwrap_or_default();
    for (n, r) in list.iter().enumerate() {
        let title = r.get("title").and_then(Value::as_str).unwrap_or("");
        let url = r.get("url").and_then(Value::as_str).unwrap_or("");
        let row = Rect::new(x, y, w, b.px(40.0));
        label(b, row, title, url, None);
        let rw = b.button_width("Remove", false, true);
        let kw = b.button_width("Paste key", false, true);
        b.button(key(("routine-key", n)), Rect::new(x + w - rw - kw - b.px(6.0), y + b.px(6.0), kw, b.px(28.0)), "Paste key", None, ButtonKind::Secondary, true, Action::Set(Edit::new(DocId::Studio, format!("/routine_key/{n}"), "Key"), Value::Null));
        let mut rest = list.clone();
        rest.remove(n);
        b.button(key(("routine-remove", n)), Rect::new(x + w - rw, y + b.px(6.0), rw, b.px(28.0)), "Remove", None, ButtonKind::Ghost, true, Action::Set(Edit::new(DocId::Integrations, "/grok/routines", "Remove routine"), Value::Array(rest)));
        y += b.px(46.0);
    }
    let half = (w - b.px(82.0)) / 2.0;
    text_field(app, b, Rect::new(x, y, half, b.px(32.0)), FieldId::Text(DocId::Studio, "/routine_title".into()), None, "A routine's name", "", "Routine name");
    text_field(app, b, Rect::new(x + half + b.px(6.0), y, half, b.px(32.0)), FieldId::Text(DocId::Studio, "/routine_url".into()), None, "https://", "", "Routine address");
    b.button(key("routine-add"), Rect::new(x + w - b.px(70.0), y, b.px(70.0), b.px(32.0)), "Add", None, ButtonKind::Secondary, true, Action::Set(Edit::new(DocId::Studio, "/routine_add", "Add routine"), Value::Null));
    let _ = p;
    y + b.px(44.0)
}

/// Integration actions (they arrive as Studio pseudo-paths through `pages::intercept`).
pub fn intercept(app: &mut App, path: &str) -> bool {
    let n = |prefix: &str| path.strip_prefix(prefix).and_then(|n| n.parse::<usize>().ok());
    if let Some(i) = n("/integration_advanced/") {
        ADVANCED.with_borrow_mut(|set| {if !set.remove(&i){set.insert(i);}});
        app.dirty=true;
    } else if let Some(i) = n("/integration_on/") {
        connect(app, i, true);
    } else if let Some(i) = n("/integration_off/") {
        connect(app, i, false);
    } else if let Some(i) = n("/integration_module/") {
        let Some(module) = LIST.get(i).and_then(|item| item.module) else { return true };
        let shown = module_shown(app, module);
        app.apply("Shows in", module_ops(app, module, !shown));
    } else if let Some(i) = n("/integration_secret/") {
        let Some((k, name)) = LIST.get(i).and_then(|item| item.secret) else { return true };
        paste_secret(app, k, name);
    } else if let Some(i) = n("/integration_forget/") {
        let Some((k, name)) = LIST.get(i).and_then(|item| item.secret) else { return true };
        match mypaper_integrations::credentials::remove(k) {
            Ok(()) => app.toast(format!("Forgot the {}", name.to_lowercase()), None),
            Err(e) => app.alert(format!("{e:#}")),
        }
    } else if path == "/integration_access" {
        grant_access(app);
    } else if let Some(n) = n("/routine_key/") {
        let id = app.get(DocId::Integrations, &format!("/grok/routines/{n}/id")).and_then(Value::as_str).map(str::to_owned);
        if let Some(id) = id {
            paste_secret(app, &format!("grok/{id}"), "Key");
        }
    } else if path == "/routine_add" {
        let title = app.text(&FieldId::Text(DocId::Studio, "/routine_title".into())).trim().to_owned();
        let url = app.text(&FieldId::Text(DocId::Studio, "/routine_url".into())).trim().to_owned();
        if title.is_empty() || !url.starts_with("https://") {
            app.alert("A routine needs a name and an https:// address");
            return true;
        }
        let id: String = crate::library::slug(&title).chars().take(64).collect();
        let mut list = app.get(DocId::Integrations, "/grok/routines").and_then(Value::as_array).cloned().unwrap_or_default();
        list.push(json!({ "id": id, "title": title, "url": url }));
        app.apply("Add routine", vec![Op { doc: DocId::Integrations, path: "/grok/routines".into(), value: Some(Value::Array(list)) }]);
        app.texts.remove(&FieldId::Text(DocId::Studio, "/routine_title".into()));
        app.texts.remove(&FieldId::Text(DocId::Studio, "/routine_url".into()));
    } else if matches!(path, "/routine_title" | "/routine_url") {
        // Typed text: kept until Add.
        return false;
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
        let mut items = app.get(DocId::Bars, &path).and_then(Value::as_array).cloned().unwrap_or_default();
        let had = items.iter().any(|m| m.as_str() == Some(module));
        if !show && had {
            items.retain(|m| m.as_str() != Some(module));
            ops.push(Op { doc: DocId::Bars, path, value: Some(Value::Array(items)) });
        } else if show && list == "right" && !module_shown(app, module) {
            // Before the clock, when there is one.
            let at = items.iter().position(|m| m.as_str() == Some("clock")).unwrap_or(items.len());
            items.insert(at, json!(module));
            ops.push(Op { doc: DocId::Bars, path, value: Some(Value::Array(items)) });
        }
    }
    ops
}

/// Connect (on, and its module shown) or Disconnect (off, and its module gone): one step.
fn connect(app: &mut App, i: usize, on: bool) {
    let Some(item) = LIST.get(i) else { return };
    let Some(section) = item.section else { return };
    if on && !app.fixture && item.name == "T3 Code" {
        let endpoint=app.get(DocId::Integrations,"/t3/endpoint").and_then(Value::as_str).unwrap_or("").to_owned();
        match mypaper_integrations::pairing::connect_t3_blocking(&endpoint) {
            Ok(true)=>{},Ok(false)=>return,Err(error)=>{app.alert(format!("Could not connect T3 Code: {error}"));return;}
        }
    }
    if on && !app.fixture && item.name == "Grok Bot" {
        let grok=mypaper_integrations::grok::discover();
        if !grok.installed {app.alert("Grok Bot was not found on this PC. Install it, then Connect again.");return;}
        if !grok.signed_in {
            let _=mypaper_integrations::launcher::open(&mypaper_integrations::launcher::Recent{title:String::new(),target:"grokbot://app/v1/open".into(),arguments:String::new()});
        }
    }
    let mut ops = vec![Op { doc: DocId::Integrations, path: format!("/{section}/enabled"), value: Some(json!(on)) }];
    if on && item.name == "T3 Code" {
        ops.push(Op{doc:DocId::Integrations,path:"/t3/liveVerified".into(),value:Some(json!(true))});
        if app.get(DocId::Integrations,"/t3/endpoint").and_then(Value::as_str).is_none_or(str::is_empty) {
            ops.push(Op{doc:DocId::Integrations,path:"/t3/endpoint".into(),value:Some(json!(mypaper_integrations::t3_client::DEFAULT_ENDPOINT))});
        }
    }
    if let Some(module) = item.module
        && !item.builtin
    {
        ops.extend(module_ops(app, module, on));
    }
    let label = if on { format!("Connect {}", item.name) } else { format!("Disconnect {}", item.name) };
    app.apply(&label, ops);
    if on && has_secret(app, item) == Some(false) {
        app.toast(format!("{} is on: it needs its {}", item.name, item.secret.map_or("setup", |s| s.1).to_lowercase()), None);
    } else if on {
        app.toast(format!("{} is on", item.name), item.module.map(|_| ("Show me".to_owned(), Action::Nav(Page::Bars))));
    } else {
        app.toast_undo(format!("{} is off", item.name));
    }
}

/// Stores the clipboard's text as a secret, then clears the clipboard. The value is never shown.
fn paste_secret(app: &mut App, credential: &str, name: &str) {
    let Some(text) = clipboard_text() else {
        app.alert(format!("Copy the {} first, then Paste", name.to_lowercase()));
        return;
    };
    let text = text.trim().to_owned();
    if text.is_empty() || text.len() > 2560 {
        app.alert(format!("That doesn't look like a {}", name.to_lowercase()));
        return;
    }
    match mypaper_integrations::credentials::write(credential, &text) {
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
    match mypaper_integrations::notifications::request_access() {
        Ok(operation) => match operation.join() {
            Ok(status) => {
                use windows::UI::Notifications::Management::UserNotificationListenerAccessStatus as Status;
                let text = match status {
                    Status::Allowed => "Notifications: access granted",
                    Status::Denied => "Notifications: access denied (Settings › Privacy › Notifications)",
                    _ => "Notifications: Windows didn't decide",
                };
                app.toast(text, None);
            }
            Err(e) => app.alert(format!("{e}")),
        },
        Err(e) => app.alert(format!("Notification access isn't available here: {e}")),
    }
}
