//! Native panels for the integrations, on the kit: a header with its buttons, the state as a
//! line or a notice, lists in a scroll view (only the list scrolls), explicit controls. Cached
//! lists; nothing ticks except a scroll's glide.
use crate::flyouts::{
    Cx, Effect, Panel,
    kit::{Kit, link},
};
use mypaper_integrations::{
    Action, Availability, Data, Lease, Provider, Snapshot, launcher::Recent,
};
use mypaper_ui::icons::Icon;
use mypaper_ui::layout::{Align, Stack};
use mypaper_ui::{
    Accessory, ButtonKind, Frame, HitId, Lead, ListRow, Rect, Scroll, TextRole,
};
use std::time::Instant;
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
    /// The list's scroll position (only the list scrolls).
    scroll: Scroll,
    generation: u64,
}

/// A panel's header: its title, and icon buttons at the right.
fn header(f: &mut Frame, column: &mut Stack, title: &str, buttons: &[(HitId, Icon, &str)]) {
    let c = f.theme.color;
    let mut rest = column.next(f.px(32.0));
    for &(id, icon, label) in buttons.iter().rev() {
        let (button, left) = rest.take_right(f.px(30.0), f.px(2.0));
        f.icon_button(id, button, icon, 30.0, label, false);
        rest = left;
    }
    f.text_in(rest, title, TextRole::Title2, c.label, Align::Start);
}

/// Wrapped secondary text (a status, a hint), at most three lines.
fn line(f: &mut Frame, column: &mut Stack, text: &str) {
    let c = f.theme.color;
    let width = column.rect.width();
    let lines = f.line_count(text, TextRole::Callout, width, 3);
    let at = column.next(lines as f32 * f.line(TextRole::Callout));
    f.paragraph(at.left, at.top, width, text, TextRole::Callout, c.secondary, 3);
}

/// A centred notice where a list would be.
fn notice(f: &mut Frame, column: &mut Stack, icon: Icon, title: &str, detail: Option<&str>) {
    let h = f.empty(column.rest(), Some(icon), title, detail);
    column.next(h);
}

/// Buttons side by side across the column, `(id, label, kind)`.
fn buttons(f: &mut Frame, column: &mut Stack, buttons: &[(HitId, &str, ButtonKind)]) {
    let row = column.next(f.px(f.theme.metrics.control));
    let gap = f.px(8.0);
    let n = buttons.len().max(1) as f32;
    let w = (row.width() - gap * (n - 1.0)) / n;
    for (i, &(id, label, kind)) in buttons.iter().enumerate() {
        let r = Rect::new((row.left + i as f32 * (w + gap)).round(), row.top, w.round(), row.height());
        f.button(id, r, label, None, kind, true);
    }
}

/// `rows` in a scroll view from `column`, as tall as they are up to the room left.
fn list(f: &mut Frame, column: &mut Stack, scroll: &Scroll, rows: &[ListRow]) {
    let heights: Vec<f32> = rows.iter().map(|r| f.row_height(r)).collect();
    let content: f32 = heights.iter().sum();
    let viewport = column.next(content.min(column.rest().height().max(f.px(46.0))));
    let width = viewport.width();
    f.scroll(viewport, scroll, Instant::now(), |f, at| {
        let mut stack = Stack::column(Rect::new(at.x, at.y, width, content), 0.0);
        for (row, h) in rows.iter().zip(&heights) {
            f.row(row, stack.next(*h));
        }
        content
    });
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
            scroll: Scroll::default(),
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
            scroll: Scroll::default(),
            generation: super::generation(),
        }
    }
    fn title(&self) -> &'static str {
        match self.provider {
            Provider::Notifications => "Notifications",
            Provider::Media => "Now playing",
            Provider::T3 => "T3 Code",
            Provider::Obs => "OBS Studio",
            Provider::Calendar => "Calendar",
            Provider::Discord => "Discord",
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
impl Panel for Integration {
    fn width(&self) -> f32 {
        420.0
    }
    fn view(&self, kit: &mut Kit) {
        kit.frame(|f, area| {
            let mut column = Stack::column(area, f.px(8.0));
            let mut top = vec![(RETRY, Icon::Refresh, "Refresh / reconnect")];
            if matches!(&self.state.data, Some(Data::Notifications(s)) if !s.cards.is_empty()) {
                top.push((PRIMARY, Icon::Close, "Dismiss all retained notifications"));
            }
            header(f, &mut column, self.title(), &top);
            if let Some(error) = self.error.as_ref().or(self.state.action_error.as_ref()) {
                line(f, &mut column, error);
            }
            match &self.state.availability {
                Availability::Connecting if self.lease.is_some() => {
                    line(f, &mut column, "Connecting…")
                }
                Availability::Cached => line(f, &mut column, "Snapshot · refresh to update"),
                Availability::Stale(error) => line(f, &mut column, &format!("Stale · {error}")),
                Availability::Unavailable(error) => line(f, &mut column, error),
                _ => {}
            }
            let connected = matches!(
                self.state.availability,
                Availability::Live | Availability::Cached
            );
            let section = |f: &mut Frame, column: &mut Stack, title: &str| {
                f.section_header(column.next(f.px(24.0)), title, None);
            };
            match &self.state.data {
                Some(Data::Notifications(state)) => {
                    section(
                        f,
                        &mut column,
                        "Retained Windows notifications · not unread messages",
                    );
                    if state.cards.is_empty() {
                        notice(f, &mut column, Icon::Bell, "No retained notifications", None);
                    }
                    let subtitles: Vec<String> = state
                        .cards
                        .iter()
                        .map(|card| {
                            let count = state.counts.get(&card.app_id).copied().unwrap_or(0);
                            let plural = if count == 1 { "" } else { "s" };
                            format!("{} · {count} notification{plural} · {}", card.app, card.body)
                        })
                        .collect();
                    let rows: Vec<ListRow> = state
                        .cards
                        .iter()
                        .zip(&subtitles)
                        .enumerate()
                        .map(|(i, (card, subtitle))| ListRow {
                            id: LIST + i as u32 * 2,
                            lead: Lead::Icon(Icon::Bell),
                            title: &card.title,
                            subtitle: Some(subtitle),
                            accessory: Accessory::Button(
                                LIST + i as u32 * 2 + 1,
                                Icon::Close,
                                "Dismiss",
                            ),
                            selected: false,
                            enabled: connected && !card.app_id.is_empty(),
                        })
                        .collect();
                    list(f, &mut column, &self.scroll, &rows);
                }
                Some(Data::T3(state)) => {
                    let running = state.threads.iter().filter(|t| t.running()).count();
                    let attention = state.threads.iter().filter(|t| t.attention()).count();
                    section(
                        f,
                        &mut column,
                        &format!("{running} running · {attention} need input"),
                    );
                    let config = super::config();
                    let details: Vec<String> = state
                        .threads
                        .iter()
                        .map(|thread| {
                            format!(
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
                            )
                        })
                        .collect();
                    let rows: Vec<ListRow> = state
                        .threads
                        .iter()
                        .zip(&details)
                        .enumerate()
                        .map(|(i, (thread, detail))| ListRow {
                            id: LIST + i as u32,
                            lead: Lead::Badge(Icon::Message, thread.attention()),
                            title: &thread.title,
                            subtitle: Some(detail),
                            accessory: Accessory::None,
                            selected: false,
                            enabled: config.t3.thread_links.contains_key(&thread.id),
                        })
                        .collect();
                    if state.threads.is_empty() {
                        notice(f, &mut column, Icon::Message, "No active or recent threads", None);
                    }
                    if !state.threads.is_empty() && config.t3.thread_links.is_empty() {
                        // The hint stays above the list, which takes the rest.
                        line(
                            f,
                            &mut column,
                            "Copy thread links from T3 into Integration settings to enable Open.",
                        );
                    }
                    list(f, &mut column, &self.scroll, &rows);
                }
                Some(Data::Obs(state)) => {
                    section(f, &mut column, &state.scene);
                    line(
                        f,
                        &mut column,
                        &format!(
                            "Recording: {} · Streaming: {}",
                            if state.recording {
                                if state.paused { "Paused" } else { "Active" }
                            } else {
                                "Stopped"
                            },
                            if state.streaming { "Live" } else { "Stopped" }
                        ),
                    );
                    if connected {
                        let record = if state.recording {
                            ("Stop recording", ButtonKind::Destructive)
                        } else {
                            ("Start recording", ButtonKind::Gray)
                        };
                        let stream = if state.streaming {
                            ("Stop streaming", ButtonKind::Destructive)
                        } else {
                            ("Start streaming", ButtonKind::Gray)
                        };
                        buttons(
                            f,
                            &mut column,
                            &[(PRIMARY, record.0, record.1), (SECONDARY, stream.0, stream.1)],
                        );
                    }
                }
                Some(Data::Docker(state)) => {
                    if state.containers.is_empty() {
                        notice(f, &mut column, Icon::Layers, "No selected containers", None);
                    }
                    let details: Vec<String> = state
                        .containers
                        .iter()
                        .map(|container| {
                            format!(
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
                            )
                        })
                        .collect();
                    let rows: Vec<ListRow> = state
                        .containers
                        .iter()
                        .zip(&details)
                        .enumerate()
                        .map(|(i, (container, detail))| ListRow {
                            id: LIST + i as u32,
                            lead: Lead::Badge(Icon::Layers, container.state == "running"),
                            title: &container.name,
                            subtitle: Some(detail),
                            accessory: Accessory::None,
                            selected: false,
                            enabled: connected,
                        })
                        .collect();
                    list(f, &mut column, &self.scroll, &rows);
                }
                Some(Data::Media(state)) => {
                    let c = f.theme.color;
                    let mut rest =
                        column.next(f.line(TextRole::Headline) + f.line(TextRole::Callout));
                    if state.toggle {
                        let (button, left) = rest.take_right(f.px(40.0), f.px(10.0));
                        let (icon, label) = if state.playing {
                            (Icon::Pause, "Pause")
                        } else {
                            (Icon::Play, "Play")
                        };
                        f.icon_button(PRIMARY, button, icon, 40.0, label, true);
                        rest = left;
                    }
                    let (title, artist) = rest.take_top(f.line(TextRole::Headline), 0.0);
                    f.text_in(title, &state.title, TextRole::Headline, c.label, Align::Start);
                    f.text_in(artist, &state.artist, TextRole::Callout, c.secondary, Align::Start);
                }
                Some(Data::Calendar(_) | Data::Discord(_)) | None => {}
            }
            area.top + column.used()
        });
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
    fn wheel(&mut self, _: Option<HitId>, delta: i16, cx: &mut Cx) -> Effect {
        if self.scroll.wheel(delta, cx.scale, Instant::now()) {
            Effect::Redraw
        } else {
            Effect::None
        }
    }
    fn tick(&mut self, now: Instant, _: &mut Cx) -> (bool, Option<Instant>) {
        // Only a scroll's glide and fade; `None` at rest.
        self.scroll.tick(now)
    }
}

/// The grouped center shares its notification lease with badges and app actions.
pub type NotificationCenter = crate::flyouts::notifications::Center;

pub struct AppActions {
    app_id: String,
    name: String,
    recent: Vec<Recent>,
    task: Option<super::Task<Vec<Recent>>>,
    error: Option<String>,
    scroll: Scroll,
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
            scroll: Scroll::default(),
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
            scroll: Scroll::default(),
        }
    }
}
impl Panel for AppActions {
    fn width(&self) -> f32 {
        420.0
    }
    fn view(&self, kit: &mut Kit) {
        kit.frame(|f, area| {
            let mut column = Stack::column(area, f.px(8.0));
            header(f, &mut column, &self.name, &[]);
            f.section_header(column.next(f.px(24.0)), "Automatic Recent destinations", None);
            if let Some(error) = &self.error {
                line(f, &mut column, error);
            }
            if self.task.is_some() {
                notice(f, &mut column, Icon::Clock, "Reading recent items…", None);
            }
            if self.recent.is_empty() && self.task.is_none() {
                notice(
                    f,
                    &mut column,
                    Icon::File,
                    "No automatic Recent items",
                    Some("Pinned and custom Jump List categories are not exposed by Windows."),
                );
            }
            let rows: Vec<ListRow> = self
                .recent
                .iter()
                .enumerate()
                .map(|(i, item)| ListRow {
                    id: LIST + i as u32,
                    lead: Lead::Icon(Icon::File),
                    title: &item.title,
                    subtitle: Some(&item.target),
                    accessory: Accessory::None,
                    selected: false,
                    enabled: true,
                })
                .collect();
            // The open-app row keeps its room under the list.
            let foot = if self.app_id.is_empty() {
                0.0
            } else {
                f.px(f.theme.metrics.row_single) + f.px(8.0)
            };
            let room = Rect {
                bottom: column.rest().bottom - foot,
                ..column.rest()
            };
            let mut inner = Stack::column(room, 0.0);
            list(f, &mut inner, &self.scroll, &rows);
            column.next(inner.used());
            if !self.app_id.is_empty() {
                link(f, &mut column, PRIMARY, "Open app");
            }
            area.top + column.used()
        });
    }
    fn wheel(&mut self, _: Option<HitId>, delta: i16, cx: &mut Cx) -> Effect {
        if self.scroll.wheel(delta, cx.scale, Instant::now()) {
            Effect::Redraw
        } else {
            Effect::None
        }
    }
    fn tick(&mut self, now: Instant, _: &mut Cx) -> (bool, Option<Instant>) {
        self.scroll.tick(now)
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
        kit.frame(|f, area| {
            let mut column = Stack::column(area, f.px(8.0));
            header(f, &mut column, "Grok Bot", &[]);
            f.section_header(column.next(f.px(24.0)), &self.title, None);
            let icon = match &self.result {
                None => Icon::Clock,
                Some(Ok(())) => Icon::Check,
                Some(Err(_)) => Icon::Warning,
            };
            let text = match &self.result {
            None => "Starting routine…",
            Some(Ok(())) => {
                "Accepted by Grok Bot. Completion will arrive through Windows notifications."
            }
            Some(Err(error)) => error,
            };
            notice(f, &mut column, icon, text, None);
            area.top + column.used()
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
