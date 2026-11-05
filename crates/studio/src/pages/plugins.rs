//! Installed plugins, their cost and capabilities, and explicit package consent.
use std::collections::BTreeMap;
use std::time::Instant;

use mypaper_ui::controls::{Align, ButtonKind, Font, key};
use mypaper_ui::{Point, Rect, Role};
use mypaper_widgets::manifest::PluginManifest;
use mypaper_widgets::plugin_manager::{self as manager, Package, Preferences, Request, Snapshot};
use serde_json::{Value, json};

use crate::actions::{Action, Choices, Edit, FieldId};
use crate::app::{App, Overlay};
use crate::docs::DocId;
use crate::theme::glyph;
use crate::ui::{B, label, text_field};

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
pub fn tabs(app: &App, b: &mut B, r: Rect) -> f32 {
    let width = b.segmented_width(&["Connections", "Plugins"]);
    b.segmented(
        key("connection-tabs"),
        Rect::new(
            r.left,
            r.top + (r.height() - b.px(30.0)) / 2.0,
            width,
            b.px(30.0),
        ),
        &["Connections", "Plugins"],
        Some(usize::from(app.nav.plugins)),
        "Integration area",
        true,
        |i| action("/plugins/tab", json!(i == 1)),
    );
    r.left + width + b.px(16.0)
}
pub fn bar(app: &App, b: &mut B, bar: Rect, right: f32) -> f32 {
    let y = bar.top + (bar.height() - b.px(32.0)) / 2.0;
    let mut x = right;
    for (title, path) in [
        ("Install from file…", "/plugins/file"),
        ("Folder…", "/plugins/folder"),
    ] {
        let w = b.button_width(title, false, false);
        x -= w;
        b.button(
            key(path),
            Rect::new(x, y, w, b.px(32.0)),
            title,
            None,
            ButtonKind::Secondary,
            app.plugins.busy.is_none(),
            action(path, Value::Null),
        );
        x -= b.px(8.0);
    }
    x
}
fn state(app: &App, m: &PluginManifest) -> String {
    if app.plugins.snapshot.plugins.get(&m.id).is_some_and(|s| s.status == "Uninstalling…") { return "Uninstalling…".into(); }
    if app.plugins.preferences.disabled.contains(&m.id) {
        return "Stopped: Disabled".into();
    }
    app.plugins
        .snapshot
        .plugins
        .get(&m.id)
        .map(|s| s.status.clone())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Unloaded".into())
}
pub fn view(app: &App, b: &mut B, body: Rect) {
    let p = *b.p;
    let caption = b.font(Font::Caption);
    let strong = b.font(Font::Strong);
    let heading = b.font(Font::Title2);
    let x = body.left + b.px(24.0);
    let w = body.width() - b.px(48.0);
    let y = body.top + b.px(20.0);
    b.text_at(x, y, "Your plugins", &heading, p.ink1);
    let active = app
        .plugins
        .installed
        .iter()
        .filter(|m| state(app, m) == "Live")
        .count();
    b.text_at(
        x,
        y + b.px(29.0),
        &format!(
            "{} installed · {active} live · refreshes while you’re here",
            app.plugins.installed.len()
        ),
        &caption,
        p.ink3,
    );
    let notice = app
        .plugins
        .busy
        .as_deref()
        .or(app.plugins.channel_error.as_deref())
        .or(app.plugins.snapshot.error.as_deref());
    let mut top = y + b.px(68.0);
    if let Some(notice) = notice {
        top += b.paragraph(x, top, w, notice, &caption, b.px(17.0), p.ink2) + b.px(16.0);
    }
    let list_w = (w * 0.54).max(b.px(380.0));
    let list = Rect::new(x, top, list_w, body.bottom - top - b.px(24.0));
    let details = Rect::new(
        list.right + b.px(20.0),
        top,
        w - list_w - b.px(20.0),
        list.height(),
    );
    let cpu_x = list.right - b.px(136.0);
    let mem_x = list.right - b.px(63.0);
    b.section_header(Rect::new(x, top, list.width(), b.px(20.0)), "Installed");
    b.text_at(cpu_x, top, "CPU µs", &caption, p.ink3);
    b.text_at(mem_x, top, "Memory", &caption, p.ink3);
    let rows = Rect::new(
        list.left,
        list.top + b.px(30.0),
        list.width(),
        list.height() - b.px(30.0),
    );
    let scroll = key("plugin-list");
    b.scroll(scroll,rows,app.nav.scroll(scroll),Action::None,|b,top| {
        let mut y=top;
        if app.plugins.installed.is_empty() {
            b.text_at(x,y+b.px(16.0),"No plugins installed",&strong,p.ink1);
            y += b.px(44.0);
            y += b.paragraph(x,y,list.width(),"Install a package or choose a plugin folder. You review its permissions before anything is installed.",&caption,b.px(18.0),p.ink3);
        }
        for m in &app.plugins.installed {
            let r=Rect::new(x-b.px(8.0),y,list.width()+b.px(8.0),b.px(84.0));
            let k=key(("plugin",m.id.as_str())); let selected=app.plugins.selected.as_deref()==Some(&m.id);
            if selected { b.fill(r,b.px(10.0),p.selected); } else { b.wash(k,r,b.px(10.0)); }
            let left=x+b.px(6.0);
            let name_w=cpu_x-left-b.px(12.0);
            b.text_in(Rect::new(left,y+b.px(8.0),name_w,b.px(20.0)),&m.name,&strong,p.ink1,Align::Start);
            b.text_in(Rect::new(left,y+b.px(30.0),name_w,b.px(17.0)),&format!("{} · {}",m.version.as_deref().unwrap_or("Unversioned"),m.author.as_deref().unwrap_or("Author not provided")),&caption,p.ink3,Align::Start);
            let status=state(app,m);
            b.text_in(Rect::new(left,y+b.px(53.0),name_w,b.px(17.0)),&status,&caption,p.ink2,Align::Start);
            let metrics=app.plugins.snapshot.plugins.get(&m.id).filter(|m|m.measured);
            let cpu=metrics.map(|s|s.cpu_us.to_string()).unwrap_or_else(||"—".into());
            let memory=metrics.map(|s|format!("{:.1} MB",s.memory as f64/1048576.0)).unwrap_or_else(||"—".into());
            b.text_in(Rect::new(cpu_x,y,b.px(68.0),b.px(84.0)),&cpu,&caption,p.ink2,Align::Start);
            b.text_in(Rect::new(mem_x,y,b.px(64.0),b.px(84.0)),&memory,&caption,p.ink2,Align::Start);
            b.hit_checked(k,r,Role::ListItem,&format!("{}: {status}; CPU {cpu} microseconds; {memory}",m.name),Some(selected),action("/plugins/select",json!(m.id)),mypaper_ui::controls::Behavior::Click);
            y+=b.px(90.0);
        }
        y
    });
    detail(app, b, details);
}
fn detail(app: &App, b: &mut B, r: Rect) {
    let Some(m) = app
        .plugins
        .installed
        .iter()
        .find(|m| Some(m.id.as_str()) == app.plugins.selected.as_deref())
    else {
        return;
    };
    let p = *b.p;
    b.fill(r, b.px(12.0), p.panel);
    let x = r.left + b.px(18.0);
    let w = r.width() - b.px(36.0);
    let small = b.font(Font::Caption);
    let title = b.font(Font::Title2);
    let scroll = key(("plugin-detail", m.id.as_str()));
    b.scroll(scroll,r,app.nav.scroll(scroll),Action::None,|b,top| {
        let mut y=top+b.px(18.0);
        b.text_at(x,y,&m.name,&title,p.ink1); y+=b.px(30.0);
        y+=b.paragraph(x,y,w,&state(app,m),&small,b.px(17.0),p.ink2)+b.px(15.0);
        let disabled=app.plugins.preferences.disabled.contains(&m.id);
        let enable=if disabled {"Enable"} else {"Disable"};
        let bw=b.button_width(enable,false,false);
        b.button(key("plugin-toggle"),Rect::new(x,y,bw,b.px(32.0)),enable,None,ButtonKind::Secondary,app.plugins.busy.is_none(),action(if disabled {"/plugins/enable"} else {"/plugins/disable"},json!(m.id)));
        let uw=b.button_width("Uninstall…",false,false);
        b.button(key("plugin-uninstall"),Rect::new(x+bw+b.px(8.0),y,uw,b.px(32.0)),"Uninstall…",None,ButtonKind::Ghost,app.plugins.busy.is_none(),action("/plugins/uninstall",json!(m.id)));
        y+=b.px(54.0);
        b.section_header(Rect::new(x,y,w,b.px(18.0)),"Permissions"); y+=b.px(28.0);
        for line in permissions(m) { y+=b.paragraph(x,y,w,&line,&small,b.px(18.0),p.ink2)+b.px(8.0); }
        y+=b.px(16.0);
        b.section_header(Rect::new(x,y,w,b.px(18.0)),"Adds to your desktop"); y+=b.px(28.0);
        for (kind,names) in contributions(m) {
            y+=b.paragraph(x,y,w,&format!("{kind}: {}",names.join(", ")),&small,b.px(18.0),p.ink2)+b.px(8.0);
        }
        if m.faces.is_empty() && m.bars.is_empty() && m.panels.is_empty() && m.menu.is_empty() && m.commands.is_empty() { b.text_at(x,y,"No visual contributions",&small,p.ink3); y+=b.px(24.0); }
        y+=b.px(16.0);
        b.section_header(Rect::new(x,y,w,b.px(18.0)),"Settings"); y+=b.px(28.0);
        if m.settings.is_empty() { b.text_at(x,y,"This plugin has no settings.",&small,p.ink3); y+=b.px(24.0); }
        for schema in &m.settings { y=setting(app,b,m,schema,x,y,w); }
        if let Some(metrics)=app.plugins.snapshot.plugins.get(&m.id) {
            y+=b.px(18.0);
            y+=b.paragraph(x,y,w,&format!("{} events · CPU is cumulative plugin execution time, in microseconds. Memory is WebAssembly linear memory.",metrics.events),&small,b.px(18.0),p.ink3);
        }
        y+b.px(20.0)
    });
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
fn contributions(m: &PluginManifest) -> Vec<(&'static str, Vec<String>)> {
    let mut out = Vec::new();
    for (kind, faces) in [
        ("Widgets", &m.faces),
        ("Bar modules", &m.bars),
        ("Panels", &m.panels),
    ] {
        if !faces.is_empty() {
            out.push((kind, faces.iter().map(|f| f.name.clone()).collect()));
        }
    }
    for (kind, items) in [("Menu items", &m.menu), ("Commands", &m.commands)] {
        if !items.is_empty() {
            out.push((
                kind,
                items
                    .iter()
                    .map(|v| {
                        v["label"]
                            .as_str()
                            .or(v["id"].as_str())
                            .unwrap_or("Unnamed")
                            .to_owned()
                    })
                    .collect(),
            ));
        }
    }
    out
}
fn setting(app: &App, b: &mut B, m: &PluginManifest, s: &Value, x: f32, y: f32, w: f32) -> f32 {
    let Some(k) = s["key"].as_str() else { return y };
    let name = s["label"].as_str().unwrap_or(k);
    let path = format!(
        "/plugins/setting/{}/{}",
        m.id,
        k.replace('~', "~0").replace('/', "~1")
    );
    let value = app
        .plugins
        .preferences
        .settings
        .get(&m.id)
        .and_then(|v| v.get(k))
        .unwrap_or(&s["default"]);
    let edit = Edit::new(DocId::Studio, path.clone(), name);
    match s["type"].as_str().unwrap_or("string") {
        "boolean" => {
            label(
                b,
                Rect::new(x, y, w - b.px(48.0), b.px(40.0)),
                name,
                "",
                None,
            );
            b.switch(
                key(path.as_str()),
                Point::new(x + w - b.px(36.0), y + b.px(10.0)),
                value.as_bool().unwrap_or(false),
                name,
                Action::Toggle(edit, value.as_bool().unwrap_or(false)),
            );
            y + b.px(46.0)
        }
        "number" | "slider" => {
            let min = s["min"].as_f64().unwrap_or(0.0);
            let max = s["max"].as_f64().unwrap_or(100.0);
            let step = s["step"].as_f64().unwrap_or(1.0);
            label(b, Rect::new(x, y, w, b.px(25.0)), name, "", None);
            let v = value.as_f64().unwrap_or(min);
            b.slider(
                key(path.as_str()),
                Rect::new(x, y + b.px(25.0), w, b.px(36.0)),
                v,
                (min, max, step),
                &format!("{v:.1}"),
                b.px(48.0),
                name,
                Action::Slider(edit),
            );
            y + b.px(69.0)
        }
        "select" => {
            let options = s["options"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .map(|v| {
                            let val = v.get("value").unwrap_or(v).clone();
                            let text = v["label"]
                                .as_str()
                                .or(val.as_str())
                                .unwrap_or("Option")
                                .to_owned();
                            (val, text)
                        })
                        .collect()
                })
                .unwrap_or_default();
            label(b, Rect::new(x, y, w, b.px(24.0)), name, "", None);
            let text = value.as_str().unwrap_or("Choose");
            b.button(
                key(path.as_str()),
                Rect::new(x, y + b.px(26.0), w, b.px(32.0)),
                text,
                Some(glyph::CHEVRON_DOWN),
                ButtonKind::Secondary,
                true,
                Action::Choose(Choices {
                    edit,
                    options,
                    current: Some(value.clone()),
                }),
            );
            y + b.px(66.0)
        }
        _ => {
            let font = b.font(Font::Body);
            b.text_at(x, y, name, &font, b.p.ink1);
            let text = value.as_str().map(str::to_owned).unwrap_or_else(|| {
                if value.is_null() {
                    String::new()
                } else {
                    value.to_string()
                }
            });
            text_field(
                app,
                b,
                Rect::new(x, y + b.px(25.0), w, b.px(32.0)),
                FieldId::Text(DocId::Studio, path.into()),
                None,
                "",
                &text,
                name,
            );
            y + b.px(66.0)
        }
    }
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
            ) {
                if let Some(inbox) = app.inbox.clone() {
                    app.plugins.busy = Some("Reading package…".into());
                    let thread = app.thread;
                    std::thread::spawn(move || {
                        let result = Package::prepare(&source).map_err(|e| format!("{e:#}"));
                        let _ = inbox.send(crate::app::Msg::PluginPrepared(result));
                        crate::wake(thread);
                    });
                }
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
    let source = std::env::temp_dir().join(format!("mypaper-studio-consent-{}",std::process::id()));
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(source.join("plugin.json"),r#"{"id":"example","name":"Local companion","version":"1.0.0","author":"Example author","component":"example.wasm","api":"0.1","native":["module.dll"],"permissions":["network:ws://127.0.0.1:3773","storage","secrets","native-code"]}"#).unwrap();
    std::fs::write(source.join("example.wasm"),b"fixture").unwrap();
    std::fs::write(source.join("module.dll"),b"fixture").unwrap();
    app.plugins.pending=Some(Package::prepare(&source).unwrap());
    std::fs::remove_dir_all(&source).unwrap();
    app.overlay=Some(Overlay::PluginConsent);
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
        let ui=mypaper_ui::Ui::new().unwrap();
        let mut app=crate::goldens::app(true,&ui);
        consent_fixture(&mut app);
        assert!(app.plugins.pending.is_some());
        app.run(Action::Dismiss);
        assert!(app.plugins.pending.is_none());
        assert_eq!(app.overlay,None);
        intercept(&mut app,"/plugins/consent",Some(json!(true)));
        assert!(app.plugins.busy.is_none());
        assert!(app.plugins.pending.is_none());
    }
}
