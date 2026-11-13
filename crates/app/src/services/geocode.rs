//! Place search as in a map app: type a city, a street, a mountain or a shop and get the matches as you type.
//!
//! The Planner's search used to look only in the offline GeoNames list (cities of 1 000 inhabitants and up), which
//! misses most places. This asks OpenStreetMap's public geocoders instead: Photon (made for search-as-you-type)
//! first, Nominatim if Photon does not answer. Both are free and keyless under a fair-use policy: the app sends one
//! request per pause in typing, identifies itself (`User-Agent: Light Studio`) and keeps the last answers in memory.
//! The text typed is sent to those services; nothing is sent until something is typed in the search box.

use serde_json::Value;

#[cfg(not(target_arch = "wasm32"))]
const PHOTON: &str = "https://photon.komoot.io/api/";
#[cfg(not(target_arch = "wasm32"))]
const NOMINATIM: &str = "https://nominatim.openstreetmap.org/search";
#[cfg(not(target_arch = "wasm32"))]
const MAX_BODY: usize = 256 * 1024;

/// One match: a name, where it is (district, city, region, country) and its position.
#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub name: String,
    pub detail: String,
    pub lat: f64,
    pub lon: f64,
}

/// `%`-escapes `s` for a URL's query string.
pub fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn valid(lat: f64, lon: f64) -> bool {
    lat.is_finite() && lon.is_finite() && lat.abs() <= 90.0 && lon.abs() <= 180.0
}

/// «District, City, Region, Country» without repeats, leaving out the name itself.
fn detail(name: &str, parts: &[Option<&str>]) -> String {
    let mut seen: Vec<&str> = vec![name];
    for p in parts.iter().flatten().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        if !seen.contains(&p) {
            seen.push(p);
        }
    }
    seen.iter().skip(1).copied().collect::<Vec<_>>().join(", ")
}

/// Photon's GeoJSON: `features[].geometry.coordinates = [lon, lat]` and `properties` with the names around it.
pub fn parse_photon(json: &str) -> Result<Vec<Hit>, String> {
    let v: Value = serde_json::from_str(json).map_err(|_| "the place search answered something unreadable".to_string())?;
    let features = v.get("features").and_then(Value::as_array).ok_or("the place search answered something unreadable")?;
    let mut out: Vec<Hit> = Vec::new();
    for f in features {
        let coords = f.pointer("/geometry/coordinates").and_then(Value::as_array);
        let (Some(lon), Some(lat)) = (coords.and_then(|c| c.first()).and_then(Value::as_f64), coords.and_then(|c| c.get(1)).and_then(Value::as_f64))
        else {
            continue;
        };
        if !valid(lat, lon) {
            continue;
        }
        let Some(p) = f.get("properties") else { continue };
        let text = |k: &str| p.get(k).and_then(Value::as_str);
        let street = match (text("street"), text("housenumber")) {
            (Some(s), Some(n)) => Some(format!("{s} {n}")),
            (Some(s), None) => Some(s.to_string()),
            _ => None,
        };
        let Some(name) = text("name").map(str::to_string).or(street.clone()).filter(|n| !n.trim().is_empty()) else { continue };
        let street_part = street.as_deref().filter(|s| *s != name);
        let d = detail(&name, &[street_part, text("district").or(text("locality")), text("city").or(text("county")), text("state"), text("country")]);
        let hit = Hit { name, detail: d, lat, lon };
        if !out.iter().any(|o| o.name == hit.name && o.detail == hit.detail) {
            out.push(hit);
        }
    }
    Ok(out)
}

/// Nominatim's list: `[{lat, lon, name, display_name}]` (the numbers come as text).
pub fn parse_nominatim(json: &str) -> Result<Vec<Hit>, String> {
    let v: Value = serde_json::from_str(json).map_err(|_| "the place search answered something unreadable".to_string())?;
    let list = v.as_array().ok_or("the place search answered something unreadable")?;
    let mut out = Vec::new();
    for item in list {
        let num = |k: &str| item.get(k).and_then(|x| x.as_str().and_then(|s| s.parse::<f64>().ok()).or(x.as_f64()));
        let (Some(lat), Some(lon)) = (num("lat"), num("lon")) else { continue };
        if !valid(lat, lon) {
            continue;
        }
        let display = item.get("display_name").and_then(Value::as_str).unwrap_or_default();
        let name = item.get("name").and_then(Value::as_str).filter(|n| !n.trim().is_empty()).map(str::to_string);
        let (name, rest) = match name {
            Some(n) => {
                let rest = display.strip_prefix(n.as_str()).unwrap_or(display).trim_start_matches([',', ' ']).to_string();
                (n, rest)
            }
            None => match display.split_once(',') {
                Some((a, b)) => (a.trim().to_string(), b.trim().to_string()),
                None => (display.trim().to_string(), String::new()),
            },
        };
        if name.is_empty() {
            continue;
        }
        // keep the end of a long «a, b, c, d, e» chain (region and country), which is what tells places apart
        let parts: Vec<&str> = rest.split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
        let keep = parts.len().saturating_sub(3);
        out.push(Hit { name, detail: parts.get(keep..).unwrap_or_default().join(", "), lat, lon });
    }
    Ok(out)
}

/// Which services answer: Photon's address and, when allowed, Nominatim's. `first`/`fallback` are
/// `LIGHT_STUDIO_GEOCODE_URL` / `LIGHT_STUDIO_GEOCODE_FALLBACK_URL`. While typing (`submitted == false`)
/// only Photon is asked: Nominatim's public policy does not allow search-as-you-type, so it is
/// reserved for a search the user sends with Return.
#[cfg(not(target_arch = "wasm32"))]
fn services(submitted: bool, first: Option<String>, fallback: Option<String>) -> (String, Option<String>) {
    let second = match (fallback, &first) {
        _ if !submitted => None,
        (Some(url), _) => Some(url),
        // (with the first one pointed elsewhere, tests don't wander off to the real second one)
        (None, Some(_)) => None,
        (None, None) => Some(NOMINATIM.to_string()),
    };
    (first.unwrap_or_else(|| PHOTON.to_string()), second)
}

#[cfg(not(target_arch = "wasm32"))]
fn run(submitted: bool, query: &str, lang: &str, limit: usize) -> Result<Vec<Hit>, String> {
    let env = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
    let (photon, nominatim) = services(submitted, env("LIGHT_STUDIO_GEOCODE_URL"), env("LIGHT_STUDIO_GEOCODE_FALLBACK_URL"));
    search_with(&photon, nominatim.as_deref(), query, lang, limit)
}

/// Looks `query` up while it is being typed (at most `limit` matches; `lang` is the app's language
/// code, for the names Photon can give in it). Photon only. `LIGHT_STUDIO_GEOCODE_URL` points it at
/// another server answering like Photon (tests).
#[cfg(not(target_arch = "wasm32"))]
pub fn search(query: &str, lang: &str, limit: usize) -> Result<Vec<Hit>, String> {
    run(false, query, lang, limit)
}

/// The search the user sent (Return): Photon, and Nominatim if Photon found nothing or did not
/// answer. `LIGHT_STUDIO_GEOCODE_FALLBACK_URL` points the second at one answering like Nominatim (tests).
#[cfg(not(target_arch = "wasm32"))]
pub fn search_submitted(query: &str, lang: &str, limit: usize) -> Result<Vec<Hit>, String> {
    run(true, query, lang, limit)
}

#[cfg(not(target_arch = "wasm32"))]
fn search_with(photon: &str, nominatim: Option<&str>, query: &str, lang: &str, limit: usize) -> Result<Vec<Hit>, String> {
    let q = query.trim();
    if q.chars().count() < 2 {
        return Ok(Vec::new());
    }
    let q: String = q.chars().take(120).collect();
    let limit = limit.clamp(1, 15);
    let photon_lang = if matches!(lang, "en" | "de" | "fr") { format!("&lang={lang}") } else { String::new() };
    let first = get(&format!("{photon}?q={}&limit={limit}{photon_lang}", encode(&q))).and_then(|t| parse_photon(&t));
    match (first, nominatim) {
        (Ok(hits), _) if !hits.is_empty() => Ok(hits),
        (first, Some(base)) => {
            let url = format!("{base}?q={}&format=jsonv2&limit={limit}&accept-language={}", encode(&q), encode(lang));
            // both failed: report the first service's problem
            get(&url).and_then(|t| parse_nominatim(&t)).map_err(|e| first.err().unwrap_or(e))
        }
        (first, None) => first,
    }
}

#[cfg(target_arch = "wasm32")]
pub fn search(_: &str, _: &str, _: usize) -> Result<Vec<Hit>, String> {
    Err("the place search needs the desktop app".into())
}

#[cfg(target_arch = "wasm32")]
pub fn search_submitted(_: &str, _: &str, _: usize) -> Result<Vec<Hit>, String> {
    Err("the place search needs the desktop app".into())
}

#[cfg(not(target_arch = "wasm32"))]
fn get(address: &str) -> Result<String, String> {
    use crate::net;
    let r = net::get(address, &[("Accept", "application/json".to_string())], net::Timeouts::secs(6, 12), MAX_BODY).map_err(|e| match e {
        net::NetError::TooBig => "the place search answered too much".to_string(),
        net::NetError::BadUrl => "the place search address is not valid".to_string(),
        net::NetError::Unreachable(_) => "could not reach the place search (is the computer online?)".to_string(),
    })?;
    if r.status != 200 {
        return Err(format!("the place search answered {}", r.status));
    }
    let out = r.body;
    String::from_utf8(out).map_err(|_| "the place search answered something unreadable".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nominatim_is_only_asked_for_a_search_the_user_sent() {
        let (p, n) = services(false, None, None);
        assert_eq!((p.as_str(), n), (PHOTON, None), "typing: Photon alone");
        let (_, n) = services(false, None, Some("http://x/".into()));
        assert_eq!(n, None, "typing: no second service even if one is configured");
        let (_, n) = services(true, None, None);
        assert_eq!(n.as_deref(), Some(NOMINATIM), "Return: Nominatim as the fallback");
        let (p, n) = services(true, Some("http://p/".into()), None);
        assert_eq!((p.as_str(), n), ("http://p/", None), "a custom first service keeps tests off the real second");
    }

    #[test]
    fn escapes_the_query() {
        assert_eq!(encode("San Sebastián"), "San%20Sebasti%C3%A1n");
        assert_eq!(encode("a&b=c?"), "a%26b%3Dc%3F");
    }

    #[test]
    fn reads_photons_features() {
        let json = r#"{"features":[
            {"geometry":{"coordinates":[-1.9812,43.3183]},"properties":{"name":"Donostia / San Sebastián","state":"Euskadi","country":"España","city":"Donostia / San Sebastián"}},
            {"geometry":{"coordinates":[-1.98,43.32]},"properties":{"street":"Calle Mayor","housenumber":"3","city":"Donostia / San Sebastián","country":"España"}},
            {"geometry":{"coordinates":[500.0,43.0]},"properties":{"name":"Nowhere"}},
            {"geometry":{"coordinates":[1.0]},"properties":{"name":"Broken"}},
            {"properties":{"name":"No geometry"}}]}"#;
        let hits = parse_photon(json).unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].name, "Donostia / San Sebastián");
        assert_eq!(hits[0].detail, "Euskadi, España");
        assert_eq!((hits[0].lat, hits[0].lon), (43.3183, -1.9812));
        assert_eq!(hits[1].name, "Calle Mayor 3");
        assert!(hits[1].detail.contains("España"));
        assert!(parse_photon("{").is_err());
        assert!(parse_photon("[]").is_err());
    }

    #[test]
    fn reads_nominatims_list() {
        let json = r#"[{"lat":"43.3183","lon":"-1.9812","name":"Donostia","display_name":"Donostia, Donostialdea, Gipuzkoa, Euskadi, España"},
            {"lat":"x","lon":"1","display_name":"Bad"},
            {"lat":"40.4","lon":"-3.7","name":"","display_name":"Madrid, Comunidad de Madrid, España"}]"#;
        let hits = parse_nominatim(json).unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].name, "Donostia");
        assert_eq!(hits[0].detail, "Gipuzkoa, Euskadi, España");
        assert_eq!(hits[1].name, "Madrid");
        assert!(parse_nominatim("{}").is_err());
    }

    /// A server on this computer answering `body` to whatever it is asked (for `n` requests); returns its address.
    fn serve(body: &'static str, status: u16, n: usize) -> String {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for _ in 0..n {
                let Ok((mut c, _)) = listener.accept() else { return };
                let mut buf = [0u8; 2048];
                let _ = c.read(&mut buf);
                let head =
                    format!("HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                let _ = c.write_all(head.as_bytes());
                let _ = c.write_all(body.as_bytes());
            }
        });
        format!("http://{addr}/api/")
    }

    #[test]
    fn asks_photon_then_nominatim() {
        let photon =
            serve(r#"{"features":[{"geometry":{"coordinates":[-1.98,43.32]},"properties":{"name":"Donostia","country":"España"}}]}"#, 200, 1);
        let hits = search_with(&photon, None, "donostia", "es", 5).unwrap();
        assert_eq!((hits[0].name.as_str(), hits[0].detail.as_str()), ("Donostia", "España"));
        // Photon down: Nominatim answers
        let down = serve("{}", 503, 1);
        let nominatim = serve(r#"[{"lat":"40.4","lon":"-3.7","name":"Madrid","display_name":"Madrid, España"}]"#, 200, 1);
        let hits = search_with(&down, Some(&nominatim), "madrid", "es", 5).unwrap();
        assert_eq!(hits[0].name, "Madrid");
        // both down: an error that says so
        let down = serve("{}", 503, 1);
        assert!(search_with(&down, None, "madrid", "es", 5).is_err());
        // Photon finds nothing: Nominatim is tried
        let empty = serve(r#"{"features":[]}"#, 200, 1);
        let nominatim = serve(r#"[{"lat":"40.4","lon":"-3.7","name":"Madrid","display_name":"Madrid, España"}]"#, 200, 1);
        assert_eq!(search_with(&empty, Some(&nominatim), "madrid", "es", 5).unwrap().len(), 1);
    }

    #[test]
    fn short_queries_ask_nothing() {
        assert_eq!(search("a", "es", 5), Ok(Vec::new()));
        assert_eq!(search("   ", "es", 5), Ok(Vec::new()));
    }
}
