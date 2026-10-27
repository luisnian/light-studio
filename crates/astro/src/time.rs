//! Calendar and clock helpers: Julian day, civil dates, and a best-effort UTC offset.
//!
//! Sources: Julian day from the Unix epoch (JD 2440587.5, textbook); civil date <-> day count
//! (Howard Hinnant's public-domain `days_from_civil` / `civil_from_days` algorithms).

/// Seconds in a day.
pub const DAY: f64 = 86_400.0;

/// Julian day of a Unix time (seconds, UTC).
pub fn jd_from_unix(unix: f64) -> f64 {
    unix / DAY + 2_440_587.5
}

/// A calendar date (proleptic Gregorian).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

impl Date {
    /// A date, with the month and day brought into range.
    pub fn new(year: i32, month: u32, day: u32) -> Date {
        let month = month.clamp(1, 12);
        Date { year, month, day: day.clamp(1, days_in_month(year, month)) }
    }

    /// Days since 1970-01-01.
    pub fn days(self) -> i64 {
        days_from_civil(self.year, self.month, self.day)
    }

    pub fn from_days(days: i64) -> Date {
        let (year, month, day) = civil_from_days(days);
        Date { year, month, day }
    }

    pub fn add_days(self, n: i64) -> Date {
        Date::from_days(self.days().saturating_add(n))
    }

    /// 0 = Sunday … 6 = Saturday.
    pub fn weekday(self) -> u32 {
        // 1970-01-01 was a Thursday
        (self.days() + 4).rem_euclid(7) as u32
    }

    /// Day of the year, 1-based.
    pub fn ordinal(self) -> u32 {
        (self.days() - days_from_civil(self.year, 1, 1) + 1) as u32
    }

    /// `YYYY-MM-DD`.
    pub fn iso(self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }

    /// Parses `YYYY-MM-DD` (also `/` and `.` as separators).
    pub fn parse(text: &str) -> Option<Date> {
        let mut parts = text.trim().split(['-', '/', '.']);
        let year: i32 = parts.next()?.trim().parse().ok()?;
        let month: u32 = parts.next()?.trim().parse().ok()?;
        let day: u32 = parts.next()?.trim().parse().ok()?;
        if parts.next().is_some() || !(1..=12).contains(&month) || day == 0 || day > days_in_month(year, month) || !(1..=9999).contains(&year) {
            return None;
        }
        Some(Date { year, month, day })
    }

    /// The Unix time of 00:00 UTC on this date.
    pub fn unix_midnight_utc(self) -> f64 {
        self.days() as f64 * DAY
    }

    /// The date of a Unix time (UTC) shifted by `offset_min` minutes.
    pub fn from_unix(unix: f64, offset_min: i32) -> Date {
        let local = unix + f64::from(offset_min) * 60.0;
        if !local.is_finite() {
            return Date::new(1970, 1, 1);
        }
        Date::from_days((local / DAY).floor() as i64)
    }
}

pub fn is_leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

pub fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if is_leap(year) => 29,
        _ => 28,
    }
}

/// Days since 1970-01-01 of a civil date.
pub fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let y = i64::from(year) - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (i64::from(month) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The civil date `days` after 1970-01-01.
pub fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    ((y + i64::from(m <= 2)) as i32, m as u32, d as u32)
}

/// The Unix time of a UTC date and time of day.
pub fn unix_from_civil(year: i32, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> f64 {
    days_from_civil(year, month, day) as f64 * DAY + f64::from(hour) * 3600.0 + f64::from(minute) * 60.0 + f64::from(second)
}

fn last_sunday(year: i32, month: u32) -> u32 {
    let last = days_in_month(year, month);
    (1..=last).rev().find(|&d| Date { year, month, day: d }.weekday() == 0).unwrap_or(last)
}

/// European summer time: from 01:00 UTC on the last Sunday of March to 01:00 UTC on the last Sunday of October.
pub fn eu_summer_time(unix: f64) -> bool {
    let year = Date::from_unix(unix, 0).year;
    let start = unix_from_civil(year, 3, last_sunday(year, 3), 1, 0, 0);
    let end = unix_from_civil(year, 10, last_sunday(year, 10), 1, 0, 0);
    unix >= start && unix < end
}

/// A best-effort UTC offset in minutes for a place and moment, for people who haven't set one:
/// Europe by its three clock zones (with summer time), elsewhere the nearest hour of longitude.
/// Borders are approximated by boxes, so on a border the person corrects it by hand.
pub fn auto_utc_offset_min(lat: f64, lon: f64, unix: f64) -> i32 {
    if !(lat.is_finite() && lon.is_finite()) {
        return 0;
    }
    let dst = if eu_summer_time(unix) { 60 } else { 0 };
    // Canary Islands and Madeira
    if (27.0..=29.6).contains(&lat) && (-18.5..=-13.0).contains(&lon) || (32.3..=33.2).contains(&lat) && (-17.4..=-16.2).contains(&lon) {
        return dst;
    }
    // Iceland
    if (63.0..=66.7).contains(&lat) && (-24.6..=-13.0).contains(&lon) {
        return 0;
    }
    let in_europe = (34.8..=71.5).contains(&lat) && (-10.8..=40.0).contains(&lon);
    if in_europe {
        let uk_ie = lat >= 49.8 && (-10.8..=1.9).contains(&lon) && (lat > 51.2 || lon < 1.3);
        let portugal = (36.8..=42.2).contains(&lat) && lon < -7.25;
        if uk_ie || portugal {
            return dst;
        }
        // Eastern Europe (Finland, the Baltics, Romania, Bulgaria, Greece, Ukraine…): one hour ahead of central
        if lon > 22.0 && lat > 41.0 || lon > 23.5 {
            return 120 + dst;
        }
        return 60 + dst;
    }
    ((lon / 15.0).round() as i32).clamp(-12, 14) * 60
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_dates_round_trip() {
        for days in (-800_000..800_000).step_by(997) {
            let (y, m, d) = civil_from_days(days);
            assert_eq!(days_from_civil(y, m, d), days, "{y}-{m}-{d}");
        }
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        assert_eq!(civil_from_days(19_723), (2024, 1, 1));
    }

    #[test]
    fn weekdays_and_ordinals() {
        assert_eq!(Date::new(2024, 1, 1).weekday(), 1, "a Monday");
        assert_eq!(Date::new(1970, 1, 1).weekday(), 4, "a Thursday");
        assert_eq!(Date::new(2024, 12, 31).ordinal(), 366);
        assert_eq!(Date::new(2023, 12, 31).ordinal(), 365);
    }

    #[test]
    fn dates_are_clamped_and_parsed() {
        assert_eq!(Date::new(2023, 2, 31), Date::new(2023, 2, 28));
        assert_eq!(Date::new(2024, 13, 0), Date::new(2024, 12, 1));
        assert_eq!(Date::parse("2024-06-21"), Some(Date::new(2024, 6, 21)));
        assert_eq!(Date::parse(" 2024/6/21 "), Some(Date::new(2024, 6, 21)));
        assert_eq!(Date::parse("2023-02-29"), None);
        assert_eq!(Date::parse("nonsense"), None);
        assert_eq!(Date::parse("2024-06-21-1"), None);
        assert_eq!(Date::new(2024, 2, 28).add_days(2), Date::new(2024, 3, 1));
        assert_eq!(Date::new(2024, 1, 1).add_days(-1), Date::new(2023, 12, 31));
    }

    #[test]
    fn julian_day_of_the_j2000_epoch() {
        // 2000-01-01 12:00 UTC is JD 2451545.0
        assert!((jd_from_unix(unix_from_civil(2000, 1, 1, 12, 0, 0)) - 2_451_545.0).abs() < 1e-9);
    }

    #[test]
    fn european_summer_time_changes_on_the_last_sundays() {
        // 2024: 31 March and 27 October
        assert!(!eu_summer_time(unix_from_civil(2024, 3, 31, 0, 59, 0)));
        assert!(eu_summer_time(unix_from_civil(2024, 3, 31, 1, 1, 0)));
        assert!(eu_summer_time(unix_from_civil(2024, 10, 27, 0, 59, 0)));
        assert!(!eu_summer_time(unix_from_civil(2024, 10, 27, 1, 1, 0)));
    }

    #[test]
    fn automatic_offsets() {
        let winter = unix_from_civil(2024, 1, 15, 12, 0, 0);
        let summer = unix_from_civil(2024, 7, 15, 12, 0, 0);
        assert_eq!(auto_utc_offset_min(40.4, -3.7, winter), 60, "Madrid in winter");
        assert_eq!(auto_utc_offset_min(40.4, -3.7, summer), 120, "Madrid in summer");
        assert_eq!(auto_utc_offset_min(51.5, -0.1, summer), 60, "London in summer");
        assert_eq!(auto_utc_offset_min(38.7, -9.1, winter), 0, "Lisbon in winter");
        assert_eq!(auto_utc_offset_min(28.3, -16.6, winter), 0, "Tenerife in winter");
        assert_eq!(auto_utc_offset_min(60.2, 24.9, winter), 120, "Helsinki");
        assert_eq!(auto_utc_offset_min(40.7, -74.0, winter), -300, "New York by longitude");
        assert_eq!(auto_utc_offset_min(f64::NAN, 0.0, winter), 0);
    }
}
