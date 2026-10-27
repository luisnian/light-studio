//! Where the Sun is. Low-precision solar coordinates (about 0.01 degrees, good for the 1900-2100 span)
//! and the conversion from equatorial to horizontal coordinates shared by every body.
//!
//! Sources: Jean Meeus, *Astronomical Algorithms* 2nd ed., ch. 25 (solar coordinates, the "low
//! accuracy" series), ch. 12 (sidereal time) and ch. 13 (equatorial to horizontal); formulas, not code.

use crate::time::jd_from_unix;

pub const KM_PER_AU: f64 = 149_597_870.7;

/// A place on Earth, in degrees: latitude north, longitude east.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Observer {
    pub lat: f64,
    pub lon: f64,
}

impl Observer {
    /// An observer, with NaN and out-of-range numbers brought to something valid.
    pub fn new(lat: f64, lon: f64) -> Observer {
        let lat = if lat.is_finite() { lat.clamp(-90.0, 90.0) } else { 0.0 };
        let lon = if lon.is_finite() { (lon + 180.0).rem_euclid(360.0) - 180.0 } else { 0.0 };
        Observer { lat, lon }
    }
}

/// A direction in the sky: azimuth in degrees clockwise from north (0 = N, 90 = E), altitude above
/// the horizon in degrees (negative below it).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Horizontal {
    pub az: f64,
    pub alt: f64,
}

/// Julian centuries from J2000.
pub(crate) fn centuries(jd: f64) -> f64 {
    (jd - 2_451_545.0) / 36_525.0
}

/// Mean obliquity of the ecliptic, degrees.
pub(crate) fn obliquity(t: f64) -> f64 {
    23.439_291 - 0.013_004_2 * t
}

/// The Sun at a moment: apparent ecliptic longitude, right ascension and declination (degrees),
/// distance (km).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SunCoords {
    pub lon: f64,
    pub ra: f64,
    pub dec: f64,
    pub dist_km: f64,
}

pub fn sun_coords(jd: f64) -> SunCoords {
    let t = centuries(jd);
    let l0 = 280.466_46 + 36_000.769_83 * t + 0.000_303_2 * t * t;
    let m = (357.529_11 + 35_999.050_29 * t - 0.000_153_7 * t * t).to_radians();
    let e = 0.016_708_634 - 0.000_042_037 * t;
    let c = (1.914_602 - 0.004_817 * t - 0.000_014 * t * t) * m.sin() + (0.019_993 - 0.000_101 * t) * (2.0 * m).sin() + 0.000_289 * (3.0 * m).sin();
    let true_lon = l0 + c;
    let nu = m + c.to_radians();
    let r_au = 1.000_001_018 * (1.0 - e * e) / (1.0 + e * nu.cos());
    let omega = (125.04 - 1_934.136 * t).to_radians();
    let lon = (true_lon - 0.005_69 - 0.004_78 * omega.sin()).rem_euclid(360.0);
    let eps = (obliquity(t) + 0.002_56 * omega.cos()).to_radians();
    let lam = lon.to_radians();
    let ra = (eps.cos() * lam.sin()).atan2(lam.cos()).to_degrees().rem_euclid(360.0);
    let dec = (eps.sin() * lam.sin()).clamp(-1.0, 1.0).asin().to_degrees();
    SunCoords { lon, ra, dec, dist_km: r_au * KM_PER_AU }
}

/// Greenwich mean sidereal time, degrees.
pub(crate) fn gmst(jd: f64) -> f64 {
    let t = centuries(jd);
    (280.460_618_37 + 360.985_647_366_29 * (jd - 2_451_545.0) + 0.000_387_933 * t * t - t * t * t / 38_710_000.0).rem_euclid(360.0)
}

/// Equatorial (RA, Dec in degrees) to the observer's horizon at `jd`.
pub fn to_horizontal(ra: f64, dec: f64, jd: f64, obs: Observer) -> Horizontal {
    let h = (gmst(jd) + obs.lon - ra).to_radians();
    let phi = obs.lat.to_radians();
    let d = dec.to_radians();
    let alt = (phi.sin() * d.sin() + phi.cos() * d.cos() * h.cos()).clamp(-1.0, 1.0).asin().to_degrees();
    // measured from the south by Meeus; +180 puts it from the north
    let az = (h.sin().atan2(h.cos() * phi.sin() - d.tan() * phi.cos()).to_degrees() + 180.0).rem_euclid(360.0);
    Horizontal { az, alt }
}

/// The Sun's direction at a Unix time (seconds, UTC).
pub fn sun_horizontal(unix: f64, obs: Observer) -> Horizontal {
    let jd = jd_from_unix(unix);
    let s = sun_coords(jd);
    to_horizontal(s.ra, s.dec, jd, obs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::unix_from_civil;

    /// Meeus example 25.a: 1992 October 13, 0h TD.
    #[test]
    fn matches_meeus_example_25a() {
        let s = sun_coords(2_448_908.5);
        assert!((s.lon - 199.908_95).abs() < 0.01, "lon {}", s.lon);
        assert!((s.ra - 198.380_83).abs() < 0.02, "ra {}", s.ra);
        assert!((s.dec - -7.785_07).abs() < 0.01, "dec {}", s.dec);
        assert!((s.dist_km / KM_PER_AU - 0.997_66).abs() < 0.000_1, "r {}", s.dist_km / KM_PER_AU);
    }

    #[test]
    fn declination_follows_the_seasons() {
        let dec = |m, d| sun_coords(jd_from_unix(unix_from_civil(2024, m, d, 12, 0, 0))).dec;
        assert!((dec(6, 20) - 23.44).abs() < 0.1, "june solstice");
        assert!((dec(12, 21) + 23.44).abs() < 0.1, "december solstice");
        assert!(dec(3, 20).abs() < 0.5, "march equinox");
        assert!(dec(9, 22).abs() < 0.5, "september equinox");
    }

    #[test]
    fn the_sun_is_south_at_noon_in_the_northern_hemisphere() {
        // Madrid, 21 June 2024: solar noon about 12:16 UTC, altitude 90 - 40.4 + 23.4 = 73
        let obs = Observer::new(40.4, -3.7);
        let h = sun_horizontal(unix_from_civil(2024, 6, 21, 12, 16, 0), obs);
        assert!((h.az - 180.0).abs() < 1.5, "az {}", h.az);
        assert!((h.alt - 73.0).abs() < 0.3, "alt {}", h.alt);
        // in the morning it is in the east
        let am = sun_horizontal(unix_from_civil(2024, 6, 21, 6, 0, 0), obs);
        assert!(am.az > 60.0 && am.az < 120.0, "az {}", am.az);
    }

    #[test]
    fn hostile_observers_are_brought_into_range() {
        let o = Observer::new(f64::NAN, 725.0);
        assert_eq!((o.lat, o.lon), (0.0, 5.0));
        assert_eq!(Observer::new(120.0, -190.0).lat, 90.0);
        assert_eq!(Observer::new(0.0, -190.0).lon, 170.0);
    }
}
