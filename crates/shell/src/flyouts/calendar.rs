//! The calendar: today in full, the time, and a month you can page through
//! (`mypaper_ui::calendar::MonthGrid`, shared with the calendar widget). Names come from
//! Windows' regional settings. The time redraws once a minute while it is open.

use std::time::{Duration, Instant};

use mypaper_bars::clock;
use mypaper_bars::config::ClockConfig;
use mypaper_bars::system;
use mypaper_ui::calendar::{Date, MonthGrid};
use mypaper_ui::{HitId, Point, Rect};
use windows::Win32::Foundation::SYSTEMTIME;
use windows::Win32::System::SystemInformation::GetLocalTime;

use super::kit::{Kit, glyph};
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

    /// The month grid and its title, as the calendar and the notification center show it.
    pub fn month(&self, kit: &mut Kit, buttons: bool) {
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
        let style = kit.font(14.0, 600);
        let height = kit.px(30.0);
        let (_, h) = kit.text.measure(&title, &style);
        let (left, y, room) = (kit.left, kit.y, kit.width() - kit.px(110.0));
        kit.text_at(
            left,
            y + (height - h) / 2.0,
            &title,
            &style,
            kit.look.ink,
            room,
        );
        if buttons {
            let center = y + height / 2.0;
            let right = kit.right;
            kit.icon_button(
                NEXT,
                Point::new(right - kit.px(15.0), center),
                glyph::CHEVRON_RIGHT,
                "Next month",
            );
            kit.icon_button(
                PREVIOUS,
                Point::new(right - kit.px(47.0), center),
                glyph::CHEVRON_LEFT,
                "Previous month",
            );
            if self.shown
                != (Date {
                    day: 1,
                    ..today(&self.now)
                })
            {
                let label = kit.font(12.0, 600);
                let (w, th) = kit.text.measure("Today", &label);
                let r = Rect::new(
                    right - kit.px(70.0) - w - kit.px(12.0),
                    y + kit.px(4.0),
                    w + kit.px(12.0),
                    height - kit.px(8.0),
                );
                kit.state(TODAY, r, kit.px(8.0));
                let accent = kit.look.accent;
                kit.text_at(
                    r.left + kit.px(6.0),
                    y + (height - th) / 2.0,
                    "Today",
                    &label,
                    accent,
                    w + 1.0,
                );
                kit.hit(TODAY, r, mypaper_ui::Role::Button, "Back to today", true);
            }
        }
        kit.y += height + kit.px(4.0);
        let grid = MonthGrid::new(self.shown, self.week_start);
        let rows = grid.cells.len() / 7 + 1;
        let rect = Rect::new(kit.left, kit.y, kit.width(), kit.px(32.0) * rows as f32);
        let font = kit.font(13.0, 400);
        let (ink, accent) = (kit.look.ink, kit.look.accent.with_alpha(0.55));
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
            &mut kit.list,
            rect,
            today(&self.now),
            ink,
            accent,
            DAYS,
            &font,
            &weekdays.each_ref().map(String::as_str),
        );
        kit.y = rect.bottom + kit.px(6.0);
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
        let small = kit.font(13.0, 400);
        let big = kit.font(17.0, 600);
        let clock = kit.font(28.0, 300);
        let (tw, th) = kit.text.measure(&time, &clock);
        let (left, y, room) = (kit.left, kit.y, kit.width() - tw - kit.px(12.0));
        let (_, h1) = kit.text_at(left, y, &weekday, &small, kit.look.dim(), room);
        let (_, h2) = kit.text_at(left, y + h1, &date, &big, kit.look.ink, room);
        let block = (h1 + h2).max(th);
        kit.text_at(
            kit.right - tw,
            y + (block - th) / 2.0,
            &time,
            &clock,
            kit.look.ink,
            tw + 1.0,
        );
        kit.y += block;
        kit.separator();
        self.month(kit, true);
        for zone in &self.zones {
            let label = zone.label.clone().unwrap_or_else(|| {
                zone.zone
                    .rsplit('/')
                    .next()
                    .unwrap_or(&zone.zone)
                    .replace('_', " ")
            });
            let Some((time, detail)) = zone_clock(&zone.zone, self.utc_ms, &self.now) else {
                continue;
            };
            let y = kit.y;
            let title = kit.font(13.0, 600);
            let small = kit.font(11.0, 400);
            let clock = kit.font(20.0, 400);
            let (w, h) = kit.text.measure(&time, &clock);
            let left = kit.left;
            let room = kit.width() - w - kit.px(12.0);
            kit.text_at(left, y, &label, &title, kit.look.ink, room);
            kit.text_at(
                left,
                y + kit.px(19.0),
                &detail,
                &small,
                kit.look.dim(),
                room,
            );
            kit.text_at(
                kit.right - w,
                y + (kit.px(44.0) - h) / 2.0,
                &time,
                &clock,
                kit.look.ink,
                w + 1.0,
            );
            kit.y += kit.px(44.0);
        }
        kit.footer_link(OUTLOOK, "Calendar");
        kit.footer_link(SETTINGS, "Date & time settings");
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
