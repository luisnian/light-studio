//! The total solar eclipse of 2027-08-02 against the Instituto Geográfico Nacional's published figures for the cities in
//! the path (https://astronomia.ign.es), in Spanish official time (UTC+2): the start of totality, to the minute, and its
//! duration, to the second. The IGN's table gives no first or last contact, so those are only checked for sense.

use light_studio_astro::eclipse::solar_eclipses;
use light_studio_astro::sun::Observer;
use light_studio_astro::time::unix_from_civil;

/// (city, latitude, longitude, totality starts (hh, mm), duration in seconds)
const IGN: [(&str, f64, f64, (u32, u32), f64); 4] = [
    ("Cádiz", 36.53, -6.29, (10, 45), 174.0),
    ("Málaga", 36.72, -4.42, (10, 48), 108.0),
    ("Ceuta", 35.89, -5.32, (10, 45), 288.0),
    ("Melilla", 35.29, -2.94, (10, 48), 274.0),
];

fn eclipse_at(lat: f64, lon: f64) -> light_studio_astro::eclipse::SolarEclipse {
    let found = solar_eclipses(unix_from_civil(2027, 8, 1, 0, 0, 0), unix_from_civil(2027, 8, 3, 0, 0, 0), Observer::new(lat, lon));
    assert_eq!(found.len(), 1, "one eclipse");
    found[0]
}

#[test]
fn totality_starts_within_the_minute_the_ign_gives_and_lasts_as_long() {
    for (name, lat, lon, (h, m), secs) in IGN {
        let e = eclipse_at(lat, lon);
        assert!(e.total, "{name} is in the path");
        let (c2, c3) = (e.c2.expect("c2"), e.c3.expect("c3"));
        // the IGN's start is truncated to the minute; official time is UTC+2
        let start = unix_from_civil(2027, 8, 2, h - 2, m, 0);
        assert!(c2 >= start - 30.0 && c2 <= start + 90.0, "{name}: totality starts {:.0} s after {h}:{m:02} official time", c2 - start);
        assert!(((c3 - c2) - secs).abs() <= 25.0, "{name}: lasts {:.0} s, the IGN says {secs}", c3 - c2);
        assert!(e.c1.is_some_and(|c1| c1 < c2) && e.c4.is_some_and(|c4| c4 > c3));
    }
}

#[test]
fn outside_the_path_the_eclipse_is_partial_but_deep() {
    // the IGN: at least 70 % of the Sun covered anywhere in mainland Spain
    for (name, lat, lon) in
        [("Donostia", 43.32, -1.98), ("Bilbao", 43.26, -2.93), ("Madrid", 40.42, -3.70), ("Barcelona", 41.39, 2.17), ("A Coruña", 43.37, -8.40)]
    {
        let e = eclipse_at(lat, lon);
        assert!(!e.total && e.c2.is_none() && e.c3.is_none(), "{name}");
        assert!(e.obscuration >= 0.70 && e.obscuration < 1.0, "{name}: {:.3}", e.obscuration);
        assert!(e.sun_alt > 30.0, "{name}: the Sun is high");
    }
}
