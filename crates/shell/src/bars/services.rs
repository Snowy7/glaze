//! Bar subscriptions and their deadlines. Providers start only for visible modules.
use crate::integrations::media::BarMedia as Media;
use mypaper_bars::{
    self as services, BarsConfig, ModuleRef,
    actions::*,
    apps::{self, Apps, Item, Pin, Running},
    audio::{Audio, Volume},
    custom::{Custom, Spec},
    desktops::{Desktops, Workspaces},
    flyouts::{
        self, Done, Watched,
        language::{self, LanguageWatch},
        notifications::NotificationsLane,
    },
    hooks::{self, Hooks},
    icons::{Icons, Source},
    media::MediaInfo,
    network::{self, NetworkInfo, NetworkWatch},
    system::{self, Battery, Sampler, Stats},
    tray::{Tray, TrayItem},
    tray_host::TrayHost,
    wake::{Msg, Waker},
};
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    path::PathBuf,
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use windows::Win32::{
    Foundation::{HANDLE, WAIT_OBJECT_0},
    System::{
        SystemInformation::GetLocalTime,
        Threading::{GetCurrentThreadId, WaitForSingleObject},
    },
    UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId},
};

pub struct Model {
    pub foreground: Option<isize>,
    pub app_name: String,
    pub app_icon: Option<String>,
    pub media: Option<MediaInfo>,
    pub volume: Option<Volume>,
    pub network: Option<NetworkInfo>,
    pub battery: Option<Battery>,
    pub stats: Option<Stats>,
    pub workspaces: Workspaces,
    pub language: String,
    pub notifications: Option<usize>,
    pub tray: Vec<TrayItem>,
    pub clock: windows::Win32::Foundation::SYSTEMTIME,
    pub custom: Custom,
    pub trash_full: bool,
}

pub struct Services {
    pub stats_costs: (u64, u64, u64),
    pub model: Model,
    pub apps: Apps,
    pub icons: Icons,
    pub audio: Option<Audio>,
    pub media: Media,
    pub tray: Tray,
    pub tray_host: TrayHost,
    pub waker: Waker,
    pub attention: HashSet<isize>,
    rx: Receiver<Msg>,
    hooks: Option<Hooks>,
    network: Option<NetworkWatch>,
    desktops: Option<Desktops>,
    language: Option<LanguageWatch>,
    sampler: Option<Sampler>,
    topics: BTreeSet<Topic>,
    notification_lane: Option<flyouts::Lane<NotificationsLane>>,
    notification_pending: bool,
    pub clock_due: Option<Instant>,
    stats_due: Option<Instant>,
    notifications_due: Option<Instant>,
    custom_due: Option<Instant>,
    windows_due: Option<Instant>,
    seconds: bool,
}

pub struct Changes {
    pub data: bool,
    pub stats: bool,
    pub config: bool,
    pub theme: bool,
    pub moved: bool,
    pub taskbar: bool,
}

impl Services {
    pub fn new() -> anyhow::Result<Self> {
        services::wake::make_queue();
        let (tx, rx) = mpsc::channel();
        // SAFETY: thread id is used only to post wake messages to this loop.
        let waker = Waker::new(tx, unsafe { GetCurrentThreadId() });
        let dir = services::files::local("icons")?;
        Ok(Self {
            stats_costs: (0, 0, 0),
            model: Model {
                foreground: None,
                app_name: "Desktop".into(),
                app_icon: None,
                media: None,
                volume: None,
                network: None,
                battery: None,
                stats: None,
                workspaces: Workspaces {
                    count: 1,
                    current: 0,
                },
                language: String::new(),
                notifications: None,
                tray: vec![],
                // SAFETY: read-only clock query.
                clock: unsafe { GetLocalTime() },
                custom: Custom::new(waker.clone()),
                trash_full: false,
            },
            apps: Apps::default(),
            icons: Icons::new(waker.clone(), dir.clone()),
            audio: None,
            media: Media::new(waker.clone()),
            tray: Tray::new(waker.clone(), dir),
            tray_host: TrayHost::new(waker.clone()),
            waker,
            rx,
            hooks: None,
            network: None,
            desktops: None,
            language: None,
            sampler: None,
            topics: BTreeSet::new(),
            notification_lane: None,
            notification_pending: false,
            attention: HashSet::new(),
            clock_due: None,
            stats_due: None,
            notifications_due: None,
            custom_due: None,
            windows_due: None,
            seconds: false,
        })
    }

    /// WinEvent delivery and provider workers are unnecessary for the first software frame.
    pub fn start(&mut self) {
        self.hooks = Some(Hooks::install());
    }

    /// Reconciles services with the union of the visible bars' topics.
    pub fn subscribe(
        &mut self,
        topics: BTreeSet<Topic>,
        configs: &[&BarsConfig],
        seconds: bool,
        host: bool,
    ) {
        let now = Instant::now();
        let on = |t| topics.contains(&t);
        if topics != self.topics {
            self.model.battery = on(Topic::Battery).then(system::battery).flatten();
            self.media.set_active(on(Topic::Media));
            if on(Topic::Volume) != self.audio.is_some() {
                self.audio = on(Topic::Volume)
                    .then(|| Audio::new(self.waker.clone()))
                    .flatten();
            }
            if on(Topic::Network) != self.network.is_some() {
                self.network = on(Topic::Network).then(|| NetworkWatch::start(self.waker.clone()));
                self.model.network = self.network.as_ref().map(|n| network::read(n.wlan()));
            }
            if on(Topic::Workspaces) != self.desktops.is_some() {
                self.desktops = on(Topic::Workspaces).then(Desktops::watch).flatten();
                if let Some(d) = &self.desktops {
                    self.model.workspaces = d.read();
                }
            }
            if on(Topic::Language) != self.language.is_some() {
                self.language = on(Topic::Language)
                    .then(|| LanguageWatch::start(self.waker.clone()))
                    .flatten();
            }
            if on(Topic::Stats) != self.sampler.is_some() {
                self.sampler = on(Topic::Stats).then(Sampler::new);
                self.stats_due = on(Topic::Stats).then_some(now + Duration::from_secs(2));
            }
            self.notifications_due = None;
            if !on(Topic::Notifications) {
                self.notification_lane = None;
            }
            self.clock_due = on(Topic::Clock).then_some(now);
            self.topics = topics;
            self.refresh_apps();
        }
        if self.seconds != seconds {
            self.seconds = seconds;
            self.clock_due = self.topics.contains(&Topic::Clock).then_some(now);
        }
        let tray = self.topics.contains(&Topic::Tray);
        self.tray_host.set_active(host);
        self.tray.set_active(tray && !host);
        let mut custom = BTreeMap::new();
        if self.topics.contains(&Topic::Custom) {
            for config in configs {
                for (id, options) in config.custom_modules() {
                    let shown = config
                        .top
                        .modules()
                        .chain(config.dock.modules())
                        .any(|m| matches!(m, ModuleRef::Custom(key) if key == id));
                    if shown && let Some(command) = &options.command {
                        custom.insert(
                            id.to_string(),
                            Spec::new(command, &options.args, options.interval),
                        );
                    }
                }
            }
        }
        self.model.custom.sync(custom);
        self.custom_due = self.model.custom.tick(now);
        self.model.volume = self.audio.as_ref().and_then(Audio::read);
    }

    pub fn handles(&self) -> impl Iterator<Item = HANDLE> + '_ {
        self.desktops.iter().map(Desktops::event)
    }
    pub fn deadline(&self) -> Option<Instant> {
        [
            self.clock_due,
            self.stats_due,
            self.notifications_due,
            self.custom_due,
            self.windows_due,
        ]
        .into_iter()
        .flatten()
        .min()
    }

    /// Drains typed worker messages; returns config/appearance changes separately.
    pub fn tick(&mut self, now: Instant) -> Changes {
        let (mut changed, mut config, mut theme) = (false, false, false);
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Config => config = true,
                Msg::Theme => theme = true,
                Msg::Described(source, value) => {
                    self.icons.on_described(source, value);
                    self.refresh_apps();
                    changed = true;
                }
                Msg::Media(info) => {
                    self.model.media = info;
                    changed = true;
                }
                Msg::Volume => {
                    if let Some(a) = &mut self.audio {
                        a.follow_default();
                        self.model.volume = a.read();
                    }
                    changed = true;
                }
                Msg::Network => {
                    self.model.network = self.network.as_ref().map(|n| network::read(n.wlan()));
                    changed = true;
                }
                Msg::Tray(items) => {
                    self.model.tray = items;
                    changed = true;
                }
                Msg::Custom(id, output) => {
                    changed |= self.model.custom.finished(id, output);
                    self.custom_due = self.model.custom.tick(now);
                }
                Msg::Flyout(Done::Changed(Watched::Core(Topic::Language))) => {
                    self.read_language();
                    changed = true;
                }
                Msg::Flyout(Done::NotificationCount(count)) => {
                    changed |= self.model.notifications != count;
                    self.model.notifications = count;
                    self.notification_pending = false;
                }
                Msg::WatchError(path, error) => log::warn!("{}: {error}", path.display()),
                _ => {}
            }
        }
        let bits = self.hooks.as_mut().map_or(0, Hooks::take);
        if bits & (hooks::WINDOWS | hooks::FOREGROUND) != 0 {
            self.windows_due
                .get_or_insert(now + Duration::from_millis(50));
        }
        if take_due(&mut self.windows_due, now) {
            self.refresh_apps();
            changed = true;
        }
        if let Some(d) = &self.desktops {
            // SAFETY: owned event handle, non-blocking read.
            if unsafe { WaitForSingleObject(d.event(), 0) } == WAIT_OBJECT_0 {
                self.model.workspaces = d.changed();
                changed = true;
            }
        }
        if take_due(&mut self.clock_due, now) {
            // SAFETY: read-only system time query.
            self.model.clock = unsafe { GetLocalTime() };
            self.clock_due = Some(now + next_tick(self.seconds));
            changed = true;
        }
        let mut stats_changed = false;
        if take_due(&mut self.stats_due, now)
            && let Some(s) = &mut self.sampler
        {
            let started = Instant::now();
            let stats = s.sample();
            self.stats_costs.0 += 1;
            self.stats_costs.2 += started.elapsed().as_micros() as u64;
            stats_changed = self
                .model
                .stats
                .as_ref()
                .is_none_or(|old| displayed_stats(old) != displayed_stats(&stats));
            self.model.stats = Some(stats);
            self.stats_costs.1 += u64::from(stats_changed);
            self.stats_due = Some(now + Duration::from_secs(2));
        }
        if take_due(&mut self.custom_due, now) {
            self.custom_due = self.model.custom.tick(now);
        }
        Changes {
            data: changed,
            stats: stats_changed,
            config,
            theme,
            moved: bits & hooks::MOVED != 0,
            taskbar: bits & hooks::TASKBAR_SHOWN != 0,
        }
    }

    pub fn refresh_apps(&mut self) {
        self.apps.refresh();
        if let Some(hooks) = &mut self.hooks {
            hooks.track(
                self.apps
                    .groups
                    .iter()
                    .flat_map(|g| g.windows.iter().map(|w| w.hwnd)),
            );
        }
        // SAFETY: read-only window/process queries; ignore shell-owned windows.
        unsafe {
            let front = GetForegroundWindow();
            let mut pid = 0;
            GetWindowThreadProcessId(front, Some(&mut pid));
            if pid != std::process::id() && !front.is_invalid() {
                self.model.foreground = Some(apps::root_owner(front.0 as isize));
            }
        }
        self.attention.retain(|w| {
            self.apps
                .groups
                .iter()
                .any(|g| g.windows.iter().any(|v| v.hwnd == *w))
        });
        if let Some(front) = self.model.foreground {
            self.attention.remove(&front);
        }
        let group = self
            .model
            .foreground
            .and_then(|w| self.apps.group_of(w))
            .cloned();
        crate::integrations::foreground(group.as_ref());
        if let Some(g) = group {
            let d = self
                .icons
                .get(&Source::App {
                    aumid: g.aumid,
                    exe: g.exe.clone(),
                })
                .cloned()
                .unwrap_or_default();
            self.model.app_name = d
                .name
                .or_else(|| g.exe.as_deref().map(apps::file_stem))
                .unwrap_or_else(|| "Desktop".into());
            self.model.app_icon = d.icon;
        } else {
            self.model.app_name = "Desktop".into();
            self.model.app_icon = None;
        }
        if self.topics.contains(&Topic::Language) {
            self.read_language();
        }
    }

    fn read_language(&mut self) {
        let id = language::current(self.model.foreground);
        self.model.language = language::layouts()
            .into_iter()
            .find(|l| Some(&l.id) == id.as_ref())
            .map_or_else(String::new, |l| format!("{}\n{}", l.short, l.region));
    }

    pub fn dock_items(&mut self, config: &BarsConfig, monitor: isize) -> Vec<Item> {
        let pins: Vec<_> = config
            .dock
            .pinned
            .iter()
            .flatten()
            .map(|p| {
                let d = self
                    .hooks
                    .as_ref()
                    .and_then(|_| self.icons.get(&Source::Path(p.path.clone())))
                    .cloned()
                    .unwrap_or_default();
                let exe = p
                    .path
                    .to_lowercase()
                    .ends_with(".exe")
                    .then(|| p.path.to_lowercase());
                let aumid = p.path.strip_prefix("shell:AppsFolder\\").map(str::to_owned);
                Pin {
                    name: p
                        .name
                        .clone()
                        .or(d.name)
                        .unwrap_or_else(|| apps::file_stem(&p.path)),
                    path: p.path.clone(),
                    icon: d.icon,
                    aumid: d.aumid.or(aumid),
                    exe: d.exe.or(exe),
                }
            })
            .collect();
        let running: Vec<_> = self
            .apps
            .groups
            .clone()
            .into_iter()
            .map(|g| {
                let d = self
                    .icons
                    .get(&Source::App {
                        aumid: g.aumid.clone(),
                        exe: g.exe.clone(),
                    })
                    .cloned()
                    .unwrap_or_default();
                Running {
                    name: d
                        .name
                        .or_else(|| g.exe.as_deref().map(apps::file_stem))
                        .unwrap_or_default(),
                    path: d.path.or(g.exe.clone()).unwrap_or_default(),
                    icon: d.icon,
                    group: g,
                }
            })
            .collect();
        let items = apps::dock_items(
            &pins,
            &running,
            &self.apps.order,
            apps::View {
                running: config.dock.running,
                monitor,
                active: self.model.foreground,
                attention: Some(&self.attention),
            },
        );
        if !config.dock.split {
            return items;
        }
        items
            .into_iter()
            .flat_map(|item| {
                if item.windows.len() <= 1 {
                    return vec![item];
                }
                item.windows
                    .iter()
                    .enumerate()
                    .map(|(i, w)| Item {
                        key: if i == 0 {
                            item.key.clone()
                        } else {
                            format!("{}#{}", item.key, w.hwnd)
                        },
                        name: if i == 0 {
                            item.name.clone()
                        } else {
                            w.title.clone()
                        },
                        windows: vec![w.clone()],
                        ..item.clone()
                    })
                    .collect()
            })
            .collect()
    }
}

/// Match the integer percentages in the stats item, including formatting's rounding.
fn displayed_stats(stats: &Stats) -> String {
    let memory = 100.0 * stats.memory.used as f64 / stats.memory.total.max(1) as f64;
    format!("CPU {:.0}%  RAM {memory:.0}%", stats.cpu)
}

#[cfg(test)]
mod stats_tests {
    use super::*;
    use mypaper_bars::system::Memory;

    #[test]
    fn stats_invalidation_follows_the_rendered_integer_percentages() {
        let mut stats = Stats {
            cpu: 3.1,
            memory: Memory {
                used: 701,
                total: 1000,
            },
        };
        let before = displayed_stats(&stats);
        stats.cpu = 3.4;
        stats.memory.used = 704;
        assert_eq!(before, displayed_stats(&stats));
        stats.cpu = 3.6;
        assert_ne!(before, displayed_stats(&stats));
        stats.cpu = 3.1;
        stats.memory.used = 706;
        assert_ne!(before, displayed_stats(&stats));
    }
}

pub fn take_due(timer: &mut Option<Instant>, now: Instant) -> bool {
    timer.take_if(|at| *at <= now).is_some()
}

/// Millisecond wait for a deadline; a future deadline must never become a nonblocking poll.
pub fn wait_ms(duration: Duration) -> u32 {
    duration
        .as_nanos()
        .div_ceil(1_000_000)
        .min(u128::from(u32::MAX - 1)) as u32
}

pub fn next_tick(seconds: bool) -> Duration {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let unit = if seconds { 1000 } else { 60000 };
    Duration::from_millis(unit - ms % unit + 5)
}

pub fn default_pins() -> Vec<services::config::Pinned> {
    let folder = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .map(|p| p.join(r"Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar"));
    let mut paths: Vec<_> = folder
        .and_then(|p| std::fs::read_dir(p).ok())
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("lnk")))
        .collect();
    paths.sort();
    if paths.is_empty() {
        paths.push(PathBuf::from(r"C:\Windows\explorer.exe"));
    }
    paths
        .into_iter()
        .map(|p| services::config::Pinned {
            path: p.display().to_string(),
            name: None,
        })
        .collect()
}
