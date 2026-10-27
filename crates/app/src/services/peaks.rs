//! The names of the summits round a place, for the Mountains view: OpenStreetMap's `natural=peak` points with a name and
//! a height (read through the Overpass API, free and keyless; data © OpenStreetMap contributors, ODbL — the view
//! shows the credit). What was found is kept on disk for the place for a month, so looking again asks nothing.
//! `LIGHT_STUDIO_OVERPASS_URL` points the query at another server answering like Overpass (tests).

use std::path::Path;
use std::time::{Duration, SystemTime};

pub const SERVICE: &str = "https://overpass-api.de/api/interpreter";
/// Other public servers answering like Overpass, tried in turn when the first is busy or unreachable.
pub const MIRRORS: [&str; 2] = ["https://overpass.kumi.systems/api/interpreter", "https://overpass.private.coffee/api/interpreter"];
/// How far round the place to look (metres) and how many summits at most.
pub const RADIUS: f64 = 62_000.0;
const MAX_PEAKS: usize = 4000;
const MAX_BODY: usize = 3 * 1024 * 1024;
const FRESH: Duration = Duration::from_secs(30 * 24 * 3600);
const MAX_CACHED: usize = 60;

/// A named summit.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Peak {
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    /// Metres above the sea.
    pub ele: f64,
}

/// The Overpass query for the summits round `(lat, lon)`.
pub fn query(lat: f64, lon: f64) -> String {
    format!("[out:json][timeout:25];node[\"natural\"=\"peak\"][\"name\"][\"ele\"](around:{RADIUS:.0},{lat:.5},{lon:.5});out body qt {MAX_PEAKS};")
}

/// Percent-encodes `s` for an address.
fn encode(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// An `ele` tag as metres: «1 234», «1234 m», «1,234.5», «1234;1240» (the first number).
fn parse_ele(s: &str) -> Option<f64> {
    let first = s.split(';').next()?.trim();
    let digits: String = first.chars().take_while(|c| c.is_ascii_digit() || matches!(c, '.' | ',' | '-')).collect();
    // «1,234.5» has a thousands comma; «1234,5» a decimal one
    let digits = if digits.contains('.') { digits.replace(',', "") } else { digits.replace(',', ".") };
    let v: f64 = digits.parse().ok()?;
    (v.is_finite() && (-500.0..=9_000.0).contains(&v)).then_some(v)
}

/// Reads Overpass's answer, keeping the summits with a name and a height, highest first.
pub fn parse(body: &[u8]) -> Result<Vec<Peak>, String> {
    let v: serde_json::Value = serde_json::from_slice(body).map_err(|e| format!("the map data service answered something else ({e})"))?;
    let list = v.get("elements").and_then(|e| e.as_array()).ok_or("the map data service gave no summits")?;
    let mut out: Vec<Peak> = list
        .iter()
        .filter_map(|e| {
            let tags = e.get("tags")?;
            let name = tags.get("name")?.as_str()?.trim();
            let ele = parse_ele(tags.get("ele")?.as_str()?)?;
            let (lat, lon) = (e.get("lat")?.as_f64()?, e.get("lon")?.as_f64()?);
            let ok = !name.is_empty() && name.chars().count() <= 80 && lat.abs() <= 90.0 && lon.abs() <= 180.0;
            ok.then(|| Peak { name: name.to_string(), lat, lon, ele })
        })
        .take(MAX_PEAKS)
        .collect();
    out.sort_by(|a, b| b.ele.total_cmp(&a.ele));
    Ok(out)
}

/// The named summits within [`RADIUS`] of the place (the network the first time).
pub fn near(lat: f64, lon: f64) -> Result<Vec<Peak>, String> {
    if !(lat.is_finite() && lon.is_finite() && lat.abs() <= 90.0 && lon.abs() <= 180.0) {
        return Err("not a place on Earth".into());
    }
    let dir = crate::paths::config_dir().map(|d| d.join("peaks-cache"));
    // a server of one's own (tests) is the only one asked; otherwise the main one, then the mirrors
    let bases: Vec<String> = match std::env::var("LIGHT_STUDIO_OVERPASS_URL").ok().map(|u| u.trim().to_string()).filter(|u| !u.is_empty()) {
        Some(own) => vec![own],
        None => std::iter::once(SERVICE).chain(MIRRORS).map(String::from).collect(),
    };
    let mut last = String::from("no map data service to ask");
    for base in &bases {
        match near_with(dir.as_deref(), base, lat, lon) {
            Ok(p) => return Ok(p),
            Err(e) => last = e,
        }
    }
    Err(last)
}

fn near_with(dir: Option<&Path>, base: &str, lat: f64, lon: f64) -> Result<Vec<Peak>, String> {
    // (places a few hundred metres apart share a lookup)
    let file = dir.map(|d| d.join(format!("{:.2}_{:.2}.json", lat, lon)));
    if let Some(f) = file.as_deref()
        && let Some(peaks) = read_cache(f)
    {
        return Ok(peaks);
    }
    let address = format!("{base}?data={}", encode(&query(lat, lon)));
    let body = get(&address)?;
    let peaks = parse(&body)?;
    if let Some(f) = file.as_deref() {
        write_cache(f, &peaks);
    }
    Ok(peaks)
}

fn read_cache(file: &Path) -> Option<Vec<Peak>> {
    let age = SystemTime::now().duration_since(std::fs::metadata(file).ok()?.modified().ok()?).unwrap_or_default();
    if age > FRESH {
        return None;
    }
    serde_json::from_slice(&std::fs::read(file).ok()?).ok()
}

fn write_cache(file: &Path, peaks: &[Peak]) {
    let Some(dir) = file.parent() else { return };
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let Ok(bytes) = serde_json::to_vec(peaks) else { return };
    let tmp = file.with_extension("tmp");
    if std::fs::write(&tmp, bytes).is_ok() && std::fs::rename(&tmp, file).is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    files.sort();
    let extra = files.len().saturating_sub(MAX_CACHED);
    for (_, old) in files.into_iter().take(extra) {
        let _ = std::fs::remove_file(old);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn get(address: &str) -> Result<Vec<u8>, String> {
    use crate::net;
    let r = net::get(address, &[("Accept", "application/json".to_string())], net::Timeouts::secs(8, 35), MAX_BODY).map_err(|e| match e {
        net::NetError::TooBig => "the map data service answered too much".to_string(),
        net::NetError::BadUrl => "the map data service address is not valid".to_string(),
        net::NetError::Unreachable(_) => "could not reach the map data service (is the computer online?)".to_string(),
    })?;
    if r.status == 429 || r.status == 504 {
        return Err("the map data service is busy; the names will come next time".into());
    }
    if r.status != 200 {
        return Err(format!("the map data service answered {}", r.status));
    }
    Ok(r.body)
}

#[cfg(target_arch = "wasm32")]
fn get(_: &str) -> Result<Vec<u8>, String> {
    Err("the summit names need the desktop app".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heights_are_read_from_the_way_people_write_them() {
        assert_eq!(parse_ele("1234"), Some(1234.0));
        assert_eq!(parse_ele("1234 m"), Some(1234.0));
        assert_eq!(parse_ele("1,234.5"), Some(1234.5));
        assert_eq!(parse_ele("1234,5"), Some(1234.5));
        assert_eq!(parse_ele("1500;1510"), Some(1500.0));
        assert_eq!(parse_ele("high"), None);
        assert_eq!(parse_ele("99999"), None);
        assert_eq!(parse_ele(""), None);
    }

    #[test]
    fn the_answer_keeps_named_summits_with_a_height_highest_first() {
        let body = br#"{"elements":[
            {"type":"node","lat":43.0,"lon":-1.0,"tags":{"name":"Aizkorri","ele":"1528"}},
            {"type":"node","lat":43.1,"lon":-1.1,"tags":{"name":"Txindoki","ele":"1346 m"}},
            {"type":"node","lat":43.2,"lon":-1.2,"tags":{"name":"Sin altura"}},
            {"type":"node","lat":43.3,"lon":-1.3,"tags":{"ele":"900"}},
            {"type":"node","lat":99.0,"lon":-1.3,"tags":{"name":"Fuera","ele":"900"}},
            {"type":"node","lat":43.4,"lon":-1.4,"tags":{"name":"Hierro","ele":"x"}}]}"#;
        let p = parse(body).unwrap();
        assert_eq!(p.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["Aizkorri", "Txindoki"]);
        assert!(parse(b"<html>").is_err());
        assert!(parse(br#"{"remark":"busy"}"#).is_err());
    }

    #[test]
    fn the_query_names_the_place_and_is_encoded_for_an_address() {
        let q = query(43.3, -1.98);
        assert!(q.contains("around:62000,43.30000,-1.98000") && q.contains("natural"));
        let e = encode(&q);
        assert!(!e.contains(' ') && !e.contains('"') && !e.contains('[') && e.contains("%5B"));
    }

    use std::io::{BufRead, BufReader, Write};

    #[test]
    fn a_lookup_is_asked_once_and_kept() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}/api", listener.local_addr().unwrap());
        let count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let seen = count.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let mut reader = BufReader::new(&stream);
                loop {
                    let mut h = String::new();
                    if reader.read_line(&mut h).unwrap_or(0) <= 2 {
                        break;
                    }
                }
                let body = r#"{"elements":[{"lat":43.0,"lon":-1.0,"tags":{"name":"Aizkorri","ele":"1528"}}]}"#;
                let mut s = &stream;
                let _ = write!(
                    s,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        let dir = std::env::temp_dir().join(format!("lc-peaks-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let first = near_with(Some(&dir), &base, 43.3, -1.98).unwrap();
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].name, "Aizkorri");
        let again = near_with(Some(&dir), &base, 43.3004, -1.9804).unwrap();
        assert_eq!(again, first);
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 1, "kept for the place");
        assert!(near(f64::NAN, 0.0).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
