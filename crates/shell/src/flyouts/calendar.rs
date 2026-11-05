//! The calendar: today in full, the time, and a month you can page through
//! (`mypaper_ui::calendar::MonthGrid`, shared with the calendar widget). Names come from
//! Windows' regional settings. The time redraws once a minute while it is open.

use std::time::{Duration, Instant};

use mypaper_bars::clock;
use mypaper_bars::config::ClockConfig;
use mypaper_bars::system;
use mypaper_ui::calendar::{Date, MonthGrid};
use mypaper_ui::icons::Icon;
use mypaper_ui::layout::{Align, Stack};
use mypaper_ui::{ButtonKind, Frame, HitId, Rect, TextRole};
use windows::Win32::Foundation::SYSTEMTIME;
use windows::Win32::System::SystemInformation::GetLocalTime;

use super::kit::{Kit, footer, link};
use super::{Cx, Effect, Panel};

const PREVIOUS: HitId = 1;
const NEXT: HitId = 2;
const TODAY: HitId = 3;
const SETTINGS: HitId = 4;
const OUTLOOK: HitId = 5;
/// Day cells: `DAYS + day` (MonthGrid's own ids).
const DAYS: HitId = 100;

pub struct Calendar {
    now: SYSTEMTIME,
    /// The month shown (day 1).
    shown: Date,
    week_start: u32,
    zones: Vec<mypaper_bars::config::Zone>,
    utc_ms: i64,
}

impl Calendar {
    pub fn new(cx: &mut Cx) -> Self {
        // SAFETY: a read-only clock query.
        let mut calendar = Self::at(unsafe { GetLocalTime() }, cx.look.first_day);
        calendar.zones = cx.bars.config().calendar.zones.clone();
        calendar
    }

    /// The calendar at `now` (tests use a fixed day).
    pub fn at(now: SYSTEMTIME, first_day: Option<u32>) -> Self {
        let today = today(&now);
        Self {
            now,
            shown: Date { day: 1, ..today },
            week_start: first_day.unwrap_or_else(locale_first_day),
            zones: vec![],
            utc_ms: mypaper_bars::flyouts::index::epoch_ms() as i64,
        }
    }

    #[cfg(test)]
    pub fn with_zones(
        now: SYSTEMTIME,
        utc_ms: i64,
        zones: Vec<mypaper_bars::config::Zone>,
    ) -> Self {
        let mut calendar = Self::at(now, Some(1));
        calendar.utc_ms = utc_ms;
        calendar.zones = zones;
        calendar
    }

    /// The month grid and its title from the top of `area`, as the calendar and the
    /// notification center show it. The bottom of what it drew.
    pub fn month(&self, f: &mut Frame, area: Rect, buttons: bool) -> f32 {
        let c = f.theme.color;
        let title = clock::format(
            &ClockConfig {
                seconds: false,
                format: Some("MMMM yyyy".into()),
            },
            &SYSTEMTIME {
                wYear: self.shown.year as u16,
                wMonth: self.shown.month as u16,
                wDay: 1,
                ..self.now
            },
        );
        let (head, rest) = area.take_top(f.px(30.0), f.px(4.0));
        let mut label = head;
        if buttons {
            for (id, icon, name) in [
                (NEXT, Icon::ChevronRight, "Next month"),
                (PREVIOUS, Icon::ChevronLeft, "Previous month"),
            ] {
                let (button, left) = label.take_right(f.px(28.0), f.px(2.0));
                f.icon_button(id, button, icon, 28.0, name, false);
                label = left;
            }
            if self.shown
                != (Date {
                    day: 1,
                    ..today(&self.now)
                })
            {
                let (w, _) = f.button_size("Today", false);
                let (button, left) = label.take_right(w, f.px(4.0));
                let button = button.place(w, f.px(26.0), Align::End, Align::Center);
                f.button(TODAY, button, "Today", None, ButtonKind::Plain, true);
                label = left;
            }
        }
        f.text_in(label, &title, TextRole::Title3, c.label, Align::Start);
        let grid = MonthGrid::new(self.shown, self.week_start);
        let rows = grid.cells.len() / 7 + 1;
        let rect = Rect::new(rest.left, rest.top, rest.width(), f.px(32.0) * rows as f32);
        let font = f.font(TextRole::Callout);
        let (ink, accent) = (c.label, c.accent.with_alpha(0.55));
        let weekdays: [String; 7] = std::array::from_fn(|i| {
            clock::format(
                &ClockConfig {
                    seconds: false,
                    format: Some("ddd".into()),
                },
                &SYSTEMTIME {
                    wYear: 2026,
                    wMonth: 2,
                    wDay: i as u16 + 1,
                    wDayOfWeek: i as u16,
                    ..Default::default()
                },
            )
        });
        grid.view_with_weekdays(
            &mut f.list,
            rect,
            today(&self.now),
            ink,
            accent,
            DAYS,
            &font,
            &weekdays.each_ref().map(String::as_str),
        );
        rect.bottom
    }

    /// Pages the month: `PREVIOUS`, `NEXT` or `TODAY`.
    pub fn page(&mut self, id: HitId) -> bool {
        self.shown = match id {
            PREVIOUS => self.shown.shift_month(-1),
            NEXT => self.shown.shift_month(1),
            TODAY => Date {
                day: 1,
                ..today(&self.now)
            },
            _ => return false,
        };
        true
    }

    pub fn wheel_page(&mut self, delta: i16) {
        self.page(if delta > 0 { PREVIOUS } else { NEXT });
    }

    /// Follows the clock: whether the minute changed, and the next minute's start.
    pub fn follow_clock(&mut self, now: Instant) -> (bool, Option<Instant>) {
        // SAFETY: a read-only clock query.
        let local = unsafe { GetLocalTime() };
        let changed = (local.wMinute, local.wHour, local.wDay)
            != (self.now.wMinute, self.now.wHour, self.now.wDay);
        self.now = local;
        self.utc_ms = mypaper_bars::flyouts::index::epoch_ms() as i64;
        let left = 60_000 - (u64::from(local.wSecond) * 1000 + u64::from(local.wMilliseconds));
        (changed, Some(now + Duration::from_millis(left + 5)))
    }
}

impl Panel for Calendar {
    fn width(&self) -> f32 {
        300.0
    }

    fn view(&self, kit: &mut Kit) {
        let format = |pattern: &str| {
            clock::format(
                &ClockConfig {
                    seconds: false,
                    format: Some(pattern.into()),
                },
                &self.now,
            )
        };
        let weekday = format("dddd");
        let date = format("d MMMM yyyy");
        let time = format("HH:mm");
        let zones: Vec<_> = self
            .zones
            .iter()
            .filter_map(|zone| {
                let label = zone.label.clone().unwrap_or_else(|| {
                    zone.zone
                        .rsplit('/')
                        .next()
                        .unwrap_or(&zone.zone)
                        .replace('_', " ")
                });
                let (time, detail) = zone_clock(&zone.zone, self.utc_ms, &self.now)?;
                Some((label, time, detail))
            })
            .collect();
        kit.frame(|f, area| {
            let c = f.theme.color;
            let mut column = Stack::column(area, f.px(6.0));
            // Today: the weekday over the date, the time at the right.
            let head = column.next(f.line(TextRole::Callout) + f.line(TextRole::Title2));
            let style = f.font(TextRole::LargeTitle);
            let (w, _) = f.text.measure(&time, &style);
            let (clock, text) = head.take_right(w.ceil(), f.px(12.0));
            f.text_in(clock, &time, TextRole::LargeTitle, c.label, Align::End);
            let (top, bottom) = text.take_top(f.line(TextRole::Callout), 0.0);
            f.text_in(top, &weekday, TextRole::Callout, c.secondary, Align::Start);
            f.text_in(bottom, &date, TextRole::Title2, c.label, Align::Start);
            let line = column.next(f.px(9.0));
            f.separator(line.left, line.right, line.top + f.px(4.0));
            let bottom = self.month(f, column.rest(), true);
            column.next(bottom - column.rest().top);
            // Other clocks, one card.
            if !zones.is_empty() {
                let row = f.px(f.theme.metrics.row);
                let pad = f.px(f.theme.metrics.pad);
                let card = column.next(zones.len() as f32 * row + pad);
                let inner = f.card(card).inflate(pad / 2.0);
                let mut rows = Stack::column(Rect { top: card.top + pad / 2.0, ..inner }, 0.0);
                for (label, time, detail) in &zones {
                    let r = rows.next(row);
                    let style = f.font(TextRole::Title1);
                    let (w, _) = f.text.measure(time, &style);
                    let (clock, text) = r.take_right(w.ceil(), f.px(12.0));
                    f.text_in(clock, time, TextRole::Title1, c.label, Align::End);
                    let (top, bottom) = text.inflate(-f.px(4.0)).take_top(f.line(TextRole::Headline), 0.0);
                    f.text_in(top, label, TextRole::Headline, c.label, Align::Start);
                    f.text_in(bottom, detail, TextRole::Footnote, c.secondary, Align::Start);
                }
            }
            footer(f, &mut column, OUTLOOK, "Calendar");
            link(f, &mut column, SETTINGS, "Date & time settings");
            area.top + column.used()
        });
    }

    fn activate(&mut self, id: HitId, _: &mut Cx) -> Effect {
        if self.page(id) {
            return Effect::Redraw;
        }
        if id == SETTINGS {
            system::open("ms-settings:dateandtime".into());
            return Effect::Close;
        }
        if id == OUTLOOK {
            system::open("outlookcal:".into());
            return Effect::Close;
        }
        Effect::None
    }

    fn wheel(&mut self, _: Option<HitId>, delta: i16, _: &mut Cx) -> Effect {
        self.wheel_page(delta);
        Effect::Redraw
    }

    fn tick(&mut self, now: Instant, _: &mut Cx) -> (bool, Option<Instant>) {
        self.follow_clock(now)
    }
}

pub fn today(now: &SYSTEMTIME) -> Date {
    Date::new(
        i32::from(now.wYear),
        u32::from(now.wMonth),
        u32::from(now.wDay),
    )
    .unwrap_or(Date {
        year: 2026,
        month: 1,
        day: 1,
    })
}

/// Converts one instant through Windows' IANA database, including DST and fractional offsets.
pub fn zone_clock(zone: &str, utc_ms: i64, local: &SYSTEMTIME) -> Option<(String, String)> {
    let cal = windows::Globalization::Calendar::new().ok()?;
    cal.ChangeTimeZone(&windows::core::HSTRING::from(zone))
        .ok()?;
    cal.ChangeClock(windows::core::h!("24HourClock")).ok()?;
    cal.SetDateTime(windows::Foundation::DateTime {
        UniversalTime: (utc_ms + 11_644_473_600_000) * 10_000,
    })
    .ok()?;
    let date = Date::new(
        cal.Year().ok()?,
        cal.Month().ok()? as u32,
        cal.Day().ok()? as u32,
    )?;
    let (hour, minute) = (cal.Hour().ok()?, cal.Minute().ok()?);
    let offset = (date.ordinal() - today(local).ordinal()) * 1440 + i64::from(hour * 60 + minute)
        - i64::from(local.wHour * 60 + local.wMinute);
    let time = format!("{hour:02}:{minute:02}");
    let sign = if offset < 0 { "−" } else { "+" };
    let detail = format!(
        "{} · {sign}{}:{:02}",
        ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"][date.weekday() as usize],
        offset.abs() / 60,
        offset.abs() % 60
    );
    Some((time, detail))
}

/// The locale's first day of the week (`LOCALE_IFIRSTDAYOFWEEK`: 0 is Monday), Sunday = 0.
fn locale_first_day() -> u32 {
    use windows::Win32::Globalization::{GetLocaleInfoEx, LOCALE_IFIRSTDAYOFWEEK};
    use windows::core::PCWSTR;
    let mut buffer = [0u16; 4];
    // SAFETY: the user's locale (null), a writable local buffer.
    let len = unsafe { GetLocaleInfoEx(PCWSTR::null(), LOCALE_IFIRSTDAYOFWEEK, Some(&mut buffer)) };
    let monday_based = (len > 1)
        .then(|| String::from_utf16_lossy(&buffer[..len as usize - 1]))
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0);
    (monday_based + 1) % 7
}

#[cfg(test)]
mod tests {
    use super::*;
    fn utc(year: u16, month: u16, day: u16, hour: u16, minute: u16) -> (SYSTEMTIME, i64) {
        let local = SYSTEMTIME {
            wYear: year,
            wMonth: month,
            wDay: day,
            wHour: hour,
            wMinute: minute,
            ..Default::default()
        };
        let ms = today(&local).ordinal() * 86_400_000 + i64::from(hour * 60 + minute) * 60_000;
        (local, ms)
    }
    #[test]
    fn extra_clocks_follow_dst_fractional_offsets_and_date_rollover() {
        let (local, ms) = utc(2026, 6, 1, 12, 0);
        assert_eq!(
            zone_clock("America/New_York", ms, &local).unwrap().0,
            "08:00"
        );
        let (local, ms) = utc(2026, 11, 4, 9, 41);
        let (time, detail) = zone_clock("America/New_York", ms, &local).unwrap();
        assert_eq!(time, "04:41");
        assert!(detail.ends_with("5:00"));
        let (time, detail) = zone_clock("Asia/Kathmandu", ms, &local).unwrap();
        assert_eq!(time, "15:26");
        assert!(detail.ends_with("+5:45"));
        let (local, ms) = utc(2026, 11, 4, 23, 30);
        assert_eq!(
            zone_clock("Asia/Tokyo", ms, &local).unwrap(),
            ("08:30".into(), "Thu · +9:00".into())
        );
        assert!(zone_clock("Not/AZone", ms, &local).is_none());
    }
}
