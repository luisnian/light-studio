//! "Find the dates" for a Sun or Moon direction: the days in a range when the body stands at an
//! azimuth (and, if asked, an altitude) within a tolerance, with the Moon's phase for each.

use crate::moon::{moon_horizontal, moon_phase};
use crate::sun::{Horizontal, Observer, sun_horizontal};
use crate::time::DAY;

/// The body to look for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Sun,
    Moon,
}

/// A day on which the body reached the direction asked.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub unix: f64,
    pub at: Horizontal,
    /// How far from the direction asked, degrees.
    pub error: f64,
    /// Lit fraction of the Moon's disc then.
    pub illumination: f64,
}

fn az_diff(a: f64, b: f64) -> f64 {
    (a - b + 180.0).rem_euclid(360.0) - 180.0
}

/// For each of `days` days from `first_day_start` (Unix seconds at a local midnight), the moment the
/// body came closest to `az` (and `alt`, when given) while above the horizon, if within `tolerance`
/// degrees. At most 400 days are searched.
pub fn find_dates(target: Target, obs: Observer, first_day_start: f64, days: u32, az: f64, alt: Option<f64>, tolerance: f64) -> Vec<Hit> {
    let mut out = Vec::new();
    if !(az.is_finite() && first_day_start.is_finite() && tolerance.is_finite()) || alt.is_some_and(|a| !a.is_finite()) {
        return out;
    }
    let tolerance = tolerance.clamp(0.01, 90.0);
    let step = if days > 120 { 240.0 } else { 120.0 };
    let position = |t: f64| match target {
        Target::Sun => sun_horizontal(t, obs),
        Target::Moon => moon_horizontal(t, obs),
    };
    for d in 0..days.min(400) {
        let day = first_day_start + f64::from(d) * DAY;
        let mut best: Option<(f64, f64, Horizontal)> = None;
        let mut t = day;
        while t < day + DAY {
            let h = position(t);
            if h.alt >= 0.0 {
                let e = match alt {
                    Some(a) => az_diff(h.az, az).abs().hypot(h.alt - a),
                    None => az_diff(h.az, az).abs(),
                };
                if best.is_none_or(|b| e < b.0) {
                    best = Some((e, t, h));
                }
            }
            t += step;
        }
        if let Some((error, unix, at)) = best
            && error <= tolerance
        {
            out.push(Hit { unix, at, error, illumination: moon_phase(unix).illumination });
        }
    }
    out
}

/// The directions (whole degrees clockwise from north; `true` = the body stands above the horizon there at some moment)
/// the body reaches in `days` days from `first_day_start`. The rest are where it is never seen in that stretch.
/// At most 400 days are looked at.
pub fn reachable_azimuths(target: Target, obs: Observer, first_day_start: f64, days: u32) -> [bool; 360] {
    let mut seen = [false; 360];
    if !first_day_start.is_finite() {
        return seen;
    }
    let step = match (target, days) {
        (Target::Sun, d) if d > 120 => 1800.0,
        (Target::Sun, _) => 900.0,
        (Target::Moon, d) if d > 120 => 1800.0,
        (Target::Moon, _) => 1200.0,
    };
    let mut t = first_day_start;
    let end = first_day_start + f64::from(days.clamp(1, 400)) * DAY;
    let mut last: Option<Horizontal> = None;
    while t <= end {
        let h = match target {
            Target::Sun => sun_horizontal(t, obs),
            Target::Moon => moon_horizontal(t, obs),
        };
        if h.alt >= 0.0 {
            mark(&mut seen, h.az);
            // between two samples the body moved: the directions in between are reached too
            if let Some(p) = last.filter(|p| p.alt >= 0.0) {
                let d = az_diff(h.az, p.az);
                let n = d.abs().ceil() as i32;
                for k in 1..n {
                    mark(&mut seen, p.az + d * f64::from(k) / f64::from(n));
                }
            }
        }
        last = Some(h);
        t += step;
    }
    seen
}

fn mark(seen: &mut [bool; 360], az: f64) {
    if az.is_finite() {
        let i = (az.rem_euclid(360.0).floor() as usize) % 360;
        if let Some(s) = seen.get_mut(i) {
            *s = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::unix_from_civil;

    #[test]
    fn the_sun_rises_due_east_only_near_the_equinoxes() {
        let obs = Observer::new(40.4, -3.7);
        // sunrise azimuth 90 is on the equinox; "az 90 at low altitude" finds those days
        let start = unix_from_civil(2024, 1, 1, 0, 0, 0);
        let hits = find_dates(Target::Sun, obs, start, 366, 90.0, Some(1.0), 2.0);
        assert!(!hits.is_empty());
        for h in &hits {
            let day = (h.unix - start) / DAY;
            let near_march = (day - 79.0).abs() < 14.0;
            let near_sept = (day - 265.0).abs() < 14.0;
            assert!(near_march || near_sept, "day {day}");
            assert!(h.error <= 2.0 && h.at.alt >= 0.0);
        }
    }

    #[test]
    fn finds_the_moon_in_the_south_at_a_height() {
        let obs = Observer::new(40.4, -3.7);
        let start = unix_from_civil(2024, 6, 1, 0, 0, 0);
        let hits = find_dates(Target::Moon, obs, start, 30, 180.0, Some(20.0), 4.0);
        assert!(!hits.is_empty());
        assert!(hits.iter().all(|h| (h.at.az - 180.0).abs() < 5.0 && (0.0..=1.0).contains(&h.illumination)));
    }

    #[test]
    fn hostile_input_gives_nothing() {
        let obs = Observer::new(40.0, 0.0);
        assert!(find_dates(Target::Sun, obs, f64::NAN, 10, 90.0, None, 1.0).is_empty());
        assert!(find_dates(Target::Sun, obs, 0.0, 10, f64::NAN, None, 1.0).is_empty());
        assert!(find_dates(Target::Sun, obs, 0.0, 0, 90.0, None, 1.0).is_empty());
    }

    #[test]
    fn the_sun_never_stands_north_of_the_tropics() {
        let obs = Observer::new(43.3, -1.98);
        let start = unix_from_civil(2026, 1, 1, 0, 0, 0);
        let seen = reachable_azimuths(Target::Sun, obs, start, 366);
        // due south is reached, due north is not (at 43°N the Sun never gets there while above the horizon)
        assert!(seen[180], "south");
        assert!(!seen[0] && !seen[359] && !seen[1], "north");
        // one winter day: only a narrow arc round south, and a sunrise well south of east
        let winter = reachable_azimuths(Target::Sun, obs, unix_from_civil(2026, 12, 21, 0, 0, 0), 1);
        assert!(winter[180] && !winter[90] && !winter[270], "the winter Sun stays in the south");
        // garbage in, nothing out and no panic
        assert!(reachable_azimuths(Target::Moon, obs, f64::NAN, 5).iter().all(|b| !b));
    }
}
