//! "Find the dates" for a Sun or Moon direction: the days in a range when the body stands at an
//! azimuth (and, if asked, an altitude) within a tolerance, with the Moon's phase for each.

use crate::horizon::Skyline;
use crate::moon::{moon_horizontal, moon_phase};
use crate::sky::core_horizontal;
use crate::sun::{Horizontal, Observer, sun_horizontal};
use crate::time::DAY;

/// The body to look for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Sun,
    Moon,
    /// The Milky Way's galactic core.
    Core,
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
    /// The Sun's altitude then, degrees (the light: golden hour, blue hour, night…).
    pub sun_alt: f64,
    /// How far the body stands above the skyline in its direction then, degrees (negative: behind the ground), when
    /// a skyline was used.
    pub margin: Option<f64>,
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
        Target::Core => core_horizontal(t, obs),
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
            out.push(Hit { unix, at, error, illumination: moon_phase(unix).illumination, sun_alt: sun_horizontal(unix, obs).alt, margin: None });
        }
    }
    out
}

/// What else must hold at the moment found, besides the direction.
#[derive(Clone, Copy, Debug, Default)]
pub struct Conditions<'a> {
    /// The Sun's altitude must lie in this range, degrees (golden hour -4..6, blue hour -6..-4, dark -18..-90…).
    pub sun_alt: Option<(f64, f64)>,
    /// The Moon's lit fraction must lie in this range (0 new, 1 full).
    pub moon_illumination: Option<(f64, f64)>,
    /// The Moon must be at most this high (degrees), e.g. below the horizon for a Milky Way night.
    pub moon_alt_max: Option<f64>,
    /// The skyline of the place and the range, degrees, the body must stand above it in its direction (0..1: just
    /// clearing the summit; -1..0: just behind it).
    pub skyline: Option<(&'a Skyline, f64, f64)>,
}

impl Conditions<'_> {
    fn none(&self) -> bool {
        self.sun_alt.is_none() && self.moon_illumination.is_none() && self.moon_alt_max.is_none() && self.skyline.is_none()
    }
}

fn finite_range(r: Option<(f64, f64)>) -> bool {
    r.is_none_or(|(a, b)| a.is_finite() && b.is_finite())
}

/// As [`find_dates`], but the moment chosen each day is the one that also meets `conditions` (the Moon in the
/// south *in the blue hour*, the core over a ridge in the dark with no Moon…), not the day's best moment rejected
/// afterwards. With no condition it is `find_dates`. At most 400 days are searched.
#[allow(clippy::too_many_arguments)]
pub fn find_with(
    target: Target,
    obs: Observer,
    first_day_start: f64,
    days: u32,
    az: f64,
    alt: Option<f64>,
    tolerance: f64,
    conditions: Conditions,
) -> Vec<Hit> {
    if conditions.none() {
        return find_dates(target, obs, first_day_start, days, az, alt, tolerance);
    }
    let mut out = Vec::new();
    let sane = az.is_finite()
        && first_day_start.is_finite()
        && tolerance.is_finite()
        && alt.is_none_or(f64::is_finite)
        && finite_range(conditions.sun_alt)
        && finite_range(conditions.moon_illumination)
        && finite_range(conditions.skyline.map(|(_, a, b)| (a, b)))
        && conditions.moon_alt_max.is_none_or(f64::is_finite);
    if !sane {
        return out;
    }
    let tolerance = tolerance.clamp(0.01, 90.0);
    let step = if days > 200 {
        240.0
    } else if days > 60 {
        120.0
    } else {
        60.0
    };
    let position = |t: f64| match target {
        Target::Sun => sun_horizontal(t, obs),
        Target::Moon => moon_horizontal(t, obs),
        Target::Core => core_horizontal(t, obs),
    };
    // the Hit at a moment if every condition holds then (the cheap tests first)
    let at = |t: f64| -> Option<Hit> {
        let h = position(t);
        if !h.az.is_finite() || !h.alt.is_finite() {
            return None;
        }
        let mut error = az_diff(h.az, az).abs();
        if let Some(a) = alt {
            error = error.hypot(h.alt - a);
        }
        if error > tolerance {
            return None;
        }
        let mut margin = None;
        match conditions.skyline {
            Some((sky, lo, hi)) => {
                let m = h.alt - sky.altitude_at(h.az);
                if !(lo.min(hi)..=lo.max(hi)).contains(&m) {
                    return None;
                }
                error = error.hypot(m - (lo + hi) / 2.0);
                margin = Some(m);
            }
            None if h.alt < 0.0 => return None,
            None => {}
        }
        let sun_alt = if target == Target::Sun { h.alt } else { sun_horizontal(t, obs).alt };
        if let Some((lo, hi)) = conditions.sun_alt
            && !(lo.min(hi)..=lo.max(hi)).contains(&sun_alt)
        {
            return None;
        }
        if let Some(limit) = conditions.moon_alt_max {
            let moon_alt = if target == Target::Moon { h.alt } else { moon_horizontal(t, obs).alt };
            if moon_alt > limit {
                return None;
            }
        }
        let illumination = moon_phase(t).illumination;
        if let Some((lo, hi)) = conditions.moon_illumination
            && !(lo.min(hi)..=lo.max(hi)).contains(&illumination)
        {
            return None;
        }
        Some(Hit { unix: t, at: h, error, illumination, sun_alt, margin })
    };
    for d in 0..days.min(400) {
        let day = first_day_start + f64::from(d) * DAY;
        let mut best: Option<Hit> = None;
        let mut t = day;
        while t < day + DAY {
            if let Some(hit) = at(t)
                && best.is_none_or(|b| hit.error < b.error)
            {
                best = Some(hit);
            }
            t += step;
        }
        // a finer look round the best moment (the bodies move a quarter degree a minute)
        if let Some(coarse) = best {
            let fine = step / 12.0;
            let mut t = coarse.unix - step;
            while t <= coarse.unix + step {
                if let Some(hit) = at(t)
                    && best.is_none_or(|b| hit.error < b.error)
                {
                    best = Some(hit);
                }
                t += fine;
            }
            out.extend(best);
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
        (Target::Moon | Target::Core, d) if d > 120 => 1800.0,
        (Target::Moon | Target::Core, _) => 1200.0,
    };
    let mut t = first_day_start;
    let end = first_day_start + f64::from(days.clamp(1, 400)) * DAY;
    let mut last: Option<Horizontal> = None;
    while t <= end {
        let h = match target {
            Target::Sun => sun_horizontal(t, obs),
            Target::Moon => moon_horizontal(t, obs),
            Target::Core => core_horizontal(t, obs),
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

    fn flat_skyline_with_summit(az: f64, alt: f64) -> Skyline {
        let n = 120;
        let alts: Vec<f64> = (0..n)
            .map(|i| {
                let a = i as f64 * 3.0;
                if az_diff(a, az).abs() <= 3.0 { alt } else { 0.0 }
            })
            .collect();
        Skyline { near: 500.0, far: 60_000.0, ground: 0.0, alt: alts, dist: vec![10_000.0; n], height: vec![0.0; n] }
    }

    #[test]
    fn conditions_hold_at_every_moment_found() {
        let obs = Observer::new(43.3, -1.98);
        let start = unix_from_civil(2025, 1, 1, 0, 0, 0);
        // the Moon in the south-east in the blue hour, between a quarter and full
        let c = Conditions { sun_alt: Some((-6.0, -4.0)), moon_illumination: Some((0.5, 1.0)), ..Conditions::default() };
        let hits = find_with(Target::Moon, obs, start, 365, 150.0, None, 40.0, c);
        assert!(!hits.is_empty(), "a year has such evenings");
        for h in &hits {
            assert!((-6.0..=-4.0).contains(&h.sun_alt), "{h:?}");
            assert!((0.5..=1.0).contains(&h.illumination), "{h:?}");
            assert!(az_diff(h.at.az, 150.0).abs() <= 40.0 && h.at.alt >= 0.0);
            assert!((h.sun_alt - sun_horizontal(h.unix, obs).alt).abs() < 1e-9);
        }
    }

    #[test]
    fn the_core_is_found_only_in_the_dark_without_the_moon() {
        let obs = Observer::new(43.3, -1.98);
        let start = unix_from_civil(2025, 5, 1, 0, 0, 0);
        let c = Conditions { sun_alt: Some((-90.0, -18.0)), moon_alt_max: Some(-1.0), ..Conditions::default() };
        let hits = find_with(Target::Core, obs, start, 120, 180.0, None, 60.0, c);
        assert!(!hits.is_empty());
        for h in &hits {
            assert!(h.sun_alt <= -18.0 && moon_horizontal(h.unix, obs).alt <= -1.0, "{h:?}");
            assert!(h.at.alt >= 0.0);
        }
    }

    #[test]
    fn the_skyline_margin_decides_which_moment_is_taken() {
        let obs = Observer::new(43.3, -1.98);
        let start = unix_from_civil(2025, 1, 1, 0, 0, 0);
        let sky = flat_skyline_with_summit(120.0, 3.0);
        // the Moon just clearing a 3 degree summit at azimuth 120: its height is the summit's, within the margin
        let c = Conditions { skyline: Some((&sky, 0.0, 0.5)), ..Conditions::default() };
        let hits = find_with(Target::Moon, obs, start, 120, 120.0, None, 2.0, c);
        assert!(!hits.is_empty(), "the Moon passes there every month");
        for h in &hits {
            let m = h.margin.expect("a skyline was used");
            assert!((0.0..=0.5).contains(&m), "{h:?}");
            assert!((m - (h.at.alt - sky.altitude_at(h.at.az))).abs() < 1e-9);
            assert!(h.at.alt > 2.0, "over a 3 degree summit the Moon is high: {h:?}");
        }
    }

    #[test]
    fn hostile_conditions_give_nothing_instead_of_a_panic() {
        let obs = Observer::new(43.3, -1.98);
        let start = unix_from_civil(2025, 1, 1, 0, 0, 0);
        let c = Conditions { sun_alt: Some((f64::NAN, 0.0)), ..Conditions::default() };
        assert!(find_with(Target::Sun, obs, start, 10, 90.0, None, 5.0, c).is_empty());
        let c = Conditions { moon_alt_max: Some(f64::NAN), ..Conditions::default() };
        assert!(find_with(Target::Sun, obs, start, 10, 90.0, None, 5.0, c).is_empty());
        let c = Conditions { sun_alt: Some((-6.0, 6.0)), ..Conditions::default() };
        assert!(find_with(Target::Sun, obs, f64::NAN, 10, 90.0, None, 5.0, c).is_empty());
        assert!(find_with(Target::Sun, obs, start, 0, 90.0, None, 5.0, c).is_empty());
    }

    #[test]
    fn no_conditions_is_the_plain_search() {
        let obs = Observer::new(40.4, -3.7);
        let start = unix_from_civil(2024, 1, 1, 0, 0, 0);
        let a = find_dates(Target::Moon, obs, start, 60, 180.0, Some(30.0), 5.0);
        let b = find_with(Target::Moon, obs, start, 60, 180.0, Some(30.0), 5.0, Conditions::default());
        assert_eq!(a, b);
    }
}
