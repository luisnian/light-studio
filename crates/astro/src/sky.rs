//! The night sky beyond the Sun and Moon: the Milky Way's core and the meteor showers.
//!
//! The core is the Galactic Centre (Sagittarius A*), whose J2000 position is a published
//! constant. Meteor showers are listed by their IMO-calendar peak, activity span, typical hourly
//! rate and radiant: approximate figures, enough to plan a night, not an ephemeris.
//!
//! Sources: Galactic Centre J2000 RA 17h45m37.2s, Dec -28d56m10s (Reid & Brunthaler 2004, IAU
//! Galactic coordinate system); annual precession rates (Meeus ch. 21, formulas); shower data from
//! the International Meteor Organization's published calendar (facts).

use crate::sun::{Horizontal, Observer, centuries, to_horizontal};
use crate::time::{Date, jd_from_unix};

/// The Galactic Centre at J2000, degrees.
pub const CORE_RA_J2000: f64 = 266.404_99;
pub const CORE_DEC_J2000: f64 = -28.936_17;

/// J2000 equatorial coordinates carried to the date by precession (small angles: good to a few arc minutes for decades).
pub fn precess(ra_j2000: f64, dec_j2000: f64, jd: f64) -> (f64, f64) {
    let years = centuries(jd) * 100.0;
    let (ra, dec) = (ra_j2000.to_radians(), dec_j2000.to_radians());
    // m, n in seconds of time per year; n' in arc seconds per year
    let m = 3.075_f64;
    let n = 1.336_f64;
    let d_ra = (m + n * ra.sin() * dec.tan()) * 15.0 / 3600.0 * years;
    let d_dec = 20.043 * ra.cos() / 3600.0 * years;
    ((ra_j2000 + d_ra).rem_euclid(360.0), (dec_j2000 + d_dec).clamp(-90.0, 90.0))
}

/// The Milky Way's core as seen from the ground at a Unix time (seconds, UTC).
pub fn core_horizontal(unix: f64, obs: Observer) -> Horizontal {
    let jd = jd_from_unix(unix);
    let (ra, dec) = precess(CORE_RA_J2000, CORE_DEC_J2000, jd);
    to_horizontal(ra, dec, jd, obs)
}

/// The north galactic pole and the galactic longitude of the celestial pole (J2000, IAU 1958 system, as
/// fixed by Blaauw et al. and carried to ICRS): published constants.
const GAL_POLE_RA: f64 = 192.859_48;
const GAL_POLE_DEC: f64 = 27.128_25;
const GAL_NCP_L: f64 = 122.931_92;

/// Galactic longitude and latitude (degrees) to J2000 right ascension and declination (degrees).
pub fn galactic_to_equatorial(l: f64, b: f64) -> (f64, f64) {
    let (l, b) = (l.to_radians(), b.to_radians());
    let (pole_ra, pole_dec) = (GAL_POLE_RA.to_radians(), GAL_POLE_DEC.to_radians());
    let dl = GAL_NCP_L.to_radians() - l;
    let dec = (b.sin() * pole_dec.sin() + b.cos() * pole_dec.cos() * dl.cos()).clamp(-1.0, 1.0).asin();
    let y = b.cos() * dl.sin();
    let x = b.sin() * pole_dec.cos() - b.cos() * pole_dec.sin() * dl.cos();
    let ra = pole_ra + y.atan2(x);
    (ra.to_degrees().rem_euclid(360.0), dec.to_degrees())
}

/// A point of the Milky Way's plane seen from the ground.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlanePoint {
    /// Galactic longitude, degrees (0 = the core).
    pub l: f64,
    pub at: Horizontal,
}

/// The galactic plane as it sits in the sky at a Unix time: one point every `step` degrees of galactic
/// longitude (0 is the core), above and below the horizon.
pub fn milky_way_plane(unix: f64, obs: Observer, step: f64) -> Vec<PlanePoint> {
    let step = if step.is_finite() { step.clamp(0.5, 90.0) } else { 5.0 };
    let jd = jd_from_unix(unix);
    let n = (360.0 / step).floor() as u32;
    (0..n)
        .map(|i| {
            let l = f64::from(i) * step;
            let (ra, dec) = galactic_to_equatorial(l, 0.0);
            let (ra, dec) = precess(ra, dec, jd);
            PlanePoint { l, at: to_horizontal(ra, dec, jd, obs) }
        })
        .collect()
}

/// A meteor shower.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shower {
    pub name: &'static str,
    /// Peak (month, day).
    pub peak: (u32, u32),
    /// First and last day of activity (month, day); the span may cross New Year.
    pub active: ((u32, u32), (u32, u32)),
    /// Typical zenithal hourly rate at the peak under a dark sky.
    pub zhr: u32,
    /// Radiant, degrees (J2000).
    pub ra: f64,
    pub dec: f64,
}

pub const SHOWERS: [Shower; 12] = [
    Shower { name: "Quadrantids", peak: (1, 3), active: ((12, 28), (1, 12)), zhr: 110, ra: 230.0, dec: 49.0 },
    Shower { name: "Lyrids", peak: (4, 22), active: ((4, 14), (4, 30)), zhr: 18, ra: 271.0, dec: 34.0 },
    Shower { name: "Eta Aquariids", peak: (5, 6), active: ((4, 19), (5, 28)), zhr: 50, ra: 338.0, dec: -1.0 },
    Shower { name: "Southern Delta Aquariids", peak: (7, 30), active: ((7, 12), (8, 23)), zhr: 25, ra: 340.0, dec: -16.0 },
    Shower { name: "Perseids", peak: (8, 12), active: ((7, 17), (8, 24)), zhr: 100, ra: 48.0, dec: 58.0 },
    Shower { name: "Draconids", peak: (10, 8), active: ((10, 6), (10, 10)), zhr: 10, ra: 262.0, dec: 54.0 },
    Shower { name: "Orionids", peak: (10, 21), active: ((10, 2), (11, 7)), zhr: 20, ra: 95.0, dec: 16.0 },
    Shower { name: "Southern Taurids", peak: (11, 5), active: ((9, 28), (12, 2)), zhr: 5, ra: 52.0, dec: 13.0 },
    Shower { name: "Northern Taurids", peak: (11, 12), active: ((10, 20), (12, 10)), zhr: 5, ra: 58.0, dec: 22.0 },
    Shower { name: "Leonids", peak: (11, 17), active: ((11, 6), (11, 30)), zhr: 15, ra: 152.0, dec: 22.0 },
    Shower { name: "Geminids", peak: (12, 14), active: ((12, 4), (12, 20)), zhr: 150, ra: 112.0, dec: 33.0 },
    Shower { name: "Ursids", peak: (12, 22), active: ((12, 17), (12, 26)), zhr: 10, ra: 217.0, dec: 76.0 },
];

/// A (month, day) as a day number in a non-leap reference year.
fn day_number(month: u32, day: u32) -> i32 {
    let before = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    let m = month.clamp(1, 12) as usize;
    before.get(m - 1).copied().unwrap_or(0) + day.clamp(1, 31) as i32
}

impl Shower {
    /// The peak of this shower in `year`.
    pub fn peak_date(&self, year: i32) -> Date {
        Date::new(year, self.peak.0, self.peak.1)
    }

    /// Whether the shower is active on a date.
    pub fn active_on(&self, date: Date) -> bool {
        let d = day_number(date.month, date.day);
        let (a, b) = (day_number(self.active.0.0, self.active.0.1), day_number(self.active.1.0, self.active.1.1));
        if a <= b { (a..=b).contains(&d) } else { d >= a || d <= b }
    }

    /// Days from `date` to the next peak (0 on the peak itself; at most 365).
    pub fn days_to_peak(&self, date: Date) -> i32 {
        let (d, p) = (day_number(date.month, date.day), day_number(self.peak.0, self.peak.1));
        (p - d).rem_euclid(365)
    }

    /// The radiant's direction at a Unix time (seconds, UTC).
    pub fn radiant_horizontal(&self, unix: f64, obs: Observer) -> Horizontal {
        let jd = jd_from_unix(unix);
        let (ra, dec) = precess(self.ra, self.dec, jd);
        to_horizontal(ra, dec, jd, obs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::unix_from_civil;

    #[test]
    fn the_core_peaks_at_the_expected_altitude() {
        // transit altitude = 90 - |lat - dec|; at 40 N that is 90 - 40 - 28.9 = 21.1
        let obs = Observer::new(40.0, 0.0);
        let day = unix_from_civil(2024, 7, 1, 0, 0, 0);
        let best = (0..288).map(|i| core_horizontal(day + f64::from(i) * 300.0, obs).alt).fold(f64::MIN, f64::max);
        assert!((best - 21.1).abs() < 0.3, "{best}");
        // from the southern hemisphere it passes almost overhead (lat -29)
        let south = Observer::new(-29.0, 0.0);
        let best = (0..288).map(|i| core_horizontal(day + f64::from(i) * 300.0, south).alt).fold(f64::MIN, f64::max);
        assert!(best > 89.0, "{best}");
    }

    #[test]
    fn the_core_is_in_the_south_east_to_south_in_a_northern_evening() {
        // Madrid, midsummer, 1 am local (23:00 UTC): the core is up in the south
        let h = core_horizontal(unix_from_civil(2024, 7, 14, 23, 0, 0), Observer::new(40.4, -3.7));
        assert!(h.alt > 5.0 && h.alt < 25.0, "{h:?}");
        assert!(h.az > 150.0 && h.az < 210.0, "{h:?}");
    }

    #[test]
    fn galactic_origin_is_the_core_and_the_pole_is_up() {
        let (ra, dec) = galactic_to_equatorial(0.0, 0.0);
        assert!((ra - CORE_RA_J2000).abs() < 0.05 && (dec - CORE_DEC_J2000).abs() < 0.05, "{ra} {dec}");
        let (ra, dec) = galactic_to_equatorial(10.0, 90.0);
        assert!((ra - GAL_POLE_RA).abs() < 0.01 && (dec - GAL_POLE_DEC).abs() < 0.01);
        // the anticentre is opposite the core
        let (ra, dec) = galactic_to_equatorial(180.0, 0.0);
        assert!((ra - 86.4).abs() < 0.2 && (dec - 28.94).abs() < 0.1, "{ra} {dec}");
    }

    #[test]
    fn the_plane_is_a_great_circle_through_the_core() {
        let obs = Observer::new(40.0, 0.0);
        let t = unix_from_civil(2024, 7, 14, 23, 0, 0);
        let plane = milky_way_plane(t, obs, 10.0);
        assert_eq!(plane.len(), 36);
        let core = core_horizontal(t, obs);
        let p0 = plane[0].at;
        assert!((p0.alt - core.alt).abs() < 0.3 && (p0.az - core.az).abs() < 0.3, "{p0:?} {core:?}");
        // a great circle has points on both sides of the horizon, half the sky each
        assert!(plane.iter().any(|p| p.at.alt > 0.0) && plane.iter().any(|p| p.at.alt < 0.0));
    }

    #[test]
    fn precession_is_small_and_grows_with_time() {
        let (ra0, dec0) = precess(CORE_RA_J2000, CORE_DEC_J2000, 2_451_545.0);
        assert!((ra0 - CORE_RA_J2000).abs() < 1e-9 && (dec0 - CORE_DEC_J2000).abs() < 1e-9);
        let (ra, dec) = precess(CORE_RA_J2000, CORE_DEC_J2000, jd_from_unix(unix_from_civil(2026, 6, 1, 0, 0, 0)));
        assert!((ra - CORE_RA_J2000).abs() > 0.1 && (ra - CORE_RA_J2000).abs() < 0.6, "{ra}");
        assert!((dec - CORE_DEC_J2000).abs() < 0.05, "{dec}");
    }

    #[test]
    fn showers_are_active_around_their_peak_and_across_new_year() {
        let perseids = &SHOWERS[4];
        assert_eq!(perseids.name, "Perseids");
        assert!(perseids.active_on(Date::new(2024, 8, 12)));
        assert!(!perseids.active_on(Date::new(2024, 9, 12)));
        let quad = &SHOWERS[0];
        assert!(quad.active_on(Date::new(2024, 12, 30)) && quad.active_on(Date::new(2025, 1, 5)));
        assert!(!quad.active_on(Date::new(2024, 6, 1)));
        assert_eq!(perseids.days_to_peak(Date::new(2024, 8, 12)), 0);
        assert_eq!(perseids.days_to_peak(Date::new(2024, 8, 2)), 10);
        assert_eq!(perseids.days_to_peak(Date::new(2024, 8, 13)), 364);
    }

    #[test]
    fn every_shower_lists_a_valid_calendar() {
        for s in &SHOWERS {
            assert!(s.active_on(s.peak_date(2025)), "{} is active on its own peak", s.name);
            assert!((0.0..360.0).contains(&s.ra) && (-90.0..=90.0).contains(&s.dec), "{}", s.name);
            assert!(s.zhr > 0);
        }
    }

    #[test]
    fn a_radiant_is_circumpolar_far_enough_north() {
        // Ursids (dec +76) never set at 50 N
        let obs = Observer::new(50.0, 10.0);
        let day = unix_from_civil(2024, 12, 22, 0, 0, 0);
        let lowest = (0..96).map(|i| SHOWERS[11].radiant_horizontal(day + f64::from(i) * 900.0, obs).alt).fold(f64::MAX, f64::min);
        assert!(lowest > 0.0, "{lowest}");
    }
}
