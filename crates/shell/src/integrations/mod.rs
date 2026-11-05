//! Integration UI/registration. Shared providers live in `mypaper-integrations`; all drawing stays here.
pub mod media;
pub mod widgets;
mod panels;
#[cfg(test)]
mod tests;

use mypaper_bars::{Module, ModuleRef};
use mypaper_integrations::{Availability, Data, Hub, Lease, Provider, Snapshot, config::Config};
use mypaper_ui::FileWatch;
use mypaper_ui::{DisplayList, Op, Rect, Rgba, TextStyle, Ui};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use windows::Win32::{
    Foundation::{LPARAM, WPARAM},
    System::Threading::GetCurrentThreadId,
    UI::WindowsAndMessaging::{PostThreadMessageW, WM_APP},
};

const WAKE: u32 = WM_APP + 146;
thread_local! { static STATE: RefCell<State> = RefCell::new(State::new()); }
struct State {
    config: Config,
    hub: Hub,
    bars: BTreeMap<Provider, Lease>,
    dirty: Arc<AtomicBool>,
    reload: Arc<AtomicBool>,
    consent_complete: Arc<AtomicBool>,
    _watcher: Option<FileWatch>,
    app: Option<(String, String)>,
    generation: u64,
    locked: bool,
    display_off: bool,
    foreground_id: Option<String>,
    #[cfg(test)]
    fixtures: BTreeMap<Provider, Snapshot>,
}
pub fn config_path() -> PathBuf {
    PathBuf::from(std::env::var_os("APPDATA").unwrap_or_default()).join("mypaper/integrations.json")
}
fn load() -> anyhow::Result<Config> {
    match std::fs::read_to_string(config_path()) {
        Ok(text) => Config::parse(&text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
        Err(e) => Err(e.into()),
    }
}
fn wake(thread: u32) {
    // COM can consume thread messages. The inbox retains the wake; the thread wake covers startup.
    crate::inbox::post(WAKE, 0);
    // SAFETY: posting to an id never dereferences foreign memory.
    let _ = unsafe { PostThreadMessageW(thread, WAKE, WPARAM(0), LPARAM(0)) };
}
impl State {
    fn new() -> Self {
        // SAFETY: each State belongs to the calling UI thread.
        let thread = unsafe { GetCurrentThreadId() };
        let dirty = Arc::new(AtomicBool::new(false));
        let changed = dirty.clone();
        let hub = Hub::new(move |_| {
            if !changed.swap(true, Ordering::AcqRel) {
                wake(thread);
            }
        });
        let reload = Arc::new(AtomicBool::new(false));
        let reload_flag = reload.clone();
        let watcher = FileWatch::new(&config_path(), move || {
            reload_flag.store(true, Ordering::Release);
            wake(thread);
        })
        .inspect_err(|e| log::warn!("integration config watcher: {e:#}"))
        .ok();
        let config = load()
            .inspect_err(|e| log::warn!("integrations.json: {e:#}"))
            .unwrap_or_default();
        let consent_complete = Arc::new(AtomicBool::new(false));
        if config.notifications.enabled {
            request_notification_consent(thread, &consent_complete);
        }
        Self {
            config,
            hub,
            bars: BTreeMap::new(),
            dirty,
            reload,
            consent_complete,
            _watcher: watcher,
            app: None,
            generation: 0,
            locked: false,
            display_off: false,
            foreground_id: None,
            #[cfg(test)]
            fixtures: BTreeMap::new(),
        }
    }
}

fn request_notification_consent(thread: u32, complete: &Arc<AtomicBool>) {
    if cfg!(test) {
        return;
    }
    let complete = complete.clone();
    if let Err(e) = mypaper_integrations::notifications::request_access_once(move || {
        complete.store(true, Ordering::Release);
        wake(thread);
    }) {
        log::warn!("notification consent: {e:#}");
    }
}
pub fn config() -> Config {
    STATE.with_borrow(|s| s.config.clone())
}
pub fn acquire(provider: Provider) -> anyhow::Result<Lease> {
    STATE.with_borrow(|s| {
        anyhow::ensure!(!s.locked && !s.display_off, "integrations suspended");
        s.hub.acquire(provider, &s.config)
    })
}
pub fn generation() -> u64 {
    STATE.with_borrow(|s| s.generation)
}
pub fn fact(wparam: usize, lparam: isize) {
    use mypaper_desktop::hold::Fact;
    let changed = STATE.with_borrow_mut(|s| {
        let before = s.locked || s.display_off;
        match Fact::from_wparam(wparam) {
            Some(Fact::SessionLocked) => s.locked = lparam != 0,
            Some(Fact::DisplayOff) => s.display_off = lparam != 0,
            _ => {}
        }
        if before == (s.locked || s.display_off) {
            return false;
        }
        s.hub.stop_all();
        s.bars.clear();
        s.generation += 1;
        true
    });
    if changed {
        media::changed(true);
    }
}
pub fn latest(provider: Provider) -> Option<Snapshot> {
    STATE.with_borrow(|s| {
        #[cfg(test)]
        if let Some(state) = s.fixtures.get(&provider) {
            return Some(state.clone());
        }
        s.bars.get(&provider).map(Lease::snapshot)
    })
}

/// Called by the shell after messages, before presenting. Reading unchanged cached state does no IO.
pub fn process() -> bool {
    let (changed, reload) = STATE.with_borrow_mut(|state| {
        let reload_requested = state.reload.swap(false, Ordering::AcqRel);
        let mut reload = false;
        if reload_requested {
            match load() {
                Ok(config) if state.config != config => {
                    if config.notifications.enabled && !state.config.notifications.enabled {
                        // Studio's Enable writes this file; consent must originate on this UI thread.
                        // SAFETY: obtains only the calling thread's numeric id.
                        request_notification_consent(
                            unsafe { GetCurrentThreadId() },
                            &state.consent_complete,
                        );
                    }
                    state.hub.stop_all();
                    state.bars.clear();
                    state.config = config;
                    state.generation += 1;
                    reload = true;
                }
                Ok(_) => {}
                Err(e) => log::warn!("integrations.json: {e:#}; keeping valid config"),
            }
        }
        if state.consent_complete.swap(false, Ordering::AcqRel) {
            state.hub.stop_all();
            state.bars.clear();
            state.generation += 1;
            reload = true;
        }
        (state.dirty.swap(false, Ordering::AcqRel), reload)
    });
    if changed || reload {
        media::changed(reload);
    }
    changed || reload
}

fn provider(module: &ModuleRef) -> Option<Provider> {
    match module {
        ModuleRef::Builtin(Module::Notifications) => Some(Provider::Notifications),
        ModuleRef::Builtin(Module::T3) => Some(Provider::T3),
        ModuleRef::Builtin(Module::Obs) => Some(Provider::Obs),
        ModuleRef::Builtin(Module::Docker) => Some(Provider::Docker),
        ModuleRef::Builtin(Module::Tailscale) => Some(Provider::Tailscale),
        _ => None,
    }
}
pub fn bar_subscriptions<'a>(modules: impl Iterator<Item = &'a ModuleRef>, dock: bool) {
    STATE.with_borrow_mut(|state| {
        let mut wanted: BTreeSet<_> = modules
            .filter_map(|module| {
                provider(module).or_else(|| {
                    matches!(module, ModuleRef::Builtin(Module::App))
                        .then_some(Provider::Notifications)
                })
            })
            .filter(|p| p.enabled(&state.config))
            .collect();
        if dock && state.config.notifications.enabled {
            wanted.insert(Provider::Notifications);
        }
        wanted.retain(|p| match p {
            Provider::Notifications => state.config.notifications.live_verified,
            Provider::T3 => state.config.t3.live_verified,
            Provider::Tailscale => state.config.tailscale.live_verified,
            _ => true,
        });
        if state.locked || state.display_off {
            wanted.clear();
        }
        state.bars.retain(|p, _| wanted.contains(p));
        for p in wanted {
            if !state.bars.contains_key(&p)
                && let Ok(lease) = state.hub.acquire(p, &state.config)
            {
                state.bars.insert(p, lease);
            }
        }
    });
}

pub fn foreground(group: Option<&mypaper_bars::apps::Group>) {
    STATE.with_borrow_mut(|s| {
        s.foreground_id = group.and_then(|g| {
            g.aumid.clone().or_else(|| {
                g.exe
                    .as_ref()
                    .and_then(|path| s.config.notifications.app_ids.get(path).cloned())
            })
        })
    });
}
pub fn app_title(name: &str) -> String {
    let count = STATE.with_borrow(|s| {
        s.foreground_id.as_ref().and_then(|id| {
            s.bars
                .get(&Provider::Notifications)
                .map(Lease::snapshot)
                .and_then(|snapshot| match snapshot {
                    Snapshot {
                        availability: Availability::Live,
                        data: Some(Data::Notifications(n)),
                        ..
                    } => n.counts.get(id).copied(),
                    _ => None,
                })
        })
    });
    match count {
        Some(count) if count > 0 => format!("{name} · {count}"),
        _ => name.into(),
    }
}
pub fn bar_content(module: &ModuleRef) -> Option<(String, &'static str)> {
    let provider = provider(module)?;
    if !provider.enabled(&config()) {
        return if provider == Provider::Notifications {
            None
        } else {
            Some((String::new(), ""))
        };
    }
    let snapshot = latest(provider);
    let live = snapshot
        .as_ref()
        .filter(|s| s.availability == Availability::Live);
    let text = match live.and_then(|s| s.data.as_ref()) {
        Some(Data::Notifications(s)) => format!("{}", s.cards.len()),
        Some(Data::T3(s)) => format!(
            "{} running · {} need input",
            s.threads.iter().filter(|t| t.running()).count(),
            s.threads.iter().filter(|t| t.attention()).count()
        ),
        Some(Data::Obs(s)) => format!(
            "{}{}{}",
            if s.recording { "REC " } else { "" },
            if s.streaming { "LIVE " } else { "" },
            s.scene
        ),
        Some(Data::Docker(s)) => format!(
            "{} containers{}",
            s.containers.iter().filter(|c| c.state == "running").count(),
            if s.containers.iter().any(|c| c.failed) {
                " · !"
            } else {
                ""
            }
        ),
        Some(Data::Tailscale(s)) => s.state.clone(),
        _ => match provider {
            Provider::Notifications => "Notifications",
            Provider::T3 => "T3",
            Provider::Obs => "OBS",
            Provider::Docker => "Docker",
            Provider::Tailscale => "Tailscale",
            Provider::Media => "Media",
        }
        .into(),
    };
    Some((
        text,
        match provider {
            Provider::Notifications => "\u{ea8f}",
            Provider::T3 => "\u{e8f2}",
            Provider::Obs => "\u{e714}",
            Provider::Docker => "\u{e7b8}",
            Provider::Tailscale => "\u{e72e}",
            Provider::Media => "\u{e768}",
        },
    ))
}

pub fn module_click(module: &ModuleRef, anchor: Rect, monitor: &mypaper_desktop::Monitor) -> bool {
    if let Some(provider) = provider(module) {
        crate::flyouts::open(
            if provider == Provider::Notifications {
                crate::flyouts::Kind::Notifications
            } else {
                crate::flyouts::Kind::Integration(provider)
            },
            anchor,
            monitor,
        );
        true
    } else {
        false
    }
}
pub fn panel(provider: Provider) -> Box<dyn crate::flyouts::Panel> {
    Box::new(panels::Integration::new(provider))
}
pub fn routine_panel() -> Box<dyn crate::flyouts::Panel> {
    panels::routine_panel()
}
pub fn notification_center(cx: &mut crate::flyouts::Cx) -> Box<dyn crate::flyouts::Panel> {
    Box::new(panels::NotificationCenter::new(cx))
}

pub fn dock_badge(
    list: &mut DisplayList,
    ui: &mut Ui,
    app: &mypaper_bars::apps::Item,
    icon: Rect,
    scale: f32,
) {
    let Some(state) =
        latest(Provider::Notifications).filter(|s| s.availability == Availability::Live)
    else {
        return;
    };
    let Some(Data::Notifications(state)) = state.data else {
        return;
    };
    let id = app_id(app);
    let count = id
        .as_ref()
        .and_then(|id| state.counts.get(id))
        .copied()
        .unwrap_or(0);
    if count == 0 {
        return;
    }
    if let Some(hit) = list.hits.last_mut() {
        hit.label = format!(
            "{} · {count} retained Windows notification{}",
            hit.label,
            if count == 1 { "" } else { "s" }
        )
        .into();
    }
    let rect = Rect::new(
        icon.right - 14.0 * scale,
        icon.bottom - 16.0 * scale,
        16.0 * scale,
        16.0 * scale,
    );
    list.ops.push(Op::Fill {
        rect,
        radius: 8.0 * scale,
        color: Rgba::rgb8([207, 59, 68], 1.0),
    });
    let text = count.min(99).to_string();
    let style = TextStyle::new("Segoe UI", 10.0 * scale);
    let (w, h) = ui.text.measure(&text, &style);
    list.ops.push(Op::Text {
        rect: Rect::new(
            rect.left + (rect.width() - w) / 2.0,
            rect.top + (rect.height() - h) / 2.0,
            w,
            h,
        ),
        text: text.into(),
        style,
        color: Rgba::rgb8([255, 255, 255], 1.0),
    });
}
fn app_id(app: &mypaper_bars::apps::Item) -> Option<String> {
    app.aumid.clone().or_else(|| {
        STATE.with_borrow(|s| {
            s.config
                .notifications
                .app_ids
                .get(&app.key)
                .or_else(|| s.config.notifications.app_ids.get(&app.path.to_lowercase()))
                .cloned()
        })
    })
}
pub fn open_app(app: &mypaper_bars::apps::Item, anchor: Rect, monitor: &mypaper_desktop::Monitor) {
    let id = app_id(app).unwrap_or_default();
    STATE.with_borrow_mut(|s| s.app = Some((id, app.name.clone())));
    crate::flyouts::open(crate::flyouts::Kind::AppActions, anchor, monitor);
}
pub fn app_panel() -> Box<dyn crate::flyouts::Panel> {
    let app = STATE.with_borrow_mut(|s| s.app.take()).unwrap_or_default();
    Box::new(panels::AppActions::new(app.0, app.1))
}

pub fn launcher_entries(apps: &mut Vec<mypaper_bars::flyouts::index::App>) {
    let config = config();
    if !config.launcher.enabled {
        return;
    }
    let mut add = |id: String, name: String, path: String| {
        apps.push(mypaper_bars::flyouts::index::App {
            id,
            name,
            path,
            icon: None,
            keywords: vec!["integration".into()],
            packaged: false,
        });
    };
    add(
        "integration:discord".into(),
        "Discord: open direct messages".into(),
        "https://discord.com/channels/@me".into(),
    );
    if config.grok.enabled {
        for routine in config.grok.routines {
            add(
                format!("grok:{}", routine.id),
                format!("Grok Bot: {}", routine.title),
                format!("mypaper-integration:grok/{}", routine.id),
            );
        }
    }
}
pub fn launcher_action(path: &str) -> bool {
    let Some(id) = path.strip_prefix("mypaper-integration:grok/") else {
        return false;
    };
    let config = config();
    if config.launcher.enabled
        && config.grok.enabled
        && let Some(routine) = config.grok.routines.into_iter().find(|r| r.id == id)
    {
        panels::run_routine(routine);
    }
    true
}

/// One bounded worker result; the panel holds it, so late replies cannot affect a replacement panel.
struct Task<T> {
    result: std::sync::mpsc::Receiver<Result<T, String>>,
}
impl<T: Send + 'static> Task<T> {
    fn spawn(job: impl FnOnce() -> anyhow::Result<T> + Send + 'static) -> Self {
        // SAFETY: only used as a message destination.
        let thread = unsafe { GetCurrentThreadId() };
        let (tx, result) = std::sync::mpsc::channel();
        let _ = std::thread::Builder::new()
            .name("integration-action".into())
            .stack_size(512 * 1024)
            .spawn(move || {
                let result = job().map_err(|e| e.to_string());
                if tx.send(result).is_ok() {
                    wake(thread);
                }
            });
        Self { result }
    }
    fn take(&self) -> Option<Result<T, String>> {
        match self.result.try_recv() {
            Ok(result) => Some(result),
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                Some(Err("Integration worker could not finish".into()))
            }
        }
    }
}
