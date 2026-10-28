//! The place's real UTC offset, from Open-Meteo's `timezone=auto` (free, no key). One small request per place; the answer
//! is kept with the place's state, and the planner's own estimate (see `light_studio_astro::time`) stays as the fallback.
//! `LIGHT_STUDIO_TIMEZONE_URL` points the request at another server answering like Open-Meteo (tests).

use serde_json::Value;

pub const SERVICE: &str = "https://api.open-meteo.com/v1/forecast";
const MAX_BODY: usize = 64 * 1024;

/// What the service said about a place.
#[derive(Clone, Debug, PartialEq)]
pub struct Zone {
    /// UTC offset in force when it was asked, in minutes.
    pub offset_min: i32,
    /// The IANA name (`Europe/Madrid`), when it gave one.
    pub name: String,
}

pub fn url(base: &str, lat: f64, lon: f64) -> String {
    format!("{base}?latitude={lat:.4}&longitude={lon:.4}&timezone=auto&forecast_days=1&daily=sunrise")
}

/// Reads the answer, refusing anything that is not a plausible offset (whole quarter-hours between −12 h and +14 h).
pub fn parse(text: &str) -> Result<Zone, String> {
    let v: Value = serde_json::from_str(text).map_err(|_| "the time zone service gave an unreadable answer".to_string())?;
    let secs = v.get("utc_offset_seconds").and_then(Value::as_i64).ok_or("the time zone service gave no offset")?;
    if secs % 900 != 0 || !(-12 * 3600..=14 * 3600).contains(&secs) {
        return Err("the time zone service gave an impossible offset".into());
    }
    let name = v.get("timezone").and_then(Value::as_str).map(str::trim).filter(|s| s.len() <= 64).unwrap_or("").to_string();
    Ok(Zone { offset_min: (secs / 60) as i32, name })
}

/// Asks for the place's zone (blocking: run it off the interface thread).
pub fn fetch(lat: f64, lon: f64) -> Result<Zone, String> {
    if !lat.is_finite() || !lon.is_finite() || lat.abs() > 90.0 || lon.abs() > 180.0 {
        return Err("no place to ask about".into());
    }
    let base = std::env::var("LIGHT_STUDIO_TIMEZONE_URL").ok().map(|u| u.trim().to_string()).filter(|u| !u.is_empty());
    let r = crate::net::get(
        &url(base.as_deref().unwrap_or(SERVICE), lat, lon),
        &[("Accept", "application/json".to_string())],
        crate::net::Timeouts::secs(6, 12),
        MAX_BODY,
    )
    .map_err(|e| e.to_string())?;
    if r.status != 200 {
        return Err(format!("the time zone service answered {}", r.status));
    }
    parse(&String::from_utf8(r.body).map_err(|_| "unreadable answer".to_string())?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_an_answer() {
        let z = parse(r#"{"latitude":38.7,"utc_offset_seconds":-21600,"timezone":"America/Denver","daily":{}}"#).unwrap();
        assert_eq!(z, Zone { offset_min: -360, name: "America/Denver".into() });
        assert_eq!(parse(r#"{"utc_offset_seconds":19800}"#).unwrap().offset_min, 330);
    }

    #[test]
    fn refuses_nonsense() {
        for bad in ["", "[]", "{}", r#"{"utc_offset_seconds":"3600"}"#, r#"{"utc_offset_seconds":999999}"#, r#"{"utc_offset_seconds":1234}"#] {
            assert!(parse(bad).is_err(), "{bad}");
        }
        assert!(fetch(f64::NAN, 0.0).is_err() && fetch(0.0, 400.0).is_err());
        assert!(url("http://x", 1.0, 2.0).contains("timezone=auto"));
    }
}
