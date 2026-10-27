//! «My location» for the planner: where this computer is. On a Mac the system's location service is asked first (through a
//! short JavaScript-for-Automation script, so the app needs no unsafe code); otherwise, and when that gives nothing, a
//! public IP-geolocation service answers, good to a city or so. That reveals the IP address to the service, so nothing is
//! requested until the person clicks.

use serde_json::Value;

use crate::net;

/// A position found.
#[derive(Clone, Debug, PartialEq)]
pub struct Located {
    pub lat: f64,
    pub lon: f64,
    /// «City, Region, Country», as far as the service gave it.
    pub label: String,
}

/// How a lookup ended.
pub type Outcome = Result<Located, String>;

/// Services tried in order (https, no key).
const SERVICES: [&str; 2] = ["https://ipwho.is/", "https://ipapi.co/json/"];

/// Reads the system script's answer, `latitude,longitude,accuracy_in_metres`.
pub fn parse_system(text: &str) -> Option<Located> {
    let mut it = text.trim().split(',').map(|x| x.trim().parse::<f64>().ok().filter(|v| v.is_finite()));
    let (lat, lon, acc) = (it.next()??, it.next()??, it.next()??);
    if lat.abs() > 90.0 || lon.abs() > 180.0 || (lat == 0.0 && lon == 0.0) || acc < 0.0 {
        return None;
    }
    Some(Located { lat, lon, label: format!("± {} m", acc.round() as i64) })
}

/// Reads a service's JSON answer: `latitude`/`longitude` with the place names around them.
pub fn parse(json: &str) -> Outcome {
    let v: Value = serde_json::from_str(json).map_err(|_| "the location service answered something unreadable".to_string())?;
    if v.get("success").and_then(Value::as_bool) == Some(false) || v.get("error").and_then(Value::as_bool) == Some(true) {
        return Err("the location service could not place this connection".into());
    }
    let num = |k: &str| v.get(k).and_then(Value::as_f64).filter(|x| x.is_finite());
    let (Some(lat), Some(lon)) = (num("latitude"), num("longitude")) else {
        return Err("the location service gave no position".into());
    };
    if lat.abs() > 90.0 || lon.abs() > 180.0 {
        return Err("the location service gave an impossible position".into());
    }
    let text = |k: &str| v.get(k).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty());
    let label = [text("city"), text("region").or(text("region_name")), text("country").or(text("country_name"))]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(", ");
    Ok(Located { lat, lon, label })
}

/// The Mac's own location service. macOS may ask once for permission; anything else (no permission, no answer in time) is
/// `None` and the IP lookup follows.
#[cfg(target_os = "macos")]
fn system_location() -> Option<Located> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    const SCRIPT: &str = r#"
ObjC.import('CoreLocation'); ObjC.import('Foundation');
var m = $.CLLocationManager.alloc.init;
m.desiredAccuracy = $.kCLLocationAccuracyBest;
try { m.requestWhenInUseAuthorization; } catch (e) {}
m.startUpdatingLocation;
var end = Date.now() + 9000, best = null;
while (Date.now() < end) {
  $.NSRunLoop.currentRunLoop.runUntilDate($.NSDate.dateWithTimeIntervalSinceNow(0.25));
  var l = m.location;
  if (l && l.horizontalAccuracy >= 0) {
    if (!best || l.horizontalAccuracy < best.a) { best = {a: l.horizontalAccuracy, x: l.coordinate.latitude, y: l.coordinate.longitude}; }
    if (best.a <= 65) { break; }
  }
}
m.stopUpdatingLocation;
best ? (best.x + ',' + best.y + ',' + best.a) : '';
"#;
    let mut child =
        Command::new("/usr/bin/osascript").args(["-l", "JavaScript", "-e", SCRIPT]).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let started = std::time::Instant::now();
    loop {
        if started.elapsed() > std::time::Duration::from_secs(14) {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(100)),
            Err(_) => return None,
        }
    }
    let mut out = String::new();
    child.stdout.take()?.take(256).read_to_string(&mut out).ok()?;
    parse_system(&out)
}

/// Finds this computer's position (blocking: run it off the interface thread).
pub fn lookup() -> Outcome {
    #[cfg(target_os = "macos")]
    if let Some(l) = system_location() {
        return Ok(l);
    }
    let mut errors = Vec::new();
    for url in SERVICES {
        let got = net::get(url, &[("Accept", "application/json".to_string())], net::Timeouts::secs(6, 10), 64 * 1024)
            .map_err(|e| e.to_string())
            .and_then(|r| {
                if r.status == 200 {
                    String::from_utf8(r.body).map_err(|_| "unreadable answer".to_string())
                } else {
                    Err(format!("answered {}", r.status))
                }
            })
            .and_then(|t| parse(&t));
        match got {
            Ok(l) => return Ok(l),
            Err(e) => errors.push(e),
        }
    }
    Err(format!("Could not find your location: {}. Check the connection and try again.", errors.join("; ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_systems_answer() {
        let l = parse_system("43.3209,-1.9841,38.4\n").unwrap();
        assert_eq!((l.lat, l.lon, l.label.as_str()), (43.3209, -1.9841, "± 38 m"));
        for bad in ["", "x,y,z", "43.3,-1.9", "0,0,10", "95,10,10", "nan,1,1", "43,2,-1"] {
            assert!(parse_system(bad).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn reads_both_services_answers() {
        let a = parse(r#"{"success":true,"latitude":43.32,"longitude":-1.98,"city":"San Sebastián","region":"Basque Country","country":"Spain"}"#)
            .unwrap();
        assert_eq!((a.lat, a.lon, a.label.as_str()), (43.32, -1.98, "San Sebastián, Basque Country, Spain"));
        let b = parse(r#"{"latitude":40.4,"longitude":-3.7,"city":"Madrid","region":"Madrid","country_name":"Spain"}"#).unwrap();
        assert_eq!(b.label, "Madrid, Madrid, Spain");
    }

    #[test]
    fn refuses_bad_answers_without_panicking() {
        for bad in [
            "",
            "{}",
            "not json",
            r#"{"success":false}"#,
            r#"{"latitude":95,"longitude":0}"#,
            r#"{"latitude":"x","longitude":1}"#,
            r#"{"error":true}"#,
        ] {
            assert!(parse(bad).is_err(), "{bad}");
        }
    }
}
