//! The light along a walk: what kind of light a place has at a moment, and how long a walk takes, so a route can be
//! laid over the day ("I reach the viewpoint at 19:42, the Sun is 3° up and behind me").
//!
//! The kinds follow the bands the day plan already uses ([`GOLDEN_TOP`], [`GOLDEN_BOTTOM`], [`BLUE_BOTTOM`]); the
//! walking speed is Waldo Tobler's hiking function (1993), a formula, not code.

use crate::moon::moon_horizontal;
use crate::plan::{BLUE_BOTTOM, GOLDEN_BOTTOM, GOLDEN_TOP};
use crate::sun::{Horizontal, Observer, sun_horizontal};

/// Below this the sky is fully dark (end of astronomical twilight).
pub const DARK_BELOW: f64 = -18.0;

/// The kind of light, from the Sun's altitude.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Light {
    /// Sun well up (more than 6° above the horizon).
    Day,
    /// Golden hour: Sun from 6° to -4°.
    Golden,
    /// Blue hour: Sun from -4° to -6°.
    Blue,
    /// Twilight: Sun from -6° to -18°, the sky still has some colour.
    Twilight,
    /// Night: Sun more than 18° down.
    Night,
}

impl Light {
    /// The light for a Sun altitude in degrees (NaN counts as day).
    pub fn of_sun_altitude(alt: f64) -> Light {
        if alt.is_nan() || alt > GOLDEN_TOP {
            Light::Day
        } else if alt > GOLDEN_BOTTOM {
            Light::Golden
        } else if alt > BLUE_BOTTOM {
            Light::Blue
        } else if alt > DARK_BELOW {
            Light::Twilight
        } else {
            Light::Night
        }
    }

    /// A stable name for control, MCP and settings.
    pub fn id(self) -> &'static str {
        match self {
            Light::Day => "day",
            Light::Golden => "golden",
            Light::Blue => "blue",
            Light::Twilight => "twilight",
            Light::Night => "night",
        }
    }

    /// An English label (translated by the UI).
    pub fn label(self) -> &'static str {
        match self {
            Light::Day => "Daylight",
            Light::Golden => "Golden hour",
            Light::Blue => "Blue hour",
            Light::Twilight => "Twilight",
            Light::Night => "Night",
        }
    }
}

/// The sky at a place and moment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spot {
    pub sun: Horizontal,
    pub moon: Horizontal,
    pub light: Light,
}

/// The Sun, the Moon and the kind of light at a place (degrees) and a Unix time (seconds, UTC).
pub fn spot(unix: f64, lat: f64, lon: f64) -> Spot {
    let obs = Observer::new(lat, lon);
    let sun = sun_horizontal(unix, obs);
    Spot { sun, moon: moon_horizontal(unix, obs), light: Light::of_sun_altitude(sun.alt) }
}

/// Just the kind of light (no Moon): cheaper, for colouring a whole route.
pub fn light_at(unix: f64, lat: f64, lon: f64) -> Light {
    Light::of_sun_altitude(sun_horizontal(unix, Observer::new(lat, lon)).alt)
}

/// Walking speed (m/s) on a slope (rise over run, so 0.1 is a 10 % climb), by Tobler's hiking function: about 5 km/h
/// on the flat, fastest on a gentle descent (-5 %), slower on any steep ground. Slopes beyond ±60 % count as ±60 %; NaN
/// as flat.
pub fn walking_speed(slope: f64) -> f64 {
    let s = if slope.is_nan() { 0.0 } else { slope.clamp(-0.6, 0.6) };
    6.0 * (-3.5 * (s + 0.05).abs()).exp() / 3.6
}

/// Seconds spent getting from each point of a line to the next, summed: the time at each point since the start (the
/// first is 0). `legs` gives each step's length (m) and height change (m); `pace` scales the speed (1 = Tobler's
/// walker, 0.75 a slow pace, 1.3 a quick one) and is kept between 0.25 and 4.
pub fn walk_seconds(legs: &[(f64, f64)], pace: f64) -> Vec<f64> {
    let pace = if pace.is_finite() { pace.clamp(0.25, 4.0) } else { 1.0 };
    let mut t = 0.0;
    let mut out = Vec::with_capacity(legs.len().saturating_add(1));
    out.push(0.0);
    for &(run, rise) in legs {
        let run = if run.is_finite() { run.max(0.0) } else { 0.0 };
        let slope = if run > 0.0 && rise.is_finite() { rise / run } else { 0.0 };
        t += run / (walking_speed(slope) * pace);
        out.push(t);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::unix_from_civil;

    #[test]
    fn the_light_follows_the_bands_of_the_day_plan() {
        let alts = [(30.0, Light::Day), (6.1, Light::Day), (6.0, Light::Golden), (0.0, Light::Golden), (-3.9, Light::Golden)];
        for (alt, want) in alts {
            assert_eq!(Light::of_sun_altitude(alt), want, "{alt}");
        }
        for (alt, want) in [
            (-4.1, Light::Blue),
            (-5.9, Light::Blue),
            (-6.1, Light::Twilight),
            (-17.9, Light::Twilight),
            (-18.1, Light::Night),
            (-60.0, Light::Night),
        ] {
            assert_eq!(Light::of_sun_altitude(alt), want, "{alt}");
        }
        assert_eq!(Light::of_sun_altitude(f64::NAN), Light::Day);
        let ids: Vec<_> = [Light::Day, Light::Golden, Light::Blue, Light::Twilight, Light::Night].iter().map(|l| l.id()).collect();
        assert_eq!(ids, ["day", "golden", "blue", "twilight", "night"]);
    }

    #[test]
    fn a_summer_evening_in_donostia_goes_from_day_to_night() {
        // 2026-06-21, local = UTC+2
        let at = |h: u32, m: u32| unix_from_civil(2026, 6, 21, h - 2, m, 0);
        let (lat, lon) = (43.32, -1.98);
        assert_eq!(light_at(at(14, 0), lat, lon), Light::Day);
        assert_eq!(light_at(at(21, 30), lat, lon), Light::Golden);
        assert_eq!(light_at(at(22, 20), lat, lon), Light::Blue);
        assert_eq!(light_at(at(22, 50), lat, lon), Light::Twilight);
        assert_eq!(light_at(at(25, 0), lat, lon), Light::Night, "the Sun gets 23° below the horizon at 43°N");
        let s = spot(at(13, 45), lat, lon);
        assert!(s.sun.alt > 60.0 && (150.0..210.0).contains(&s.sun.az), "{s:?}");
        let deep = spot(unix_from_civil(2026, 12, 21, 1, 0, 0), lat, lon);
        assert_eq!(deep.light, Light::Night);
    }

    #[test]
    fn walking_is_about_five_km_an_hour_on_the_flat_and_slower_uphill() {
        let flat = walking_speed(0.0) * 3.6;
        assert!((flat - 5.04).abs() < 0.05, "{flat}");
        assert!(walking_speed(-0.05) > walking_speed(0.0), "a gentle descent is the fastest");
        assert!(walking_speed(0.3) < walking_speed(0.1) && walking_speed(0.1) < walking_speed(0.0));
        assert!(walking_speed(-0.4) < walking_speed(-0.05));
        assert_eq!(walking_speed(5.0), walking_speed(0.6), "beyond a wall it stops getting slower");
        assert_eq!(walking_speed(f64::NAN), walking_speed(0.0));
    }

    #[test]
    fn the_times_add_up_and_the_pace_scales_them() {
        let legs = [(1000.0, 0.0), (1000.0, 100.0), (500.0, -50.0)];
        let t = walk_seconds(&legs, 1.0);
        assert_eq!(t.len(), 4);
        assert_eq!(t[0], 0.0);
        assert!(t.windows(2).all(|w| w[1] > w[0]));
        assert!((t[1] - 1000.0 / walking_speed(0.0)).abs() < 1e-9);
        let slow = walk_seconds(&legs, 0.5);
        assert!((slow[3] / t[3] - 2.0).abs() < 1e-9);
        // hostile numbers
        let odd = walk_seconds(&[(f64::NAN, 1.0), (-5.0, f64::INFINITY), (10.0, f64::NAN)], f64::NAN);
        assert!(odd.iter().all(|v| v.is_finite()), "{odd:?}");
        assert_eq!(walk_seconds(&[], 1.0), vec![0.0]);
        assert_eq!(walk_seconds(&[(100.0, 0.0)], 0.0), walk_seconds(&[(100.0, 0.0)], 0.25));
    }
}
