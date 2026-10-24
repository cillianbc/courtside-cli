//! Calendar dates without a date-time dependency.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    pub year: i32,
    pub month: u8,
    pub day: u8,
}

impl Date {
    pub fn new(year: i32, month: u8, day: u8) -> Option<Self> {
        let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
        let days = match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if leap => 29,
            2 => 28,
            _ => return None,
        };
        (1..=days)
            .contains(&day)
            .then_some(Date { year, month, day })
    }

    /// From a spreadsheet date serial (day 0 = 1899-12-30), as DataVolley
    /// writes on the second `[3MATCH]` line.
    pub fn from_serial(serial: i64) -> Option<Self> {
        const UNIX_EPOCH_SERIAL: i64 = 25_569;
        Self::from_unix_days(serial - UNIX_EPOCH_SERIAL)
    }

    /// Parses a written date strictly: `YYYY/MM/DD`, or `A/B/YYYY` with
    /// one- or two-digit `A` and `B`, all with the same separator (`/`,
    /// `.` or `-`). Which of `A` and `B` is the month is the writing PC's
    /// setting, so `A/B/YYYY` can give two dates.
    pub fn parse_written(text: &str) -> Option<Written> {
        let text = text.trim();
        let sep = text.chars().find(|c| matches!(c, '/' | '.' | '-'))?;
        let parts: Vec<&str> = text.split(sep).collect();
        let [a, b, c] = parts[..] else { return None };
        if !parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
        {
            return None;
        }
        if a.len() == 4 && b.len() <= 2 && c.len() <= 2 {
            return Date::new(a.parse().ok()?, b.parse().ok()?, c.parse().ok()?).map(Written::One);
        }
        if c.len() != 4 || a.len() > 2 || b.len() > 2 {
            return None;
        }
        let (a, b, year): (u8, u8, i32) = (a.parse().ok()?, b.parse().ok()?, c.parse().ok()?);
        match (Date::new(year, a, b), Date::new(year, b, a)) {
            (Some(m), Some(d)) if m != d => Some(Written::Two {
                month_first: m,
                day_first: d,
            }),
            (Some(m), _) => Some(Written::One(m)),
            (None, Some(d)) => Some(Written::One(d)),
            (None, None) => None,
        }
    }

    /// Days since 1970-01-01 (Hinnant's `days_from_civil`).
    pub fn to_unix_days(self) -> i64 {
        let y = i64::from(self.year) - i64::from(self.month <= 2);
        let era = y.div_euclid(400);
        let yoe = y.rem_euclid(400);
        let m = i64::from(self.month);
        let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(self.day) - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    /// Days since 1970-01-01 to a date (Hinnant's `civil_from_days`).
    fn from_unix_days(days: i64) -> Option<Self> {
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = doy - (153 * mp + 2) / 5 + 1;
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        let year = yoe + era * 400 + i64::from(month <= 2);
        Date::new(
            i32::try_from(year).ok()?,
            u8::try_from(month).ok()?,
            u8::try_from(day).ok()?,
        )
    }
}

/// A date as written, see [`Date::parse_written`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Written {
    One(Date),
    /// Both orders give a valid, different date.
    Two {
        month_first: Date,
        day_first: Date,
    },
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates() {
        assert!(Date::new(2024, 2, 29).is_some());
        assert!(Date::new(2023, 2, 29).is_none());
        assert!(Date::new(2026, 13, 1).is_none());
        assert!(Date::new(2026, 4, 31).is_none());
    }

    #[test]
    fn written_dates() {
        let d = |y, m, day| Date::new(y, m, day).unwrap();
        assert_eq!(
            Date::parse_written("23/03/2024"),
            Some(Written::One(d(2024, 3, 23)))
        );
        assert_eq!(
            Date::parse_written("2024/03/23"),
            Some(Written::One(d(2024, 3, 23)))
        );
        assert_eq!(
            Date::parse_written("08/01/2026"),
            Some(Written::Two {
                month_first: d(2026, 8, 1),
                day_first: d(2026, 1, 8)
            })
        );
        assert_eq!(
            Date::parse_written("05/05/2026"),
            Some(Written::One(d(2026, 5, 5)))
        );
        for bad in [
            "",
            "1/2",
            "01/02/26",
            "01/02-2026",
            "13/13/2026",
            "a1/02/2026",
            "001/02/2026",
        ] {
            assert_eq!(Date::parse_written(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn unix_days_round_trip() {
        for days in [-25_569, -1, 0, 59, 19_000, 20_665] {
            let date = Date::from_unix_days(days).unwrap();
            assert_eq!(date.to_unix_days(), days);
        }
    }

    #[test]
    fn serials_seen_in_corpus() {
        assert_eq!(Date::from_serial(46_234), Date::new(2026, 7, 31));
        assert_eq!(Date::from_serial(45_317), Date::new(2024, 1, 26));
        assert_eq!(Date::from_serial(25_569), Date::new(1970, 1, 1));
        assert_eq!(Date::from_serial(0), Date::new(1899, 12, 30));
    }
}
