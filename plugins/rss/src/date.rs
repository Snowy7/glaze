//! Hand-written date parsers for feeds: RFC 2822 (RSS `pubDate`) and RFC 3339 / W3C-DTF (Atom
//! `updated`, Dublin Core `dc:date`). Both return unix seconds (UTC).

/// Parses either format; feeds mix them up often enough that being strict about which element
/// carries which isn't worth it. `None` when it is neither.
pub fn parse_date(text: &str) -> Option<i64> {
    let text = text.trim();
    parse_rfc3339(text).or_else(|| parse_rfc2822(text))
}

/// Days since 1970-01-01 of a proleptic Gregorian date (Howard Hinnant's `days_from_civil`).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year.rem_euclid(400);
    let shifted_month = (month + 9) % 12; // March = 0
    let day_of_year = (153 * shifted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn is_leap(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        2 if is_leap(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Validates the fields and assembles unix seconds. `offset` is the zone's offset east of UTC in
/// seconds. A leap second (`60`) is folded into `59`.
fn assemble(
    (year, month, day): (i64, i64, i64),
    (hour, minute, second): (i64, i64, i64),
    offset: i64,
) -> Option<i64> {
    let valid = (1..=12).contains(&month)
        && (1..=days_in_month(year, month)).contains(&day)
        && (0..24).contains(&hour)
        && (0..60).contains(&minute)
        && (0..=60).contains(&second);
    valid.then(|| {
        days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second.min(59)
            - offset
    })
}

/// Parses a run of ASCII digits (and nothing else).
fn number(text: &str) -> Option<i64> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

// ------------------------------------------------------------------ RFC 2822

const MONTHS: [&str; 12] = [
    "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
];
const WEEKDAYS: [&str; 7] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];

/// `[Tue,] 3 Nov 2026 10:00[:00] +0100` with a numeric zone or one of GMT/UT/UTC/EST/EDT/CST/CDT/
/// MST/MDT/PST/PDT (anything else counts as UTC, like the single-letter military zones). Two and
/// three digit years are widened as RFC 2822 says (`00..=49` is 20xx, the rest 19xx).
pub fn parse_rfc2822(text: &str) -> Option<i64> {
    let text = text.replace(',', " ");
    let mut words = text.split_whitespace().peekable();

    let is_weekday = |word: &str| {
        word.get(..3)
            .is_some_and(|d| WEEKDAYS.iter().any(|w| w.eq_ignore_ascii_case(d)))
            && word.bytes().all(|b| b.is_ascii_alphabetic() || b == b'.')
    };
    if words.peek().is_some_and(|w| is_weekday(w)) {
        words.next();
    }

    let day = number(words.next()?)?;
    let month_word = words.next()?;
    let month = MONTHS.iter().position(|m| {
        month_word
            .get(..3)
            .is_some_and(|p| p.eq_ignore_ascii_case(m))
    })? as i64
        + 1;
    let year_word = words.next()?;
    let year = match (number(year_word)?, year_word.len()) {
        (y, 1..=2) if y < 50 => 2000 + y,
        (y, 1..=3) => 1900 + y,
        (y, _) => y,
    };

    let mut time = words.next()?.split(':');
    let hour = number(time.next()?)?;
    let minute = number(time.next()?)?;
    let second = time.next().map_or(Some(0), number)?;
    if time.next().is_some() {
        return None;
    }

    let offset = words.next().map_or(0, zone_offset);
    assemble((year, month, day), (hour, minute, second), offset)
}

/// Seconds east of UTC for an RFC 2822 zone.
fn zone_offset(zone: &str) -> i64 {
    if let Some(sign) = zone
        .strip_prefix('+')
        .map(|_| 1)
        .or_else(|| zone.strip_prefix('-').map(|_| -1))
    {
        let digits = &zone[1..];
        let (hours, minutes) = match digits.len() {
            4 => (&digits[..2], &digits[2..]),
            2 => (digits, "0"),
            _ => return 0,
        };
        return match (number(hours), number(minutes)) {
            (Some(h), Some(m)) => sign * (h * 3600 + m * 60),
            _ => 0,
        };
    }
    let hours = match zone.to_ascii_uppercase().as_str() {
        "EDT" => -4,
        "EST" | "CDT" => -5,
        "CST" | "MDT" => -6,
        "MST" | "PDT" => -7,
        "PST" => -8,
        _ => 0, // GMT, UT, UTC, military letters, junk
    };
    hours * 3600
}

// ------------------------------------------------------------------ RFC 3339

/// `2026-11-03T10:00:00[.123]Z`, with a `+01:00` / `+0100` / `+01` offset, a space instead of
/// `T`, no seconds (W3C-DTF), or a bare date. A missing zone means UTC.
pub fn parse_rfc3339(text: &str) -> Option<i64> {
    let (date, rest) = match text.split_once(['T', 't', ' ']) {
        Some((date, rest)) => (date, Some(rest.trim())),
        None => (text, None),
    };

    let mut parts = date.split('-');
    let year = number(parts.next().filter(|y| y.len() == 4)?)?;
    let month = number(parts.next().filter(|m| m.len() == 2)?)?;
    let day = number(parts.next().filter(|d| d.len() == 2)?)?;
    if parts.next().is_some() {
        return None;
    }

    let Some(rest) = rest else {
        return assemble((year, month, day), (0, 0, 0), 0);
    };

    // Split the time from the zone at the first `Z`, `+` or `-`.
    let zone_start = rest.find(['Z', 'z', '+', '-']);
    let (time, zone) = match zone_start {
        Some(i) => rest.split_at(i),
        None => (rest, ""),
    };

    let mut fields = time.split(':');
    let hour = number(fields.next()?)?;
    let minute = number(fields.next()?)?;
    let second = match fields.next() {
        None => 0,
        // Fractional seconds are dropped.
        Some(s) => number(s.split_once(['.', ',']).map_or(s, |(whole, _)| whole))?,
    };
    if fields.next().is_some() {
        return None;
    }

    let offset = match zone.as_bytes().first() {
        None | Some(b'Z' | b'z') => 0,
        Some(sign) => {
            let sign = if *sign == b'-' { -1 } else { 1 };
            let digits: String = zone[1..].chars().filter(|c| *c != ':').collect();
            let (hours, minutes) = match digits.len() {
                4 => (&digits[..2], &digits[2..]),
                2 => (digits.as_str(), "0"),
                _ => return None,
            };
            sign * (number(hours)? * 3600 + number(minutes)? * 60)
        }
    };
    assemble((year, month, day), (hour, minute, second), offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    // 2026-11-03T10:00:00Z, checked against `date -u -d @1793700000`.
    const NOV_3_10_UTC: i64 = 1_793_700_000;

    #[test]
    fn civil_days() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(1969, 12, 31), -1);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        assert_eq!(days_from_civil(2024, 2, 29), 19_782);
    }

    #[test]
    fn rfc2822_forms() {
        assert_eq!(
            parse_rfc2822("Tue, 03 Nov 2026 10:00:00 GMT"),
            Some(NOV_3_10_UTC)
        );
        // No weekday, single-digit day, no seconds, no zone.
        assert_eq!(parse_rfc2822("3 Nov 2026 10:00"), Some(NOV_3_10_UTC));
        // Numeric offsets move the instant the other way.
        assert_eq!(
            parse_rfc2822("Tue, 03 Nov 2026 11:00:00 +0100"),
            Some(NOV_3_10_UTC)
        );
        assert_eq!(
            parse_rfc2822("Tue, 03 Nov 2026 05:30:00 -0430"),
            Some(NOV_3_10_UTC)
        );
        // Names.
        for (zone, hour) in [
            ("UTC", 10),
            ("UT", 10),
            ("EST", 5),
            ("EDT", 6),
            ("CST", 4),
            ("CDT", 5),
            ("MST", 3),
            ("MDT", 4),
            ("PST", 2),
            ("PDT", 3),
        ] {
            assert_eq!(
                parse_rfc2822(&format!("Tue, 03 Nov 2026 {hour:02}:00:00 {zone}")),
                Some(NOV_3_10_UTC),
                "{zone}"
            );
        }
        // Two digit years, and a fully lowercase message.
        assert_eq!(parse_rfc2822("03 Nov 26 10:00 GMT"), Some(NOV_3_10_UTC));
        assert_eq!(parse_rfc2822("01 Jan 99 00:00 GMT"), Some(915_148_800));
        assert_eq!(
            parse_rfc2822("tue, 03 nov 2026 10:00:00 gmt"),
            Some(NOV_3_10_UTC)
        );
        // Leap day, and a leap second folded into the previous one.
        assert_eq!(
            parse_rfc2822("29 Feb 2024 00:00:00 GMT"),
            Some(1_709_164_800)
        );
        assert_eq!(
            parse_rfc2822("03 Nov 2026 10:00:60 GMT"),
            Some(NOV_3_10_UTC + 59)
        );
    }

    #[test]
    fn rfc2822_rejects_garbage() {
        for bad in [
            "",
            "yesterday",
            "Tue, 32 Nov 2026 10:00:00 GMT",
            "29 Feb 2026 10:00:00 GMT",
            "03 Foo 2026 10:00:00 GMT",
            "03 Nov 2026 25:00:00 GMT",
            "03 Nov 2026 10:61:00 GMT",
            "03 Nov 2026",
        ] {
            assert_eq!(parse_rfc2822(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn rfc3339_forms() {
        assert_eq!(parse_rfc3339("2026-11-03T10:00:00Z"), Some(NOV_3_10_UTC));
        assert_eq!(
            parse_rfc3339("2026-11-03T10:00:00.123456Z"),
            Some(NOV_3_10_UTC)
        );
        assert_eq!(
            parse_rfc3339("2026-11-03T11:00:00+01:00"),
            Some(NOV_3_10_UTC)
        );
        assert_eq!(
            parse_rfc3339("2026-11-03T05:30:00-04:30"),
            Some(NOV_3_10_UTC)
        );
        // Tolerated variations: lowercase, space separator, +hhmm, no seconds, no zone.
        assert_eq!(parse_rfc3339("2026-11-03t10:00:00z"), Some(NOV_3_10_UTC));
        assert_eq!(parse_rfc3339("2026-11-03 10:00:00Z"), Some(NOV_3_10_UTC));
        assert_eq!(
            parse_rfc3339("2026-11-03T11:00:00+0100"),
            Some(NOV_3_10_UTC)
        );
        assert_eq!(parse_rfc3339("2026-11-03T10:00Z"), Some(NOV_3_10_UTC));
        assert_eq!(parse_rfc3339("2026-11-03T10:00:00"), Some(NOV_3_10_UTC));
        assert_eq!(parse_rfc3339("2026-11-03"), Some(NOV_3_10_UTC - 36_000));
    }

    #[test]
    fn rfc3339_rejects_garbage() {
        for bad in [
            "",
            "2026-13-03T10:00:00Z",
            "2026-11-31T10:00:00Z",
            "2026-11-03T10:00:00+1",
            "26-11-03",
            "2026-11-03T10",
            "Tue, 03 Nov 2026 10:00:00 GMT",
        ] {
            assert_eq!(parse_rfc3339(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn parse_date_accepts_either_format() {
        assert_eq!(
            parse_date("  Tue, 03 Nov 2026 10:00:00 GMT "),
            Some(NOV_3_10_UTC)
        );
        assert_eq!(parse_date("2026-11-03T10:00:00Z"), Some(NOV_3_10_UTC));
        assert_eq!(parse_date("not a date"), None);
    }
}
