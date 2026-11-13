//! Besselian elements of a solar eclipse and the track of the Moon's shadow they give on the oblate Earth.
//!
//! The Sun and Moon of this crate (Meeus's series, apparent places of date) are turned into the usual description of
//! an eclipse: a fundamental plane through the Earth's centre, perpendicular to the shadow axis, in which the axis
//! sits at (x, y) and the penumbral and umbral cones have radii `l1` and `l2` (negative when the umbra reaches the
//! ground). The axis and the edge of the umbra are then intersected with the Earth's ellipsoid, not a sphere, for the
//! central line, the two limits of the path, its width and the duration of totality (or annularity) on the line.
//!
//! Constants follow Espenak's conventions (the Sun's radius 696,000 km, the Moon's k = 0.2725076 for the penumbra and
//! 0.272281 for the umbra, the Earth's equatorial radius 6378.14 km), so the figures compare directly with NASA's.
//!
//! Sources: Jean Meeus, *Astronomical Algorithms* 2nd ed., ch. 54; *Explanatory Supplement to the Astronomical
//! Almanac*, ch. 11 (the geometry); formulas, not code.

use crate::eclipse::{EclipsePath, PathPoint};
use crate::moon::moon_coords;
use crate::sun::{Observer, gmst, sun_coords, sun_horizontal};
use crate::time::jd_from_unix;
use std::f64::consts::TAU;

/// The Earth's equatorial radius, km: the unit of every length of the elements.
const A_KM: f64 = 6_378.14;
/// The square of the Earth's eccentricity (flattening 1/298.257).
const E2: f64 = 0.006_694_38;
const SUN_KM: f64 = 696_000.0;
const K_PENUMBRA: f64 = 0.272_507_6;
const K_UMBRA: f64 = 0.272_281;
/// Half the spacing of the instants used to get the rates of the elements, seconds.
const H: f64 = 30.0;

/// The Besselian elements at an instant. Lengths are in Earth equatorial radii, angles in radians.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Elements {
    /// The shadow axis in the fundamental plane (x east, y north).
    pub x: f64,
    pub y: f64,
    /// The declination of the axis, and the Greenwich hour angle of its direction (including the equation of the equinoxes).
    pub d: f64,
    pub mu: f64,
    /// The radii of the penumbra and the umbra in the fundamental plane (the umbra's is negative for a total eclipse).
    pub l1: f64,
    pub l2: f64,
    pub tan_f1: f64,
    pub tan_f2: f64,
}

impl Elements {
    /// The distance of the axis from the Earth's centre (gamma, when it is least).
    pub fn gamma(&self) -> f64 {
        self.x.hypot(self.y)
    }
}

fn vector(ra: f64, dec: f64, dist: f64) -> [f64; 3] {
    let (ra, dec) = (ra.to_radians(), dec.to_radians());
    [dist * dec.cos() * ra.cos(), dist * dec.cos() * ra.sin(), dist * dec.sin()]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// The elements at a Unix time (UTC), or `None` for a time that is not a number.
pub fn elements(unix: f64) -> Option<Elements> {
    if !unix.is_finite() {
        return None;
    }
    let jd = jd_from_unix(unix);
    let (s, m) = (sun_coords(jd), moon_coords(jd));
    let sv = vector(s.ra, s.dec, s.dist_km);
    let mv = vector(m.ra, m.dec, m.dist_km);
    let axis = [sv[0] - mv[0], sv[1] - mv[1], sv[2] - mv[2]];
    let len = dot(axis, axis).sqrt();
    if len.is_nan() || len <= SUN_KM {
        return None;
    }
    let z = [axis[0] / len, axis[1] / len, axis[2] / len];
    let d = z[2].clamp(-1.0, 1.0).asin();
    let a = z[1].atan2(z[0]);
    let ((sa, ca), (sd, cd)) = (a.sin_cos(), d.sin_cos());
    let ex = [-sa, ca, 0.0];
    let ey = [-sd * ca, -sd * sa, cd];
    let (x, y, zm) = (dot(mv, ex) / A_KM, dot(mv, ey) / A_KM, dot(mv, z) / A_KM);
    let sin_f1 = (SUN_KM + K_PENUMBRA * A_KM) / len;
    let sin_f2 = (SUN_KM - K_UMBRA * A_KM) / len;
    let (cos_f1, cos_f2) = ((1.0 - sin_f1 * sin_f1).sqrt(), (1.0 - sin_f2 * sin_f2).sqrt());
    let (tan_f1, tan_f2) = (sin_f1 / cos_f1, sin_f2 / cos_f2);
    // the Earth's rotation angle: mean sidereal time plus the equation of the equinoxes (nutation in longitude, its main term)
    let omega = (125.04 - 1_934.136 * crate::sun::centuries(jd)).to_radians();
    let equinoxes = -0.004_78 * omega.sin() * crate::sun::obliquity(crate::sun::centuries(jd)).to_radians().cos();
    let mu = (gmst(jd) + equinoxes).to_radians() - a;
    Some(Elements { x, y, d, mu: mu.rem_euclid(TAU), l1: zm * tan_f1 + K_PENUMBRA / cos_f1, l2: zm * tan_f2 - K_UMBRA / cos_f2, tan_f1, tan_f2 })
}

/// A point of the Earth's surface met by the line through the fundamental plane at (xi, eta) parallel to the axis,
/// on the side facing the Sun.
#[derive(Clone, Copy, Debug)]
struct Ground {
    /// Geodetic latitude and east longitude, degrees.
    lat: f64,
    lon: f64,
    /// Height of the point above the fundamental plane, Earth radii.
    zeta: f64,
    /// The component of the point's position that lies in the plane of the equator, towards the hour angle of the axis.
    w: f64,
}

fn ground(el: &Elements, xi: f64, eta: f64) -> Option<Ground> {
    let (s, c) = el.d.sin_cos();
    let q = 1.0 / (1.0 - E2);
    // xi^2 + (zeta c - eta s)^2 + q (eta c + zeta s)^2 = 1: the ellipsoid in the frame of the plane
    let a = c * c + q * s * s;
    let b = eta * s * c * (q - 1.0);
    let k = xi * xi + eta * eta * (s * s + q * c * c) - 1.0;
    let disc = b * b - a * k;
    if disc.is_nan() || disc < 0.0 || a.is_nan() || a <= 0.0 {
        return None;
    }
    let zeta = (-b + disc.sqrt()) / a;
    let (z, w) = (eta * c + zeta * s, zeta * c - eta * s);
    let lat_geocentric = z.atan2((xi * xi + w * w).sqrt());
    let lat = (lat_geocentric.tan() * q).atan().to_degrees();
    let lon = ((xi.atan2(w) - el.mu).to_degrees() + 540.0).rem_euclid(360.0) - 180.0;
    Some(Ground { lat, lon, zeta, w })
}

/// The east and north distance in km from one point to another, on the ellipsoid, for nearby points.
fn offset_km(from: (f64, f64), to: (f64, f64)) -> (f64, f64) {
    let phi = from.0.to_radians();
    let w = (1.0 - E2 * phi.sin().powi(2)).sqrt();
    let meridian = A_KM * (1.0 - E2) / (w * w * w);
    let prime_vertical = A_KM / w;
    let dlon = (to.1 - from.1 + 540.0).rem_euclid(360.0) - 180.0;
    (dlon.to_radians() * prime_vertical * phi.cos(), (to.0 - from.0).to_radians() * meridian)
}

/// The point of the umbra's (or antumbra's) edge on the ground in the direction `theta` from the axis in the plane.
fn footprint(el: &Elements, centre: &Ground, theta: f64) -> Option<Ground> {
    let mut zeta = centre.zeta;
    let mut found = None;
    for _ in 0..8 {
        let r = (el.l2 - zeta * el.tan_f2).abs();
        let g = ground(el, el.x + r * theta.cos(), el.y + r * theta.sin())?;
        let settled = (g.zeta - zeta).abs() < 1e-7;
        zeta = g.zeta;
        found = Some(g);
        if settled {
            break;
        }
    }
    found
}

/// The change of the elements per second round an instant, by central differences.
fn rates(unix: f64) -> Option<(f64, f64, f64, f64)> {
    let (a, b) = (elements(unix - H)?, elements(unix + H)?);
    let wrap = |d: f64| (d + std::f64::consts::PI).rem_euclid(TAU) - std::f64::consts::PI;
    let span = 2.0 * H;
    Some(((b.x - a.x) / span, (b.y - a.y) / span, (b.d - a.d) / span, wrap(b.mu - a.mu) / span))
}

/// The track's point at an instant, with the distance of the axis from the Earth's centre. `None` when the axis misses
/// the Earth or the Sun is not up where it meets it.
fn point(unix: f64) -> Option<(PathPoint, f64)> {
    let el = elements(unix)?;
    let centre = ground(&el, el.x, el.y)?;
    let sun_alt = sun_horizontal(unix, Observer::new(centre.lat, centre.lon)).alt;
    if sun_alt <= 0.0 {
        return None;
    }
    let (l1, l2) = (el.l1 - centre.zeta * el.tan_f1, el.l2 - centre.zeta * el.tan_f2);

    // how long the shadow takes to pass a point of the central line: the shadow moves across the plane, and the
    // observer is carried along by the Earth's turning
    let (dx, dy, dd, dmu) = rates(unix)?;
    let xi_rate = dmu * centre.w;
    let eta_rate = dmu * el.x * el.d.sin() - centre.zeta * dd;
    let relative = (dx - xi_rate).hypot(dy - eta_rate);
    let duration_s = if relative > 0.0 { 2.0 * l2.abs() / relative } else { 0.0 };

    // the direction the shadow is travelling over the ground, from the centre a moment before to a moment after
    let before = elements(unix - H).and_then(|e| ground(&e, e.x, e.y));
    let after = elements(unix + H).and_then(|e| ground(&e, e.x, e.y));
    let here = (centre.lat, centre.lon);
    let (de, dn) = match (before, after) {
        (Some(b), Some(a)) => offset_km((b.lat, b.lon), (a.lat, a.lon)),
        (Some(b), None) => offset_km((b.lat, b.lon), here),
        (None, Some(a)) => offset_km(here, (a.lat, a.lon)),
        (None, None) => (0.0, 0.0),
    };
    let norm = de.hypot(dn);
    // the two limits of the path: the farthest the umbra's edge reaches to either side of the direction of travel
    let mut left: Option<(f64, (f64, f64))> = None;
    let mut right: Option<(f64, (f64, f64))> = None;
    if norm > 1e-6 {
        let (ux, uy) = (de / norm, dn / norm);
        for i in 0..48 {
            let theta = f64::from(i) * TAU / 48.0;
            let Some(g) = footprint(&el, &centre, theta) else { continue };
            let (oe, on) = offset_km(here, (g.lat, g.lon));
            let side = -oe * uy + on * ux;
            let p = (g.lat, g.lon);
            if left.is_none_or(|(s, _)| side > s) {
                left = Some((side, p));
            }
            if right.is_none_or(|(s, _)| side < s) {
                right = Some((side, p));
            }
        }
    }
    let ((sl, pl), (sr, pr)) = match (left, right) {
        (Some(l), Some(r)) => (l, r),
        // the edge cannot be followed (the axis grazes the Earth's limb): the circle of the cone, not stretched
        _ => ((l2.abs() * A_KM, here), (-l2.abs() * A_KM, here)),
    };
    let half = (sl - sr) / 2.0;
    let gamma = el.gamma();
    // the ratio of the Moon's to the Sun's apparent diameter on the line, which is the magnitude listed for central eclipses
    let magnitude = if l1 + l2 > 0.0 { (l1 - l2) / (l1 + l2) } else { 0.0 };
    let point = PathPoint {
        unix,
        lat: centre.lat,
        lon: centre.lon,
        half_width_km: if l2 < 0.0 { half } else { -half },
        penumbra_km: l1 * A_KM,
        sun_alt,
        duration_s,
        magnitude,
        left: pl,
        right: pr,
    };
    Some((point, gamma))
}

/// The instant, within `span` seconds of `centre`, at which the axis passes nearest the Earth's centre, by narrowing the
/// interval (the distance falls and rises once there).
fn nearest_axis(centre: f64, span: f64) -> f64 {
    let g = |t: f64| elements(t).map_or(f64::INFINITY, |e| e.gamma());
    let (mut a, mut b) = (centre - span, centre + span);
    for _ in 0..40 {
        let (m1, m2) = (a + (b - a) / 3.0, b - (b - a) / 3.0);
        if g(m1) < g(m2) {
            b = m2;
        } else {
            a = m1;
        }
    }
    (a + b) / 2.0
}

/// The instants every twenty minutes round a new moon, and the one among them where the axis is nearest the Earth's
/// centre; `None` when it stays well clear of the Earth all through (a partial eclipse, or none).
fn coarse_nearest(new_moon: f64) -> Option<f64> {
    let (t, gamma) = (0..=21)
        .map(|i| new_moon - 3.5 * 3600.0 + f64::from(i) * 1200.0)
        .filter_map(|t| elements(t).map(|e| (t, e.gamma())))
        .min_by(|a, b| a.1.total_cmp(&b.1))?;
    (gamma < 1.2).then_some(t)
}

/// The point of greatest eclipse of the eclipse round a new moon, where the axis passes nearest the Earth's centre: the
/// place, the moment, the width of the path and the duration there. `None` when the axis misses the Earth.
pub fn greatest_point(new_moon: f64) -> Option<PathPoint> {
    if !new_moon.is_finite() {
        return None;
    }
    let t = nearest_axis(coarse_nearest(new_moon)?, 1200.0);
    point(t).map(|(p, _)| p)
}

/// The track of the Moon's shadow axis over the Earth round a new moon: a point every two minutes within three and a
/// half hours of it, plus the instant of greatest eclipse (when the axis passes nearest the Earth's centre). Only the
/// sunlit part (the Sun above the horizon) is kept. `None` when the axis misses the Earth: a partial eclipse.
pub fn track(new_moon: f64) -> Option<EclipsePath> {
    if !new_moon.is_finite() {
        return None;
    }
    // a coarse look first: an axis that stays well clear of the Earth all through needs no more work
    coarse_nearest(new_moon)?;
    let step = 120.0;
    let mut points = Vec::new();
    let mut gamma = Vec::new();
    for i in 0..=(7.0 * 3600.0 / step) as i32 {
        if let Some((p, g)) = point(new_moon - 3.5 * 3600.0 + f64::from(i) * step) {
            points.push(p);
            gamma.push(g);
        }
    }
    let nearest = gamma.iter().enumerate().min_by(|a, b| a.1.total_cmp(b.1)).map(|(i, _)| i)?;
    // the greatest eclipse between the two neighbours of the nearest sample
    let mut greatest = nearest;
    if let Some(t0) = points.get(nearest).map(|p| p.unix)
        && let Some((p, _)) = point(nearest_axis(t0, step))
    {
        let at = points.partition_point(|q| q.unix < p.unix);
        points.insert(at, p);
        greatest = at;
    }
    (points.len() >= 2).then_some(EclipsePath { points, greatest })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::moon::{Principal, next_phase};
    use crate::time::{delta_t, unix_from_civil};

    fn km(a: (f64, f64), b: (f64, f64)) -> f64 {
        let (e, n) = offset_km(a, b);
        e.hypot(n)
    }

    /// The seconds past `h:m:s` Terrestrial Time on a date at which the point's instant falls.
    fn tt_after(p: &PathPoint, day: (i32, u32, u32), hms: (u32, u32, f64)) -> f64 {
        let reference = unix_from_civil(day.0, day.1, day.2, hms.0, hms.1, 0) + hms.2;
        p.unix + delta_t(p.unix) - reference
    }

    fn greatest_of(year: i32, month: u32, day: u32) -> (EclipsePath, PathPoint) {
        let nm = next_phase(unix_from_civil(year, month, day, 0, 0, 0), Principal::New).expect("a new moon");
        let path = track(nm).expect("the axis meets the Earth");
        let g = path.points[path.greatest];
        (path, g)
    }

    #[test]
    fn the_total_eclipse_of_august_2027_agrees_with_espenaks_figures() {
        let (_, g) = greatest_of(2027, 8, 1);
        // 10:07:50.2 TD at 25°30.2'N 33°11.0'E, path 257.7 km wide, 6m22.6s, magnitude 1.0790, Sun 81.7 degrees high
        let late = tt_after(&g, (2027, 8, 2), (10, 7, 50.2));
        assert!(late.abs() < 45.0, "greatest eclipse {late:.1} s off");
        assert!(km((g.lat, g.lon), (25.503, 33.183)) < 25.0, "{g:?}");
        assert!((g.half_width_km * 2.0 - 257.7).abs() < 6.0, "width {:.1} km", g.half_width_km * 2.0);
        assert!((g.duration_s - 382.6).abs() < 8.0, "{:.1} s", g.duration_s);
        assert!((g.magnitude - 1.0790).abs() < 0.004, "{}", g.magnitude);
        assert!((g.sun_alt - 81.7).abs() < 1.5, "{}", g.sun_alt);
        assert!(g.half_width_km > 0.0, "total");
    }

    #[test]
    fn the_total_eclipse_of_april_2024_and_the_annular_one_of_october_2024() {
        let (_, g) = greatest_of(2024, 4, 1);
        assert!(tt_after(&g, (2024, 4, 8), (18, 18, 29.4)).abs() < 45.0);
        assert!(km((g.lat, g.lon), (25.290, -104.138)) < 25.0, "{g:?}");
        assert!((g.half_width_km * 2.0 - 197.5).abs() < 6.0, "{:.1}", g.half_width_km * 2.0);
        assert!((g.duration_s - 268.1).abs() < 8.0, "{:.1}", g.duration_s);
        // the annular one: the width is negative in the point, 266.5 km and 7m25.1s
        let (_, a) = greatest_of(2024, 9, 25);
        assert!(tt_after(&a, (2024, 10, 2), (18, 46, 13.3)).abs() < 45.0);
        assert!(km((a.lat, a.lon), (-21.953, -114.508)) < 25.0, "{a:?}");
        assert!(a.half_width_km < 0.0, "annular");
        assert!((a.half_width_km.abs() * 2.0 - 266.5).abs() < 6.0, "{:.1}", a.half_width_km * 2.0);
        assert!((a.duration_s - 445.1).abs() < 10.0, "{:.1}", a.duration_s);
        assert!((a.magnitude - 0.9326).abs() < 0.004, "{}", a.magnitude);
    }

    #[test]
    fn the_limits_of_the_path_straddle_the_central_line() {
        let (path, g) = greatest_of(2027, 8, 1);
        let (l, r) = (km((g.lat, g.lon), g.left), km((g.lat, g.lon), g.right));
        // each side about half the width (the slant makes the two a little different)
        assert!((l + r - 257.7).abs() < 8.0 && l > 100.0 && r > 100.0, "{l:.1} {r:.1}");
        // every point of the path has its limits on either side, the same way round all along it
        let side = |p: &PathPoint, to: (f64, f64)| {
            let (e, n) = offset_km((p.lat, p.lon), to);
            (e, n)
        };
        for w in path.points.windows(2) {
            let [a, b] = w else { continue };
            // near the terminator the shadow is smeared along the ground and "sides" lose their meaning
            if (b.unix - a.unix) > 200.0 || a.sun_alt < 12.0 {
                continue;
            }
            let (de, dn) = offset_km((a.lat, a.lon), (b.lat, b.lon));
            let (le, ln) = side(a, a.left);
            let (re, rn) = side(a, a.right);
            // left of the direction of travel is positive on the cross product
            assert!(de * ln - dn * le > -1.0 && de * rn - dn * re < 1.0, "{a:?}");
        }
    }

    #[test]
    fn the_greatest_point_alone_is_the_one_of_the_track() {
        let nm = next_phase(unix_from_civil(2027, 7, 28, 0, 0, 0), Principal::New).expect("a new moon");
        let (alone, path) = (greatest_point(nm).expect("a point"), track(nm).expect("a track"));
        let g = path.points[path.greatest];
        assert!((alone.unix - g.unix).abs() < 1.0 && (alone.duration_s - g.duration_s).abs() < 0.5, "{alone:?} {g:?}");
        assert!(greatest_point(f64::NAN).is_none());
        let none = next_phase(unix_from_civil(2025, 5, 20, 0, 0, 0), Principal::New).expect("a new moon");
        assert!(greatest_point(none).is_none());
    }

    #[test]
    fn a_partial_eclipse_has_no_track() {
        // 29 March 2025: the axis passed north of the Earth
        let nm = next_phase(unix_from_civil(2025, 3, 25, 0, 0, 0), Principal::New).expect("a new moon");
        assert!(track(nm).is_none());
        assert!(track(f64::NAN).is_none());
        // an ordinary new moon too
        let nm = next_phase(unix_from_civil(2025, 5, 20, 0, 0, 0), Principal::New).expect("a new moon");
        assert!(track(nm).is_none());
    }

    /// The Besselian local solution: the times at which the shadow's edge reaches an observer (the standard method),
    /// to compare with the direct one of `eclipse`: `(c2, c3)` of the totality.
    fn local_totality(obs: Observer, around: f64) -> Option<(f64, f64)> {
        let inside = |t: f64| -> f64 {
            let Some(el) = elements(t) else { return f64::NAN };
            let u = (0.996_647_19 * obs.lat.to_radians().tan()).atan();
            let (rho_sin, rho_cos) = (0.996_647_19 * u.sin(), u.cos());
            let h = el.mu + obs.lon.to_radians();
            let (s, c) = el.d.sin_cos();
            let xi = rho_cos * h.sin();
            let eta = rho_sin * c - rho_cos * h.cos() * s;
            let zeta = rho_sin * s + rho_cos * h.cos() * c;
            let m = (el.x - xi).hypot(el.y - eta);
            (el.l2 - zeta * el.tan_f2).abs() - m
        };
        let mut t = around - 1800.0;
        let mut found = Vec::new();
        while t < around + 1800.0 {
            let (a, b) = (inside(t), inside(t + 5.0));
            if (a < 0.0) != (b < 0.0) {
                let (mut lo, mut hi) = (t, t + 5.0);
                for _ in 0..30 {
                    let mid = (lo + hi) / 2.0;
                    if (inside(mid) < 0.0) == (inside(lo) < 0.0) {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                found.push((lo + hi) / 2.0);
            }
            t += 5.0;
        }
        match found.as_slice() {
            [a, b] => Some((*a, *b)),
            _ => None,
        }
    }

    #[test]
    fn the_besselian_local_solution_and_the_direct_one_agree_in_the_path() {
        for (name, lat, lon) in [("Cádiz", 36.53, -6.29), ("Málaga", 36.72, -4.42), ("Melilla", 35.29, -2.94)] {
            let obs = Observer::new(lat, lon);
            let direct = crate::eclipse::solar_eclipses(unix_from_civil(2027, 8, 1, 0, 0, 0), unix_from_civil(2027, 8, 3, 0, 0, 0), obs)[0];
            let (c2, c3) = local_totality(obs, direct.greatest).expect("totality");
            let (d2, d3) = (direct.c2.expect("c2"), direct.c3.expect("c3"));
            assert!((c2 - d2).abs() < 12.0 && (c3 - d3).abs() < 12.0, "{name}: {:.1} s / {:.1} s apart", c2 - d2, c3 - d3);
        }
    }

    #[test]
    fn elements_are_sane_and_the_sphere_is_not_the_ellipsoid() {
        let nm = next_phase(unix_from_civil(2027, 7, 28, 0, 0, 0), Principal::New).expect("a new moon");
        let el = elements(nm).expect("elements");
        // the Moon is some sixty Earth radii off the Earth's centre but the axis passes within one of it; the cones
        // are a few thousandths of a radian wide
        assert!(el.gamma() < 1.0 && el.l2 < 0.0 && el.l1 > 0.5 && el.l1 < 0.6, "{el:?}");
        assert!(el.tan_f1 > 0.0045 && el.tan_f1 < 0.0050 && el.tan_f2 > 0.0045 && el.tan_f2 < 0.0050, "{el:?}");
        assert!(elements(f64::NAN).is_none() && elements(f64::INFINITY).is_none());
        // a point on the ellipsoid: the latitude at the pole is the pole, and the surface is nearer in at the pole
        let polar = Elements { x: 0.0, y: 0.0, d: std::f64::consts::FRAC_PI_2, mu: 0.0, l1: 0.5, l2: -0.01, tan_f1: 0.0047, tan_f2: 0.0046 };
        let g = ground(&polar, 0.0, 0.0).expect("the pole");
        assert!((g.lat - 90.0).abs() < 1e-6 && (g.zeta - 0.996_647_19).abs() < 1e-6, "{g:?}");
        assert!(ground(&polar, 1.01, 0.0).is_none());
    }
}
