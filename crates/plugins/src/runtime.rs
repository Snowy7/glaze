//! One Wasmtime engine for every plugin (docs/plugins.md, "Budgets"): pooling allocator, a
//! per-plugin `ResourceLimiter`, epoch interruption for the per-event CPU budget, and the AOT
//! cache. Host imports only record [`Effect`]s; the loop in `host.rs` applies them after the
//! call returns, so a plugin call never blocks on I/O.
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};
use mypaper_plugin_api::wire::{StylePatch, TokenValue};
use mypaper_plugin_sdk::view as sdk;
use serde::Deserialize;
use wasmtime::component::{Component, HasSelf, Linker, ResourceTable};
use wasmtime::{Config, Engine, InstanceAllocationStrategy, PoolingAllocationConfig, ResourceLimiter, Store};
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

use crate::{caps, images, io};

wasmtime::component::bindgen!({ path: "../plugin-api/wit", world: "plugin" });

use exports::mypaper::plugin::lifecycle::{Event, InitInfo};
use mypaper::plugin::events::Topic;
use mypaper::plugin::types::{Error as WitError, Tone};
use mypaper::plugin::view::{Node, NodeKind};

/// Hard cap on one plugin's linear memory; a manifest may only lower it.
pub const MEMORY_CAP: usize = 32 << 20;
/// Epoch tick while a plugin call runs. The ticker sleeps otherwise (idle 0% CPU).
pub const TICK: Duration = Duration::from_millis(5);
/// Per-event CPU budget in ticks (50 ms); `init` gets four times as much.
pub const EVENT_TICKS: u64 = 10;
const MAX_NODES: usize = 4096;

// ---------------------------------------------------------------------------------------------
// Manifest

#[derive(Debug, Clone, Deserialize)]
pub struct FaceSpec {
    pub id: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Limits {
    pub memory_mb: Option<u32>,
}

/// A WASM plugin's `plugin.json` (docs/plugins.md, "Manifest"); other fields are the shell's.
#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    pub id: String,
    #[serde(default)]
    pub name: String,
    pub component: String,
    pub api: String,
    #[serde(default)]
    pub faces: Vec<FaceSpec>,
    #[serde(default)]
    pub bars: Vec<FaceSpec>,
    #[serde(default)]
    pub panels: Vec<FaceSpec>,
    #[serde(default)]
    pub styles: Vec<String>,
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default)]
    pub limits: Limits,
    /// Native module DLLs in the plugin folder (`native-code`), run by `mypaper-native-host`.
    #[serde(default)]
    pub native: Vec<String>,
}

/// `location` is the legacy native weather exe's; no WIT capability uses it.
const WORDS: [&str; 7] = ["launch", "notifications", "secrets", "storage", "windows", "native-code", "location"];
pub const SURFACES: [&str; 6] = ["top-bar", "dock", "panel", "menu", "widget", "glass"];

impl Manifest {
    /// Reads and validates `<dir>/plugin.json`; refuses what this host can't honor.
    pub fn load(dir: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(dir.join("plugin.json")).with_context(|| format!("reading {}", dir.display()))?;
        let manifest: Self = serde_json::from_str(&text).context("plugin.json")?;
        let folder = dir.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        ensure!(manifest.id == folder, "id {:?} isn't its folder's name {folder:?}", manifest.id);
        let (major, minor) = manifest.api.split_once('.').and_then(|(a, b)| Some((a.parse::<u32>().ok()?, b.parse::<u32>().ok()?))).context("api must be \"<major>.<minor>\"")?;
        ensure!(major == mypaper_plugin_api::API_MAJOR && minor <= mypaper_plugin_api::API_MINOR, "built for plugin api {}; this mypaper has {}.{} (needs a newer mypaper)", manifest.api, mypaper_plugin_api::API_MAJOR, mypaper_plugin_api::API_MINOR);
        for permission in &manifest.permissions {
            let known = WORDS.contains(&permission.as_str())
                || permission.strip_prefix("network:").is_some_and(|o| caps::origin_allowed(&[permission.clone()], o))
                || permission.strip_prefix("pipe:").is_some_and(io::pipe_name_ok);
            ensure!(known, "unknown or malformed permission {permission:?}");
        }
        for surface in &manifest.styles {
            ensure!(SURFACES.contains(&surface.as_str()), "unknown style surface {surface:?}");
        }
        ensure!(!manifest.component.contains(['/', '\\']) && manifest.component.ends_with(".wasm"), "component must be a .wasm file in the plugin folder");
        ensure!(dir.join(&manifest.component).is_file(), "{} is missing", manifest.component);
        Ok(manifest)
    }

    pub fn contributes(&self, face: &str) -> bool {
        self.faces.iter().chain(&self.bars).chain(&self.panels).any(|f| f.id == face)
    }

    fn memory_cap(&self) -> usize {
        self.limits.memory_mb.map_or(MEMORY_CAP, |mb| (mb as usize) << 20).min(MEMORY_CAP)
    }
}

// ---------------------------------------------------------------------------------------------
// Store state and effects

/// What a plugin asked for during one call; applied by the host loop afterwards.
#[derive(Debug)]
pub enum Effect {
    View(String, sdk::Face),
    Clear(String),
    Style(String, Option<StylePatch>),
    After(String, u32),
    Every(String, u32),
    Cancel(String),
    Fetch { id: u64, method: &'static str, url: String, headers: Vec<(String, String)>, body: Option<Vec<u8>>, timeout_ms: u32 },
    Native { id: u64, module: String, method: String, bytes: Vec<u8> },
    /// Start the queued websocket `id` (its [`io::Conn`] is in the store).
    Socket { id: u64, request: io::SocketRequest },
    /// Start the queued pipe `id`.
    Pipe { id: u64, name: String },
    PipeHttp { id: u64, name: String, method: &'static str, url: String, headers: Vec<(String, String)>, body: Option<Vec<u8>> },
    Notify { title: String, body: String, plugin_name: String },
}

/// Tracks linear memory against the plugin's cap (and reports it in stats).
pub struct Limiter {
    cap: usize,
    pub used: usize,
}

impl ResourceLimiter for Limiter {
    fn memory_growing(&mut self, current: usize, desired: usize, _maximum: Option<usize>) -> wasmtime::Result<bool> {
        let next = self.used.saturating_sub(current) + desired;
        if next > self.cap {
            return Ok(false);
        }
        self.used = next;
        Ok(true)
    }
    fn table_growing(&mut self, _current: usize, desired: usize, _maximum: Option<usize>) -> wasmtime::Result<bool> {
        Ok(desired <= 65_536)
    }
    fn instances(&self) -> usize {
        32
    }
    fn tables(&self) -> usize {
        32
    }
    fn memories(&self) -> usize {
        4
    }
}

pub struct State {
    wasi: WasiCtx,
    table: ResourceTable,
    pub limiter: Limiter,
    pub id: String,
    permissions: Vec<String>,
    styles: Vec<String>,
    native: Vec<String>,
    data_dir: PathBuf,
    pub effects: Vec<Effect>,
    /// The manifest's display name, for notification attribution.
    name: String,
    sockets: HashMap<u64, io::Conn>,
    pipes: HashMap<u64, io::Conn>,
    topics: Vec<Topic>,
    /// Connections opened and not yet reported closed (a `close` keeps the id until `closed`).
    live: std::collections::HashSet<u64>,
    pub images: images::Images,
}

impl WasiView for State {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView { ctx: &mut self.wasi, table: &mut self.table }
    }
}

impl State {
    fn allowed(&self, permission: &str) -> Result<(), WitError> {
        if self.permissions.iter().any(|p| p == permission) {
            Ok(())
        } else {
            log::warn!("plugin {}: refused, needs {permission:?}", self.id);
            Err(WitError::Denied(format!("the manifest doesn't declare {permission:?}")))
        }
    }
    /// Unique across the process, so a late event from a dropped instance's request can never
    /// match a new instance's id.
    fn request_id(&mut self) -> u64 {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        NEXT.fetch_add(1, Ordering::Relaxed)
    }
    /// The topics this plugin subscribed to (the host routes system events by them).
    pub fn subscribed(&self, topic: Topic) -> bool {
        self.topics.contains(&topic) && (topic != Topic::Windows || self.permissions.iter().any(|p| p == "windows"))
    }
    /// The open websocket or pipe `id`, to start its thread or return delivery credit.
    pub fn conn(&self, id: u64) -> Option<&io::Conn> {
        self.sockets.get(&id).or_else(|| self.pipes.get(&id))
    }
    /// Forgets a connection whose thread ended; `false` if this instance never opened it (it
    /// belonged to an instance that trapped or unloaded).
    pub fn closed(&mut self, id: u64) -> bool {
        self.sockets.remove(&id);
        self.pipes.remove(&id);
        self.live.remove(&id)
    }
}

fn failed(e: String) -> WitError {
    WitError::Failed(e)
}

impl mypaper::plugin::types::Host for State {}
impl mypaper::plugin::view::Host for State {}

impl mypaper::plugin::ui::Host for State {
    fn set_view(&mut self, instance: String, face: mypaper::plugin::view::Face) {
        match tree(&face.nodes) {
            Ok(body) => self.effects.push(Effect::View(instance, sdk::Face { body, footer: face.footer, refresh: face.refresh })),
            Err(e) => log::warn!("plugin {}: bad view for {instance}: {e}", self.id),
        }
    }
    fn clear(&mut self, instance: String) {
        self.effects.push(Effect::Clear(instance));
    }
    fn put_image(&mut self, width: u32, height: u32, pixels: Vec<u8>) -> Result<u32, WitError> {
        self.images.put(width, height, &pixels).map_err(WitError::Invalid)
    }
    fn drop_image(&mut self, id: u32) {
        self.images.remove(id);
    }
}

impl mypaper::plugin::style::Host for State {
    fn apply(&mut self, patch: mypaper::plugin::style::StylePatch) -> Result<(), WitError> {
        use mypaper::plugin::style::TokenValue as V;
        let surface = surface_name(patch.surface);
        if !self.styles.iter().any(|s| s == surface) {
            return Err(WitError::Denied(format!("the manifest's styles don't list {surface:?}")));
        }
        let tokens = patch.tokens.into_iter().map(|t| {
            let value = match t.value {
                V::Color(c) => TokenValue::Color(c),
                V::Number(n) => TokenValue::Number(n),
                V::Tone(tone) => TokenValue::Tone(tone_name(&tone)),
                V::Flag(f) => TokenValue::Flag(f),
            };
            (t.name, value)
        });
        let patch = StylePatch { surface: surface.into(), tokens: tokens.collect() };
        self.effects.push(Effect::Style(surface.into(), Some(patch)));
        Ok(())
    }
    fn reset(&mut self, surface: mypaper::plugin::style::Surface) {
        let surface = surface_name(surface);
        if self.styles.iter().any(|s| s == surface) {
            self.effects.push(Effect::Style(surface.into(), None));
        }
    }
}

fn surface_name(surface: mypaper::plugin::style::Surface) -> &'static str {
    use mypaper::plugin::style::Surface::*;
    match surface {
        TopBar => "top-bar",
        Dock => "dock",
        Panel => "panel",
        Menu => "menu",
        Widget => "widget",
        Glass => "glass",
    }
}

impl mypaper::plugin::timers::Host for State {
    fn after(&mut self, id: String, ms: u32) {
        self.effects.push(Effect::After(id, ms));
    }
    fn every(&mut self, id: String, ms: u32) -> Result<(), WitError> {
        if ms < 1000 {
            return Err(WitError::Invalid("repeating timers are at least 1000 ms".into()));
        }
        self.effects.push(Effect::Every(id, ms));
        Ok(())
    }
    fn cancel(&mut self, id: String) {
        self.effects.push(Effect::Cancel(id));
    }
}

impl mypaper::plugin::events::Host for State {
    fn subscribe(&mut self, topics: Vec<mypaper::plugin::events::Topic>) -> Result<(), WitError> {
        if topics.contains(&Topic::Windows) {
            self.allowed("windows")?;
        }
        for topic in topics {
            if !self.topics.contains(&topic) {
                self.topics.push(topic);
            }
        }
        Ok(())
    }
    fn unsubscribe(&mut self, topics: Vec<mypaper::plugin::events::Topic>) {
        self.topics.retain(|t| !topics.contains(t));
    }
}

impl mypaper::plugin::http::Host for State {
    fn fetch(&mut self, request: mypaper::plugin::http::Request) -> Result<u64, WitError> {
        if !caps::origin_allowed(&self.permissions, &request.url) {
            log::warn!("plugin {}: refused fetch of an undeclared origin", self.id);
            return Err(WitError::Denied(format!("no network permission for {}", request.url.split('?').next().unwrap_or(""))));
        }
        let method = http_method(request.method);
        let id = self.request_id();
        let headers = request.headers.into_iter().map(|h| (h.name, h.value)).collect();
        let timeout_ms = request.timeout_ms.unwrap_or(15_000);
        self.effects.push(Effect::Fetch { id, method, url: request.url, headers, body: request.body, timeout_ms });
        Ok(id)
    }
}

fn http_method(method: mypaper::plugin::http::Method) -> &'static str {
    use mypaper::plugin::http::Method;
    match method {
        Method::Get => "GET",
        Method::Post => "POST",
        Method::Put => "PUT",
        Method::Patch => "PATCH",
        Method::Delete => "DELETE",
    }
}

impl mypaper::plugin::websocket::Host for State {
    fn connect(&mut self, url: String) -> Result<u64, WitError> {
        self.connect_with(mypaper::plugin::websocket::ConnectRequest { url, headers: Vec::new(), protocols: Vec::new() })
    }
    fn connect_with(&mut self, request: mypaper::plugin::websocket::ConnectRequest) -> Result<u64, WitError> {
        if !request.url.starts_with("ws") || !caps::origin_allowed(&self.permissions, &request.url) {
            log::warn!("plugin {}: refused a websocket to an undeclared origin", self.id);
            return Err(WitError::Denied(format!("no network permission for {}", request.url.split('?').next().unwrap_or(""))));
        }
        if self.sockets.len() >= io::MAX_CONNECTIONS {
            return Err(WitError::Failed(format!("at most {} open websockets", io::MAX_CONNECTIONS)));
        }
        let id = self.request_id();
        self.sockets.insert(id, io::socket().map_err(failed)?);
        self.live.insert(id);
        let headers = request.headers.into_iter().map(|h| (h.name, h.value)).collect();
        self.effects.push(Effect::Socket { id, request: io::SocketRequest { url: request.url, headers, protocols: request.protocols } });
        Ok(id)
    }
    fn send(&mut self, socket: u64, message: mypaper::plugin::websocket::Message) -> Result<(), WitError> {
        self.sockets.get(&socket).ok_or_else(|| WitError::Invalid("no such socket".into()))?.send(message).map_err(failed)
    }
    fn close(&mut self, socket: u64) {
        self.sockets.remove(&socket); // dropping the `Conn` closes it; `closed` still arrives
    }
}

impl mypaper::plugin::pipes::Host for State {
    fn open(&mut self, name: String) -> Result<u64, WitError> {
        self.allowed(&format!("pipe:{name}"))?;
        if self.pipes.len() >= io::MAX_CONNECTIONS {
            return Err(WitError::Failed(format!("at most {} open pipes", io::MAX_CONNECTIONS)));
        }
        let id = self.request_id();
        self.pipes.insert(id, io::pipe().map_err(failed)?);
        self.live.insert(id);
        self.effects.push(Effect::Pipe { id, name });
        Ok(id)
    }
    fn write(&mut self, pipe: u64, bytes: Vec<u8>) -> Result<(), WitError> {
        let conn = self.pipes.get(&pipe).ok_or_else(|| WitError::Invalid("no such pipe".into()))?;
        conn.send(mypaper::plugin::websocket::Message::Binary(bytes)).map_err(failed)
    }
    fn close(&mut self, pipe: u64) {
        self.pipes.remove(&pipe);
    }
    fn http_request(&mut self, name: String, request: mypaper::plugin::http::Request) -> Result<u64, WitError> {
        self.allowed(&format!("pipe:{name}"))?;
        let id = self.request_id();
        let headers = request.headers.into_iter().map(|h| (h.name, h.value)).collect();
        self.effects.push(Effect::PipeHttp { id, name, method: http_method(request.method), url: request.url, headers, body: request.body });
        Ok(id)
    }
}

impl mypaper::plugin::launch::Host for State {
    fn open(&mut self, target: String) -> Result<(), WitError> {
        self.allowed("launch")?;
        caps::launch(&target).map_err(failed)
    }
}

impl mypaper::plugin::notify::Host for State {
    fn show(&mut self, notification: mypaper::plugin::notify::Notification) -> Result<(), WitError> {
        self.allowed("notifications")?;
        if notification.title.is_empty() || notification.title.len() + notification.body.len() > 2000 {
            return Err(WitError::Invalid("a title, and at most 2000 bytes of text".into()));
        }
        self.effects.push(Effect::Notify { title: notification.title, body: notification.body, plugin_name: self.name.clone() });
        Ok(())
    }
}

impl mypaper::plugin::secrets::Host for State {
    fn get(&mut self, key: String) -> Result<Option<String>, WitError> {
        self.allowed("secrets")?;
        caps::secret_get(&self.id, &key).map_err(failed)
    }
    fn set(&mut self, key: String, value: String) -> Result<(), WitError> {
        self.allowed("secrets")?;
        caps::secret_set(&self.id, &key, &value).map_err(failed)
    }
    fn remove(&mut self, key: String) -> Result<(), WitError> {
        self.allowed("secrets")?;
        caps::secret_remove(&self.id, &key).map_err(failed)
    }
}

impl mypaper::plugin::storage::Host for State {
    fn get(&mut self, key: String) -> Result<Option<Vec<u8>>, WitError> {
        self.allowed("storage")?;
        caps::storage_get(&self.data_dir, &key).map_err(failed)
    }
    fn set(&mut self, key: String, value: Vec<u8>) -> Result<(), WitError> {
        self.allowed("storage")?;
        caps::storage_set(&self.data_dir, &key, &value).map_err(failed)
    }
    fn remove(&mut self, key: String) -> Result<(), WitError> {
        self.allowed("storage")?;
        caps::storage_remove(&self.data_dir, &key).map_err(failed)
    }
}

impl mypaper::plugin::system::Host for State {
    fn monitors(&mut self) -> Vec<mypaper::plugin::system::Monitor> {
        caps::monitors()
            .into_iter()
            .map(|m| mypaper::plugin::system::Monitor { id: m.id, x: m.rect.0, y: m.rect.1, width: m.rect.2, height: m.rect.3, scale: 1.0, primary: m.primary })
            .collect()
    }
    fn theme(&mut self) -> mypaper::plugin::system::ThemeInfo {
        mypaper::plugin::system::ThemeInfo { dark: caps::dark(), accent: String::new() }
    }
    fn power(&mut self) -> mypaper::plugin::system::PowerInfo {
        let (on_battery, percent, saver) = caps::power();
        mypaper::plugin::system::PowerInfo { on_battery, percent, saver }
    }
    fn foreground(&mut self) -> Result<Option<mypaper::plugin::system::WindowInfo>, WitError> {
        self.allowed("windows")?;
        Ok(caps::foreground().map(|w| mypaper::plugin::system::WindowInfo { id: w.id, title: w.title, process: w.process, x: w.rect.0, y: w.rect.1, width: w.rect.2, height: w.rect.3 }))
    }
}

impl mypaper::plugin::native::Host for State {
    fn call(&mut self, module: String, method: String, bytes: Vec<u8>) -> Result<u64, WitError> {
        self.allowed("native-code")?;
        if !self.native.contains(&module) {
            return Err(WitError::Denied(format!("{module:?} isn't in the manifest's native modules")));
        }
        let id = self.request_id();
        self.effects.push(Effect::Native { id, module, method, bytes });
        Ok(id)
    }
}

impl mypaper::plugin::log::Host for State {
    fn write(&mut self, level: mypaper::plugin::log::Level, message: String) {
        use mypaper::plugin::log::Level;
        let level = match level {
            Level::Debug => log::Level::Debug,
            Level::Info => log::Level::Info,
            Level::Warn => log::Level::Warn,
            Level::Error => log::Level::Error,
        };
        log::log!(level, "[{}] {}", self.id, message.chars().take(2000).collect::<String>());
    }
}

// ---------------------------------------------------------------------------------------------
// WIT view → the kit tree the shell draws

fn tone_name(tone: &Tone) -> String {
    match tone {
        Tone::Label => "label".into(),
        Tone::Secondary => "secondary".into(),
        Tone::Tertiary => "tertiary".into(),
        Tone::Accent => "accent".into(),
        Tone::Warning => "warning".into(),
        Tone::Destructive => "destructive".into(),
        Tone::Positive => "positive".into(),
        Tone::Hex(hex) => hex.clone(),
    }
}

fn tone(tone: &Tone) -> sdk::Tone {
    match tone {
        Tone::Label => sdk::Tone::Label,
        Tone::Secondary => sdk::Tone::Secondary,
        Tone::Tertiary => sdk::Tone::Tertiary,
        Tone::Accent => sdk::Tone::Accent,
        Tone::Warning => sdk::Tone::Warning,
        Tone::Destructive => sdk::Tone::Destructive,
        Tone::Positive => sdk::Tone::Positive,
        Tone::Hex(hex) => sdk::Tone::Hex(hex.clone()),
    }
}

fn tones(t: &Option<Tone>) -> Option<sdk::Tone> {
    t.as_ref().map(tone)
}

fn role(role: mypaper::plugin::types::TextRole) -> sdk::Role {
    use mypaper::plugin::types::TextRole::*;
    match role {
        LargeTitle => sdk::Role::LargeTitle,
        Title1 => sdk::Role::Title1,
        Title2 => sdk::Role::Title2,
        Title3 => sdk::Role::Title3,
        Headline => sdk::Role::Headline,
        Body => sdk::Role::Body,
        Callout => sdk::Role::Callout,
        Subheadline => sdk::Role::Subheadline,
        Footnote => sdk::Role::Footnote,
        Caption => sdk::Role::Caption,
    }
}

/// Rebuilds the tree from the flat pre-order list; refuses malformed or oversized lists.
pub fn tree(nodes: &[Node]) -> Result<sdk::View, String> {
    if nodes.is_empty() || nodes.len() > MAX_NODES {
        return Err(format!("a face has 1-{MAX_NODES} nodes"));
    }
    let mut at = 0;
    let view = node(nodes, &mut at, 0)?;
    if at != nodes.len() {
        return Err(format!("{} nodes after the root's subtree", nodes.len() - at));
    }
    Ok(view)
}

fn node(nodes: &[Node], at: &mut usize, depth: u32) -> Result<sdk::View, String> {
    use mypaper::plugin::types::Align;
    use sdk::View as V;
    if depth > 64 {
        return Err("views nest at most 64 deep".into());
    }
    let n = nodes.get(*at).ok_or("a node's children run past the list")?;
    *at += 1;
    let mut children = Vec::with_capacity(n.children as usize);
    for _ in 0..n.children {
        children.push(node(nodes, at, depth + 1)?);
    }
    let only = |mut children: Vec<V>| if children.len() == 1 { Ok(children.remove(0)) } else { Err("needs exactly one child".to_owned()) };
    Ok(match &n.kind {
        NodeKind::Stack(gap) => V::Stack { children, gap: *gap },
        NodeKind::Row(gap) => V::Row { children, gap: *gap },
        NodeKind::List(gap) => V::List { children, gap: *gap },
        NodeKind::Grid(g) => V::Grid { children, columns: g.columns.max(1), gap: g.gap },
        NodeKind::Card(c) => V::Card { title: c.title.clone(), child: Box::new(only(children)?) },
        NodeKind::Animated(s) => V::Animated { stiffness: s.stiffness, damping: s.damping, child: Box::new(only(children)?) },
        NodeKind::Text(t) => V::Text {
            text: t.text.clone(),
            role: role(t.role),
            color: tones(&t.color),
            align: match t.align {
                Align::Start => sdk::Align::Start,
                Align::Center => sdk::Align::Center,
                Align::End => sdk::Align::End,
            },
        },
        NodeKind::Icon(i) => V::Icon { icon: i.icon.clone(), size: i.size, color: tones(&i.color) },
        NodeKind::Button(b) => V::Button { label: b.label.clone(), action: b.action.clone(), icon: b.icon.clone(), primary: b.primary },
        NodeKind::IconButton(b) => V::IconButton { icon: b.icon.clone(), label: b.label.clone(), action: b.action.clone(), enabled: b.enabled },
        NodeKind::Select(s) => V::Select {
            label: s.label.clone(),
            action: s.action.clone(),
            options: s.options.iter().map(|o| sdk::SelectOption { label: o.label.clone(), value: o.value.clone() }).collect(),
            selected: s.selected.clone(),
        },
        NodeKind::Field(f) => V::Field { placeholder: f.placeholder.clone(), action: f.action.clone() },
        NodeKind::ListRow(r) => V::ListRow { title: r.title.clone(), detail: r.detail.clone(), icon: r.icon.clone(), trailing: r.trailing.clone(), action: r.action.clone() },
        NodeKind::Stat(s) => V::Stat { label: s.label.clone(), value: s.value.clone(), detail: s.detail.clone() },
        NodeKind::Metric(m) => V::Metric { value: m.value.clone(), label: m.label.clone(), color: tones(&m.color), width: m.width },
        NodeKind::Ring(r) => V::Ring { fraction: r.fraction, label: r.label.clone(), color: tones(&r.color) },
        NodeKind::LimitBar(l) => V::LimitBar { left: l.left, pace: l.pace, color: tones(&l.color), badge: l.badge.clone(), label: l.label.clone(), reset: l.reset.clone() },
        NodeKind::SegmentedBar(s) => V::SegmentedBar { segments: s.iter().map(|s| sdk::Segment { fraction: s.fraction, color: tone(&s.color) }).collect() },
        NodeKind::Sparkline(s) => V::Sparkline { values: s.values.clone(), color: tones(&s.color) },
        NodeKind::AreaChart(a) => {
            let label = |i: usize| a.x_labels.get(i).cloned().unwrap_or_default();
            V::AreaChart {
                series: a.series.iter().map(|s| sdk::Series { label: s.label.clone(), values: s.values.clone(), color: tones(&s.color) }).collect(),
                x_labels: [label(0), label(1), label(2)],
                unit: a.unit.clone(),
            }
        }
        NodeKind::Empty(e) => V::Empty { icon: e.icon.clone(), title: e.title.clone(), detail: e.detail.clone() },
        NodeKind::Spacer => V::Spacer,
        NodeKind::Canvas(c) => V::Canvas { height: c.height, commands: c.commands.iter().map(draw).collect() },
    })
}

/// A WIT canvas command as the shell renderer's (`sdk::Draw`).
fn draw(command: &mypaper::plugin::view::Draw) -> sdk::Draw {
    use mypaper::plugin::view::{Draw as D, Paint as P};
    let paint = |p: &P| match p {
        P::Fill(c) => sdk::Paint::Fill(tone(c)),
        P::Stroke((c, w)) => sdk::Paint::Stroke(tone(c), *w),
    };
    match command {
        D::Rect((r, radius, p)) => sdk::Draw::Rect { x: r.x, y: r.y, w: r.w, h: r.h, radius: *radius, paint: paint(p) },
        D::Circle((c, r, p)) => sdk::Draw::Circle { x: c.x, y: c.y, r: *r, paint: paint(p) },
        D::Line((a, b, c, w)) => sdk::Draw::Line { x1: a.x, y1: a.y, x2: b.x, y2: b.y, color: tone(c), width: *w },
        D::Path((d, p)) => sdk::Draw::Path { d: d.clone(), paint: paint(p) },
        D::Text((at, text, r, c)) => sdk::Draw::Text { x: at.x, y: at.y, text: text.clone(), role: role(*r), color: tone(c) },
        D::Image((r, id)) => sdk::Draw::Image { x: r.x, y: r.y, w: r.w, h: r.h, id: *id },
    }
}

// ---------------------------------------------------------------------------------------------
// Engine, cache, instances

/// Runs the epoch ticker only while a plugin call executes, so an idle runtime never wakes.
#[derive(Clone)]
pub struct Ticker(Arc<(Mutex<bool>, Condvar)>);

impl Ticker {
    fn start(engine: Engine) -> Self {
        let shared = Arc::new((Mutex::new(false), Condvar::new()));
        let ticker = Arc::clone(&shared);
        std::thread::Builder::new()
            .name("epoch".into())
            .stack_size(64 << 10)
            .spawn(move || {
                let (lock, wake) = &*ticker;
                let mut on = lock.lock().expect("ticker lock");
                loop {
                    while !*on {
                        on = wake.wait(on).expect("ticker lock");
                    }
                    drop(on);
                    std::thread::sleep(TICK);
                    engine.increment_epoch();
                    on = lock.lock().expect("ticker lock");
                }
            })
            .expect("epoch thread");
        Self(shared)
    }

    fn set(&self, on: bool) {
        let (lock, wake) = &*self.0;
        *lock.lock().expect("ticker lock") = on;
        wake.notify_one();
    }
}

pub struct Runtime {
    pub engine: Engine,
    linker: Linker<State>,
    ticker: Ticker,
    pub cache: PathBuf,
    pub data: PathBuf,
}

/// `%LOCALAPPDATA%\mypaper\plugins\cache` and `%APPDATA%\mypaper\plugin-data`.
pub fn default_dirs() -> (PathBuf, PathBuf) {
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    (env("LOCALAPPDATA").join(r"mypaper\plugins\cache"), env("APPDATA").join(r"mypaper\plugin-data"))
}

impl Runtime {
    pub fn new(cache: PathBuf, data: PathBuf) -> Result<Self> {
        let mut pool = PoolingAllocationConfig::default();
        // Sized for ~16 live plugins: on Windows the pool commits its table and instance slots up
        // front, so these counts are the runtime's base memory (docs/plugins.md, "Budgets").
        pool.total_component_instances(16)
            .total_core_instances(48)
            .total_memories(24)
            .total_tables(48)
            .table_elements(8192)
            .max_core_instance_size(128 << 10)
            .max_component_instance_size(128 << 10)
            .max_memory_size(MEMORY_CAP)
            .max_memories_per_module(2)
            .max_tables_per_module(4);
        let mut config = Config::new();
        config
            .wasm_component_model(true)
            .epoch_interruption(true)
            .memory_reservation(MEMORY_CAP as u64)
            .memory_guard_size(64 << 10)
            .memory_reservation_for_growth(0)
            .allocation_strategy(InstanceAllocationStrategy::Pooling(pool));
        let engine = Engine::new(&config)?;
        let mut linker = Linker::new(&engine);
        wasmtime_wasi::p2::add_to_linker_sync(&mut linker)?;
        Plugin::add_to_linker::<State, HasSelf<State>>(&mut linker, |state| state)?;
        let ticker = Ticker::start(engine.clone());
        Ok(Self { engine, linker, ticker, cache, data })
    }

    /// The component, from the AOT cache when it's current (`warm`), else compiled and cached.
    pub fn component(&self, manifest: &Manifest, dir: &Path) -> Result<(Component, bool)> {
        let bytes = std::fs::read(dir.join(&manifest.component))?;
        let mut hasher = DefaultHasher::new();
        bytes.hash(&mut hasher);
        self.engine.precompile_compatibility_hash().hash(&mut hasher);
        let path = self.cache.join(format!("{}-{:016x}.cwasm", manifest.id, hasher.finish()));
        if path.is_file() {
            // SAFETY: the file is ours: written below by `precompile_component` for this engine
            // configuration (the compatibility hash is in its name), in the user's own cache dir.
            match unsafe { Component::deserialize_file(&self.engine, &path) } {
                Ok(component) => return Ok((component, true)),
                Err(e) => log::warn!("plugin {}: stale cache ({e:#}); recompiling", manifest.id),
            }
        }
        let compiled = self.engine.precompile_component(&bytes).map_err(anyhow::Error::from).with_context(|| format!("compiling {}", manifest.component))?;
        std::fs::create_dir_all(&self.cache)?;
        if let Ok(entries) = std::fs::read_dir(&self.cache) {
            let prefix = format!("{}-", manifest.id);
            for entry in entries.flatten().filter(|e| e.file_name().to_string_lossy().starts_with(&prefix)) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
        std::fs::write(&path, &compiled)?;
        // SAFETY: as above; we just wrote it.
        Ok((unsafe { Component::deserialize_file(&self.engine, &path) }?, false))
    }

    pub fn data_dir(&self, id: &str) -> PathBuf {
        self.data.join(id)
    }

    /// Checks imports without executing plugin code, during installation.
    pub fn validate_component(&self, component: &Component) -> Result<()> {
        self.linker.instantiate_pre(component)?;
        Ok(())
    }

    /// Instantiates and initializes a plugin with its saved state.
    pub fn instantiate(&self, manifest: &Manifest, component: &Component, state: Option<Vec<u8>>) -> Result<Instance> {
        self.instantiate_with_settings(manifest, component, state, "{}")
    }

    /// Initializes with the plugin settings reviewed and saved in Studio.
    pub fn instantiate_with_settings(&self, manifest: &Manifest, component: &Component, state: Option<Vec<u8>>, settings: &str) -> Result<Instance> {
        let wasi = WasiCtxBuilder::new().inherit_stderr().build();
        let data = State {
            wasi,
            table: ResourceTable::new(),
            limiter: Limiter { cap: manifest.memory_cap(), used: 0 },
            id: manifest.id.clone(),
            permissions: manifest.permissions.clone(),
            styles: manifest.styles.clone(),
            native: manifest.native.clone(),
            data_dir: self.data_dir(&manifest.id),
            effects: Vec::new(),
            name: if manifest.name.is_empty() { manifest.id.clone() } else { manifest.name.clone() },
            sockets: HashMap::new(),
            pipes: HashMap::new(),
            topics: Vec::new(),
            live: Default::default(),
            images: images::Images::default(),
        };
        let mut store = Store::new(&self.engine, data);
        store.limiter(|state| &mut state.limiter);
        store.set_epoch_deadline(EVENT_TICKS * 4);
        let bindings = Plugin::instantiate(&mut store, component, &self.linker)?;
        let info = InitInfo { plugin: manifest.id.clone(), settings: settings.into(), state };
        self.ticker.set(true);
        let result = bindings.mypaper_plugin_lifecycle().call_init(&mut store, &info);
        self.ticker.set(false);
        match result? {
            Ok(()) => Ok(Instance { store, bindings, ticker: self.ticker.clone(), cpu: Duration::ZERO, events: 0 }),
            Err(why) => bail!("init failed: {why}"),
        }
    }
}

pub struct Instance {
    pub store: Store<State>,
    bindings: Plugin,
    ticker: Ticker,
    pub cpu: Duration,
    pub events: u64,
}

impl Instance {
    /// Delivers one event within the CPU budget. `Err` is a trap (panic, out of memory, or the
    /// budget ran out): the instance must be dropped.
    pub fn event(&mut self, event: &Event) -> Result<Vec<Effect>> {
        self.store.set_epoch_deadline(EVENT_TICKS);
        let started = Instant::now();
        self.ticker.set(true);
        let result = self.bindings.mypaper_plugin_lifecycle().call_on_event(&mut self.store, event);
        self.ticker.set(false);
        self.cpu += started.elapsed();
        self.events += 1;
        let effects = std::mem::take(&mut self.store.data_mut().effects);
        result.map(|()| effects).map_err(Into::into)
    }

    pub fn save(&mut self) -> Result<Vec<u8>> {
        self.store.set_epoch_deadline(EVENT_TICKS * 4);
        self.ticker.set(true);
        let result = self.bindings.mypaper_plugin_lifecycle().call_save(&mut self.store);
        self.ticker.set(false);
        result.map_err(Into::into)
    }

    /// Effects recorded during `init` (its first views and timers).
    pub fn take_effects(&mut self) -> Vec<Effect> {
        std::mem::take(&mut self.store.data_mut().effects)
    }

    pub fn memory(&self) -> usize {
        self.store.data().limiter.used
    }
}

