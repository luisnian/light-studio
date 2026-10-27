//! A day (and a night) of the sky for one place: when the Sun, the Moon and the Milky Way's core
//! rise and set, the twilights, the golden and blue hours, and which nights are dark enough to
//! shoot the core.
//!
//! Everything is found the same way: sample a yes/no question ("is the Sun above -6 degrees?")
//! every few minutes, then bisect each change to the second. That is slower than closed formulas
//! but cannot be wrong at the poles or around midnight.
//!
//! Golden hour is the Sun between -4 and +6 degrees, blue hour between -6 and -4 (the usual photographers'
//! convention). Twilights are the Sun at -6 (civil), -12 (nautical) and -18 degrees (astronomical).

use crate::moon::{moon_horizontal, moon_rise_altitude};
use crate::sky::core_horizontal;
use crate::sun::{Horizontal, Observer, sun_horizontal};
use crate::time::DAY;

/// Sunrise and sunset: the Sun's centre at -50' (34' of refraction and 16' of radius).
pub const SUN_RISE_ALT: f64 = -50.0 / 60.0;
pub const CIVIL: f64 = -6.0;
pub const NAUTICAL: f64 = -12.0;
pub const ASTRONOMICAL: f64 = -18.0;
pub const GOLDEN_TOP: f64 = 6.0;
pub const GOLDEN_BOTTOM: f64 = -4.0;
pub const BLUE_BOTTOM: f64 = -6.0;
/// The Milky Way's core counts as up for photography above this altitude.
pub const CORE_MIN_ALT: f64 = 5.0;
/// The moon counts as out of the way below this altitude.
pub const MOON_DOWN_ALT: f64 = -1.0;

const STEP: f64 = 300.0;

/// A stretch of time, Unix seconds (UTC).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Span {
    pub start: f64,
    pub end: f64,
}

impl Span {
    pub fn minutes(&self) -> f64 {
        (self.end - self.start) / 60.0
    }

    pub fn contains(&self, t: f64) -> bool {
        t >= self.start && t <= self.end
    }
}

/// The stretches of `[t0, t1]` in which `inside` holds, edges found to a fraction of a second.
pub fn spans(inside: &dyn Fn(f64) -> bool, t0: f64, t1: f64, step: f64) -> Vec<Span> {
    let mut out = Vec::new();
    if !(t0.is_finite() && t1.is_finite() && t1 > t0 && step > 0.0) {
        return out;
    }
    let mut open: Option<f64> = inside(t0).then_some(t0);
    let mut a = t0;
    let mut in_a = open.is_some();
    while a < t1 {
        let b = (a + step).min(t1);
        let in_b = inside(b);
        if in_a != in_b {
            let (mut lo, mut hi) = (a, b);
            for _ in 0..24 {
                let mid = (lo + hi) / 2.0;
                if inside(mid) == in_a {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            let edge = (lo + hi) / 2.0;
            if in_b {
                open = Some(edge);
            } else if let Some(start) = open.take() {
                out.push(Span { start, end: edge });
            }
        }
        a = b;
        in_a = in_b;
    }
    if let Some(start) = open {
        out.push(Span { start, end: t1 });
    }
    out
}

/// The highest point of `alt` in `[t0, t1]`: its time and altitude.
fn culmination(alt: &dyn Fn(f64) -> f64, t0: f64, t1: f64) -> (f64, f64) {
    let n = ((t1 - t0) / STEP).ceil().max(1.0) as usize;
    let mut best = (t0, f64::MIN);
    for i in 0..=n {
        let t = (t0 + i as f64 * STEP).min(t1);
        let a = alt(t);
        if a > best.1 {
            best = (t, a);
        }
    }
    // refine around the best sample by golden-section search
    let (mut lo, mut hi) = ((best.0 - STEP).max(t0), (best.0 + STEP).min(t1));
    let g = 0.618_033_988_75;
    for _ in 0..40 {
        let (m1, m2) = (hi - (hi - lo) * g, lo + (hi - lo) * g);
        if alt(m1) < alt(m2) {
            lo = m1;
        } else {
            hi = m2;
        }
    }
    let t = (lo + hi) / 2.0;
    let a = alt(t);
    if a >= best.1 { (t, a) } else { best }
}

/// Dawn and dusk for one twilight depth.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Twilight {
    pub dawn: Option<f64>,
    pub dusk: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SunState {
    Normal,
    /// Up all day.
    MidnightSun,
    /// Down all day.
    PolarNight,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SunDay {
    pub state: SunState,
    pub rise: Option<f64>,
    pub set: Option<f64>,
    pub rise_az: Option<f64>,
    pub set_az: Option<f64>,
    /// Highest point of the day (solar noon), and its altitude and azimuth.
    pub noon: f64,
    pub noon_alt: f64,
    pub noon_az: f64,
    pub civil: Twilight,
    pub nautical: Twilight,
    pub astronomical: Twilight,
    pub golden_am: Option<Span>,
    pub golden_pm: Option<Span>,
    pub blue_am: Option<Span>,
    pub blue_pm: Option<Span>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MoonDay {
    pub rise: Option<f64>,
    pub set: Option<f64>,
    pub rise_az: Option<f64>,
    pub set_az: Option<f64>,
    pub transit: f64,
    pub transit_alt: f64,
    pub transit_az: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CoreDay {
    pub rise: Option<f64>,
    pub set: Option<f64>,
    pub rise_az: Option<f64>,
    pub set_az: Option<f64>,
    pub transit: f64,
    pub transit_alt: f64,
    pub transit_az: f64,
    /// Stretches of the day in which the core is above [`CORE_MIN_ALT`] and the sky is astronomically dark.
    pub dark: Vec<Span>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DayPlan {
    /// The day shown: `[start, start + 24 h)`.
    pub start: f64,
    pub sun: SunDay,
    pub moon: MoonDay,
    pub core: CoreDay,
}

/// One body above `threshold` (a fixed altitude): its rise and set within the window.
fn rise_set(alt: &dyn Fn(f64) -> f64, threshold: &dyn Fn(f64) -> f64, t0: f64, t1: f64) -> (Option<f64>, Option<f64>, Vec<Span>) {
    let up = |t: f64| alt(t) > threshold(t);
    let sp = spans(&up, t0, t1, STEP);
    let rise = sp.iter().map(|s| s.start).find(|&s| s > t0);
    let set = sp.iter().map(|s| s.end).find(|&e| e < t1);
    (rise, set, sp)
}

/// The plan for the 24 hours from `start` (Unix seconds, UTC): the local midnight of the day to look at.
pub fn plan_day(obs: Observer, start: f64) -> DayPlan {
    let end = start + DAY;
    let sun_alt = |t: f64| sun_horizontal(t, obs).alt;
    let sun_az = |t: f64| sun_horizontal(t, obs).az;

    // Sun
    let (rise, set, sun_up) = rise_set(&sun_alt, &|_| SUN_RISE_ALT, start, end);
    let state = if sun_up.is_empty() {
        SunState::PolarNight
    } else if rise.is_none() && set.is_none() {
        SunState::MidnightSun
    } else {
        SunState::Normal
    };
    let (noon, noon_alt) = culmination(&sun_alt, start, end);
    let twilight = |depth: f64| {
        let (dawn, dusk, _) = rise_set(&sun_alt, &|_| depth, start, end);
        Twilight { dawn, dusk }
    };
    // a band of altitudes, morning side then evening side
    let band = |lo: f64, hi: f64| {
        let in_band = |t: f64| {
            let a = sun_alt(t);
            a > lo && a <= hi
        };
        let sp = spans(&in_band, start, end, STEP);
        // split into the part before solar noon and the part after
        let am = sp.iter().find(|s| s.end <= noon + 1.0).copied();
        let pm = sp.iter().rev().find(|s| s.start >= noon - 1.0).copied();
        (am, pm)
    };
    let (golden_am, golden_pm) = band(GOLDEN_BOTTOM, GOLDEN_TOP);
    let (blue_am, blue_pm) = band(BLUE_BOTTOM, GOLDEN_BOTTOM);
    let sun = SunDay {
        state,
        rise,
        set,
        rise_az: rise.map(sun_az),
        set_az: set.map(sun_az),
        noon,
        noon_alt,
        noon_az: sun_az(noon),
        civil: twilight(CIVIL),
        nautical: twilight(NAUTICAL),
        astronomical: twilight(ASTRONOMICAL),
        golden_am,
        golden_pm,
        blue_am,
        blue_pm,
    };

    // Moon: its rise altitude depends (slightly) on its distance
    let moon_alt = |t: f64| moon_horizontal(t, obs).alt;
    let moon_az = |t: f64| moon_horizontal(t, obs).az;
    let (m_rise, m_set, _) = rise_set(&moon_alt, &moon_rise_altitude, start, end);
    let (m_transit, m_transit_alt) = culmination(&moon_alt, start, end);
    let moon = MoonDay {
        rise: m_rise,
        set: m_set,
        rise_az: m_rise.map(moon_az),
        set_az: m_set.map(moon_az),
        transit: m_transit,
        transit_alt: m_transit_alt,
        transit_az: moon_az(m_transit),
    };

    // Milky Way core
    let core_alt = |t: f64| core_horizontal(t, obs).alt;
    let core_az = |t: f64| core_horizontal(t, obs).az;
    let (c_rise, c_set, _) = rise_set(&core_alt, &|_| 0.0, start, end);
    let (c_transit, c_transit_alt) = culmination(&core_alt, start, end);
    let dark_core = |t: f64| core_alt(t) > CORE_MIN_ALT && sun_alt(t) < ASTRONOMICAL;
    let core = CoreDay {
        rise: c_rise,
        set: c_set,
        rise_az: c_rise.map(core_az),
        set_az: c_set.map(core_az),
        transit: c_transit,
        transit_alt: c_transit_alt,
        transit_az: core_az(c_transit),
        dark: spans(&dark_core, start, end, STEP),
    };

    DayPlan { start, sun, moon, core }
}

/// The stretches of the night that starts on the evening of the local day beginning at `day_start`
/// (12:00 to 12:00 the next day) in which the core is above [`CORE_MIN_ALT`] in an astronomically
/// dark sky. One night is one stretch, even across midnight.
pub fn dark_core_windows(obs: Observer, day_start: f64) -> Vec<Span> {
    let t0 = day_start + DAY / 2.0;
    let dark_core = |t: f64| sun_horizontal(t, obs).alt < ASTRONOMICAL && core_horizontal(t, obs).alt > CORE_MIN_ALT;
    spans(&dark_core, t0, t0 + DAY, STEP)
}

/// One night laid out for a year chart: the night that starts on the evening of a local day (12:00
/// to 12:00 the next day), as stretches of time.
#[derive(Clone, Debug, PartialEq)]
pub struct NightProfile {
    /// Local midnight of the evening the night starts.
    pub day_start: f64,
    /// The Sun below the horizon (sunset to sunrise); empty in the midnight sun.
    pub night: Vec<Span>,
    /// The Sun below -18 degrees.
    pub dark: Vec<Span>,
    /// The Moon above the horizon.
    pub moon_up: Vec<Span>,
    /// The core above [`CORE_MIN_ALT`] in the dark.
    pub core: Vec<Span>,
}

/// The night that starts on the evening of the local day beginning at `day_start`, in stretches.
pub fn night_profile(obs: Observer, day_start: f64, step: f64) -> NightProfile {
    let t0 = day_start + DAY / 2.0;
    let t1 = t0 + DAY;
    let sun = |t: f64| sun_horizontal(t, obs).alt;
    let core = |t: f64| core_horizontal(t, obs).alt;
    NightProfile {
        day_start,
        night: spans(&|t| sun(t) < SUN_RISE_ALT, t0, t1, step),
        dark: spans(&|t| sun(t) < ASTRONOMICAL, t0, t1, step),
        moon_up: spans(&|t| moon_horizontal(t, obs).alt > MOON_DOWN_ALT, t0, t1, step),
        core: spans(&|t| sun(t) < ASTRONOMICAL && core(t) > CORE_MIN_ALT, t0, t1, step),
    }
}

/// How good one night is for the Milky Way's core.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NightScore {
    /// Local midnight of the evening the night starts (Unix seconds, UTC).
    pub day_start: f64,
    /// Minutes with the core up and the sky astronomically dark.
    pub dark_core_minutes: f64,
    /// Of those, the minutes with the Moon also below the horizon.
    pub moonless_minutes: f64,
    /// Highest altitude of the core in the dark, degrees.
    pub best_altitude: f64,
}

/// Scores the night that starts on the evening of the local day beginning at `day_start`: from 12:00 to 12:00 the next day.
pub fn score_night(obs: Observer, day_start: f64, step: f64) -> NightScore {
    let t0 = day_start + DAY / 2.0;
    let t1 = t0 + DAY;
    let sun = |t: f64| sun_horizontal(t, obs).alt;
    let core = |t: f64| core_horizontal(t, obs).alt;
    let dark_core = |t: f64| sun(t) < ASTRONOMICAL && core(t) > CORE_MIN_ALT;
    let dark_core_minutes: f64 = spans(&dark_core, t0, t1, step).iter().map(Span::minutes).sum();
    let moonless = |t: f64| dark_core(t) && moon_horizontal(t, obs).alt < MOON_DOWN_ALT;
    let moonless_minutes: f64 = spans(&moonless, t0, t1, step).iter().map(Span::minutes).sum();
    let n = ((t1 - t0) / step).floor() as usize;
    let best_altitude = (0..=n).map(|i| t0 + i as f64 * step).filter(|&t| sun(t) < ASTRONOMICAL).map(core).fold(f64::NEG_INFINITY, f64::max);
    NightScore { day_start, dark_core_minutes, moonless_minutes, best_altitude: if best_altitude.is_finite() { best_altitude } else { -90.0 } }
}

/// Scores `days` nights from the local day beginning at `first_day_start`.
pub fn score_nights(obs: Observer, first_day_start: f64, days: usize, step: f64) -> Vec<NightScore> {
    (0..days.min(400)).map(|i| score_night(obs, first_day_start + i as f64 * DAY, step)).collect()
}

/// The Sun's, Moon's and core's directions at one moment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Skies {
    pub sun: Horizontal,
    pub moon: Horizontal,
    pub core: Horizontal,
}

pub fn skies_at(obs: Observer, unix: f64) -> Skies {
    Skies { sun: sun_horizontal(unix, obs), moon: moon_horizontal(unix, obs), core: core_horizontal(unix, obs) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::{Date, unix_from_civil};

    fn madrid() -> Observer {
        Observer::new(40.4168, -3.7038)
    }

    #[test]
    fn spans_find_the_edges_of_a_wave() {
        let inside = |t: f64| (t / 1000.0).sin() > 0.0;
        let s = spans(&inside, 0.0, 20_000.0, 300.0);
        // sin > 0 on (0, pi*1000), (2pi*1000, 3pi*1000), (4pi*1000, 5pi*1000) … but 20 000 < 6.4 pi*1000
        assert_eq!(s.len(), 4, "{s:?}");
        assert!(s[0].start.abs() < 1.0 || s[0].start < 1.0);
        assert!((s[0].end - std::f64::consts::PI * 1000.0).abs() < 0.01, "{:?}", s[0]);
        assert!((s[1].start - 2.0 * std::f64::consts::PI * 1000.0).abs() < 0.01, "{:?}", s[1]);
        assert!(spans(&inside, 5.0, 5.0, 300.0).is_empty());
        assert!(spans(&inside, 0.0, 10.0, 0.0).is_empty());
        assert!(spans(&|_| true, f64::NAN, 10.0, 1.0).is_empty());
    }

    #[test]
    fn midsummer_in_madrid() {
        // 21 June 2024; Madrid is UTC+2 in summer, so local midnight is 22:00 UTC the evening before
        let start = Date::new(2024, 6, 21).unix_midnight_utc() - 2.0 * 3600.0;
        let p = plan_day(madrid(), start);
        let (rise, set) = (p.sun.rise.expect("sunrise"), p.sun.set.expect("sunset"));
        // sunrise about 06:45 and sunset about 21:48 local
        assert!(((rise - start) / 3600.0 - 6.75).abs() < 0.12, "rise {}", (rise - start) / 3600.0);
        assert!(((set - start) / 3600.0 - 21.8).abs() < 0.12, "set {}", (set - start) / 3600.0);
        assert_eq!(p.sun.state, SunState::Normal);
        // azimuths: north-east and north-west of the east/west points
        assert!(p.sun.rise_az.expect("az") > 50.0 && p.sun.rise_az.expect("az") < 65.0, "{:?}", p.sun.rise_az);
        assert!(p.sun.set_az.expect("az") > 295.0 && p.sun.set_az.expect("az") < 310.0, "{:?}", p.sun.set_az);
        assert!((p.sun.noon_alt - 73.0).abs() < 0.3, "{}", p.sun.noon_alt);
        assert!((p.sun.noon_az - 180.0).abs() < 2.0);
        // the order of the light: astronomical < nautical < civil dawn < sunrise
        let (a, n, c) = (p.sun.astronomical.dawn.expect("astronomical"), p.sun.nautical.dawn.expect("nautical"), p.sun.civil.dawn.expect("civil"));
        assert!(a < n && n < c && c < rise, "{a} {n} {c} {rise}");
        let (a, n, c) = (p.sun.astronomical.dusk.expect("astronomical"), p.sun.nautical.dusk.expect("nautical"), p.sun.civil.dusk.expect("civil"));
        assert!(set < c && c < n && n < a, "{set} {c} {n} {a}");
        // golden hour: after dawn's blue hour, before sunset's… and the evening one ends after sunset
        let (g_pm, b_pm) = (p.sun.golden_pm.expect("golden"), p.sun.blue_pm.expect("blue"));
        assert!(g_pm.start < set && g_pm.end > set);
        assert!(b_pm.start >= g_pm.end - 1.0);
    }

    #[test]
    fn day_length_matches_the_formula_at_the_equator() {
        // at the equator on the equinox the day is 12 h plus about 7 minutes of refraction and radius
        let start = Date::new(2024, 3, 20).unix_midnight_utc();
        let p = plan_day(Observer::new(0.0, 0.0), start);
        let len = (p.sun.set.expect("set") - p.sun.rise.expect("rise")) / 3600.0;
        assert!((len - 12.12).abs() < 0.06, "{len}");
    }

    #[test]
    fn polar_day_and_night() {
        let obs = Observer::new(80.0, 10.0);
        let summer = plan_day(obs, Date::new(2024, 6, 21).unix_midnight_utc());
        assert_eq!(summer.sun.state, SunState::MidnightSun);
        assert!(summer.sun.rise.is_none() && summer.sun.set.is_none());
        let winter = plan_day(obs, Date::new(2024, 12, 21).unix_midnight_utc());
        assert_eq!(winter.sun.state, SunState::PolarNight);
        assert!(winter.sun.rise.is_none() && winter.sun.golden_am.is_none());
    }

    #[test]
    fn moon_rises_in_the_east_and_sets_in_the_west() {
        let start = Date::new(2024, 4, 23).unix_midnight_utc() - 2.0 * 3600.0;
        let p = plan_day(madrid(), start);
        // the full moon of 23 April 2024 rose near sunset
        let rise = p.moon.rise.expect("moonrise");
        assert!(p.moon.rise_az.expect("az") < 135.0, "{:?}", p.moon.rise_az);
        let sunset = plan_day(madrid(), start).sun.set.expect("sunset");
        assert!((rise - sunset).abs() < 3.0 * 3600.0, "full moon rises about sunset: {} h apart", (rise - sunset) / 3600.0);
        assert!(p.moon.transit_alt > 30.0 && p.moon.transit_alt < 90.0, "{}", p.moon.transit_alt);
    }

    #[test]
    fn core_season_in_madrid() {
        // in July the core is up in the dark; in December it is not
        let july = plan_day(madrid(), Date::new(2024, 7, 14).unix_midnight_utc() - 2.0 * 3600.0);
        let dark: f64 = july.core.dark.iter().map(Span::minutes).sum();
        assert!(dark > 60.0, "{dark} minutes");
        assert!((july.core.transit_alt - 21.0).abs() < 1.0, "{}", july.core.transit_alt);
        let dec = plan_day(madrid(), Date::new(2024, 12, 14).unix_midnight_utc() - 3600.0);
        assert!(dec.core.dark.is_empty(), "{:?}", dec.core.dark);
    }

    #[test]
    fn a_dark_night_is_one_window_across_midnight() {
        // Madrid, the night of 14 July 2024 (UTC+2): the core is up in the dark from about 23:43 to 04:04
        let start = Date::new(2024, 7, 14).unix_midnight_utc() - 2.0 * 3600.0;
        let w = dark_core_windows(madrid(), start);
        assert_eq!(w.len(), 1, "{w:?}");
        let (from, to) = ((w[0].start - start) / 3600.0, (w[0].end - start) / 3600.0);
        assert!((from - 23.7).abs() < 0.2, "starts {from}");
        assert!((to - 28.07).abs() < 0.2, "ends {to} hours after midnight");
        assert!((w[0].minutes() - 261.0).abs() < 12.0, "{}", w[0].minutes());
        // it agrees with the night's score
        let score = score_night(madrid(), start, 300.0);
        assert!((score.dark_core_minutes - w[0].minutes()).abs() < 1.0);
        // and there is none in midwinter
        assert!(dark_core_windows(madrid(), Date::new(2024, 12, 14).unix_midnight_utc() - 3600.0).is_empty());
    }

    #[test]
    fn a_night_profile_nests_the_twilights() {
        let start = Date::new(2024, 7, 14).unix_midnight_utc() - 2.0 * 3600.0;
        let p = night_profile(madrid(), start, 600.0);
        assert_eq!((p.night.len(), p.dark.len()), (1, 1), "{p:?}");
        let (night, dark) = (p.night[0], p.dark[0]);
        assert!(night.start < dark.start && dark.end < night.end, "the dark sky sits inside the night");
        let core = p.core.first().expect("the core is up in July");
        assert!(core.start >= dark.start - 1.0 && core.end <= dark.end + 1.0, "and the core inside that");
        // the Sun sets about 21:44 and rises about 06:57 the next morning
        assert!(((night.start - start) / 3600.0 - 21.7).abs() < 0.2, "{}", (night.start - start) / 3600.0);
        assert!(((night.end - start) / 3600.0 - 30.95).abs() < 0.2, "{}", (night.end - start) / 3600.0);
        // in midsummer at 80 N there is no night at all
        assert!(night_profile(Observer::new(80.0, 10.0), Date::new(2024, 6, 21).unix_midnight_utc(), 900.0).night.is_empty());
    }

    #[test]
    fn a_new_moon_night_beats_a_full_moon_night() {
        let obs = madrid();
        // new moon 8 April 2024 is too early in the year for the core; use the July new moon (5 July) and full moon (21 July)
        let dark = score_night(obs, Date::new(2024, 7, 5).unix_midnight_utc() - 2.0 * 3600.0, 600.0);
        let bright = score_night(obs, Date::new(2024, 7, 21).unix_midnight_utc() - 2.0 * 3600.0, 600.0);
        assert!(dark.dark_core_minutes > 60.0 && bright.dark_core_minutes > 60.0);
        assert!(dark.moonless_minutes > bright.moonless_minutes + 30.0, "{dark:?} vs {bright:?}");
        assert!(dark.moonless_minutes <= dark.dark_core_minutes);
        assert!(dark.best_altitude > 5.0 && dark.best_altitude < 30.0);
    }

    #[test]
    fn nights_are_scored_in_a_row() {
        let v = score_nights(madrid(), unix_from_civil(2024, 7, 1, 0, 0, 0), 5, 900.0);
        assert_eq!(v.len(), 5);
        assert!(v.windows(2).all(|w| (w[1].day_start - w[0].day_start - DAY).abs() < 1e-6));
        assert!(score_nights(madrid(), 0.0, 100_000, 3600.0).len() <= 400, "a request is capped");
    }

    #[test]
    fn hostile_numbers_do_not_panic() {
        for &(lat, lon, t) in &[(f64::NAN, f64::NAN, f64::NAN), (95.0, 400.0, 1.0e18), (-90.0, 0.0, -1.0e12), (90.0, 180.0, 0.0)] {
            let _ = plan_day(Observer::new(lat, lon), t);
            let _ = score_night(Observer::new(lat, lon), t, 600.0);
        }
    }
}
