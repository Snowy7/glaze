//! Native panels: cached lists, explicit controls, and no tick/deadline override.
use crate::flyouts::{
    Cx, Effect, Panel,
    kit::{Icon, Kit, Row, glyph},
};
use mypaper_integrations::{
    Action, Availability, Data, Lease, Provider, Snapshot, launcher::Recent,
};
use mypaper_ui::HitId;
use std::{cell::RefCell, path::PathBuf};

const RETRY: HitId = 10;
const PRIMARY: HitId = 20;
const SECONDARY: HitId = 21;
const LIST: HitId = 1000;
pub struct Integration {
    provider: Provider,
    lease: Option<Lease>,
    state: Snapshot,
    error: Option<String>,
    top: usize,
    generation: u64,
}
impl Integration {
    pub fn new(provider: Provider) -> Self {
        let lease = super::acquire(provider);
        let error = lease.as_ref().err().map(ToString::to_string);
        let lease = lease.ok();
        let state = lease.as_ref().map(Lease::snapshot).unwrap_or_default();
        Self {
            provider,
            lease,
            state,
            error,
            top: 0,
            generation: super::generation(),
        }
    }
    #[cfg(test)]
    pub fn fixture(provider: Provider, data: Data) -> Self {
        Self {
            provider,
            lease: None,
            state: Snapshot {
                availability: Availability::Live,
                data: Some(data),
                ..Default::default()
            },
            error: None,
            top: 0,
            generation: super::generation(),
        }
    }
    fn title(&self) -> &'static str {
        match self.provider {
            Provider::Notifications => "Notifications",
            Provider::Media => "Now playing",
            Provider::T3 => "T3 Code",
            Provider::Obs => "OBS Studio",
            Provider::Docker => "Docker Desktop",
            Provider::Tailscale => "Tailscale",
        }
    }
    fn action(&mut self, action: Action) -> Effect {
        self.error = self
            .lease
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("integration disabled"))
            .and_then(|l| l.action(action))
            .err()
            .map(|e| e.to_string());
        Effect::Redraw
    }
}
fn row(kit: &mut Kit, id: HitId, title: &str, detail: &str, enabled: bool) {
    kit.row(Row {
        id,
        icon: Icon::Glyph(glyph::APP),
        title,
        detail: Some(detail),
        trailing: None,
        selected: false,
        enabled,
    });
}
impl Panel for Integration {
    fn width(&self) -> f32 {
        420.0
    }
    fn view(&self, kit: &mut Kit) {
        let mut buttons = vec![(RETRY, glyph::REFRESH, "Refresh / reconnect")];
        if matches!(&self.state.data,Some(Data::Notifications(s)) if !s.cards.is_empty()) {
            buttons.push((PRIMARY, glyph::CLEAR, "Dismiss all retained notifications"));
        }
        kit.header(self.title(), &buttons);
        if let Some(error) = self.error.as_ref().or(self.state.action_error.as_ref()) {
            kit.notice(error);
        }
        match &self.state.availability {
            Availability::Connecting if self.lease.is_some() => kit.notice("Connecting…"),
            Availability::Cached => kit.notice("Snapshot · refresh to update"),
            Availability::Stale(error) => kit.notice(&format!("Stale · {error}")),
            Availability::Unavailable(error) => kit.notice(error),
            _ => {}
        }
        let connected = matches!(
            self.state.availability,
            Availability::Live | Availability::Cached
        );
        match &self.state.data {
            Some(Data::Notifications(state)) => {
                kit.caption("Retained Windows notifications · not unread messages");
                if state.cards.is_empty() {
                    kit.notice("No retained notifications");
                }
                for (i, card) in state.cards.iter().enumerate().skip(self.top).take(6) {
                    if kit.y + kit.px(110.0) > kit.limit {
                        break;
                    }
                    let count = state.counts.get(&card.app_id).copied().unwrap_or(0);
                    kit.caption(&format!(
                        "{} · {count} notification{}",
                        card.app,
                        if count == 1 { "" } else { "s" }
                    ));
                    let background = kit.list.ops.len();
                    kit.fill(
                        mypaper_ui::Rect::new(kit.left, kit.y, kit.width(), 0.0),
                        kit.px(12.0),
                        kit.look.control(),
                    );
                    row(
                        kit,
                        LIST + i as u32 * 2,
                        &card.title,
                        &card.body,
                        connected && !card.app_id.is_empty(),
                    );
                    kit.buttons(&[(LIST + i as u32 * 2 + 1, "Dismiss", false)]);
                    if let mypaper_ui::Op::Fill { rect, .. } = &mut kit.list.ops[background] {
                        rect.bottom = kit.y;
                    }
                    kit.y += kit.px(10.0);
                }
            }
            Some(Data::T3(state)) => {
                let running = state.threads.iter().filter(|t| t.running()).count();
                let attention = state.threads.iter().filter(|t| t.attention()).count();
                kit.caption(&format!("{running} running · {attention} need input"));
                let config = super::config();
                for (i, thread) in state.threads.iter().enumerate().skip(self.top).take(8) {
                    if kit.y + kit.px(64.0) > kit.limit {
                        break;
                    }
                    let detail = format!(
                        "{} · {} · {}",
                        state
                            .projects
                            .get(&thread.project_id)
                            .unwrap_or(&thread.project_id),
                        thread.provider_instance_id,
                        if thread.attention() {
                            "Needs input".into()
                        } else {
                            format!("{:?}", thread.status)
                        }
                    );
                    row(
                        kit,
                        LIST + i as u32,
                        &thread.title,
                        &detail,
                        config.t3.thread_links.contains_key(&thread.id),
                    );
                }
                if state.threads.is_empty() {
                    kit.notice("No active or recent threads");
                }
                if !state.threads.is_empty() && config.t3.thread_links.is_empty() {
                    kit.notice(
                        "Copy thread links from T3 into Integration settings to enable Open.",
                    );
                }
            }
            Some(Data::Obs(state)) => {
                kit.caption(&state.scene);
                kit.notice(&format!(
                    "Recording: {} · Streaming: {}",
                    if state.recording {
                        if state.paused { "Paused" } else { "Active" }
                    } else {
                        "Stopped"
                    },
                    if state.streaming { "Live" } else { "Stopped" }
                ));
                if connected {
                    kit.buttons(&[
                        (
                            PRIMARY,
                            if state.recording {
                                "Stop recording"
                            } else {
                                "Start recording"
                            },
                            state.recording,
                        ),
                        (
                            SECONDARY,
                            if state.streaming {
                                "Stop streaming"
                            } else {
                                "Start streaming"
                            },
                            state.streaming,
                        ),
                    ]);
                }
            }
            Some(Data::Docker(state)) => {
                if state.containers.is_empty() {
                    kit.notice("No selected containers");
                }
                for (i, container) in state.containers.iter().enumerate().skip(self.top).take(8) {
                    if kit.y + kit.px(64.0) > kit.limit {
                        break;
                    }
                    row(
                        kit,
                        LIST + i as u32,
                        &container.name,
                        &format!(
                            "{}{} · click to {}",
                            container.state,
                            if container.failed {
                                " · needs attention"
                            } else {
                                ""
                            },
                            if container.state == "running" {
                                "stop"
                            } else {
                                "start"
                            }
                        ),
                        connected,
                    );
                }
            }
            Some(Data::Tailscale(state)) => {
                kit.notice(&state.state);
                for address in &state.addresses {
                    kit.caption(address);
                }
                kit.caption(&format!(
                    "Exit node: {}",
                    state.exit_node.as_deref().unwrap_or("None")
                ));
                // Cached mode performs one local API action/read per click, with no watch left open.
                if connected {
                    kit.buttons(&[(
                        PRIMARY,
                        if state.state == "Running" {
                            "Disconnect"
                        } else {
                            "Connect"
                        },
                        false,
                    )]);
                }
                kit.buttons(&[(SECONDARY, "Open Tailscale", false)]);
            }
            Some(Data::Media(state)) => {
                kit.notice(&state.title);
                kit.caption(&state.artist);
                if state.toggle {
                    kit.buttons(&[(PRIMARY, if state.playing { "Pause" } else { "Play" }, false)]);
                }
            }
            None => {}
        }
    }
    fn activate(&mut self, id: HitId, _: &mut Cx) -> Effect {
        if id == RETRY {
            if let Some(lease) = &self.lease {
                lease.retry();
            } else {
                self.lease = super::acquire(self.provider).ok();
            }
            return Effect::Redraw;
        }
        let Some(data) = &self.state.data else {
            return Effect::None;
        };
        match data {
            Data::Notifications(s) if id == PRIMARY => {
                self.action(Action::Dismiss(s.cards.iter().map(|c| c.id).collect()))
            }
            Data::Notifications(s) if id >= LIST => {
                let Some(card) = s.cards.get(((id - LIST) / 2) as usize) else {
                    return Effect::None;
                };
                if (id - LIST) % 2 == 1 {
                    self.action(Action::Dismiss(vec![card.id]))
                } else {
                    mypaper_bars::system::open(format!("shell:AppsFolder\\{}", card.app_id));
                    Effect::Close
                }
            }
            Data::T3(s) if id >= LIST => {
                if let Some(thread) = s.threads.get((id - LIST) as usize)
                    && let Some(link) = super::config().t3.thread_links.get(&thread.id)
                {
                    mypaper_bars::system::open(link.clone());
                    return Effect::Close;
                }
                Effect::None
            }
            Data::Obs(s) => {
                let action = match id {
                    PRIMARY => {
                        if s.recording {
                            mypaper_integrations::obs::Action::StopRecording
                        } else {
                            mypaper_integrations::obs::Action::StartRecording
                        }
                    }
                    SECONDARY => {
                        if s.streaming {
                            mypaper_integrations::obs::Action::StopStreaming
                        } else {
                            mypaper_integrations::obs::Action::StartStreaming
                        }
                    }
                    _ => return Effect::None,
                };
                self.action(Action::Obs(action))
            }
            Data::Docker(s) if id >= LIST => {
                let Some(container) = s.containers.get((id - LIST) as usize) else {
                    return Effect::None;
                };
                self.action(Action::Docker {
                    id: container.id.clone(),
                    start: container.state != "running",
                })
            }
            Data::Tailscale(s) if id == PRIMARY => self.action(Action::Tailscale {
                connected: s.state != "Running",
            }),
            Data::Tailscale(_) if id == SECONDARY => {
                let path = PathBuf::from(
                    std::env::var_os("ProgramFiles").unwrap_or_else(|| "C:/Program Files".into()),
                )
                .join("Tailscale/tailscale-ipn.exe");
                mypaper_bars::system::open(path.to_string_lossy().into_owned());
                Effect::Close
            }
            Data::Media(_) if id == PRIMARY => {
                self.action(Action::Media(mypaper_integrations::media::Action::Toggle))
            }
            _ => Effect::None,
        }
    }
    fn refresh(&mut self, _: &mut Cx) -> bool {
        if self.generation != super::generation() {
            *self = Self::new(self.provider);
            return true;
        }
        let Some(lease) = &self.lease else {
            return false;
        };
        let next = lease.snapshot();
        if self.state == next {
            return false;
        }
        self.state = next;
        true
    }
    fn wheel(&mut self, _: Option<HitId>, delta: i16, _: &mut Cx) -> Effect {
        let count = match &self.state.data {
            Some(Data::Notifications(s)) => s.cards.len().saturating_sub(1),
            Some(Data::T3(s)) => s.threads.len().saturating_sub(1),
            Some(Data::Docker(s)) => s.containers.len().saturating_sub(1),
            _ => 0,
        };
        let next = self
            .top
            .saturating_add_signed(if delta < 0 { 1 } else { -1 })
            .min(count);
        if next == self.top {
            return Effect::None;
        }
        self.top = next;
        Effect::Redraw
    }
}

/// The existing calendar stays underneath the event-driven notification list.
pub struct NotificationCenter {
    notifications: Integration,
    calendar: crate::flyouts::calendar::Calendar,
}
impl NotificationCenter {
    pub fn new(cx: &mut Cx) -> Self {
        Self {
            notifications: Integration::new(Provider::Notifications),
            calendar: crate::flyouts::calendar::Calendar::new(cx),
        }
    }
    #[cfg(test)]
    pub fn fixture(data: Data) -> Self {
        Self {
            notifications: Integration::fixture(Provider::Notifications, data),
            calendar: crate::flyouts::calendar::Calendar::at(
                windows::Win32::Foundation::SYSTEMTIME {
                    wYear: 2026,
                    wMonth: 11,
                    wDay: 5,
                    wDayOfWeek: 4,
                    wHour: 9,
                    wMinute: 41,
                    ..Default::default()
                },
                Some(0),
            ),
        }
    }
}
impl Panel for NotificationCenter {
    fn width(&self) -> f32 {
        self.notifications.width()
    }
    fn view(&self, kit: &mut Kit) {
        let limit = kit.limit;
        kit.limit = (limit - kit.px(276.0)).max(kit.px(160.0));
        self.notifications.view(kit);
        kit.limit = limit;
        kit.separator();
        self.calendar.month(kit, true);
    }
    fn activate(&mut self, id: HitId, cx: &mut Cx) -> Effect {
        if self.calendar.page(id) {
            Effect::Redraw
        } else {
            self.notifications.activate(id, cx)
        }
    }
    fn refresh(&mut self, cx: &mut Cx) -> bool {
        self.notifications.refresh(cx)
    }
    fn wheel(&mut self, id: Option<HitId>, delta: i16, cx: &mut Cx) -> Effect {
        if id.is_some_and(|id| (100..140).contains(&id)) {
            self.calendar.wheel_page(delta);
            Effect::Redraw
        } else {
            self.notifications.wheel(id, delta, cx)
        }
    }
    fn tick(&mut self, now: std::time::Instant, _: &mut Cx) -> (bool, Option<std::time::Instant>) {
        self.calendar.follow_clock(now)
    }
}

pub struct AppActions {
    app_id: String,
    name: String,
    recent: Vec<Recent>,
    task: Option<super::Task<Vec<Recent>>>,
    error: Option<String>,
}
impl AppActions {
    #[cfg(test)]
    pub fn fixture() -> Self {
        Self {
            app_id: "discord".into(),
            name: "Discord".into(),
            recent: vec![Recent {
                title: "Project notes".into(),
                target: "C:/Projects/notes.md".into(),
                arguments: String::new(),
            }],
            task: None,
            error: None,
        }
    }
    pub fn new(app_id: String, name: String) -> Self {
        let task = if super::config().launcher.enabled && !app_id.is_empty() {
            let id = app_id.clone();
            Some(super::Task::spawn(move || {
                mypaper_integrations::launcher::recent(&id, false)
            }))
        } else {
            None
        };
        Self {
            app_id,
            name,
            recent: Vec::new(),
            task,
            error: None,
        }
    }
}
impl Panel for AppActions {
    fn width(&self) -> f32 {
        420.0
    }
    fn view(&self, kit: &mut Kit) {
        kit.header(&self.name, &[]);
        kit.caption("Automatic Recent destinations");
        if let Some(error) = &self.error {
            kit.notice(error);
        }
        if self.task.is_some() {
            kit.notice("Reading recent items…");
        }
        if self.recent.is_empty() && self.task.is_none() {
            kit.notice("No automatic Recent items. Pinned and custom Jump List categories are not exposed by Windows.");
        }
        for (i, item) in self.recent.iter().enumerate() {
            row(kit, LIST + i as u32, &item.title, &item.target, true);
        }
        if !self.app_id.is_empty() {
            kit.buttons(&[(PRIMARY, "Open app", false)]);
        }
    }
    fn activate(&mut self, id: HitId, _: &mut Cx) -> Effect {
        if id == PRIMARY && !self.app_id.is_empty() {
            mypaper_bars::system::open(format!("shell:AppsFolder\\{}", self.app_id));
            return Effect::Close;
        }
        if let Some(item) = id
            .checked_sub(LIST)
            .and_then(|i| self.recent.get(i as usize))
        {
            self.error = mypaper_integrations::launcher::open(item)
                .err()
                .map(|e| e.to_string());
            return if self.error.is_none() {
                Effect::Close
            } else {
                Effect::Redraw
            };
        }
        Effect::None
    }
    fn refresh(&mut self, _: &mut Cx) -> bool {
        let Some(result) = self.task.as_ref().and_then(super::Task::take) else {
            return false;
        };
        self.task = None;
        match result {
            Ok(items) => self.recent = items,
            Err(error) => self.error = Some(error),
        }
        true
    }
}

thread_local! { static ROUTINE: RefCell<Option<RoutineResult>> = const { RefCell::new(None) }; }
pub fn run_routine(routine: mypaper_integrations::config::Routine) {
    let title = routine.title.clone();
    ROUTINE.with_borrow_mut(|s| {
        *s = Some(RoutineResult {
            title,
            task: Some(super::Task::spawn(move || {
                mypaper_integrations::launcher::start_routine(&routine)
            })),
            result: None,
        })
    });
}
pub fn routine_panel() -> Box<dyn Panel> {
    Box::new(
        ROUTINE
            .with_borrow_mut(Option::take)
            .unwrap_or(RoutineResult {
                title: "Routine".into(),
                task: None,
                result: Some(Err("No routine selected".into())),
            }),
    )
}
#[cfg(test)]
pub fn routine_fixture() -> Box<dyn Panel> {
    Box::new(RoutineResult {
        title: "Daily brief".into(),
        task: None,
        result: Some(Ok(())),
    })
}
struct RoutineResult {
    title: String,
    task: Option<super::Task<()>>,
    result: Option<Result<(), String>>,
}
impl Panel for RoutineResult {
    fn width(&self) -> f32 {
        380.0
    }
    fn view(&self, kit: &mut Kit) {
        kit.header("Grok Bot", &[]);
        kit.caption(&self.title);
        kit.notice(match &self.result {
            None => "Starting routine…",
            Some(Ok(())) => {
                "Accepted by Grok Bot. Completion will arrive through Windows notifications."
            }
            Some(Err(error)) => error,
        });
    }
    fn activate(&mut self, _: HitId, _: &mut Cx) -> Effect {
        Effect::None
    }
    fn refresh(&mut self, _: &mut Cx) -> bool {
        if let Some(result) = self.task.as_ref().and_then(super::Task::take) {
            self.task = None;
            self.result = Some(result);
            true
        } else {
            false
        }
    }
}
