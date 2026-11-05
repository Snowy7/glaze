//! Shared calendar month model and native view (desktop widget and calendar flyout).
use crate::{DisplayList, Hit, Op, Rect, Rgba, Role, TextStyle};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Date {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}
impl Date {
    pub fn new(year: i32, month: u32, day: u32) -> Option<Self> {
        ((1..=12).contains(&month) && day >= 1 && day <= days_in_month(year, month))
            .then_some(Self { year, month, day })
    }
    pub fn parse(value: &str) -> Option<Self> {
        let mut parts = value.split('-');
        let date = Self::new(
            parts.next()?.parse().ok()?,
            parts.next()?.parse().ok()?,
            parts.next()?.parse().ok()?,
        )?;
        parts.next().is_none().then_some(date)
    }
    pub fn key(self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
    /// Days since 1970-01-01; civil arithmetic avoids DST errors.
    pub fn ordinal(self) -> i64 {
        let y = i64::from(self.year) - i64::from(self.month <= 2);
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let m = i64::from(self.month) + if self.month > 2 { -3 } else { 9 };
        let doy = (153 * m + 2) / 5 + i64::from(self.day) - 1;
        era * 146097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719468
    }
    pub fn from_ordinal(day: i64) -> Self {
        let z = day + 719468;
        let era = z.div_euclid(146097);
        let doe = z - era * 146097;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = mp + if mp < 10 { 3 } else { -9 };
        Self {
            year: (y + i64::from(m <= 2)) as i32,
            month: m as u32,
            day: d as u32,
        }
    }
    pub fn weekday(self) -> u32 {
        (self.ordinal() + 4).rem_euclid(7) as u32
    }
    /// ISO week number; the week containing January 4 is week one.
    pub fn iso_week(self) -> u32 {
        let thursday = self.add_days(3 - i64::from((self.weekday() + 6) % 7));
        let jan4 = Self {
            year: thursday.year,
            month: 1,
            day: 4,
        };
        let first = jan4.add_days(-i64::from((jan4.weekday() + 6) % 7));
        ((thursday.ordinal() - first.ordinal()) / 7 + 1) as u32
    }
    pub fn add_days(self, days: i64) -> Self {
        Self::from_ordinal(self.ordinal() + days)
    }
    pub fn shift_month(self, delta: i32) -> Self {
        let month = self.year * 12 + self.month as i32 - 1 + delta;
        Self {
            year: month.div_euclid(12),
            month: (month.rem_euclid(12) + 1) as u32,
            day: 1,
        }
    }
}
pub fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        2 => {
            if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) {
                29
            } else {
                28
            }
        }
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}
pub const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
pub const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

#[derive(Debug, Clone, PartialEq)]
pub struct MonthGrid {
    pub month: Date,
    pub week_start: u32,
    pub cells: Vec<Option<Date>>,
}
impl MonthGrid {
    pub fn new(month: Date, week_start: u32) -> Self {
        let month = Date { day: 1, ..month };
        let week_start = week_start % 7;
        let lead = (month.weekday() + 7 - week_start) % 7;
        let count = (lead + days_in_month(month.year, month.month)).div_ceil(7) * 7;
        let cells = (0..count)
            .map(|i| {
                if i < lead {
                    None
                } else {
                    Date::new(month.year, month.month, i - lead + 1)
                }
            })
            .collect();
        Self {
            month,
            week_start,
            cells,
        }
    }
    /// Appends the grid. Hit ids `base + day` let each consumer own selection and agenda storage.
    #[allow(
        clippy::too_many_arguments,
        reason = "the month view takes independent style and hit-id inputs shared by the flyout and widget"
    )]
    pub fn view(
        &self,
        list: &mut DisplayList,
        rect: Rect,
        today: Date,
        text: Rgba,
        accent: Rgba,
        base: u32,
        font: &TextStyle,
    ) {
        self.view_with_weekdays(list, rect, today, text, accent, base, font, &WEEKDAYS);
    }

    /// Same calendar with Sunday-first localized weekday names supplied by the host.
    #[allow(clippy::too_many_arguments)]
    pub fn view_with_weekdays(
        &self,
        list: &mut DisplayList,
        rect: Rect,
        today: Date,
        text: Rgba,
        accent: Rgba,
        base: u32,
        font: &TextStyle,
        weekdays: &[&str; 7],
    ) {
        let width = rect.width() / 7.0;
        let rows = self.cells.len() / 7;
        let height = rect.height() / (rows + 1) as f32;
        for col in 0..7 {
            label(
                list,
                Rect::new(rect.left + col as f32 * width, rect.top, width, height),
                weekdays[(col + self.week_start as usize) % 7],
                font,
                text.with_alpha(0.55),
            );
        }
        for (index, date) in self.cells.iter().enumerate() {
            let Some(date) = date else { continue };
            let cell = Rect::new(
                rect.left + (index % 7) as f32 * width,
                rect.top + (index / 7 + 1) as f32 * height,
                width,
                height,
            );
            if *date == today {
                list.ops.push(Op::Fill {
                    rect: cell.inflate(-2.0),
                    radius: height.min(width) * 0.35,
                    color: accent,
                });
            }
            label(list, cell, &date.day.to_string(), font, text);
            list.hits.push(Hit {
                id: base + date.day,
                rect: cell,
                role: Role::Button,
                label: date.key().into(),
                enabled: true,
                checked: Some(*date == today),
            });
        }
    }
}
fn label(list: &mut DisplayList, rect: Rect, text: &str, font: &TextStyle, color: Rgba) {
    list.ops.push(Op::Text {
        rect: Rect::new(
            rect.left + rect.width() * 0.25,
            rect.top + (rect.height() - font.size * 1.4) / 2.0,
            rect.width(),
            font.size * 1.4,
        ),
        text: text.into(),
        style: font.clone(),
        color,
    });
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn civil_roundtrip_and_leap_months() {
        for day in -100_000..100_000 {
            assert_eq!(Date::from_ordinal(day).ordinal(), day);
        }
        assert_eq!(Date::new(2024, 2, 29).unwrap().weekday(), 4);
        assert_eq!(Date::parse("2026-02-29"), None);
        assert_eq!(Date::new(2021, 1, 1).unwrap().iso_week(), 53);
        assert_eq!(Date::new(2021, 1, 4).unwrap().iso_week(), 1);
        let grid = MonthGrid::new(Date::new(2026, 11, 1).unwrap(), 1);
        assert_eq!(grid.cells.len(), 42);
        assert_eq!(grid.cells[6].unwrap().day, 1);
    }
}
