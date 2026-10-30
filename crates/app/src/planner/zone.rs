//! The place's time zone, found on this computer: the zone polygons of tzf-rs say which zone a point is in and the time zone
//! database of chrono-tz gives its offset on any day (summer time included). No network, no estimate, in any country.
//! Where the lookup gives nothing usable (it should not happen), the old estimate by boxes and longitude stands.

use chrono::{Offset, TimeZone};
use light_studio_astro::time::auto_utc_offset_min;
use std::sync::OnceLock;

/// Where an offset came from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Source {
    /// From the time zone database.
    Real,
    /// The rough estimate.
    Estimate,
}

fn finder() -> &'static tzf_rs::DefaultFinder {
    static FINDER: OnceLock<tzf_rs::DefaultFinder> = OnceLock::new();
    FINDER.get_or_init(tzf_rs::DefaultFinder::new)
}

/// The name of the zone of a place ("Europe/Madrid"), if the coordinates are valid and a zone is there.
pub fn zone_name(lat: f64, lon: f64) -> Option<&'static str> {
    if !(lat.is_finite() && lon.is_finite() && lat.abs() <= 90.0 && lon.abs() <= 180.0) {
        return None;
    }
    let name = finder().get_tz_name(lon, lat);
    (!name.is_empty()).then_some(name)
}

/// The offset in minutes east of UTC that a zone has at a Unix time.
fn zone_offset(name: &str, unix: f64) -> Option<i32> {
    if !unix.is_finite() {
        return None;
    }
    let tz: chrono_tz::Tz = name.parse().ok()?;
    let at = chrono::DateTime::from_timestamp(unix.round().clamp(-62_000_000_000.0, 253_000_000_000.0) as i64, 0)?;
    let secs = tz.offset_from_utc_datetime(&at.naive_utc()).fix().local_minus_utc();
    let min = secs / 60;
    (-720..=840).contains(&min).then_some(min)
}

/// The offset in force at `noon` (unix time of local noon on the day) for a place.
pub fn offset_at(lat: f64, lon: f64, noon: f64) -> (i32, Source) {
    match zone_name(lat, lon).and_then(|n| zone_offset(n, noon)) {
        Some(min) => (min, Source::Real),
        None => (auto_utc_offset_min(lat, lon, noon), Source::Estimate),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use light_studio_astro::time::Date;

    fn noon(y: i32, m: u32, d: u32) -> f64 {
        Date::new(y, m, d).unix_midnight_utc() + 43_200.0
    }

    #[test]
    fn places_get_their_real_zone_and_summer_time() {
        // (lat, lon, offset in January, offset in July)
        let cases = [
            (43.32, -1.98, 60, 120),      // Donostia
            (28.47, -16.25, 0, 60),       // Tenerife (Canary Islands: not Madrid's zone)
            (51.5, -0.12, 0, 60),         // London
            (40.76, -111.89, -420, -360), // Salt Lake City (the case the old estimate got wrong)
            (33.45, -112.07, -420, -420), // Phoenix: no summer time
            (21.3, -157.86, -600, -600),  // Honolulu
            (-33.87, 151.2, 660, 600),    // Sydney: summer in January
            (28.61, 77.2, 330, 330),      // Delhi: a half-hour zone
            (27.7, 85.3, 345, 345),       // Kathmandu: a 45-minute zone
            (-36.85, 174.76, 780, 720),   // Auckland
            (35.68, 139.7, 540, 540),     // Tokyo
        ];
        for (lat, lon, jan, jul) in cases {
            assert_eq!(offset_at(lat, lon, noon(2026, 1, 20)), (jan, Source::Real), "{lat},{lon} in January");
            assert_eq!(offset_at(lat, lon, noon(2026, 7, 20)), (jul, Source::Real), "{lat},{lon} in July");
        }
    }

    #[test]
    fn the_change_of_clocks_falls_on_the_right_day() {
        // Spain: the clocks went forward on 2027-03-28 and back on 2027-10-31 (last Sundays)
        assert_eq!(offset_at(43.3, -2.0, noon(2027, 3, 27)).0, 60);
        assert_eq!(offset_at(43.3, -2.0, noon(2027, 3, 28)).0, 120);
        assert_eq!(offset_at(43.3, -2.0, noon(2027, 10, 30)).0, 120);
        assert_eq!(offset_at(43.3, -2.0, noon(2027, 10, 31)).0, 60);
    }

    #[test]
    fn the_open_sea_and_hostile_numbers_never_panic() {
        for (lat, lon) in [(0.0, -30.0), (-80.0, 0.0), (89.9, 0.0), (90.0, 180.0), (f64::NAN, 0.0), (0.0, f64::INFINITY), (1.0e9, 1.0e9)] {
            let (off, _) = offset_at(lat, lon, noon(2026, 7, 1));
            assert!((-720..=840).contains(&off), "{lat},{lon}: {off}");
        }
        assert_eq!(offset_at(43.3, -2.0, f64::NAN).1, Source::Estimate);
        assert_eq!(zone_name(43.3, -2.0), Some("Europe/Madrid"));
    }
}
