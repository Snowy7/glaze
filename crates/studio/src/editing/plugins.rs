//! Plugin state and editing, including package consent and uninstall dialogs.
use std::collections::BTreeMap;
use std::time::Instant;

use mypaper_ui::Rect;
use mypaper_ui::controls::{ButtonKind, Font, key};
use mypaper_widgets::manifest::PluginManifest;
use mypaper_widgets::plugin_manager::{self as manager, Package, Preferences, Request, Snapshot};
use serde_json::{Value, json};

use crate::actions::{Action, Edit};
use crate::app::{App, Overlay};
use crate::docs::DocId;
use crate::ui::B;

#[derive(Default)]
pub struct State {
    pub installed: Vec<PluginManifest>,
    pub selected: Option<String>,
    pub snapshot: Snapshot,
    pub preferences: Preferences,
    pub pending: Option<Package>,
    pub busy: Option<String>,
    pub refreshed: Option<Instant>,
    pub channel_error: Option<String>,
}
impl State {
    pub fn refresh(&mut self) {
        self.preferences = Preferences::load();
        let mut installed = BTreeMap::new();
        // User folders take precedence, just as they do in the shell registry.
        for root in mypaper_widgets::registry::plugin_roots().into_iter().rev() {
            if let Ok(entries) = std::fs::read_dir(root) {
                for entry in entries.flatten() {
                    if let Ok(manifest) = PluginManifest::load(&entry.path())
                        && !self.preferences.uninstalled.contains(&manifest.id)
                    {
                        installed.insert(manifest.id.clone(), manifest);
                    }
                }
            }
        }
        self.installed = installed.into_values().collect();
        self.snapshot = Snapshot::load();
        self.channel_error = manager::send(&Request::Refresh)
            .err()
            .map(|e| format!("{e:#}"));
        self.refreshed = Some(Instant::now());
        if self
            .selected
            .as_ref()
            .is_none_or(|id| !self.installed.iter().any(|m| &m.id == id))
        {
            self.selected = self.installed.first().map(|m| m.id.clone());
        }
    }
}

fn action(path: impl Into<String>, value: Value) -> Action {
    Action::Set(Edit::new(DocId::Studio, path.into(), "Plugins"), value)
}

pub fn permissions(m: &PluginManifest) -> Vec<String> {
    let mut lines: Vec<_> = m
        .permissions
        .iter()
        .map(|p| {
            manager::permission_text(p).unwrap_or_else(|_| format!("Requests permission: {p}"))
        })
        .collect();
    for module in &m.native {
        lines.push(format!("Runs native code: {module}"));
    }
    for surface in &m.styles {
        lines.push(format!("Can restyle the {}", surface.replace('-', " ")));
    }
    if !m.command.is_empty() {
        lines.push(format!("Runs a native executable: {}", m.command));
    }
    if lines.is_empty() {
        lines.push("No additional permissions requested".into());
    }
    lines
}

pub fn consent(app: &App, b: &mut B) {
    let Some(package) = &app.plugins.pending else {
        return;
    };
    let m = &package.manifest;
    let p = *b.p;
    let r = crate::view::sheet(
        app,
        b,
        "Review plugin permissions",
        b.px(600.0),
        b.px(520.0),
    );
    let heading = b.font(Font::Title2);
    let small = b.font(Font::Caption);
    b.text_at(r.left, r.top, &m.name, &heading, p.ink1);
    b.text_at(
        r.left,
        r.top + b.px(30.0),
        &format!(
            "{} · {}",
            m.version.as_deref().unwrap_or("Unversioned"),
            m.author.as_deref().unwrap_or("Author not provided")
        ),
        &small,
        p.ink3,
    );
    let area = Rect::new(
        r.left,
        r.top + b.px(66.0),
        r.width(),
        r.height() - b.px(150.0),
    );
    let scroll = key("plugin-consent-permissions");
    b.scroll(
        scroll,
        area,
        app.nav.scroll(scroll),
        Action::None,
        |b, top| {
            let mut y = top;
            for text in permissions(m) {
                y += b.paragraph(r.left, y, r.width(), &text, &small, b.px(20.0), p.ink1)
                    + b.px(14.0);
            }
            y
        },
    );
    b.text_at(
        r.left,
        r.bottom - b.px(74.0),
        "Install only plugins from authors you trust.",
        &small,
        p.ink3,
    );
    let install_w = b.button_width("Allow and install", false, false);
    b.button(
        key("plugin-consent-allow"),
        Rect::new(
            r.right - install_w,
            r.bottom - b.px(46.0),
            install_w,
            b.px(32.0),
        ),
        "Allow and install",
        None,
        ButtonKind::Primary,
        true,
        action("/plugins/consent", json!(true)),
    );
    let cancel_w = b.button_width("Cancel", false, false);
    b.button(
        key("plugin-consent-cancel"),
        Rect::new(
            r.right - install_w - cancel_w - b.px(8.0),
            r.bottom - b.px(46.0),
            cancel_w,
            b.px(32.0),
        ),
        "Cancel",
        None,
        ButtonKind::Secondary,
        true,
        Action::Dismiss,
    );
}

pub fn uninstall_sheet(app: &App, b: &mut B, id: &str) {
    let r = crate::view::sheet(app, b, "Uninstall plugin?", b.px(520.0), b.px(260.0));
    let font = b.font(Font::Body);
    b.paragraph(r.left,r.top,r.width(),"This removes the plugin, its compiled cache, saved data and secrets. This cannot be undone.",&font,b.px(22.0),b.p.ink2);
    let w = b.button_width("Uninstall", false, false);
    b.button(
        key("plugin-uninstall-confirm"),
        Rect::new(r.right - w, r.bottom - b.px(50.0), w, b.px(32.0)),
        "Uninstall",
        None,
        ButtonKind::Primary,
        true,
        action("/plugins/uninstall-confirmed", json!(id)),
    );
    let cancel = b.button_width("Cancel", false, false);
    b.button(
        key("plugin-uninstall-cancel"),
        Rect::new(
            r.right - w - cancel - b.px(8.0),
            r.bottom - b.px(50.0),
            cancel,
            b.px(32.0),
        ),
        "Cancel",
        None,
        ButtonKind::Secondary,
        true,
        Action::Dismiss,
    );
}

pub fn intercept(app: &mut App, path: &str, value: Option<Value>) -> bool {
    let Some(rest) = path.strip_prefix("/plugins/") else {
        return false;
    };
    let value = value.unwrap_or(Value::Null);
    match rest {
        "tab" => {
            app.nav.plugins = value.as_bool().unwrap_or(false);
            if app.nav.plugins && !app.fixture {
                app.plugins.refresh();
            }
        }
        "select" => app.plugins.selected = value.as_str().map(str::to_owned),
        "file" | "folder" => {
            if app.fixture {
                return true;
            }
            if let Some(source) = crate::dialogs::pick(
                app.window,
                "Install plugin",
                rest == "folder",
                &[("mypaper plugins", "*.mypaper-plugin")],
            )
                && let Some(inbox) = app.inbox.clone()
            {
                app.plugins.busy = Some("Reading package…".into());
                let thread = app.thread;
                std::thread::spawn(move || {
                    let result = Package::prepare(&source).map_err(|e| format!("{e:#}"));
                    let _ = inbox.send(crate::app::Msg::PluginPrepared(result));
                    crate::wake(thread);
                });
            }
        }
        "consent" => {
            if value != json!(true) {
                return true;
            }
            let Some(package) = app.plugins.pending.take() else {
                return true;
            };
            app.overlay = None;
            if let Some(inbox) = app.inbox.clone() {
                app.plugins.busy = Some(format!("Installing {}…", package.manifest.name));
                let thread = app.thread;
                std::thread::spawn(move || {
                    let id = package.manifest.id.clone();
                    let result=(||->anyhow::Result<String>{
                        use std::os::windows::process::CommandExt;
                        let exe=crate::system::find_exe("mypaper-plugins.exe").ok_or_else(||anyhow::anyhow!("mypaper-plugins.exe is missing. Update mypaper before installing plugins."))?;
                        package.install(true,&mypaper_widgets::files::user("plugins")?,|dir|{
                            let output=std::process::Command::new(&exe).arg("--compile").arg(dir).creation_flags(0x0800_0000).output()?;
                            anyhow::ensure!(output.status.success(),"Plugin compilation failed: {} {}",String::from_utf8_lossy(&output.stdout),String::from_utf8_lossy(&output.stderr));Ok(())
                        })?;
                        let mut prefs=Preferences::load();prefs.disabled.remove(&id);prefs.uninstalled.remove(&id);prefs.save()?;
                        Ok(id)
                    })().map_err(|e|format!("{e:#}"));
                    let _ = inbox.send(crate::app::Msg::PluginInstalled(result));
                    crate::wake(thread);
                });
            }
        }
        "enable" | "disable" | "uninstall" | "uninstall-confirmed" => {
            if let Some(id) = value.as_str() {
                if rest == "uninstall" {
                    app.overlay = Some(Overlay::PluginUninstall(id.into()));
                    return true;
                }
                app.overlay = None;
                let request = match rest {
                    "enable" => Request::Enable(id.into()),
                    "disable" => Request::Disable(id.into()),
                    _ => Request::Uninstall(id.into()),
                };
                if !app.fixture {
                    match manager::send(&request) {
                        Ok(()) => app.toast("Plugin request sent", None),
                        Err(e) => app.alert(format!("{e:#}")),
                    }
                } else if rest == "enable" {
                    app.plugins.preferences.disabled.remove(id);
                } else {
                    app.plugins.preferences.disabled.insert(id.into());
                }
            }
        }
        _ if rest.starts_with("setting/") => {
            if let Some((id, k)) = rest[8..].split_once('/') {
                let k = k.replace("~1", "/").replace("~0", "~");
                let schema = app
                    .plugins
                    .installed
                    .iter()
                    .find(|m| m.id == id)
                    .and_then(|m| m.settings.iter().find(|s| s["key"].as_str() == Some(&k)));
                let value = match schema.and_then(|s| s["type"].as_str()) {
                    Some("string" | "file" | "image") if value.is_null() => json!(""),
                    Some("list" | "place") => {
                        match value.as_str().map(serde_json::from_str::<Value>) {
                            Some(Ok(v)) => v,
                            _ => {
                                app.alert("Enter valid JSON for this setting");
                                return true;
                            }
                        }
                    }
                    _ => value,
                };
                let settings = app
                    .plugins
                    .preferences
                    .settings
                    .entry(id.into())
                    .or_insert_with(|| json!({}));
                settings[&k] = value;
                let request = Request::Settings(id.into(), settings.clone());
                if !app.fixture
                    && let Err(e) = manager::send(&request)
                {
                    app.alert(format!("{e:#}"));
                }
            }
        }
        _ => {}
    }
    app.dirty = true;
    true
}

#[cfg(test)]
pub fn fixture(state: &mut State) {
    let manifests = [
        json!({"id":"t3","name":"T3 Code","version":"0.4.0","author":"T3","component":"t3.wasm","api":"0.1","permissions":["network:ws://127.0.0.1:3773","storage","secrets"],"faces":[{"id":"agents","name":"Running agents","size":{"default":[4,4]}}],"bars":[{"id":"status","name":"Agent status","size":{"default":[2,1]}}],"commands":[{"id":"refresh","label":"Refresh agents"}],"settings":[{"key":"endpoint","type":"string","label":"Server address","default":"ws://127.0.0.1:3773"},{"key":"live","type":"boolean","label":"Live updates","default":true}]}),
        json!({"id":"weather","name":"Weather","version":"1.2.0","author":"mypaper","component":"weather.wasm","api":"0.1","permissions":["network:https://api.open-meteo.com"]}),
        json!({"id":"hello","name":"Hello","version":"0.1.0","author":"mypaper","component":"hello.wasm","api":"0.1"}),
        json!({"id":"docker","name":"Docker","version":"0.2.0","author":"mypaper","component":"docker.wasm","api":"0.1"}),
    ];
    state.installed = manifests
        .into_iter()
        .map(|v| PluginManifest::parse(&v.to_string(), v["id"].as_str().unwrap()).unwrap())
        .collect();
    state.selected = Some("t3".into());
    for (id, status, memory, cpu_us, events) in [
        ("t3", "Live", 1_179_648, 840, 42),
        ("weather", "Live", 1_114_112, 352, 6),
        ("hello", "Unloaded", 0, 120, 2),
        ("docker", "Stopped: CPU budget exceeded", 0, 50_000, 12),
    ] {
        state.snapshot.plugins.insert(
            id.into(),
            manager::Metrics {
                measured: true,
                memory,
                cpu_us,
                events,
                status: status.into(),
            },
        );
    }
}

#[cfg(test)]
pub fn consent_fixture(app: &mut App) {
    app.nav.page = Some(crate::actions::Page::Integrations);
    app.nav.plugins = true;
    fixture(&mut app.plugins);
    let source =
        std::env::temp_dir().join(format!("mypaper-studio-consent-{}", std::process::id()));
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(source.join("plugin.json"),r#"{"id":"example","name":"Local companion","version":"1.0.0","author":"Example author","component":"example.wasm","api":"0.1","native":["module.dll"],"permissions":["network:ws://127.0.0.1:3773","storage","secrets","native-code"]}"#).unwrap();
    std::fs::write(source.join("example.wasm"), b"fixture").unwrap();
    std::fs::write(source.join("module.dll"), b"fixture").unwrap();
    app.plugins.pending = Some(Package::prepare(&source).unwrap());
    std::fs::remove_dir_all(&source).unwrap();
    app.overlay = Some(Overlay::PluginConsent);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clearing_a_text_setting_keeps_a_valid_empty_string() {
        let ui = mypaper_ui::Ui::new().unwrap();
        let mut app = crate::goldens::app(true, &ui);
        fixture(&mut app.plugins);
        intercept(&mut app, "/plugins/setting/t3/endpoint", None);
        let settings = &app.plugins.preferences.settings["t3"];
        assert_eq!(settings["endpoint"], json!(""));
        manager::resolve_settings(&app.plugins.installed[0], settings).unwrap();
    }
    #[test]
    fn consent_can_be_cancelled_and_cannot_be_bypassed_without_a_package() {
        let ui = mypaper_ui::Ui::new().unwrap();
        let mut app = crate::goldens::app(true, &ui);
        consent_fixture(&mut app);
        assert!(app.plugins.pending.is_some());
        app.run(Action::Dismiss);
        assert!(app.plugins.pending.is_none());
        assert_eq!(app.overlay, None);
        intercept(&mut app, "/plugins/consent", Some(json!(true)));
        assert!(app.plugins.busy.is_none());
        assert!(app.plugins.pending.is_none());
    }
}
