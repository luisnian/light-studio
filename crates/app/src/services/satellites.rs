//! Orbits of the bright satellites (the space stations) from CelesTrak's public element sets (two-line elements).
//! `LIGHT_STUDIO_SATELLITES_URL` points the request at another server (tests, a mirror).

use sgp4::{Constants, Elements, MinutesSinceEpoch};

pub const SERVICE: &str = "https://celestrak.org/NORAD/elements/gp.php?GROUP=stations&FORMAT=tle";
const MAX_BODY: usize = 256 * 1024;
/// Never more objects than this: a hostile or broken answer cannot make the app work on thousands.
const MAX_OBJECTS: usize = 200;

/// One satellite: its name, the moment its elements are for, and the propagator.
#[derive(Clone, Debug)]
pub struct Orbit {
    pub name: String,
    /// The elements' epoch (Unix seconds).
    pub epoch: f64,
    constants: Constants,
}

/// Where a satellite is, in the frame of the Earth's equator and the true equinox of the date (km).
#[derive(Clone, Copy, Debug)]
pub struct Position {
    pub km: [f64; 3],
}

impl Orbit {
    /// Where the satellite is at `unix`; `None` when the model cannot follow it (a decayed orbit, a date far from the epoch).
    pub fn position(&self, unix: f64) -> Option<Position> {
        let minutes = (unix - self.epoch) / 60.0;
        if !minutes.is_finite() {
            return None;
        }
        let p = self.constants.propagate(MinutesSinceEpoch(minutes)).ok()?;
        p.position.iter().all(|c| c.is_finite()).then_some(Position { km: p.position })
    }
}

/// Reads element sets: three lines per object (the name, then the two numbered lines) or two. Objects that do not read are
/// skipped; the rest are kept.
pub fn parse(text: &str) -> Result<Vec<Orbit>, String> {
    let lines: Vec<&str> = text.lines().map(str::trim_end).filter(|l| !l.trim().is_empty()).collect();
    let mut orbits = Vec::new();
    let mut i = 0;
    while i < lines.len() && orbits.len() < MAX_OBJECTS {
        let Some(&line) = lines.get(i) else { break };
        let (name, l1, l2, used) = if line.starts_with("1 ") {
            match (lines.get(i), lines.get(i + 1)) {
                (Some(a), Some(b)) => (None, *a, *b, 2),
                _ => break,
            }
        } else {
            match (lines.get(i + 1), lines.get(i + 2)) {
                (Some(a), Some(b)) => (Some(line.trim().to_string()), *a, *b, 3),
                _ => break,
            }
        };
        i += used;
        let Ok(elements) = Elements::from_tle(name.clone(), l1.as_bytes(), l2.as_bytes()) else { continue };
        let Ok(constants) = Constants::from_elements(&elements) else { continue };
        let epoch = elements.datetime.and_utc().timestamp() as f64 + f64::from(elements.datetime.and_utc().timestamp_subsec_nanos()) / 1e9;
        orbits.push(Orbit { name: name.unwrap_or_else(|| format!("NORAD {}", elements.norad_id)), epoch, constants });
    }
    if orbits.is_empty() { Err("the satellite service gave no readable orbits".into()) } else { Ok(orbits) }
}

/// Asks for the orbits (blocking: run it off the interface thread).
pub fn fetch() -> Result<Vec<Orbit>, String> {
    let base = std::env::var("LIGHT_STUDIO_SATELLITES_URL").ok().map(|u| u.trim().to_string()).filter(|u| !u.is_empty());
    let r = crate::net::get(base.as_deref().unwrap_or(SERVICE), &[("Accept", "text/plain".to_string())], crate::net::Timeouts::secs(6, 15), MAX_BODY)
        .map_err(|e| e.to_string())?;
    if r.status != 200 {
        return Err(format!("the satellite service answered {}", r.status));
    }
    parse(&String::from_utf8(r.body).map_err(|_| "unreadable answer".to_string())?)
}

#[cfg(test)]
pub(crate) const TEST_TLE: &str = "ISS (ZARYA)
1 25544U 98067A   08264.51782528 -.00002182  00000-0 -11606-4 0  2927
2 25544  51.6416 247.4627 0006703 130.5360 325.0288 15.72125391563537
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_iss_and_puts_it_at_a_low_orbit() {
        let orbits = parse(TEST_TLE).unwrap();
        assert_eq!(orbits.len(), 1);
        assert_eq!(orbits[0].name, "ISS (ZARYA)");
        let p = orbits[0].position(orbits[0].epoch).unwrap();
        let r = p.km.iter().map(|c| c * c).sum::<f64>().sqrt();
        assert!((6_700.0..6_850.0).contains(&r), "{r}");
        // 2008-09-20 12:25:40 UTC
        assert!((orbits[0].epoch - 1_221_913_540.0).abs() < 5.0, "{}", orbits[0].epoch);
    }

    #[test]
    fn skips_what_does_not_read_and_refuses_nothing_usable() {
        assert!(parse("").is_err());
        assert!(parse("garbage\nmore garbage\nand more").is_err());
        let mixed = format!("BAD\n1 xxxx\n2 yyyy\n{TEST_TLE}");
        assert_eq!(parse(&mixed).unwrap().len(), 1);
        // two-line form
        let two: String = TEST_TLE.lines().skip(1).collect::<Vec<_>>().join("\n");
        assert_eq!(parse(&two).unwrap()[0].name, "NORAD 25544");
        // truncated
        assert!(parse("ISS\n1 25544U 98067A").is_err());
    }

    #[test]
    fn far_from_the_epoch_is_a_none_not_a_panic() {
        let o = &parse(TEST_TLE).unwrap()[0];
        assert!(o.position(f64::NAN).is_none());
        let _ = o.position(f64::MAX);
        let _ = o.position(-1e18);
    }
}
