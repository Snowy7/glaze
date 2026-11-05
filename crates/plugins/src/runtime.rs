//! One Wasmtime engine for every plugin (docs/plugins.md, "Budgets"): pooling allocator, a
//! per-plugin `ResourceLimiter`, epoch interruption for the per-event CPU budget, and the AOT
//! cache. Host imports only record [`Effect`]s; the loop in `host.rs` applies them after the
//! call returns, so a plugin call never blocks on I/O.
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};
use mypaper_plugin_api::wire::{StylePatch, TokenValue};
use mypaper_plugin_sdk::view as sdk;
use serde::Deserialize;
use wasmtime::component::{Component, HasSelf, Linker, ResourceTable};
use wasmtime::{Config, Engine, InstanceAllocationStrategy, PoolingAllocationConfig, ResourceLimiter, Store};
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

use crate::caps;

wasmtime::component::bindgen!({ path: "../plugin-api/wit", world: "plugin" });

use exports::mypaper::plugin::lifecycle::{Event, InitInfo};
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
}

const WORDS: [&str; 6] = ["launch", "notifications", "secrets", "storage", "windows", "native-code"];
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
                || permission.strip_prefix("pipe:").is_some_and(|name| !name.is_empty() && !name.contains(['\\', '/']));
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
    data_dir: PathBuf,
    pub effects: Vec<Effect>,
    next_request: u64,
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
    fn request_id(&mut self) -> u64 {
        self.next_request += 1;
        self.next_request
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
    fn put_image(&mut self, _width: u32, _height: u32, _pixels: Vec<u8>) -> Result<u32, WitError> {
        Err(WitError::Unsupported("images arrive with the canvas renderer".into()))
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
        if topics.contains(&mypaper::plugin::events::Topic::Windows) {
            self.allowed("windows")?;
        }
        // System topics are accepted and recorded; their sources land with the shell event bridge.
        Ok(())
    }
    fn unsubscribe(&mut self, _topics: Vec<mypaper::plugin::events::Topic>) {}
}

impl mypaper::plugin::http::Host for State {
    fn fetch(&mut self, request: mypaper::plugin::http::Request) -> Result<u64, WitError> {
        use mypaper::plugin::http::Method;
        if !caps::origin_allowed(&self.permissions, &request.url) {
            log::warn!("plugin {}: refused fetch of an undeclared origin", self.id);
            return Err(WitError::Denied(format!("no network permission for {}", request.url.split('?').next().unwrap_or(""))));
        }
        let method = match request.method {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Patch => "PATCH",
            Method::Delete => "DELETE",
        };
        let id = self.request_id();
        let headers = request.headers.into_iter().map(|h| (h.name, h.value)).collect();
        let timeout_ms = request.timeout_ms.unwrap_or(15_000);
        self.effects.push(Effect::Fetch { id, method, url: request.url, headers, body: request.body, timeout_ms });
        Ok(id)
    }
}

impl mypaper::plugin::websocket::Host for State {
    fn connect(&mut self, url: String) -> Result<u64, WitError> {
        if !caps::origin_allowed(&self.permissions, &url) {
            return Err(WitError::Denied(format!("no network permission for {url}")));
        }
        Err(WitError::Unsupported("websockets aren't wired in this host yet".into()))
    }
    fn send(&mut self, _socket: u64, _message: mypaper::plugin::websocket::Message) -> Result<(), WitError> {
        Err(WitError::Unsupported("websockets aren't wired in this host yet".into()))
    }
    fn close(&mut self, _socket: u64) {}
}

impl mypaper::plugin::pipes::Host for State {
    fn open(&mut self, name: String) -> Result<u64, WitError> {
        self.allowed(&format!("pipe:{name}"))?;
        Err(WitError::Unsupported("pipes aren't wired in this host yet".into()))
    }
    fn write(&mut self, _pipe: u64, _bytes: Vec<u8>) -> Result<(), WitError> {
        Err(WitError::Unsupported("pipes aren't wired in this host yet".into()))
    }
    fn close(&mut self, _pipe: u64) {}
}

impl mypaper::plugin::launch::Host for State {
    fn open(&mut self, target: String) -> Result<(), WitError> {
        self.allowed("launch")?;
        caps::launch(&target).map_err(failed)
    }
}

impl mypaper::plugin::notify::Host for State {
    fn show(&mut self, _notification: mypaper::plugin::notify::Notification) -> Result<(), WitError> {
        self.allowed("notifications")?;
        Err(WitError::Unsupported("notifications aren't wired in this host yet".into()))
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
        Err(WitError::Unsupported("foreground window info lands with the window events".into()))
    }
}

impl mypaper::plugin::native::Host for State {
    fn call(&mut self, module: String, method: String, bytes: Vec<u8>) -> Result<u64, WitError> {
        self.allowed("native-code")?;
        crate::native::call(&self.id, &module, &method, &bytes).map_err(WitError::Unsupported)
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
        // The kit's springs animate the child's changes; drawn as the child until the renderer does.
        NodeKind::Animated(_) => only(children)?,
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
        // The shell's canvas renderer is the next step; until then a canvas draws as nothing.
        NodeKind::Canvas(_) => V::Unknown,
    })
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
        pool.total_component_instances(32)
            .total_core_instances(128)
            .total_memories(64)
            .total_tables(128)
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

    /// Instantiates and initializes a plugin with its saved state.
    pub fn instantiate(&self, manifest: &Manifest, component: &Component, state: Option<Vec<u8>>) -> Result<Instance> {
        let wasi = WasiCtxBuilder::new().inherit_stderr().build();
        let data = State {
            wasi,
            table: ResourceTable::new(),
            limiter: Limiter { cap: manifest.memory_cap(), used: 0 },
            id: manifest.id.clone(),
            permissions: manifest.permissions.clone(),
            styles: manifest.styles.clone(),
            data_dir: self.data_dir(&manifest.id),
            effects: Vec::new(),
            next_request: 0,
        };
        let mut store = Store::new(&self.engine, data);
        store.limiter(|state| &mut state.limiter);
        store.set_epoch_deadline(EVENT_TICKS * 4);
        let bindings = Plugin::instantiate(&mut store, component, &self.linker)?;
        let info = InitInfo { plugin: manifest.id.clone(), settings: "{}".into(), state };
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

