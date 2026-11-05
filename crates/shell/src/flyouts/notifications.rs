//! The notification center: Windows' toasts (`UserNotificationListener`, on the
//! `notifications` lane), grouped by app with the newest group first. A group of several is a
//! stack (its newest card, and how many are under it) until clicked open. Each card dismisses
//! with its ×, a group with its own, everything with "Clear all". Do not disturb shows its
//! state and opens its setting (there is no API to change it). The month sits under the list,
//! as Windows 11 puts it (the clock opens this center).
//!
//! The list is read when it opens and every few seconds while open; nothing runs while closed
//! (the bar's badge has its own slower count).

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use mypaper_bars::flyouts::index::epoch_ms;
use mypaper_bars::flyouts::notifications::{Notification, NotificationList};
use mypaper_bars::flyouts::quick::Dnd;
use mypaper_bars::system;
use mypaper_ui::{HitId, Image, Point, Rect, Role};

use super::calendar::Calendar;
use super::kit::{Kit, glyph};
use super::{Cx, Effect, LIST, Panel, Reply};

const CLEAR_ALL: HitId = 10;
const DND: HitId = 11;
const SETTINGS: HitId = 12;
/// A group's clear button: `GROUP + index`.
const GROUP: HitId = 500;
/// A card: `LIST + 2 * index` (the card) and `+ 1` (its dismiss button), in flat list order.
const CARD: HitId = LIST;
const TOGGLE_GROUP: HitId = 20_000;

/// How often the open center reads the list again.
const POLL: Duration = Duration::from_secs(4);

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
    scroll: usize,
}

impl Center {
    pub fn new(cx: &mut Cx) -> Self {
        cx.lanes
            .notifications(|lane| Reply::Notifications(lane.list()));
        cx.lanes.quick(|lane| Reply::Quick(lane.read()));
        Self {
            list: None,
            groups: Vec::new(),
            open: BTreeSet::new(),
            dnd: None,
            icons: Vec::new(),
            calendar: Calendar::new(cx),
            now: epoch_ms(),
            scroll: 0,
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
            scroll: 0,
        }
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
        if let Some(list) = &mut self.list {
            list.notifications.retain(|n| !ids.contains(&n.id));
            self.groups = by_app(&list.notifications);
        }
        cx.lanes.notifications(move |lane| {
            for id in ids {
                if let Err(e) = lane.dismiss(id) {
                    log::warn!("notifications: dismissing {id}: {e}");
                }
            }
            Reply::Notifications(lane.list())
        });
    }
}

impl Panel for Center {
    fn width(&self) -> f32 {
        380.0
    }

    fn view(&self, kit: &mut Kit) {
        let has_any = !self.groups.is_empty();
        let mut buttons = vec![(
            DND,
            if self.dnd.is_some_and(|d| d != Dnd::Off) {
                glyph::MOON
            } else {
                glyph::BELL
            },
            "Do not disturb",
        )];
        if has_any {
            buttons.push((CLEAR_ALL, glyph::CLEAR, "Clear all"));
        }
        kit.header("Notifications", &buttons);
        // The month takes a fixed band at the bottom; the list gets what is left.
        let month = kit.px(276.0);
        let list_bottom = kit.limit - month;
        let list_top = kit.y;
        match &self.list {
            None => kit.notice("…"),
            Some(NotificationList { error: Some(_), .. }) => {
                kit.notice("Notifications can't be read. Turn on notification access in Settings.");
                kit.buttons(&[(SETTINGS, "Notification settings", false)]);
            }
            Some(_) if !has_any => kit.notice("No new notifications"),
            Some(_) => {
                let mut group_shown = None;
                for (n, (g, i)) in self
                    .cards()
                    .into_iter()
                    .enumerate()
                    .skip(self.scroll.min(self.cards().len().saturating_sub(1)))
                {
                    if kit.y + kit.px(84.0) > list_bottom {
                        let style = kit.font(12.0, 400);
                        let center = Point::new(kit.left + kit.width() / 2.0, kit.y + kit.px(8.0));
                        kit.text_centered(center, "More notifications…", &style, kit.look.faint());
                        kit.y += kit.px(18.0);
                        break;
                    }
                    let group = &self.groups[g];
                    if group_shown != Some(g) {
                        group_shown = Some(g);
                        self.group_header(kit, g, group);
                    }
                    let stacked = !self.open.contains(&group.key) && group.items.len() > 1;
                    self.card(
                        kit,
                        n,
                        &group.items[i],
                        stacked.then_some(group.items.len() - 1),
                    );
                }
            }
        }
        let count = self.cards().len();
        if count > 3 && (self.scroll > 0 || kit.y >= list_bottom - kit.px(90.0)) {
            let track = (list_bottom - list_top).max(kit.px(30.0));
            let height = (track * (3.0 / count as f32)).max(kit.px(18.0));
            let y = list_top
                + (track - height) * self.scroll.min(count - 1) as f32 / (count - 1) as f32;
            kit.fill(
                Rect::new(kit.right + kit.px(6.0), y, kit.px(2.0), height),
                kit.px(1.0),
                kit.look.faint(),
            );
        }
        kit.separator();
        self.calendar.month(kit, true);
    }

    fn activate(&mut self, id: HitId, cx: &mut Cx) -> Effect {
        if self.calendar.page(id) {
            return Effect::Redraw;
        }
        match id {
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
                if let Some(list) = &mut self.list {
                    list.notifications.clear();
                }
                self.groups.clear();
                cx.lanes.notifications(|lane| {
                    if let Err(e) = lane.clear() {
                        log::warn!("notifications: clearing: {e}");
                    }
                    Reply::Notifications(lane.list())
                });
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

    fn wheel(&mut self, id: Option<HitId>, delta: i16, _: &mut Cx) -> Effect {
        // The list scrolls independently; paging the month keeps the selected card.
        if id.is_some_and(|id| (100..140).contains(&id)) {
            self.calendar.wheel_page(delta);
            return Effect::Redraw;
        }
        let old = self.scroll;
        self.scroll = if delta > 0 {
            self.scroll.saturating_sub(1)
        } else {
            (self.scroll + 1).min(self.cards().len().saturating_sub(1))
        };
        if old != self.scroll {
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
            self.scroll = if key == VK_PRIOR.0 {
                self.scroll.saturating_sub(1)
            } else {
                (self.scroll + 1).min(self.cards().len().saturating_sub(1))
            };
            return Some(Effect::Redraw);
        }
        None
    }

    fn reply(&mut self, reply: &Reply, cx: &mut Cx) -> bool {
        match reply {
            Reply::Notifications(list) => {
                let now = epoch_ms();
                let ages_changed = self.list.as_ref().is_some_and(|old| {
                    old.notifications
                        .iter()
                        .any(|n| ago(n.at, now) != ago(n.at, self.now))
                });
                self.now = now;
                if self.list.as_ref() == Some(list) {
                    return ages_changed;
                }
                self.groups = by_app(&list.notifications);
                self.icons = self
                    .groups
                    .iter()
                    .map(|g| cx.image(g.icon.as_deref()))
                    .collect();
                self.list = Some(list.clone());
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

    fn tick(&mut self, now: Instant, cx: &mut Cx) -> (bool, Option<Instant>) {
        let (minute, next_minute) = self.calendar.follow_clock(now);
        if self.list.is_some() {
            cx.lanes
                .notifications(|lane| Reply::Notifications(lane.list()));
        }
        let next = [Some(now + POLL), next_minute].into_iter().flatten().min();
        (minute, next)
    }
}

impl Center {
    fn group_header(&self, kit: &mut Kit, g: usize, group: &Group) {
        let height = kit.px(28.0);
        let icon = kit.px(16.0);
        let y = kit.y;
        kit.hit(
            TOGGLE_GROUP + g as HitId,
            Rect::new(kit.left, y, kit.width() - kit.px(35.0), height),
            Role::Button,
            &format!("Expand or collapse {}", group.app),
            true,
        );
        match self.icons.get(g).cloned().flatten() {
            Some(image) => kit.image(
                Rect::new(kit.left, y + (height - icon) / 2.0, icon, icon),
                image,
            ),
            None => {
                let style = kit.font(12.0, 600);
                let letter = group
                    .app
                    .chars()
                    .next()
                    .unwrap_or('?')
                    .to_uppercase()
                    .to_string();
                kit.text_centered(
                    Point::new(kit.left + icon / 2.0, y + height / 2.0),
                    &letter,
                    &style,
                    kit.look.dim(),
                );
            }
        }
        let style = kit.font(12.0, 600);
        let (_, h) = kit.text.measure("Ag", &style);
        let (left, room) = (kit.left + icon + kit.px(8.0), kit.width() - kit.px(60.0));
        kit.text_at(
            left,
            y + (height - h) / 2.0,
            &group.app,
            &style,
            kit.look.dim(),
            room,
        );
        let right = kit.right;
        kit.icon_button(
            GROUP + g as HitId,
            Point::new(right - kit.px(15.0), y + height / 2.0),
            glyph::CLOSE,
            &format!("Clear {}", group.app),
        );
        kit.y += height + kit.px(2.0);
    }

    fn card(&self, kit: &mut Kit, n: usize, item: &Notification, under: Option<usize>) {
        let id = CARD + 2 * n as HitId;
        let pad = kit.px(12.0);
        let top = kit.y;
        let (left, width) = (kit.left, kit.width());
        // Measure first: title, body (two lines), and the stack's count.
        let title = kit.font(13.0, 600);
        let body = kit.font(13.0, 400);
        let small = kit.font(11.0, 400);
        let when = ago(item.at, self.now);
        let (ww, wh) = kit.text.measure(&when, &small);
        let inner = width - 2.0 * pad;
        let lines = kit.text.lines(&item.body, &body, inner, 2);
        let (_, th) = kit.text.measure("Ag", &title);
        let (_, bh) = kit.text.measure("Ag", &body);
        let mut height = pad * 2.0 + th + bh * lines.len() as f32;
        if under.is_some() {
            height += kit.px(16.0);
        }
        let rect = Rect::new(left, top, width, height);
        let radius = kit.px(12.0);
        if under.is_some() {
            // The cards under it peek out below, narrower.
            let peek = Rect::new(
                left + kit.px(10.0),
                top + kit.px(6.0),
                width - kit.px(20.0),
                height,
            );
            kit.fill(peek, radius, kit.look.control().with_alpha(0.06));
        }
        kit.fill(rect, radius, kit.look.control());
        kit.state(id, rect, radius);
        let room = inner - ww - kit.px(36.0);
        kit.text_at(
            left + pad,
            top + pad,
            &item.title,
            &title,
            kit.look.ink,
            room,
        );
        kit.text_at(
            rect.right - pad - kit.px(28.0) - ww,
            top + pad + (th - wh) / 2.0,
            &when,
            &small,
            kit.look.dim(),
            ww + 1.0,
        );
        let mut y = top + pad + th;
        for line in lines {
            let (w, h) = kit.text.measure(&line, &body);
            kit.list.ops.push(mypaper_ui::Op::Text {
                rect: Rect::new(left + pad, y, w, h),
                text: line,
                style: body.clone(),
                color: kit.look.dim(),
            });
            y += h;
        }
        if let Some(more) = under {
            let label = format!("{more} more");
            let (left, room) = (left + pad, inner);
            kit.text_at(
                left,
                y + kit.px(2.0),
                &label,
                &small,
                kit.look.faint(),
                room,
            );
        }
        kit.hit(
            id,
            rect,
            Role::Button,
            &format!("{}: {}", item.title, item.body),
            true,
        );
        kit.icon_button(
            id + 1,
            Point::new(rect.right - pad - kit.px(6.0), top + pad + th / 2.0),
            glyph::CLOSE,
            "Dismiss",
        );
        kit.y += height + kit.px(8.0);
    }
}
