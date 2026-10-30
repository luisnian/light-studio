//! The weather model's hourly cloud cover for a place, up to about 16 days ahead: Open-Meteo's forecast (free, no key).
//! It is a model forecast, kept apart from the astronomy and always labelled as such. Times are asked in UTC as Unix
//! seconds, so no time zone is involved. `LIGHT_STUDIO_FORECAST_URL` points the request at another server (tests).

use serde_json::Value;

pub const SERVICE: &str = "https://api.open-meteo.com/v1/forecast";
const MAX_BODY: usize = 256 * 1024;
const MAX_HOURS: usize = 16 * 24 + 48;

/// Hourly cloud cover (percent), by Unix time.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Clouds {
    hours: Vec<(i64, Option<f64>)>,
}

impl Clouds {
    /// Cloud cover at `unix`, from the nearest hour within half an hour; `None` outside the forecast or without a value.
    pub fn at(&self, unix: f64) -> Option<f64> {
        if !unix.is_finite() {
            return None;
        }
        let t = unix.round() as i64;
        let i = self.hours.partition_point(|(h, _)| *h < t);
        let near = |j: usize| self.hours.get(j).filter(|(h, _)| (h - t).abs() <= 1800).and_then(|(h, v)| v.map(|v| ((h - t).abs(), v)));
        let (a, b) = (i.checked_sub(1).and_then(near), near(i));
        match (a, b) {
            (Some(a), Some(b)) => Some(if a.0 <= b.0 { a.1 } else { b.1 }),
            (a, b) => a.or(b).map(|x| x.1),
        }
    }

    /// The last hour the forecast covers.
    pub fn until(&self) -> Option<i64> {
        self.hours.last().map(|h| h.0)
    }

    pub fn is_empty(&self) -> bool {
        self.hours.is_empty()
    }
}

pub fn url(base: &str, lat: f64, lon: f64) -> String {
    format!("{base}?latitude={lat:.3}&longitude={lon:.3}&hourly=cloud_cover&forecast_days=16&timezone=GMT&timeformat=unixtime")
}

pub fn parse(text: &str) -> Result<Clouds, String> {
    let v: Value = serde_json::from_str(text).map_err(|_| "the weather service gave an unreadable answer".to_string())?;
    let hourly = v.get("hourly").ok_or("the weather service gave no hourly data")?;
    let times = hourly.get("time").and_then(Value::as_array).ok_or("the weather service gave no times")?;
    let cover = hourly.get("cloud_cover").and_then(Value::as_array).ok_or("the weather service gave no cloud cover")?;
    if times.len() != cover.len() || times.len() > MAX_HOURS {
        return Err("the weather service gave an inconsistent answer".into());
    }
    let mut hours: Vec<(i64, Option<f64>)> = Vec::with_capacity(times.len());
    for (t, c) in times.iter().zip(cover) {
        let t = t.as_i64().filter(|t| (0..=4_102_444_800).contains(t)).ok_or("the weather service gave a time that is not one")?;
        if hours.last().is_some_and(|(p, _)| *p >= t) {
            return Err("the weather service gave times out of order".into());
        }
        hours.push((t, c.as_f64().filter(|c| c.is_finite() && (0.0..=100.0).contains(c))));
    }
    Ok(Clouds { hours })
}

/// Asks for the forecast (blocking: run it off the interface thread).
pub fn fetch(lat: f64, lon: f64) -> Result<Clouds, String> {
    if !lat.is_finite() || !lon.is_finite() || lat.abs() > 90.0 || lon.abs() > 180.0 {
        return Err("no place to ask about".into());
    }
    let base = std::env::var("LIGHT_STUDIO_FORECAST_URL").ok().map(|u| u.trim().to_string()).filter(|u| !u.is_empty());
    let r = crate::net::get(
        &url(base.as_deref().unwrap_or(SERVICE), lat, lon),
        &[("Accept", "application/json".to_string())],
        crate::net::Timeouts::secs(6, 15),
        MAX_BODY,
    )
    .map_err(|e| e.to_string())?;
    if r.status != 200 {
        return Err(format!("the weather service answered {}", r.status));
    }
    parse(&String::from_utf8(r.body).map_err(|_| "unreadable answer".to_string())?)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"{"hourly":{"time":[3600,7200,10800,14400],"cloud_cover":[10,null,80,100]}}"#;

    #[test]
    fn reads_hours_and_answers_the_nearest() {
        let c = parse(GOOD).unwrap();
        assert_eq!(c.at(3600.0), Some(10.0));
        assert_eq!(c.at(3600.0 + 1700.0), Some(10.0));
        assert_eq!(c.at(7200.0), None, "an hour without a value is not made up");
        assert_eq!(c.at(10800.0 - 600.0), Some(80.0));
        assert_eq!(c.at(14400.0 + 1799.0), Some(100.0));
        assert_eq!(c.at(14400.0 + 1900.0), None, "after the forecast");
        assert_eq!(c.at(0.0), None, "before it");
        assert_eq!(c.at(f64::NAN), None);
        assert_eq!(c.until(), Some(14400));
    }

    #[test]
    fn refuses_nonsense() {
        for bad in [
            "",
            "{}",
            r#"{"hourly":{}}"#,
            r#"{"hourly":{"time":[1,2],"cloud_cover":[1]}}"#,
            r#"{"hourly":{"time":[2,1],"cloud_cover":[1,2]}}"#,
            r#"{"hourly":{"time":["a"],"cloud_cover":[1]}}"#,
            r#"{"hourly":{"time":[-5],"cloud_cover":[1]}}"#,
        ] {
            assert!(parse(bad).is_err(), "{bad}");
        }
        // a value outside 0–100 is a missing value, not a cloud cover
        assert_eq!(parse(r#"{"hourly":{"time":[3600],"cloud_cover":[400]}}"#).unwrap().at(3600.0), None);
        assert!(fetch(f64::NAN, 0.0).is_err());
        assert!(url("http://x", 1.0, 2.0).contains("cloud_cover"));
    }

    /// Against the real service: the answer still has the shape this reader expects. Run with `cargo test -- --ignored`.
    #[test]
    #[ignore = "needs the network"]
    fn the_live_service_still_answers_in_the_shape_we_read() {
        let clouds = fetch(43.3, -2.0).expect("the live forecast");
        assert!(!clouds.is_empty());
        assert!(clouds.until().is_some_and(|t| t > 0));
    }
}
