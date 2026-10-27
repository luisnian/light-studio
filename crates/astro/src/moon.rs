//! Where the Moon is and how much of it is lit.
//!
//! The position is Meeus's lunar theory cut to its largest terms (30 for longitude and distance, 14
//! for latitude): within about 0.05 degrees, which is a tenth of the Moon's width and a minute or
//! two of rise and set. Phase comes from the Sun-Moon geometry.
//!
//! Sources: Jean Meeus, *Astronomical Algorithms* 2nd ed., ch. 47 (Moon position, table 47.A and
//! 47.B coefficients) and ch. 48 (illuminated fraction); formulas, not code.

use crate::sun::{Horizontal, Observer, centuries, obliquity, sun_coords, to_horizontal};
use crate::time::jd_from_unix;

const EARTH_RADIUS_KM: f64 = 6_378.14;
const MOON_RADIUS_KM: f64 = 1_737.4;

/// (D, M, M', F, longitude sine coefficient in 1e-6 degrees, distance cosine coefficient in 1e-3 km).
const LON_DIST: [(i8, i8, i8, i8, i32, i32); 30] = [
    (0, 0, 1, 0, 6_288_774, -20_905_355),
    (2, 0, -1, 0, 1_274_027, -3_699_111),
    (2, 0, 0, 0, 658_314, -2_955_968),
    (0, 0, 2, 0, 213_618, -569_925),
    (0, 1, 0, 0, -185_116, 48_888),
    (0, 0, 0, 2, -114_332, -3_149),
    (2, 0, -2, 0, 58_793, 246_158),
    (2, -1, -1, 0, 57_066, -152_138),
    (2, 0, 1, 0, 53_322, -170_733),
    (2, -1, 0, 0, 45_758, -204_586),
    (0, 1, -1, 0, -40_923, -129_620),
    (1, 0, 0, 0, -34_720, 108_743),
    (0, 1, 1, 0, -30_383, 104_755),
    (2, 0, 0, -2, 15_327, 10_321),
    (0, 0, 1, 2, -12_528, 0),
    (0, 0, 1, -2, 10_980, 79_661),
    (4, 0, -1, 0, 10_675, -34_782),
    (0, 0, 3, 0, 10_034, -23_210),
    (4, 0, -2, 0, 8_548, -21_636),
    (2, 1, -1, 0, -7_888, 24_208),
    (2, 1, 0, 0, -6_766, 30_824),
    (1, 0, -1, 0, -5_163, -8_379),
    (1, 1, 0, 0, 4_987, -16_675),
    (2, -1, 1, 0, 4_036, -12_831),
    (2, 0, 2, 0, 3_994, -10_445),
    (4, 0, 0, 0, 3_861, -11_650),
    (2, 0, -3, 0, 3_665, 14_403),
    (0, 1, -2, 0, -2_689, -7_003),
    (2, 0, -1, 2, -2_602, 0),
    (2, -1, -2, 0, 2_390, 10_056),
];

/// (D, M, M', F, latitude sine coefficient in 1e-6 degrees).
const LAT: [(i8, i8, i8, i8, i32); 14] = [
    (0, 0, 0, 1, 5_128_122),
    (0, 0, 1, 1, 280_602),
    (0, 0, 1, -1, 277_693),
    (2, 0, 0, -1, 173_237),
    (2, 0, -1, 1, 55_413),
    (2, 0, -1, -1, 46_271),
    (2, 0, 0, 1, 32_573),
    (0, 0, 2, 1, 17_198),
    (2, 0, 1, -1, 9_266),
    (0, 0, 2, -1, 8_822),
    (2, -1, 0, -1, 8_216),
    (2, 0, -2, -1, 4_324),
    (2, 0, 1, 1, 4_200),
    (2, 1, 0, -1, -3_359),
];

/// The Moon at a moment: geocentric ecliptic longitude and latitude (degrees, longitude apparent),
/// distance (km), and the equatorial coordinates (degrees) they give.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MoonCoords {
    pub lon: f64,
    pub lat: f64,
    pub dist_km: f64,
    pub ra: f64,
    pub dec: f64,
}

pub fn moon_coords(jd: f64) -> MoonCoords {
    let t = centuries(jd);
    let t2 = t * t;
    let t3 = t2 * t;
    let t4 = t3 * t;
    let lp = 218.316_447_7 + 481_267.881_234_21 * t - 0.001_578_6 * t2 + t3 / 538_841.0 - t4 / 65_194_000.0;
    let d = (297.850_192_1 + 445_267.111_403_4 * t - 0.001_881_9 * t2 + t3 / 545_868.0).to_radians();
    let m = (357.529_109_2 + 35_999.050_290_9 * t - 0.000_153_6 * t2).to_radians();
    let mp = (134.963_396_4 + 477_198.867_505_5 * t + 0.008_741_4 * t2 + t3 / 69_699.0).to_radians();
    let f = (93.272_095_0 + 483_202.017_523_3 * t - 0.003_653_9 * t2).to_radians();
    let e = 1.0 - 0.002_516 * t - 0.000_007_4 * t2;
    let ecc = |m_coef: i8| match m_coef.unsigned_abs() {
        0 => 1.0,
        1 => e,
        _ => e * e,
    };
    let a1 = (119.75 + 131.849 * t).to_radians();
    let a2 = (53.09 + 479_264.290 * t).to_radians();
    let a3 = (313.45 + 481_266.484 * t).to_radians();
    let lp_rad = lp.to_radians();

    let mut sum_l = 0.0;
    let mut sum_r = 0.0;
    for &(cd, cm, cmp, cf, l_coef, r_coef) in &LON_DIST {
        let arg = f64::from(cd) * d + f64::from(cm) * m + f64::from(cmp) * mp + f64::from(cf) * f;
        let k = ecc(cm);
        sum_l += f64::from(l_coef) * k * arg.sin();
        sum_r += f64::from(r_coef) * k * arg.cos();
    }
    let mut sum_b = 0.0;
    for &(cd, cm, cmp, cf, b_coef) in &LAT {
        let arg = f64::from(cd) * d + f64::from(cm) * m + f64::from(cmp) * mp + f64::from(cf) * f;
        sum_b += f64::from(b_coef) * ecc(cm) * arg.sin();
    }
    // additive terms (Venus, Jupiter, the flattening of the Earth)
    sum_l += 3_958.0 * a1.sin() + 1_962.0 * (lp_rad - f).sin() + 318.0 * a2.sin();
    sum_b += -2_235.0 * lp_rad.sin() + 382.0 * a3.sin() + 175.0 * (a1 - f).sin() + 175.0 * (a1 + f).sin() + 127.0 * (lp_rad - mp).sin()
        - 115.0 * (lp_rad + mp).sin();

    let omega = (125.04 - 1_934.136 * t).to_radians();
    let lon = (lp + sum_l / 1e6 - 0.004_78 * omega.sin()).rem_euclid(360.0);
    let lat = sum_b / 1e6;
    let dist_km = 385_000.56 + sum_r / 1e3;

    let eps = (obliquity(t) + 0.002_56 * omega.cos()).to_radians();
    let (lam, bet) = (lon.to_radians(), lat.to_radians());
    let ra = (lam.sin() * eps.cos() - bet.tan() * eps.sin()).atan2(lam.cos()).to_degrees().rem_euclid(360.0);
    let dec = (bet.sin() * eps.cos() + bet.cos() * eps.sin() * lam.sin()).clamp(-1.0, 1.0).asin().to_degrees();
    MoonCoords { lon, lat, dist_km, ra, dec }
}

/// The Moon's horizontal parallax, degrees (about 0.95).
pub fn parallax(dist_km: f64) -> f64 {
    (EARTH_RADIUS_KM / dist_km.max(1.0)).clamp(-1.0, 1.0).asin().to_degrees()
}

/// The Moon's apparent radius, degrees (about 0.26).
pub fn semidiameter(dist_km: f64) -> f64 {
    (MOON_RADIUS_KM / dist_km.max(1.0)).clamp(-1.0, 1.0).asin().to_degrees()
}

/// The direction of the Moon's centre as seen from the ground (parallax applied, no refraction).
pub fn moon_horizontal(unix: f64, obs: Observer) -> Horizontal {
    let jd = jd_from_unix(unix);
    let m = moon_coords(jd);
    let g = to_horizontal(m.ra, m.dec, jd, obs);
    Horizontal { az: g.az, alt: g.alt - parallax(m.dist_km) * g.alt.to_radians().cos() }
}

/// The altitude (degrees) at which the Moon's upper edge meets an ideal horizon, for the centre's
/// ground-view altitude: 34' of refraction plus the Moon's radius.
pub fn moon_rise_altitude(unix: f64) -> f64 {
    let m = moon_coords(jd_from_unix(unix));
    -(34.0 / 60.0) - semidiameter(m.dist_km)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhaseName {
    New,
    WaxingCrescent,
    FirstQuarter,
    WaxingGibbous,
    Full,
    WaningGibbous,
    LastQuarter,
    WaningCrescent,
}

impl PhaseName {
    /// The English name, the key the UI translates.
    pub fn label(self) -> &'static str {
        match self {
            PhaseName::New => "New Moon",
            PhaseName::WaxingCrescent => "Waxing Crescent",
            PhaseName::FirstQuarter => "First Quarter",
            PhaseName::WaxingGibbous => "Waxing Gibbous",
            PhaseName::Full => "Full Moon",
            PhaseName::WaningGibbous => "Waning Gibbous",
            PhaseName::LastQuarter => "Last Quarter",
            PhaseName::WaningCrescent => "Waning Crescent",
        }
    }
}

/// The Moon's phase at a moment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Phase {
    /// Lit fraction of the disc, 0 (new) to 1 (full).
    pub illumination: f64,
    /// Moon minus Sun in ecliptic longitude, degrees from 0 (new) through 180 (full) to 360.
    pub elongation: f64,
    pub name: PhaseName,
}

/// The phase at a Unix time (seconds, UTC).
pub fn moon_phase(unix: f64) -> Phase {
    let jd = jd_from_unix(unix);
    let m = moon_coords(jd);
    let s = sun_coords(jd);
    let elongation = (m.lon - s.lon).rem_euclid(360.0);
    let cos_psi = m.lat.to_radians().cos() * (m.lon - s.lon).to_radians().cos();
    let psi = cos_psi.clamp(-1.0, 1.0).acos();
    let i = (s.dist_km * psi.sin()).atan2(m.dist_km - s.dist_km * psi.cos());
    let illumination = ((1.0 + i.cos()) / 2.0).clamp(0.0, 1.0);
    let name = match ((elongation + 22.5) / 45.0).floor() as i32 % 8 {
        0 => PhaseName::New,
        1 => PhaseName::WaxingCrescent,
        2 => PhaseName::FirstQuarter,
        3 => PhaseName::WaxingGibbous,
        4 => PhaseName::Full,
        5 => PhaseName::WaningGibbous,
        6 => PhaseName::LastQuarter,
        _ => PhaseName::WaningCrescent,
    };
    Phase { illumination, elongation, name }
}

/// A principal phase.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Principal {
    New,
    FirstQuarter,
    Full,
    LastQuarter,
}

impl Principal {
    fn elongation(self) -> f64 {
        match self {
            Principal::New => 0.0,
            Principal::FirstQuarter => 90.0,
            Principal::Full => 180.0,
            Principal::LastQuarter => 270.0,
        }
    }
}

/// The first moment after `unix` at which the Moon reaches the given phase (found to about a second).
pub fn next_phase(unix: f64, which: Principal) -> Option<f64> {
    if !unix.is_finite() {
        return None;
    }
    let target = which.elongation();
    // signed distance past the target, -180..180: negative before it, positive after
    let past = |t: f64| (moon_phase(t).elongation - target + 180.0).rem_euclid(360.0) - 180.0;
    let step = 6.0 * 3600.0;
    let mut a = unix;
    let mut fa = past(a);
    for _ in 0..(36 * 4) {
        let b = a + step;
        let fb = past(b);
        if fa < 0.0 && fb >= 0.0 {
            let (mut lo, mut hi) = (a, b);
            for _ in 0..30 {
                let mid = (lo + hi) / 2.0;
                if past(mid) < 0.0 {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            return Some((lo + hi) / 2.0);
        }
        a = b;
        fa = fb;
    }
    None
}

/// The new, quarter and full moons between two Unix times, in order.
pub fn principal_phases(from: f64, to: f64) -> Vec<(f64, Principal)> {
    let mut out = Vec::new();
    if !(from.is_finite() && to.is_finite() && to > from) || to - from > 400.0 * 86_400.0 {
        return out;
    }
    for which in [Principal::New, Principal::FirstQuarter, Principal::Full, Principal::LastQuarter] {
        let mut t = from;
        // a phase comes every 29.5 days: a year holds fewer than 14 of each
        for _ in 0..16 {
            match next_phase(t, which) {
                Some(p) if p <= to => {
                    out.push((p, which));
                    t = p + 86_400.0;
                }
                _ => break,
            }
        }
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::unix_from_civil;

    #[test]
    fn lists_the_phases_of_a_month_in_order() {
        // April 2024: last quarter 2nd, new 8th, first quarter 15th, full 23rd; last quarter again 1 May
        let p = principal_phases(unix_from_civil(2024, 4, 1, 0, 0, 0), unix_from_civil(2024, 5, 2, 0, 0, 0));
        let kinds: Vec<Principal> = p.iter().map(|x| x.1).collect();
        assert_eq!(kinds, [Principal::LastQuarter, Principal::New, Principal::FirstQuarter, Principal::Full, Principal::LastQuarter], "{p:?}");
        assert!(p.windows(2).all(|w| w[0].0 < w[1].0));
        assert!((p[0].0 - unix_from_civil(2024, 4, 2, 3, 15, 0)).abs() < 20.0 * 60.0);
        assert!((p[1].0 - unix_from_civil(2024, 4, 8, 18, 21, 0)).abs() < 20.0 * 60.0);
        assert!(principal_phases(10.0, 5.0).is_empty());
        assert!(principal_phases(0.0, 1.0e12).is_empty(), "a huge span is refused");
        assert!(principal_phases(f64::NAN, 5.0).is_empty());
    }

    /// Meeus example 47.a: 1992 April 12, 0h TD (JDE 2448724.5).
    #[test]
    fn matches_meeus_example_47a() {
        let m = moon_coords(2_448_724.5);
        assert!((m.lon - 133.167_265).abs() < 0.05, "lon {}", m.lon);
        assert!((m.lat - -3.229_126).abs() < 0.05, "lat {}", m.lat);
        assert!((m.dist_km - 368_409.7).abs() < 150.0, "dist {}", m.dist_km);
        assert!((m.ra - 134.688_470).abs() < 0.06, "ra {}", m.ra);
        assert!((m.dec - 13.768_368).abs() < 0.06, "dec {}", m.dec);
    }

    #[test]
    fn phases_at_known_moments() {
        // 2024: new 8 April 18:21 UTC, first quarter 15 April 19:13, full 23 April 23:49, last quarter 1 May 11:27
        let at = |m, d, h, mi| moon_phase(unix_from_civil(2024, m, d, h, mi, 0));
        let new = at(4, 8, 18, 21);
        assert!(new.illumination < 0.005, "{}", new.illumination);
        assert_eq!(new.name, PhaseName::New);
        let q1 = at(4, 15, 19, 13);
        assert!((q1.illumination - 0.5).abs() < 0.03, "{}", q1.illumination);
        assert_eq!(q1.name, PhaseName::FirstQuarter);
        let full = at(4, 23, 23, 49);
        assert!(full.illumination > 0.995, "{}", full.illumination);
        assert_eq!(full.name, PhaseName::Full);
        let q3 = at(5, 1, 11, 27);
        assert!((q3.illumination - 0.5).abs() < 0.03, "{}", q3.illumination);
        assert_eq!(q3.name, PhaseName::LastQuarter);
        // three days after the new moon: a thin waxing crescent
        let cres = at(4, 11, 18, 0);
        assert_eq!(cres.name, PhaseName::WaxingCrescent);
        assert!(cres.illumination > 0.05 && cres.illumination < 0.25, "{}", cres.illumination);
    }

    #[test]
    fn finds_the_next_new_and_full_moon() {
        let from = unix_from_civil(2024, 4, 1, 0, 0, 0);
        let new = next_phase(from, Principal::New).expect("a new moon within a month");
        assert!((new - unix_from_civil(2024, 4, 8, 18, 21, 0)).abs() < 20.0 * 60.0, "{new}");
        let full = next_phase(from, Principal::Full).expect("a full moon within a month");
        assert!((full - unix_from_civil(2024, 4, 23, 23, 49, 0)).abs() < 20.0 * 60.0, "{full}");
        assert_eq!(next_phase(f64::NAN, Principal::Full), None);
    }

    #[test]
    fn the_moon_is_about_a_degree_wide_and_parallax_lowers_it() {
        let m = moon_coords(2_448_724.5);
        assert!((semidiameter(m.dist_km) * 2.0 - 0.54).abs() < 0.05);
        assert!((parallax(m.dist_km) - 0.95).abs() < 0.05);
        let obs = Observer::new(40.4, -3.7);
        let t = unix_from_civil(2024, 4, 10, 22, 0, 0);
        let jd = jd_from_unix(t);
        let c = moon_coords(jd);
        let geo = to_horizontal(c.ra, c.dec, jd, obs);
        let topo = moon_horizontal(t, obs);
        assert!(topo.alt < geo.alt, "parallax lowers the Moon");
        assert!(geo.alt - topo.alt < 1.0);
    }
}
