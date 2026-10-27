//! Lining a body up with something on the ground: the direction from the camera to a target (distance, azimuth,
//! the angle its top is seen at), and the days and times the Sun or the Moon stand right behind it.

use crate::moon::{moon_horizontal, moon_phase};
use crate::sun::{Horizontal, Observer, sun_horizontal};
use crate::time::DAY;

/// Mean Earth radius, metres.
pub(crate) const EARTH_R: f64 = 6_371_000.0;
/// Light bends over long sight lines: the usual coefficient (the horizon dips a little less than geometry says).
pub(crate) const REFRACTION_K: f64 = 0.13;

/// Where a target is seen from the camera.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sight {
    /// Along the ground, metres.
    pub distance: f64,
    /// Degrees clockwise from north.
    pub azimuth: f64,
    /// Degrees above the camera's horizon of the target's top (`height` metres above the camera), with the
    /// Earth's curve and the usual refraction.
    pub altitude: f64,
}

/// The sight line from `(lat, lon)` to `(lat2, lon2)` (degrees), the target's top `height` metres above the camera.
pub fn sight(lat: f64, lon: f64, lat2: f64, lon2: f64, height: f64) -> Sight {
    let (p1, p2) = (lat.to_radians(), lat2.to_radians());
    let dl = (lon2 - lon).to_radians();
    let a = ((p2 - p1) / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
    let distance = 2.0 * EARTH_R * a.sqrt().clamp(0.0, 1.0).asin();
    let y = dl.sin() * p2.cos();
    let x = p1.cos() * p2.sin() - p1.sin() * p2.cos() * dl.cos();
    let azimuth = if distance > 0.0 { y.atan2(x).to_degrees().rem_euclid(360.0) } else { 0.0 };
    let height = if height.is_finite() { height } else { 0.0 };
    let altitude = if distance > 0.5 {
        let drop = distance * distance * (1.0 - REFRACTION_K) / (2.0 * EARTH_R);
        ((height - drop) / distance).atan().to_degrees()
    } else {
        0.0
    };
    Sight { distance, azimuth, altitude }
}

/// The eye above the ground at the camera (metres).
pub const EYE: f64 = 1.6;

/// The ground between the camera and the target, with the Earth's curve (and the usual refraction) folded in, and
/// whether it hides the target's top.
#[derive(Clone, Debug, PartialEq)]
pub struct Profile {
    /// Along the ground (metres) and the ground's height as the sight line sees it (metres).
    pub points: Vec<(f64, f64)>,
    /// The eye at the camera and the target's top, on the same scale.
    pub eye: f64,
    pub top: f64,
    /// Where the ground rises highest above the sight line, if it does (distance, metres above the line).
    pub block: Option<(f64, f64)>,
}

/// The profile over `ground` (heights in metres, evenly spaced from the camera to the target, both included) for a
/// sight line `distance` metres long to an object `object` metres tall.
pub fn profile(distance: f64, ground: &[f64], object: f64) -> Option<Profile> {
    let n = ground.len();
    if n < 2 || !(distance.is_finite() && distance > 0.0) || ground.iter().any(|h| !h.is_finite()) {
        return None;
    }
    let object = if object.is_finite() { object.max(0.0) } else { 0.0 };
    let re = EARTH_R / (1.0 - REFRACTION_K);
    let step = distance / (n - 1) as f64;
    let points: Vec<(f64, f64)> = ground
        .iter()
        .enumerate()
        .map(|(i, h)| {
            let d = i as f64 * step;
            // the ground bulges up between the two ends
            (d, h + d * (distance - d) / (2.0 * re))
        })
        .collect();
    let eye = points.first().map_or(0.0, |p| p.1) + EYE;
    let top = points.last().map_or(0.0, |p| p.1) + object;
    let line = |d: f64| eye + (top - eye) * d / distance;
    let mut block: Option<(f64, f64)> = None;
    // the ends themselves don't count (the camera's own ground, the target's foot)
    for &(d, h) in points.iter().skip(2).take(n.saturating_sub(4)) {
        let over = h - line(d);
        if over > 2.0 && block.is_none_or(|(_, o)| over > o) {
            block = Some((d, over));
        }
    }
    Some(Profile { points, eye, top, block })
}

/// The shadow an object `height` metres tall casts with the Sun at `sun`: its length (metres) and its direction
/// (degrees from north). None with the Sun under half a degree (the shadow runs to the horizon).
pub fn shadow(height: f64, sun: Horizontal) -> Option<(f64, f64)> {
    if !(height.is_finite() && height > 0.0 && sun.alt.is_finite() && sun.alt > 0.5) {
        return None;
    }
    Some((height / sun.alt.to_radians().tan(), (sun.az + 180.0).rem_euclid(360.0)))
}

/// The point `distance` metres from `(lat, lon)` along `azimuth` (degrees), on a sphere.
pub fn destination(lat: f64, lon: f64, azimuth: f64, distance: f64) -> (f64, f64) {
    let (p1, l1, b) = (lat.to_radians(), lon.to_radians(), azimuth.to_radians());
    let a = distance / EARTH_R;
    let p2 = (p1.sin() * a.cos() + p1.cos() * a.sin() * b.cos()).clamp(-1.0, 1.0).asin();
    let l2 = l1 + (b.sin() * a.sin() * p1.cos()).atan2(a.cos() - p1.sin() * p2.sin());
    (p2.to_degrees(), (l2.to_degrees() + 540.0).rem_euclid(360.0) - 180.0)
}

/// Refraction near the horizon (Bennett), degrees, for a geometric altitude: what the eye adds.
pub fn refraction(alt: f64) -> f64 {
    if alt < -2.0 {
        return 0.0;
    }
    let h = alt.max(-1.9);
    1.0 / (h + 7.31 / (h + 4.4)).to_radians().tan() / 60.0
}

/// Which body lines up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Body {
    Sun,
    Moon,
}

/// A moment the Sun or the Moon is right behind the target.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Alignment {
    pub unix: f64,
    pub body: Body,
    /// The body's apparent altitude then (degrees, refraction included).
    pub alt: f64,
    /// The body's centre above (+) or below (−) the target's top, degrees.
    pub above: f64,
    /// The Moon's lit fraction (1 for the Sun).
    pub illumination: f64,
}

/// How far above or below the target's top the body's centre may be to count (degrees): from half a disc below
/// (it sits on it) to a few discs above.
pub const BELOW: f64 = 0.6;
pub const ABOVE: f64 = 2.5;

/// The alignments with a target seen at `sight` from `obs`, over `days` days from `from` (Unix seconds), first first.
/// Each body's azimuth is sampled every few minutes and each crossing of the target's azimuth refined; the crossings
/// with the body near the target's top are kept (a body that is up all day crosses once a day, the Sun twice near
/// the poles).
pub fn alignments(obs: Observer, sight: Sight, from: f64, days: u32) -> Vec<Alignment> {
    const STEP: f64 = 360.0; // six minutes
    let mut out = Vec::new();
    if !(from.is_finite() && sight.azimuth.is_finite() && sight.altitude.is_finite()) {
        return out;
    }
    let days = days.min(3660);
    let end = from + f64::from(days) * DAY;
    for (body, f) in [(Body::Sun, sun_horizontal as fn(f64, Observer) -> Horizontal), (Body::Moon, moon_horizontal)] {
        let off = |t: f64| -> (f64, f64) {
            let h = f(t, obs);
            let d = (h.az - sight.azimuth + 540.0).rem_euclid(360.0) - 180.0;
            (d, h.alt + refraction(h.alt))
        };
        let mut t = from;
        let mut prev = off(t);
        while t < end {
            let t2 = t + STEP;
            let cur = off(t2);
            // a crossing of the azimuth (not the jump from +180 to −180 on the far side), near the target's height
            let near = |alt: f64| alt > sight.altitude - 6.0 && alt < sight.altitude + 10.0;
            if prev.0.signum() != cur.0.signum() && prev.0.abs() < 30.0 && cur.0.abs() < 30.0 && (near(prev.1) || near(cur.1)) {
                let (mut a, mut b, mut da) = (t, t2, prev.0);
                for _ in 0..20 {
                    let m = (a + b) / 2.0;
                    let dm = off(m).0;
                    if dm.signum() == da.signum() {
                        a = m;
                        da = dm;
                    } else {
                        b = m;
                    }
                }
                let at = (a + b) / 2.0;
                let alt = off(at).1;
                let above = alt - sight.altitude;
                if (-BELOW..=ABOVE).contains(&above) {
                    let illumination = if body == Body::Moon { moon_phase(at).illumination } else { 1.0 };
                    out.push(Alignment { unix: at, body, alt, above, illumination });
                }
            }
            t = t2;
            prev = cur;
        }
    }
    out.sort_by(|a, b| a.unix.total_cmp(&b.unix));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sight_line_has_the_right_length_and_bearing() {
        // a degree of latitude northwards: ~111 km, due north
        let s = sight(40.0, -3.0, 41.0, -3.0, 0.0);
        assert!((s.distance - 111_195.0).abs() < 200.0, "{s:?}");
        assert!(s.azimuth.abs() < 1e-6 || (s.azimuth - 360.0).abs() < 1e-6);
        // due east along the parallel: ~90°, and a flat target far away sinks below the horizon
        let e = sight(40.0, -3.0, 40.0, -2.9, 0.0);
        assert!((e.azimuth - 90.0).abs() < 0.1, "{e:?}");
        assert!(e.altitude < 0.0);
        // a 300 m tower 1 km away: about 16.7° up
        let t = sight(40.0, -3.0, 40.009, -3.0, 300.0);
        assert!((t.altitude - 16.6).abs() < 0.3, "{t:?}");
        // the same point, or nonsense heights, don't break anything
        assert_eq!(sight(1.0, 1.0, 1.0, 1.0, f64::NAN).distance, 0.0);
    }

    #[test]
    fn the_sun_lines_up_with_a_target_due_west_around_the_equinoxes() {
        // Madrid, a flat target due west on the horizon: the Sun sets behind it in late March and late September
        let obs = Observer::new(40.4168, -3.7038);
        let s = sight(40.4168, -3.7038, 40.4168, -3.80, 0.0);
        let from = crate::time::Date { year: 2026, month: 1, day: 1 }.unix_midnight_utc();
        let found = alignments(obs, s, from, 365);
        let sun: Vec<_> = found.iter().filter(|a| a.body == Body::Sun).collect();
        assert!(!sun.is_empty(), "no sunsets in line");
        let months: Vec<u32> = sun.iter().map(|a| crate::time::Date::from_unix(a.unix, 60).month).collect();
        assert!(months.iter().all(|m| *m == 3 || *m == 4 || *m == 9 || *m == 10), "{months:?}");
        assert!(sun.iter().all(|a| (-BELOW..=ABOVE).contains(&a.above)));
        // and the Moon lines up too, some time in the year
        assert!(found.iter().any(|a| a.body == Body::Moon));
        // sorted
        assert!(found.windows(2).all(|w| w[0].unix <= w[1].unix));
    }

    #[test]
    fn a_ridge_between_hides_the_target_and_a_valley_does_not() {
        // 10 km, flat at 100 m, a target 50 m tall on the far end
        let flat = vec![100.0; 21];
        let p = profile(10_000.0, &flat, 50.0).unwrap();
        assert_eq!(p.points.len(), 21);
        assert!((p.eye - 101.6).abs() < 1e-9 && (p.top - 150.0).abs() < 1e-9);
        assert!(p.block.is_none(), "{:?}", p.block);
        // a 200 m ridge half way
        let mut ridge = flat.clone();
        ridge[10] = 300.0;
        let b = profile(10_000.0, &ridge, 50.0).unwrap().block.unwrap();
        assert!((b.0 - 5000.0).abs() < 1.0 && b.1 > 150.0, "{b:?}");
        // the Earth's curve: over 60 km the middle bulges ~ 60 m (with refraction)
        let far = profile(60_000.0, &[0.0; 3], 0.0).unwrap();
        assert!((far.points[1].1 - 61.0).abs() < 2.0, "{:?}", far.points[1]);
        // nonsense
        assert!(profile(0.0, &flat, 1.0).is_none());
        assert!(profile(10.0, &[1.0], 1.0).is_none());
        assert!(profile(10.0, &[1.0, f64::NAN], 1.0).is_none());
    }

    #[test]
    fn shadows_grow_as_the_sun_sinks() {
        let (len, az) = shadow(10.0, Horizontal { az: 200.0, alt: 45.0 }).unwrap();
        assert!((len - 10.0).abs() < 1e-9 && (az - 20.0).abs() < 1e-9);
        let (long, _) = shadow(10.0, Horizontal { az: 270.0, alt: 5.0 }).unwrap();
        assert!(long > 100.0);
        assert!(shadow(10.0, Horizontal { az: 0.0, alt: 0.2 }).is_none());
        assert!(shadow(0.0, Horizontal { az: 0.0, alt: 30.0 }).is_none());
        // and the end of a shadow is where it should be: 1 km due east along the equator
        let (la, lo) = destination(0.0, 0.0, 90.0, 1000.0);
        assert!(la.abs() < 1e-9 && (lo - 0.008_993).abs() < 1e-5, "{la} {lo}");
    }

    #[test]
    fn bad_input_finds_nothing_and_does_not_hang() {
        let obs = Observer::new(0.0, 0.0);
        let s = Sight { distance: 1.0, azimuth: f64::NAN, altitude: 0.0 };
        assert!(alignments(obs, s, 0.0, 10).is_empty());
        assert!(alignments(obs, sight(0.0, 0.0, 0.0, 1.0, 0.0), f64::INFINITY, 10).is_empty());
        assert!(refraction(-10.0) == 0.0 && refraction(0.0) > 0.4 && refraction(0.0) < 0.6);
    }
}
