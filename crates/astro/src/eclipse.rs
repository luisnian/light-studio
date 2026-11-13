//! Eclipses seen from a place: lunar ones with the times of their contacts, solar ones with the
//! magnitude and obscuration at the observer.
//!
//! The local circumstances use the textbook geometry, not a besselian solution (the track over the Earth does: see
//! [`crate::besselian`]): the Moon against the Earth's
//! shadow (Danjon's enlargement of 1.02 on the shadow radii) for the lunar ones, and the
//! topocentric separation of the Sun and Moon discs for the solar ones. Good to a couple of
//! minutes and a few hundredths of magnitude: enough to plan, not to publish.
//!
//! Sources: Jean Meeus, *Astronomical Algorithms* ch. 54 (the shadow radii), Danjon's factor.

use crate::moon::{Principal, moon_coords, principal_phases, semidiameter};
use crate::sun::{KM_PER_AU, Observer, sun_coords, sun_horizontal};
use crate::time::jd_from_unix;

const STEP: f64 = 30.0;
const WINDOW: f64 = 4.0 * 3600.0;

/// Angular separation between two directions, degrees.
fn separation(ra1: f64, dec1: f64, ra2: f64, dec2: f64) -> f64 {
    let (d1, d2) = (dec1.to_radians(), dec2.to_radians());
    let c = d1.sin() * d2.sin() + d1.cos() * d2.cos() * (ra1 - ra2).to_radians().cos();
    c.clamp(-1.0, 1.0).acos().to_degrees()
}

/// The kind of a lunar eclipse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LunarKind {
    Penumbral,
    Partial,
    Total,
}

/// A lunar eclipse. Times are Unix seconds (UTC); a contact the eclipse does not have is `None`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LunarEclipse {
    pub kind: LunarKind,
    pub greatest: f64,
    pub umbral_magnitude: f64,
    pub penumbral_magnitude: f64,
    /// Penumbra first and last contact.
    pub p1: Option<f64>,
    pub p4: Option<f64>,
    /// Umbra first and last contact (partial phase).
    pub u1: Option<f64>,
    pub u4: Option<f64>,
    /// Totality start and end.
    pub u2: Option<f64>,
    pub u3: Option<f64>,
}

/// The Moon's distance from the shadow axis and the shadow's radii at a Unix time, degrees:
/// (separation, moon radius, umbra radius, penumbra radius).
fn shadow_state(unix: f64) -> (f64, f64, f64, f64) {
    let jd = jd_from_unix(unix);
    let m = moon_coords(jd);
    let s = sun_coords(jd);
    let r_au = (s.dist_km / KM_PER_AU).max(0.1);
    let sep = separation(m.ra, m.dec, (s.ra + 180.0).rem_euclid(360.0), -s.dec);
    let pi_m = crate::moon::parallax(m.dist_km);
    let pi_s = 0.002_442_8 / r_au;
    let ss = 0.266_56 / r_au;
    (sep, semidiameter(m.dist_km), 1.02 * (pi_m + pi_s - ss), 1.02 * (pi_m + pi_s + ss))
}

/// The time at which `f` crosses zero between `a` and `b` (values of opposite sign), by bisection.
fn root(f: &dyn Fn(f64) -> f64, mut a: f64, mut b: f64) -> f64 {
    let fa = f(a);
    for _ in 0..24 {
        let mid = (a + b) / 2.0;
        if (f(mid) < 0.0) == (fa < 0.0) {
            a = mid;
        } else {
            b = mid;
        }
    }
    (a + b) / 2.0
}

/// The lunar eclipses between two Unix times (at most a year and a bit apart), in order.
pub fn lunar_eclipses(from: f64, to: f64) -> Vec<LunarEclipse> {
    let mut out = Vec::new();
    for (full, which) in principal_phases(from - 86_400.0, to + 86_400.0) {
        if which != Principal::Full {
            continue;
        }
        // the Moon too far from the ecliptic at full moon cannot enter even the penumbra
        if moon_coords(jd_from_unix(full)).lat.abs() > 1.75 {
            continue;
        }
        let n = (2.0 * WINDOW / STEP) as usize;
        let times: Vec<f64> = (0..=n).map(|i| full - WINDOW + i as f64 * STEP).collect();
        let seps: Vec<f64> = times.iter().map(|&t| shadow_state(t).0).collect();
        let Some((imin, _)) = seps.iter().enumerate().min_by(|a, b| a.1.total_cmp(b.1)) else { continue };
        let greatest = times.get(imin).copied().unwrap_or(full);
        let (sep, sm, umbra, penumbra) = shadow_state(greatest);
        let umbral_magnitude = (umbra + sm - sep) / (2.0 * sm);
        let penumbral_magnitude = (penumbra + sm - sep) / (2.0 * sm);
        if penumbral_magnitude <= 0.0 || greatest < from || greatest > to {
            continue;
        }
        // first and last time `sep` equals a threshold, either side of the greatest
        let contact = |radius: &dyn Fn(f64) -> f64, before: bool| -> Option<f64> {
            let f = |t: f64| {
                let (s, _, _, _) = shadow_state(t);
                s - radius(t)
            };
            let range: Vec<usize> = if before { (0..imin).rev().collect() } else { (imin + 1..=n).collect() };
            let mut prev = imin;
            for i in range {
                let (a, b) = (times.get(prev).copied()?, times.get(i).copied()?);
                if f(a) < 0.0 && f(b) >= 0.0 {
                    return Some(root(&f, a.min(b), a.max(b)));
                }
                prev = i;
            }
            None
        };
        let p = |t: f64| {
            let (_, sm, _, pen) = shadow_state(t);
            pen + sm
        };
        let u = |t: f64| {
            let (_, sm, um, _) = shadow_state(t);
            um + sm
        };
        let t = |t: f64| {
            let (_, sm, um, _) = shadow_state(t);
            um - sm
        };
        let kind = if umbral_magnitude >= 1.0 {
            LunarKind::Total
        } else if umbral_magnitude > 0.0 {
            LunarKind::Partial
        } else {
            LunarKind::Penumbral
        };
        out.push(LunarEclipse {
            kind,
            greatest,
            umbral_magnitude,
            penumbral_magnitude,
            p1: contact(&p, true),
            p4: contact(&p, false),
            u1: if umbral_magnitude > 0.0 { contact(&u, true) } else { None },
            u4: if umbral_magnitude > 0.0 { contact(&u, false) } else { None },
            u2: if umbral_magnitude >= 1.0 { contact(&t, true) } else { None },
            u3: if umbral_magnitude >= 1.0 { contact(&t, false) } else { None },
        });
    }
    out
}

/// A solar eclipse as one observer sees it. Times are Unix seconds (UTC).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SolarEclipse {
    pub new_moon: f64,
    /// The moment of greatest eclipse at the observer.
    pub greatest: f64,
    /// Fraction of the Sun's diameter covered (above 1 when the Moon is larger: total).
    pub magnitude: f64,
    /// Fraction of the Sun's disc covered.
    pub obscuration: f64,
    pub c1: Option<f64>,
    pub c4: Option<f64>,
    pub c2: Option<f64>,
    pub c3: Option<f64>,
    /// The Sun's altitude and azimuth at the greatest eclipse (below 0: not visible from here).
    pub sun_alt: f64,
    pub sun_az: f64,
    pub total: bool,
    pub annular: bool,
}

/// The observer's position in the equatorial frame of date (km from the Earth's centre), on the oblate Earth.
fn observer_vector(jd: f64, obs: Observer) -> [f64; 3] {
    let phi = obs.lat.to_radians();
    let u = (0.996_647_19 * phi.tan()).atan();
    let rho_sin = 0.996_647_19 * u.sin();
    let rho_cos = u.cos();
    let theta = (crate::sun::gmst(jd) + obs.lon).to_radians();
    [EQUATORIAL_KM * rho_cos * theta.cos(), EQUATORIAL_KM * rho_cos * theta.sin(), EQUATORIAL_KM * rho_sin]
}

/// Right ascension and declination (degrees) and distance (km) of a body as seen from the observer's point.
fn topocentric(ra: f64, dec: f64, dist: f64, o: [f64; 3]) -> (f64, f64, f64) {
    let v = vector(ra, dec, dist);
    let r = [v[0] - o[0], v[1] - o[1], v[2] - o[2]];
    let d = dot(r, r).sqrt().max(1.0);
    (r[1].atan2(r[0]).to_degrees().rem_euclid(360.0), (r[2] / d).clamp(-1.0, 1.0).asin().to_degrees(), d)
}

/// The fraction of a disc of radius `rs` covered by one of radius `rm` whose centre is `d` away.
pub fn overlap_fraction(rs: f64, rm: f64, d: f64) -> f64 {
    if !(rs > 0.0 && rm > 0.0 && d.is_finite()) || d >= rs + rm {
        return 0.0;
    }
    if d <= (rm - rs).abs() {
        return if rm >= rs { 1.0 } else { (rm * rm) / (rs * rs) };
    }
    let a = ((d * d + rs * rs - rm * rm) / (2.0 * d * rs)).clamp(-1.0, 1.0).acos();
    let b = ((d * d + rm * rm - rs * rs) / (2.0 * d * rm)).clamp(-1.0, 1.0).acos();
    let area = rs * rs * a + rm * rm * b - 0.5 * ((-d + rs + rm) * (d + rs - rm) * (d - rs + rm) * (d + rs + rm)).max(0.0).sqrt();
    (area / (std::f64::consts::PI * rs * rs)).clamp(0.0, 1.0)
}

/// What an observer sees of the solar eclipse around a new moon, if there is one at all.
pub fn solar_eclipse_at(new_moon: f64, obs: Observer) -> Option<SolarEclipse> {
    // everything is worked out as seen from the observer's point on the oblate Earth (parallax of both bodies and the
    // Moon's disc growing as it climbs), not from the Earth's centre
    let state = |t: f64| {
        let jd = jd_from_unix(t);
        let o = observer_vector(jd, obs);
        let (s, m) = (sun_coords(jd), moon_coords(jd));
        let (sra, sdec, sdist) = topocentric(s.ra, s.dec, s.dist_km, o);
        let (mra, mdec, mdist) = topocentric(m.ra, m.dec, m.dist_km, o);
        let rs = (SUN_DISC_KM / sdist.max(1.0)).clamp(-1.0, 1.0).asin().to_degrees();
        let rm = (MOON_KM / mdist.max(1.0)).clamp(-1.0, 1.0).asin().to_degrees();
        (separation(sra, sdec, mra, mdec), rs, rm)
    };
    let radii = |t: f64| {
        let (_, rs, rm) = state(t);
        (rs, rm)
    };
    let sep = |t: f64| state(t).0;
    let n = (2.0 * 3.0 * 3600.0 / STEP) as usize;
    let start = new_moon - 3.0 * 3600.0;
    let times: Vec<f64> = (0..=n).map(|i| start + i as f64 * STEP).collect();
    let seps: Vec<f64> = times.iter().map(|&t| sep(t)).collect();
    let (imin, _) = seps.iter().enumerate().min_by(|a, b| a.1.total_cmp(b.1))?;
    let greatest = times.get(imin).copied()?;
    let (rs, rm) = radii(greatest);
    let d = sep(greatest);
    let magnitude = (rs + rm - d) / (2.0 * rs);
    if magnitude <= 0.0 {
        return None;
    }
    let contact = |threshold: &dyn Fn(f64) -> f64, before: bool| -> Option<f64> {
        let f = |t: f64| sep(t) - threshold(t);
        let range: Vec<usize> = if before { (0..imin).rev().collect() } else { (imin + 1..=n).collect() };
        let mut prev = imin;
        for i in range {
            let (a, b) = (times.get(prev).copied()?, times.get(i).copied()?);
            if f(a) < 0.0 && f(b) >= 0.0 {
                return Some(root(&f, a.min(b), a.max(b)));
            }
            prev = i;
        }
        None
    };
    let outer = |t: f64| {
        let (rs, rm) = radii(t);
        rs + rm
    };
    let inner = |t: f64| {
        let (rs, rm) = radii(t);
        (rm * UMBRAL_SHRINK - rs).abs()
    };
    let central = d <= (rm * UMBRAL_SHRINK - rs).abs();
    let sun = sun_horizontal(greatest, obs);
    Some(SolarEclipse {
        new_moon,
        greatest,
        magnitude,
        obscuration: overlap_fraction(rs, rm, d),
        c1: contact(&outer, true),
        c4: contact(&outer, false),
        c2: if central { contact(&inner, true) } else { None },
        c3: if central { contact(&inner, false) } else { None },
        sun_alt: sun.alt,
        sun_az: sun.az,
        total: central && rm >= rs,
        annular: central && rm < rs,
    })
}

/// The solar eclipses an observer could see (Sun above the horizon at some stage of it is left to
/// the caller: see `sun_alt`) between two Unix times.
pub fn solar_eclipses(from: f64, to: f64, obs: Observer) -> Vec<SolarEclipse> {
    principal_phases(from - 86_400.0, to + 86_400.0)
        .into_iter()
        .filter(|&(_, w)| w == Principal::New)
        // a new moon too far from the ecliptic (latitude over ~1.6 degrees) cannot eclipse anywhere
        .filter(|&(t, _)| moon_coords(jd_from_unix(t)).lat.abs() < 1.7)
        .filter_map(|(t, _)| solar_eclipse_at(t, obs))
        .filter(|e| e.greatest >= from && e.greatest <= to)
        .collect()
}

/// A point of a solar eclipse's track over the Earth.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PathPoint {
    pub unix: f64,
    pub lat: f64,
    pub lon: f64,
    /// Half the width of the umbra on the ground, km, across the track and stretched by the slant: positive for totality,
    /// negative for an annular eclipse.
    pub half_width_km: f64,
    /// The radius of the penumbra across the axis, km: the edge of the partial eclipse (not stretched by the slant, so only a guide).
    pub penumbra_km: f64,
    /// The Sun's altitude there, degrees.
    pub sun_alt: f64,
    /// How long the total (or annular) phase lasts for someone on the central line here, seconds.
    pub duration_s: f64,
    /// The ratio of the Moon's to the Sun's apparent diameter on the central line here (the magnitude NASA lists for
    /// a central eclipse: above 1 for totality, below for an annular eclipse).
    pub magnitude: f64,
    /// The path's two limits across the track here, as (latitude, longitude): to the left and to the right of the
    /// direction the shadow travels.
    pub left: (f64, f64),
    pub right: (f64, f64),
}

/// The track of the Moon's shadow axis over the Earth round a new moon.
#[derive(Clone, Debug, PartialEq)]
pub struct EclipsePath {
    pub points: Vec<PathPoint>,
    /// The index of the point nearest the Earth's centre line: the greatest eclipse.
    pub greatest: usize,
}

const EQUATORIAL_KM: f64 = 6_378.14;
const MOON_KM: f64 = 1_737.4;
/// The Sun's radius for its disc as eclipse predictions take it (959.63 arcseconds at one AU).
const SUN_DISC_KM: f64 = 696_000.0;
/// Eclipse predictions shrink the Moon a little for totality and annularity, so the valleys of its limb let some light
/// through (Espenak's umbral k = 0.272281 against 0.272488).
const UMBRAL_SHRINK: f64 = 0.272_281 / 0.272_488;

fn vector(ra: f64, dec: f64, dist: f64) -> [f64; 3] {
    let (ra, dec) = (ra.to_radians(), dec.to_radians());
    [dist * dec.cos() * ra.cos(), dist * dec.cos() * ra.sin(), dist * dec.sin()]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Where the Moon's shadow axis crosses the Earth every two minutes within three and a half hours of a new moon,
/// worked out from the Besselian elements on the oblate Earth (see [`crate::besselian`]). Empty when the axis misses
/// the Earth. Only the sunlit part (the Sun above the horizon) is kept.
pub fn solar_path(new_moon: f64) -> Option<EclipsePath> {
    crate::besselian::track(new_moon)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::unix_from_civil;

    fn near(a: f64, b: f64, minutes: f64) -> bool {
        (a - b).abs() < minutes * 60.0
    }

    #[test]
    fn lunar_eclipses_of_2022_and_2025() {
        let found = lunar_eclipses(unix_from_civil(2022, 1, 1, 0, 0, 0), unix_from_civil(2022, 12, 31, 0, 0, 0));
        // 16 May and 8 November 2022, both total
        assert_eq!(found.len(), 2, "{found:?}");
        let nov = &found[1];
        assert_eq!(nov.kind, LunarKind::Total);
        assert!(near(nov.greatest, unix_from_civil(2022, 11, 8, 10, 59, 0), 8.0), "{}", nov.greatest);
        assert!((nov.umbral_magnitude - 1.359).abs() < 0.06, "{}", nov.umbral_magnitude);
        // totality lasted about 85 minutes
        let len = nov.u3.unwrap_or(0.0) - nov.u2.unwrap_or(0.0);
        assert!((len / 60.0 - 85.0).abs() < 8.0, "{}", len / 60.0);
        let sep = lunar_eclipses(unix_from_civil(2025, 9, 1, 0, 0, 0), unix_from_civil(2025, 9, 30, 0, 0, 0));
        assert_eq!(sep.len(), 1);
        assert!(near(sep[0].greatest, unix_from_civil(2025, 9, 7, 18, 11, 0), 8.0));
        assert!((sep[0].umbral_magnitude - 1.362).abs() < 0.06);
    }

    #[test]
    fn a_shallow_partial_and_no_eclipse_months() {
        let oct = lunar_eclipses(unix_from_civil(2023, 10, 20, 0, 0, 0), unix_from_civil(2023, 11, 5, 0, 0, 0));
        assert_eq!(oct.len(), 1);
        assert_eq!(oct[0].kind, LunarKind::Partial);
        assert!((oct[0].umbral_magnitude - 0.122).abs() < 0.06, "{}", oct[0].umbral_magnitude);
        assert!(oct[0].u2.is_none() && oct[0].u1.is_some());
        // an ordinary month holds none
        assert!(lunar_eclipses(unix_from_civil(2024, 1, 1, 0, 0, 0), unix_from_civil(2024, 2, 28, 0, 0, 0)).is_empty());
    }

    #[test]
    fn solar_eclipse_of_april_2024_from_dallas_and_madrid() {
        let dallas = Observer::new(32.78, -96.8);
        let e = solar_eclipses(unix_from_civil(2024, 4, 1, 0, 0, 0), unix_from_civil(2024, 4, 15, 0, 0, 0), dallas);
        assert_eq!(e.len(), 1, "{e:?}");
        assert!(near(e[0].greatest, unix_from_civil(2024, 4, 8, 18, 42, 0), 6.0), "{}", e[0].greatest);
        assert!(e[0].magnitude > 0.97 && e[0].obscuration > 0.95, "{:?}", e[0]);
        assert!(e[0].sun_alt > 50.0);
        // from Madrid it happened with the Sun far below the horizon: not visible (the caller reads `sun_alt`)
        let madrid = Observer::new(40.4, -3.7);
        let m = solar_eclipses(unix_from_civil(2024, 4, 1, 0, 0, 0), unix_from_civil(2024, 4, 15, 0, 0, 0), madrid);
        assert!(m.iter().all(|e| e.sun_alt < -5.0), "{m:?}");
        // and a month with no new-moon eclipse gives none
        assert!(solar_eclipses(unix_from_civil(2024, 5, 1, 0, 0, 0), unix_from_civil(2024, 5, 20, 0, 0, 0), dallas).is_empty());
    }

    #[test]
    fn total_eclipse_of_august_2026_in_northern_spain() {
        let oviedo = Observer::new(43.36, -5.85);
        let e = solar_eclipses(unix_from_civil(2026, 8, 1, 0, 0, 0), unix_from_civil(2026, 8, 20, 0, 0, 0), oviedo);
        assert_eq!(e.len(), 1, "{e:?}");
        assert!(near(e[0].greatest, unix_from_civil(2026, 8, 12, 18, 28, 0), 8.0), "{}", e[0].greatest);
        assert!(e[0].magnitude > 0.98, "{:?}", e[0]);
        assert!(e[0].sun_alt > 0.0 && e[0].sun_alt < 15.0, "{}", e[0].sun_alt);
    }

    #[test]
    fn overlap_is_zero_apart_and_full_when_inside() {
        assert_eq!(overlap_fraction(1.0, 1.0, 3.0), 0.0);
        assert_eq!(overlap_fraction(1.0, 1.2, 0.1), 1.0);
        assert!((overlap_fraction(1.0, 0.5, 0.2) - 0.25).abs() < 1e-9);
        let half = overlap_fraction(1.0, 1.0, 1.0);
        assert!(half > 0.38 && half < 0.40, "{half}");
        assert_eq!(overlap_fraction(f64::NAN, 1.0, 0.0), 0.0);
    }

    fn km(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
        crate::align::sight(lat1, lon1, lat2, lon2, 0.0).distance / 1000.0
    }

    #[test]
    fn the_track_of_april_2024_passes_over_texas() {
        let nm = crate::moon::next_phase(unix_from_civil(2024, 4, 1, 0, 0, 0), Principal::New).expect("new moon");
        let path = solar_path(nm).expect("the axis meets the Earth");
        let g = path.points[path.greatest];
        // the greatest eclipse was at 25.3 N 104.1 W, 18:17 UTC
        assert!(km(g.lat, g.lon, 25.3, -104.1) < 250.0, "{g:?}");
        assert!(near(g.unix, unix_from_civil(2024, 4, 8, 18, 17, 0), 8.0));
        // Dallas lies inside the band, which is about 200 km wide
        let near_dallas =
            path.points.iter().map(|p| (km(p.lat, p.lon, 32.78, -96.8), p.half_width_km)).min_by(|a, b| a.0.total_cmp(&b.0)).expect("points");
        assert!(near_dallas.0 < near_dallas.1.abs() + 60.0, "{near_dallas:?}");
        assert!((70.0..140.0).contains(&g.half_width_km), "{}", g.half_width_km);
    }

    #[test]
    fn the_track_of_august_2026_crosses_iceland_and_spain() {
        let nm = crate::moon::next_phase(unix_from_civil(2026, 8, 5, 0, 0, 0), Principal::New).expect("new moon");
        let path = solar_path(nm).expect("track");
        let g = path.points[path.greatest];
        assert!(km(g.lat, g.lon, 65.2, -25.2) < 400.0, "{g:?}");
        assert!(
            path.points.iter().any(|p| km(p.lat, p.lon, 41.65, -0.88) < 150.0),
            "{:?}",
            path.points.iter().rev().take(6).map(|p| (p.unix as i64, p.lat as i32, p.lon as i32, p.sun_alt as i32)).collect::<Vec<_>>()
        );
        assert!(solar_path(f64::NAN).is_none());
    }
}
