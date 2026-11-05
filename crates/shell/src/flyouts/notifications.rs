//! The notification center: Windows' toasts from the shared integration lease,
//! grouped by app with the newest group first. A group of several is a
//! stack (its newest card, and the cards under it peeking out) until clicked open. Each card
//! dismisses with its ×, a group with its own, everything with "Clear all". Do not disturb
//! shows its state and opens its setting (there is no API to change it). The month sits under
//! the list, as Windows 11 puts it (the clock opens this center).
//!
//! Drawn with the kit (`mypaper_ui::kit`, docs/ui.md "Porting a surface"): the header and the
//! month never move; only the list scrolls, inside a viewport whose height depends on the
//! cards, never on where the list is scrolled to. Touchpads scroll it pixel by pixel, a wheel
//! notch glides, and the overlay scrollbar lives inside the viewport and fades when idle.
//!
//! The list is read when it opens and every few seconds while open; nothing runs while closed
//! (the bar's badge has its own slower count).

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use std::time::{Duration, Instant};

use mypaper_bars::flyouts::index::epoch_ms;
use mypaper_bars::flyouts::notifications::{Notification, NotificationList};
use mypaper_bars::flyouts::quick::Dnd;
use mypaper_bars::style::Family;
use mypaper_bars::system;
use mypaper_integrations::{Action, Availability, Data, Lease, Provider};
use mypaper_ui::icons::Icon;
use mypaper_ui::interact::Interaction;
use mypaper_ui::kit::{ButtonKind, Frame};
use mypaper_ui::layout::{Align, Stack};
use mypaper_ui::{HitId, Image, Op, Rect, Role, Scheme, Scroll, Style, TextRole, Theme};

use super::calendar::Calendar;
use super::kit::Kit;
use super::look::Look;
use super::{Cx, Effect, LIST, Panel, Reply};

const CLEAR_ALL: HitId = 10;
const DND: HitId = 11;
const SETTINGS: HitId = 12;
const REFRESH: HitId = 13;
const GRANT: HitId = 14;
const STUDIO: HitId = 15;
/// A group's clear button: `GROUP + index`.
const GROUP: HitId = 500;
/// A card: `CARD + 2 * index` (the card) and `+ 1` (its dismiss button), in flat list order.
// The shared provider permits 512 app groups; reserve their full clear-button range.
const CARD: HitId = LIST + 1_000;
const TOGGLE_GROUP: HitId = 20_000;

/// How often the open center reads the list again.
const POLL: Duration = Duration::from_secs(4);
/// The month's band under the list, design px.
const MONTH: f32 = 276.0;

/// Toasts of one app, newest first.
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    pub key: String,
    pub app: String,
    pub icon: Option<String>,
    pub items: Vec<Notification>,
}

/// Toasts grouped by app identity, the apps in the order of their newest toast.
pub fn by_app(list: &[Notification]) -> Vec<Group> {
    let mut sorted = list.to_vec();
    sorted.sort_by_key(|n| std::cmp::Reverse(n.at));
    let mut groups: Vec<Group> = Vec::new();
    for item in sorted {
        let key = match (&item.aumid, &item.package_family_name) {
            (Some(aumid), _) => format!("app:{aumid}"),
            (None, Some(pfn)) => format!("package:{pfn}:{}", item.app),
            (None, None) => format!("name:{}", item.app),
        };
        match groups.iter_mut().find(|g| g.key == key) {
            Some(group) => {
                if group.icon.is_none() {
                    group.icon.clone_from(&item.icon);
                }
                group.items.push(item);
            }
            None => groups.push(Group {
                key,
                app: item.app.clone(),
                icon: item.icon.clone(),
                items: vec![item],
            }),
        }
    }
    groups
}

/// "now", "5 min", "2 h", "Yesterday", or the date: how long ago `at` (ms since the epoch) was.
pub fn ago(at: u64, now: u64) -> String {
    let minutes = now.saturating_sub(at) / 60_000;
    match minutes {
        0 => "now".into(),
        1..=59 => format!("{minutes} min"),
        60..=1439 => format!("{} h", minutes / 60),
        1440..=2879 => "Yesterday".into(),
        _ => format!("{} days", minutes / 1440),
    }
}

/// The kit's theme for the flyouts' look (the bar's family and scheme, the accent).
pub fn theme(look: &Look) -> Theme {
    let dark = mypaper_ui::theme::luminance(look.ink) > 0.5;
    let style = match look.family {
        Family::Fluent => Style::Fluent,
        Family::Material => Style::OneUi,
        Family::Mono => Style::Mono,
        Family::Macos if look.glass.is_none() && look.fill.a >= 1.0 => Style::Solid,
        Family::Macos => Style::Liquid,
    };
    let scheme = if dark { Scheme::Dark } else { Scheme::Light };
    Theme::new(style, scheme, look.accent)
}

pub struct Center {
    list: Option<NotificationList>,
    groups: Vec<Group>,
    /// Groups opened from their stack.
    open: BTreeSet<String>,
    dnd: Option<Dnd>,
    icons: Vec<Option<Arc<Image>>>,
    calendar: Calendar,
    /// The current time for "ago" labels (refreshed with each read).
    now: u64,
    /// The list's scroll position (only the list scrolls).
    scroll: Scroll,
    /// When the next read of the list is due (ticks also come for the scroll's motion).
    poll_at: Instant,
    lease: Option<Lease>,
    revision: u64,
    generation: u64,
    logos: HashMap<String, Option<String>>,
    error: Option<String>,
    availability: Availability,
}

impl Center {
    pub fn new(cx: &mut Cx) -> Self {
        cx.lanes.quick(|lane| Reply::Quick(lane.read()));
        let lease = crate::integrations::acquire(Provider::Notifications);
        let error = lease.as_ref().err().map(ToString::to_string);
        Self {
            list: None,
            groups: Vec::new(),
            open: BTreeSet::new(),
            dnd: None,
            icons: Vec::new(),
            calendar: Calendar::new(cx),
            now: epoch_ms(),
            scroll: Scroll::new(&theme(cx.look).motion),
            poll_at: Instant::now(),
            lease: lease.ok(),
            revision: u64::MAX,
            generation: crate::integrations::generation(),
            logos: HashMap::new(),
            error,
            availability: Availability::Connecting,
        }
    }

    #[cfg(test)]
    pub fn with(list: NotificationList, calendar: Calendar, now: u64) -> Self {
        let groups = by_app(&list.notifications);
        Self {
            icons: vec![None; groups.len()],
            groups,
            list: Some(list),
            open: BTreeSet::new(),
            dnd: Some(Dnd::Off),
            calendar,
            now,
            scroll: Scroll::default(),
            poll_at: Instant::now(),
            lease: None,
            revision: 0,
            generation: crate::integrations::generation(),
            logos: HashMap::new(),
            error: None,
            availability: Availability::Live,
        }
    }

    #[cfg(test)]
    pub fn fixture(data: Data) -> Self {
        let list = match data {
            Data::Notifications(state) => NotificationList {
                notifications: state.cards.into_iter().map(card).collect(),
                error: None,
            },
            _ => unreachable!("notification center fixture requires notifications"),
        };
        Self::with(
            list,
            Calendar::at(
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
            1_793_869_260_000,
        )
    }

    fn accept(&mut self, list: NotificationList, cx: &mut Cx) -> bool {
        let now = epoch_ms();
        let ages_changed = self.list.as_ref().is_some_and(|old| {
            old.notifications
                .iter()
                .any(|n| ago(n.at, now) != ago(n.at, self.now))
        });
        self.now = now;
        if self.list.as_ref() == Some(&list) {
            return ages_changed;
        }
        self.groups = by_app(&list.notifications);
        self.icons = self
            .groups
            .iter()
            .map(|g| cx.image(g.icon.as_deref()))
            .collect();
        self.list = Some(list);
        true
    }

    /// The cards shown, in order: (group, item) indices.
    fn cards(&self) -> Vec<(usize, usize)> {
        self.groups
            .iter()
            .enumerate()
            .flat_map(|(g, group)| {
                let shown = if self.open.contains(&group.key) {
                    group.items.len()
                } else {
                    1
                };
                (0..shown).map(move |i| (g, i))
            })
            .collect()
    }

    fn dismiss(&mut self, ids: Vec<u32>, cx: &mut Cx) {
        let Some(lease) = &self.lease else { return };
        if let Err(e) = lease.action(Action::Dismiss(ids.clone())) {
            self.error = Some(e.to_string());
            return;
        }
        if let Some(list) = &mut self.list {
            list.notifications.retain(|n| !ids.contains(&n.id));
            let list = list.clone();
            // Keep images aligned when removing the first app group.
            self.list = None;
            self.accept(list, cx);
        }
    }
}

fn card(card: mypaper_integrations::notifications::Card) -> Notification {
    Notification {
        id: card.id,
        app: card.app,
        aumid: Some(card.app_id),
        package_family_name: None,
        icon: None,
        title: card.title,
        body: card.body,
        at: mypaper_bars::flyouts::notifications::epoch_ms(card.at),
    }
}

impl Panel for Center {
    fn width(&self) -> f32 {
        380.0
    }

    fn view(&self, kit: &mut Kit) {
        let theme = theme(kit.look);
        let input = Interaction {
            hover: kit.hover,
            pressed: kit.pressed,
            focus: kit.focus,
            focus_visible: kit.focus.is_some(),
        };
        let (left, right, top) = (kit.left, kit.right, kit.y);
        // The month takes a fixed band at the bottom; the list gets what is left.
        let list_bottom = kit.limit - kit.px(MONTH);
        let mut f = Frame::new(kit.text, &theme, kit.s, &input);
        let c = theme.color;
        let mut column = Stack::column(
            Rect::new(left, top, right - left, (list_bottom - top).max(0.0)),
            f.px(8.0),
        );

        // The header: the title, and its buttons at the right.
        let head = column.next(f.px(32.0));
        let has_any = !self.groups.is_empty();
        let mut buttons = vec![(
            DND,
            if self.dnd.is_some_and(|d| d != Dnd::Off) {
                Icon::BellOff
            } else {
                Icon::Bell
            },
            "Do not disturb",
        )];
        buttons.push((REFRESH, Icon::Refresh, "Refresh notifications"));
        if has_any {
            buttons.push((CLEAR_ALL, Icon::Close, "Clear all"));
        }
        let mut title = head;
        for (id, icon, label) in buttons.iter().rev() {
            let (button, rest) = title.take_right(f.px(30.0), f.px(2.0));
            f.icon_button(*id, button, *icon, 30.0, label, false);
            title = rest;
        }
        f.text_in(title, "Notifications", TextRole::Title2, c.label, Align::Start);

        // A notice in place of the list (access, errors, loading, empty).
        let notice = |f: &mut Frame, column: &mut Stack, title: &str, detail: Option<&str>| {
            let rect = column.rest();
            let h = f.empty(rect, Some(Icon::Bell), title, detail);
            column.next(h);
        };
        let buttons = |f: &mut Frame, column: &mut Stack, buttons: &[(HitId, &str)]| {
            let row = column.next(f.px(theme.metrics.control));
            let mut x = row.right;
            for (id, label) in buttons.iter().rev() {
                let (w, h) = f.button_size(label, false);
                let r = Rect::new(x - w, row.top, w, h);
                f.button(*id, r, label, None, ButtonKind::Gray, true);
                x = r.left - f.px(8.0);
            }
        };
        if self.lease.is_none() && self.error.is_some() {
            notice(
                &mut f,
                &mut column,
                "Notifications are off",
                Some("Enable Windows notifications in Studio to show retained toasts here."),
            );
            buttons(&mut f, &mut column, &[(STUDIO, "Open Studio")]);
        } else if let Some(error) = &self.error {
            notice(&mut f, &mut column, "Notifications can't be read", Some(error));
            buttons(
                &mut f,
                &mut column,
                &[
                    (GRANT, "Grant access"),
                    (SETTINGS, "Notification settings"),
                ],
            );
        } else if let Availability::Stale(error) | Availability::Unavailable(error) =
            &self.availability
        {
            notice(&mut f, &mut column, "Notifications can't be read", Some(error));
            buttons(
                &mut f,
                &mut column,
                &[
                    (GRANT, "Grant access"),
                    (SETTINGS, "Notification settings"),
                ],
            );
        }
        match &self.list {
            None if self.error.is_none() && self.availability == Availability::Connecting => {
                notice(&mut f, &mut column, "Loading notifications…", None)
            }
            None => {}
            Some(NotificationList { error: Some(_), .. }) => {
                notice(
                    &mut f,
                    &mut column,
                    "Notifications can't be read",
                    Some("Turn on notification access in Settings."),
                );
                buttons(&mut f, &mut column, &[(SETTINGS, "Notification settings")]);
            }
            Some(_) if !has_any => notice(&mut f, &mut column, "No new notifications", None),
            Some(_) => {
                // The viewport's height follows the cards (up to the room there is), never the
                // scroll position: scrolling never resizes the flyout.
                let width = column.rect.width();
                let cards = self.cards();
                let mut content = 0.0;
                // Measured once: each wrap is a fresh DirectWrite layout.
                let mut heights = Vec::with_capacity(cards.len());
                let mut shown_group = None;
                for &(g, i) in &cards {
                    if shown_group != Some(g) {
                        shown_group = Some(g);
                        if self.open.contains(&self.groups[g].key) && self.groups[g].items.len() > 1 {
                            content += f.px(28.0) + f.px(4.0);
                        }
                    }
                    let h = self.card_height(&mut f, g, i, width);
                    heights.push(h);
                    content += h + f.px(8.0);
                }
                content = (content - f.px(8.0)).max(0.0);
                let room = column.rest().height();
                let viewport = column.next(content.min(room));
                f.scroll(viewport, &self.scroll, Instant::now(), |f, at| {
                    self.list(f, &cards, &heights, Rect::new(at.x, at.y, width, content))
                });
            }
        }
        kit.y = (top + column.used()).round() + f.px(8.0);
        let mut list = f.list;
        kit.list.ops.append(&mut list.ops);
        kit.list.hits.append(&mut list.hits);
        kit.separator();
        self.calendar.month(kit, true);
    }

    fn activate(&mut self, id: HitId, cx: &mut Cx) -> Effect {
        if self.calendar.page(id) {
            return Effect::Redraw;
        }
        match id {
            STUDIO => {
                crate::apps::open_studio();
                Effect::Close
            }
            REFRESH => {
                if let Some(lease) = &self.lease {
                    if matches!(self.availability, Availability::Live | Availability::Cached) {
                        self.error = lease
                            .action(Action::RefreshNotifications)
                            .err()
                            .map(|e| e.to_string());
                    } else {
                        lease.retry();
                    }
                } else {
                    let lease = crate::integrations::acquire(Provider::Notifications);
                    self.error = lease.as_ref().err().map(ToString::to_string);
                    self.lease = lease.ok();
                }
                Effect::Redraw
            }
            GRANT => {
                // WinRT requires the permission request itself on this UI thread and explicit click.
                match mypaper_integrations::notifications::request_access() {
                    Ok(operation) => cx.lanes.apps(move |_| Reply::NotificationAccess(
                        operation.join().map_err(|e| e.to_string()).and_then(|status| {
                            if status == windows::UI::Notifications::Management::UserNotificationListenerAccessStatus::Allowed {
                                Ok(())
                            } else { Err("Windows notification access was not granted".into()) }
                        })
                    )),
                    Err(e) => self.error = Some(e.to_string()),
                }
                Effect::Redraw
            }
            id if id >= TOGGLE_GROUP => {
                if let Some(group) = self.groups.get((id - TOGGLE_GROUP) as usize)
                    && !self.open.remove(&group.key)
                {
                    self.open.insert(group.key.clone());
                }
                Effect::Redraw
            }
            DND | SETTINGS => {
                system::open("ms-settings:notifications".into());
                Effect::Close
            }
            CLEAR_ALL => {
                let ids = self
                    .groups
                    .iter()
                    .flat_map(|g| g.items.iter().map(|n| n.id))
                    .collect();
                self.dismiss(ids, cx);
                Effect::Redraw
            }
            id if (GROUP..CARD).contains(&id) => {
                let Some(group) = self.groups.get((id - GROUP) as usize) else {
                    return Effect::None;
                };
                let ids = group.items.iter().map(|n| n.id).collect();
                self.dismiss(ids, cx);
                Effect::Redraw
            }
            id if id >= CARD => {
                let cards = self.cards();
                let Some(&(g, i)) = cards.get(((id - CARD) / 2) as usize) else {
                    return Effect::None;
                };
                let group = &self.groups[g];
                if (id - CARD) % 2 == 1 {
                    let dismissed = vec![group.items[i].id];
                    self.dismiss(dismissed, cx);
                    return Effect::Redraw;
                }
                if group.items.len() > 1 && !self.open.contains(&group.key) {
                    self.open.insert(group.key.clone());
                    return Effect::Redraw;
                }
                // A card opens its app (toasts can't be activated from here).
                if let Some(aumid) = &group.items[i].aumid {
                    system::open(format!("shell:AppsFolder\\{aumid}"));
                    return Effect::Close;
                }
                Effect::None
            }
            _ => Effect::None,
        }
    }

    fn wheel(&mut self, id: Option<HitId>, delta: i16, cx: &mut Cx) -> Effect {
        // The month pages on its own; everywhere else the wheel scrolls the list.
        if id.is_some_and(|id| (100..140).contains(&id)) {
            self.calendar.wheel_page(delta);
            return Effect::Redraw;
        }
        if self.scroll.wheel(delta, cx.scale, Instant::now()) {
            Effect::Redraw
        } else {
            Effect::None
        }
    }

    fn swipe(&mut self, id: HitId, _: f32, cx: &mut Cx) -> Effect {
        if let Some(index) = id.checked_sub(CARD)
            && let Some(&(g, i)) = self.cards().get((index / 2) as usize)
        {
            self.dismiss(vec![self.groups[g].items[i].id], cx);
            return Effect::Redraw;
        }
        Effect::None
    }

    fn key(&mut self, key: u16, _: &mut Cx) -> Option<Effect> {
        use windows::Win32::UI::Input::KeyboardAndMouse::{VK_NEXT, VK_PRIOR};
        if key == VK_NEXT.0 || key == VK_PRIOR.0 {
            self.scroll.page(key == VK_NEXT.0, Instant::now());
            return Some(Effect::Redraw);
        }
        None
    }

    fn reply(&mut self, reply: &Reply, cx: &mut Cx) -> bool {
        match reply {
            Reply::NotificationIcons(icons) => {
                for (id, path) in icons {
                    self.logos.insert(id.clone(), path.clone());
                }
                if let Some(mut list) = self.list.clone() {
                    for n in &mut list.notifications {
                        n.icon = n
                            .aumid
                            .as_ref()
                            .and_then(|id| self.logos.get(id))
                            .cloned()
                            .flatten();
                    }
                    return self.accept(list, cx);
                }
                false
            }
            Reply::NotificationAccess(result) => {
                self.error = result.as_ref().err().cloned();
                if result.is_ok()
                    && let Some(lease) = &self.lease
                {
                    lease.retry();
                }
                true
            }
            Reply::Quick(quick) => {
                let changed = self.dnd != quick.dnd;
                self.dnd = quick.dnd;
                changed
            }
            _ => false,
        }
    }

    fn refresh(&mut self, cx: &mut Cx) -> bool {
        let generation = crate::integrations::generation();
        if generation != self.generation {
            self.generation = generation;
            let lease = crate::integrations::acquire(Provider::Notifications);
            self.error = lease.as_ref().err().map(ToString::to_string);
            self.lease = lease.ok();
            self.revision = u64::MAX;
            self.list = None;
            self.groups.clear();
            self.icons.clear();
            if self.lease.is_none() {
                return true;
            }
        }
        let Some(state) = self.lease.as_ref().map(Lease::snapshot) else {
            return false;
        };
        if state.revision == self.revision {
            return false;
        }
        self.revision = state.revision;
        self.availability = state.availability;
        self.error = state.action_error;
        if let Some(Data::Notifications(state)) = state.data {
            let mut pending = BTreeSet::new();
            let notifications = state
                .cards
                .into_iter()
                .map(|c| {
                    let mut n = card(c);
                    if let Some(id) = &n.aumid {
                        if let Some(path) = self.logos.get(id) {
                            n.icon = path.clone();
                        } else {
                            pending.insert(id.clone());
                        }
                    }
                    n
                })
                .collect();
            if !pending.is_empty() {
                let ids: Vec<_> = pending.into_iter().collect();
                for id in &ids {
                    self.logos.insert(id.clone(), None);
                }
                cx.lanes.notification_icons(ids);
            }
            self.accept(
                NotificationList {
                    notifications,
                    error: None,
                },
                cx,
            );
        }
        true
    }

    fn tick(&mut self, now: Instant, cx: &mut Cx) -> (bool, Option<Instant>) {
        // The scroll's glide and the scrollbar's fade tick at frame rate; the list's read and
        // the minute only when due.
        let (scrolled, scroll_next) = self.scroll.tick(now);
        let (minute, next_minute) = self.calendar.follow_clock(now);
        let mut ages = false;
        if now >= self.poll_at {
            self.poll_at = now + POLL;
            if matches!(self.availability, Availability::Live | Availability::Cached)
                && let Some(lease) = &self.lease
            {
                let _ = lease.action(Action::RefreshNotifications);
            }
            ages = self.list.clone().is_some_and(|list| self.accept(list, cx));
        }
        let next = [Some(self.poll_at), next_minute, scroll_next]
            .into_iter()
            .flatten()
            .min();
        (minute || ages || scrolled, next)
    }
}

impl Center {
    /// The list's content from `rect`'s top: group headers of opened stacks, and cards.
    /// Returns its height.
    fn list(&self, f: &mut Frame, cards: &[(usize, usize)], heights: &[f32], rect: Rect) -> f32 {
        let gap = f.px(8.0);
        let mut column = Stack::column(rect, gap);
        let mut shown_group = None;
        for (n, &(g, i)) in cards.iter().enumerate() {
            let group = &self.groups[g];
            let open = self.open.contains(&group.key) && group.items.len() > 1;
            if shown_group != Some(g) {
                shown_group = Some(g);
                if open {
                    let head = column.next(f.px(28.0));
                    column.space(-gap + f.px(4.0));
                    self.group_header(f, g, group, head);
                }
            }
            let stacked = (!open && group.items.len() > 1).then_some(group.items.len() - 1);
            let h = heights[n];
            self.card(f, n, g, &group.items[i], stacked, column.next(h));
        }
        column.used()
    }

    /// A card's height: its padding, the app line, the title, up to three lines of body, and
    /// the stack's peek.
    fn card_height(&self, f: &mut Frame, g: usize, i: usize, width: f32) -> f32 {
        let group = &self.groups[g];
        let item = &group.items[i];
        let pad = f.px(12.0);
        let lines = if item.body.trim().is_empty() {
            0
        } else {
            f.line_count(&item.body, TextRole::Callout, width - 2.0 * pad, 3)
        };
        let stacked = !self.open.contains(&group.key) && group.items.len() > 1;
        let mut h = 2.0 * pad
            + f.line(TextRole::Footnote)
            + f.px(4.0)
            + f.line(TextRole::Headline)
            + lines as f32 * f.line(TextRole::Callout);
        if stacked {
            h += f.px(8.0);
        }
        h
    }

    /// An opened stack's header: the app, "Show less", and its clear button.
    fn group_header(&self, f: &mut Frame, g: usize, group: &Group, rect: Rect) {
        let (clear, rest) = rect.take_right(f.px(28.0), f.px(4.0));
        f.icon_button(
            GROUP + g as HitId,
            clear,
            Icon::Close,
            24.0,
            &format!("Clear {}", group.app),
            true,
        );
        f.section_header(
            rest,
            &group.app,
            Some((TOGGLE_GROUP + g as HitId, "Show less")),
        );
    }

    fn card(
        &self,
        f: &mut Frame,
        n: usize,
        g: usize,
        item: &Notification,
        under: Option<usize>,
        rect: Rect,
    ) {
        let t = f.theme;
        let c = t.color;
        let id = CARD + 2 * n as HitId;
        let radius = f.px(t.radius.card);
        let peek = if under.is_some() { f.px(8.0) } else { 0.0 };
        let body = Rect {
            bottom: rect.bottom - peek,
            ..rect
        };
        if under.is_some() {
            // The cards under it peek out below, narrower and fainter. Only the part below
            // the card is drawn: the card is translucent, and a second card showing through it
            // would read as a second edge.
            let below = Rect::new(
                rect.left + f.px(12.0),
                body.top + f.px(8.0),
                rect.width() - f.px(24.0),
                body.height(),
            );
            f.push(Op::Clip {
                rect: Rect {
                    top: body.bottom,
                    ..rect
                },
            });
            f.fill(below, radius, c.card.with_alpha(c.card.a * 0.6));
            f.push(Op::Unclip);
        }
        f.fill(body, radius, c.card);
        f.wash(id, body, radius, true);
        let pad = f.px(12.0);
        let inner = body.inflate(-pad);
        // The app line: icon, app, time, and the dismiss button.
        let (top, rest) = inner.take_top(f.line(TextRole::Footnote), f.px(4.0));
        let icon = f.px(16.0);
        let icon_rect = top.place(icon, icon, Align::Start, Align::Center);
        match self.icons.get(g).cloned().flatten() {
            Some(image) => f.push(Op::Image {
                rect: icon_rect,
                image,
                opacity: 1.0,
                source: None,
            }),
            None => f.icon(icon_rect, Icon::Bell, 13.0, c.secondary),
        }
        let (dismiss, top) = top.take_right(f.px(20.0), f.px(6.0));
        let (when, app) = top.take_right(f.px(72.0), f.px(6.0));
        let app_rect = Rect {
            left: icon_rect.right + f.px(6.0),
            ..app
        };
        let group = &self.groups[g];
        f.text_in(app_rect, &group.app, TextRole::Footnote, c.secondary, Align::Start);
        f.text_in(when, &ago(item.at, self.now), TextRole::Footnote, c.tertiary, Align::End);
        let (title, rest) = rest.take_top(f.line(TextRole::Headline), 0.0);
        f.text_in(title, &item.title, TextRole::Headline, c.label, Align::Start);
        if !item.body.trim().is_empty() {
            f.paragraph(
                rest.left,
                rest.top,
                rest.width(),
                &item.body,
                TextRole::Callout,
                c.secondary,
                3,
            );
        }
        f.focus_ring(id, body, radius);
        let label = match under {
            Some(more) => format!("{}: {}, {more} more", item.title, item.body),
            None => format!("{}: {}", item.title, item.body),
        };
        f.hit(id, body, Role::Button, &label, true, None);
        // Over the card's hit, so it wins.
        let d = f.px(22.0);
        let button = Rect::new(dismiss.right - d, dismiss.top + (dismiss.height() - d) / 2.0, d, d);
        f.icon_button(id + 1, button, Icon::Close, 22.0, "Dismiss", false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flyouts::{Panel as _, build};

    fn many() -> Center {
        let now = 1_793_869_260_000;
        let apps = ["Mail", "Messages", "Calendar", "Photos", "Teams", "Settings"];
        let notifications = (0..12u32)
            .map(|i| Notification {
                id: i + 1,
                app: apps[i as usize % apps.len()].into(),
                aumid: Some(format!("app{}", i as usize % apps.len())),
                package_family_name: None,
                icon: None,
                title: format!("Notification {i}"),
                body: "A body long enough to wrap onto a second line of the card in the list."
                    .into(),
                at: now - u64::from(i) * 7 * 60_000,
            })
            .collect();
        let mut center = Center::fixture(Data::Notifications(Default::default()));
        center.list = None;
        center.groups.clear();
        let list = NotificationList {
            notifications,
            error: None,
        };
        center.groups = by_app(&list.notifications);
        center.icons = vec![None; center.groups.len()];
        center.list = Some(list);
        center.now = now;
        center
    }

    /// The scroll bug: wheel over the list scrolled whole cards, resized the flyout, and the
    /// scrollbar ran down beside the month. Now only the list's viewport moves.
    #[test]
    fn only_the_list_scrolls_and_the_flyout_keeps_its_size() {
        let mut ui = mypaper_ui::Ui::new().unwrap();
        let look = Look::default();
        let mut center = many();
        let draw = |ui: &mut mypaper_ui::Ui, center: &Center| {
            build(center, &mut ui.text, &look, 1.0, 900.0, (None, None, None), false)
        };
        let (before, size) = draw(&mut ui, &center);
        let fixed = |list: &mypaper_ui::DisplayList| -> Vec<(HitId, Rect)> {
            list.hits
                .iter()
                .filter(|h| h.id < CARD && !(GROUP..CARD).contains(&h.id))
                .map(|h| (h.id, h.rect))
                .collect()
        };
        let viewport = |list: &mypaper_ui::DisplayList| {
            list.ops
                .iter()
                .find_map(|op| match op {
                    Op::Clipped { rect, .. } => Some(*rect),
                    _ => None,
                })
                .expect("the list is a scroll view")
        };
        let port = viewport(&before);
        assert!(center.scroll.max() > 0.0, "twelve cards overflow 900 px");
        for delta in [-30, -90, -120] {
            center.scroll.wheel(delta, 1.0, Instant::now());
        }
        let (after, size_after) = draw(&mut ui, &center);
        assert_eq!(size, size_after, "scrolling never resizes the flyout");
        assert_eq!(viewport(&after), port, "nor moves the viewport");
        assert_eq!(fixed(&before), fixed(&after), "header and month stay put");
        for hit in after.hits.iter().filter(|h| h.id >= CARD) {
            assert!(
                hit.rect.top >= port.top && hit.rect.bottom <= port.bottom,
                "card hits stay inside the viewport: {:?}",
                hit.rect
            );
        }
        let first = |list: &mypaper_ui::DisplayList| {
            list.hits.iter().find(|h| h.id == CARD).map(|h| h.rect.bottom)
        };
        assert_ne!(first(&before), first(&after), "the list moved");
        if std::env::var_os("MYPAPER_SHOT").is_some() {
            let dir = std::env::temp_dir().join("mypaper-golden");
            for (name, list) in [("notifications-top", &before), ("notifications-scrolled", &after)] {
                let image = mypaper_ui::render(&mut ui, list, size.0, size.1).unwrap();
                mypaper_ui::golden::write(&image, &dir.join(format!("{name}.png"))).unwrap();
            }
        }
    }
}
