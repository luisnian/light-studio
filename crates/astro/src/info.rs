//! Facts about the Sun and Moon for an information panel: sizes and distances, the Moon's age, the
//! four seasons' starts and the Moon's perigees and apogees.

use crate::moon::{moon_coords, moon_phase, semidiameter};
use crate::sun::{KM_PER_AU, sun_coords};
use crate::time::{Date, jd_from_unix, unix_from_civil};

/// Mean length of the lunar month, days.
pub const SYNODIC_MONTH: f64 = 29.530_588;

/// The Sun's apparent diameter, degrees, and its distance, km.
pub fn sun_size(unix: f64) -> (f64, f64) {
    let s = sun_coords(jd_from_unix(unix));
    (2.0 * 0.266_56 * KM_PER_AU / s.dist_km.max(1.0), s.dist_km)
}

/// The Moon's apparent diameter, degrees, and its distance, km.
pub fn moon_size(unix: f64) -> (f64, f64) {
    let m = moon_coords(jd_from_unix(unix));
    (2.0 * semidiameter(m.dist_km), m.dist_km)
}

/// Days since the last new moon (by the Moon's elongation: within a day of the true age).
pub fn moon_age_days(unix: f64) -> f64 {
    // the last new moon at or before `unix`: walk forward from 31 days back
    let mut last = None;
    let mut t = unix - 31.0 * 86_400.0;
    for _ in 0..3 {
        match crate::moon::next_phase(t, crate::moon::Principal::New) {
            Some(p) if p <= unix => {
                last = Some(p);
                t = p + 86_400.0;
            }
            _ => break,
        }
    }
    match last {
        Some(p) => (unix - p) / 86_400.0,
        None => moon_phase(unix).elongation / 360.0 * SYNODIC_MONTH,
    }
}

/// The start of astronomical spring, summer, autumn and winter (northern naming) in a year: the
/// Sun's ecliptic longitude at 0, 90, 180 and 270 degrees. Unix seconds, UTC.
pub fn seasons(year: i32) -> [Option<f64>; 4] {
    let mut out = [None; 4];
    for (k, slot) in out.iter_mut().enumerate() {
        let target = 90.0 * k as f64;
        // the usual dates, from 5 days before them (the Sun moves about a degree a day)
        let (m, d) = [(3, 20), (6, 21), (9, 22), (12, 21)].get(k).copied().unwrap_or((3, 20));
        let guess = unix_from_civil(year, m, d, 0, 0, 0) - 5.0 * 86_400.0;
        let past = |t: f64| (sun_coords(jd_from_unix(t)).lon - target + 180.0).rem_euclid(360.0) - 180.0;
        let (mut a, mut b) = (guess, guess + 12.0 * 86_400.0);
        if past(a) < 0.0 && past(b) >= 0.0 {
            for _ in 0..40 {
                let mid = (a + b) / 2.0;
                if past(mid) < 0.0 {
                    a = mid;
                } else {
                    b = mid;
                }
            }
            *slot = Some((a + b) / 2.0);
        }
    }
    out
}

/// A closest or farthest point of the Moon's orbit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Apsis {
    pub unix: f64,
    pub perigee: bool,
    pub dist_km: f64,
}

/// The Moon's perigees and apogees between two Unix times, in order (found on a 2-hour grid and
/// refined by a parabola: good to about ten minutes and a few km).
pub fn lunar_apsides(from: f64, to: f64) -> Vec<Apsis> {
    let mut out = Vec::new();
    if !(from.is_finite() && to.is_finite()) || to <= from || to - from > 400.0 * 86_400.0 {
        return out;
    }
    let step = 7_200.0;
    let dist = |t: f64| moon_coords(jd_from_unix(t)).dist_km;
    let mut t = from;
    let (mut d0, mut d1) = (dist(t - step), dist(t));
    while t < to {
        let d2 = dist(t + step);
        let low = d1 < d0 && d1 <= d2;
        let high = d1 > d0 && d1 >= d2;
        if low || high {
            let denom = d0 - 2.0 * d1 + d2;
            let shift = if denom.abs() > 1e-9 { 0.5 * (d0 - d2) / denom } else { 0.0 };
            let best = d1 - 0.25 * (d0 - d2) * shift;
            out.push(Apsis { unix: t + shift * step, perigee: low, dist_km: best });
        }
        d0 = d1;
        d1 = d2;
        t += step;
    }
    out
}

/// A supermoon: a full or new moon within 90 % of the perigee distance range (the common reading:
/// closer than 361,000 km).
pub fn is_supermoon(dist_km: f64) -> bool {
    dist_km < 361_000.0
}

/// Days from one date to another, ignoring the clock.
pub fn days_between(a: Date, b: Date) -> i64 {
    b.days() - a.days()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_are_half_a_degree() {
        let t = unix_from_civil(2024, 6, 1, 12, 0, 0);
        let (sd, sdist) = sun_size(t);
        let (md, mdist) = moon_size(t);
        assert!((sd - 0.53).abs() < 0.02 && (md - 0.52).abs() < 0.06, "{sd} {md}");
        assert!((sdist / KM_PER_AU - 1.0).abs() < 0.02 && (356_000.0..407_000.0).contains(&mdist));
    }

    #[test]
    fn the_age_follows_the_phase() {
        // new moon 8 April 2024 18:21 UTC; full 23 April 11:49
        let new = moon_age_days(unix_from_civil(2024, 4, 8, 18, 31, 0));
        assert!(new < 0.1, "{new}");
        let full = moon_age_days(unix_from_civil(2024, 4, 23, 11, 49, 0));
        assert!((full - 14.77).abs() < 0.3, "{full}");
    }

    #[test]
    fn seasons_of_2024() {
        let s = seasons(2024);
        let want = [(3, 20, 3, 6), (6, 20, 20, 51), (9, 22, 12, 44), (12, 21, 9, 20)];
        for (got, (m, d, h, mi)) in s.iter().zip(want) {
            let g = got.unwrap_or(0.0);
            let w = unix_from_civil(2024, m, d, h, mi, 0);
            assert!((g - w).abs() < 25.0 * 60.0, "{m}-{d}: {}", (g - w) / 60.0);
        }
    }

    #[test]
    fn apsides_alternate_about_two_weeks_apart() {
        let a = lunar_apsides(unix_from_civil(2024, 1, 1, 0, 0, 0), unix_from_civil(2024, 12, 31, 0, 0, 0));
        assert!(a.len() >= 24, "{}", a.len());
        for w in a.windows(2) {
            assert_ne!(w[0].perigee, w[1].perigee);
            let days = (w[1].unix - w[0].unix) / 86_400.0;
            assert!((9.0..19.0).contains(&days), "{days}");
        }
        assert!(a.iter().filter(|x| x.perigee).all(|x| (352_000.0..371_000.0).contains(&x.dist_km)));
        assert!(a.iter().filter(|x| !x.perigee).all(|x| (404_000.0..407_500.0).contains(&x.dist_km)));
        assert!(is_supermoon(357_000.0) && !is_supermoon(400_000.0));
        assert!(lunar_apsides(f64::NAN, 1.0).is_empty());
    }
}
