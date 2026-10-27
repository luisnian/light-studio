//! Ground heights for the Planner's terrain profile and Mountains view: the Open-Meteo elevation service (Copernicus
//! DEM, about 90 m between samples; free, no key), as Luis's «Mapa solar» uses.
//!
//! The free services limit how many points one address may ask for (HTTP 429), and a whole skyline is about 1 800
//! points. So: a busy answer is waited out and asked again (honouring `Retry-After`); a service that stays busy is left
//! for the rest of the lookup and the next one (OpenTopoData's Mapzen set, global) takes over from that request on; and
//! what was found is kept on disk for the place, so looking at it again asks nothing at all.
//! `LIGHT_STUDIO_ELEVATION_URL` points the first at another server answering like Open-Meteo, and
//! `LIGHT_STUDIO_ELEVATION_FALLBACK_URL` the second at one answering like OpenTopoData (tests).

use std::path::Path;
use std::time::Duration;

/// The service, and how many points it takes in one request.
pub const SERVICE: &str = "https://api.open-meteo.com/v1/elevation";
pub const MAX_PER_REQUEST: usize = 100;
/// The second service, used when the first stays busy.
pub const FALLBACK_SERVICE: &str = "https://api.opentopodata.org/v1/mapzen";
/// Most points asked for in one call (in requests of [`MAX_PER_REQUEST`]).
pub const MAX_POINTS: usize = 400;
/// The most the mountains strip asks for (one call, a few requests one after the other).
pub const MAX_PANORAMA_POINTS: usize = 2000;
const MAX_BODY: usize = 256 * 1024;
/// Lookups kept on disk.
const MAX_CACHED: usize = 40;

const MAX_WAIT: Duration = Duration::from_secs(30);
const NOT_AN_ANSWER: &str = "the elevation service gave a height that is not one";

/// How a service is asked and answers.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Flavor {
    /// `?latitude=a,b&longitude=c,d` → `{"elevation": […]}`.
    OpenMeteo,
    /// `?locations=a,c|b,d` → `{"results": [{"elevation": h or null}, …]}`.
    OpenTopo,
}

#[derive(Clone, Debug)]
struct Service {
    base: String,
    flavor: Flavor,
    /// Pause between two requests (the free services ask for it).
    gap: Duration,
}

/// The services to ask in turn, and how long to wait before asking again after a busy answer.
#[derive(Clone, Debug)]
struct Plan {
    services: Vec<Service>,
    waits: Vec<Duration>,
}

fn env_url(name: &str) -> Option<String> {
    std::env::var(name).ok().map(|u| u.trim().to_string()).filter(|u| !u.is_empty())
}

fn plan() -> Plan {
    let first = match env_url("LIGHT_STUDIO_ELEVATION_URL") {
        Some(base) => Service { base, flavor: Flavor::OpenMeteo, gap: Duration::ZERO },
        None => Service { base: SERVICE.to_string(), flavor: Flavor::OpenMeteo, gap: Duration::from_millis(150) },
    };
    let second = match env_url("LIGHT_STUDIO_ELEVATION_FALLBACK_URL") {
        Some(base) => Some(Service { base, flavor: Flavor::OpenTopo, gap: Duration::ZERO }),
        // (with the first one pointed elsewhere, tests don't wander off to the real second one)
        None if env_url("LIGHT_STUDIO_ELEVATION_URL").is_none() => {
            Some(Service { base: FALLBACK_SERVICE.to_string(), flavor: Flavor::OpenTopo, gap: Duration::from_millis(1100) })
        }
        None => None,
    };
    Plan {
        services: std::iter::once(first).chain(second).collect(),
        waits: vec![Duration::from_secs(4), Duration::from_secs(12), Duration::from_secs(25)],
    }
}

/// The request for `points` (latitude, longitude in degrees).
pub fn query(base: &str, points: &[(f64, f64)]) -> String {
    let join = |f: &dyn Fn(&(f64, f64)) -> f64| points.iter().map(|p| format!("{:.5}", f(p))).collect::<Vec<_>>().join(",");
    format!("{base}?latitude={}&longitude={}", join(&|p| p.0), join(&|p| p.1))
}

/// The request to the second kind of service.
fn query_topo(base: &str, points: &[(f64, f64)]) -> String {
    let list = points.iter().map(|p| format!("{:.5},{:.5}", p.0, p.1)).collect::<Vec<_>>().join("%7C");
    format!("{base}?locations={list}")
}

/// Reads the answer of the second kind of service; a place it has no data for (open sea) is at sea level.
fn parse_topo(body: &[u8], expected: usize) -> Result<Vec<f64>, String> {
    let v: serde_json::Value = serde_json::from_slice(body).map_err(|e| format!("the elevation service answered something else ({e})"))?;
    let list = v.get("results").and_then(|e| e.as_array()).ok_or("the elevation service gave no heights")?;
    if list.len() != expected {
        return Err(format!("the elevation service gave {} heights for {expected} points", list.len()));
    }
    list.iter()
        .map(|r| match r.get("elevation") {
            Some(serde_json::Value::Null) => Ok(0.0),
            Some(h) => h.as_f64().filter(|h| h.is_finite() && (-12_000.0..=9_000.0).contains(h)).ok_or_else(|| NOT_AN_ANSWER.to_string()),
            None => Err(NOT_AN_ANSWER.to_string()),
        })
        .collect()
}

/// Reads the answer: `{"elevation": [h, …]}` with one height (metres) per point asked for.
pub fn parse(body: &[u8], expected: usize) -> Result<Vec<f64>, String> {
    let v: serde_json::Value = serde_json::from_slice(body).map_err(|e| format!("the elevation service answered something else ({e})"))?;
    let list = v.get("elevation").and_then(|e| e.as_array()).ok_or("the elevation service gave no heights")?;
    if list.len() != expected {
        return Err(format!("the elevation service gave {} heights for {expected} points", list.len()));
    }
    list.iter().map(|h| h.as_f64().filter(|h| h.is_finite() && (-12_000.0..=9_000.0).contains(h)).ok_or_else(|| NOT_AN_ANSWER.to_string())).collect()
}

/// The ground height (metres above sea level) at each point; needs the network (unless it was looked up before).
pub fn fetch(points: &[(f64, f64)]) -> Result<Vec<f64>, String> {
    fetch_up_to(points, MAX_POINTS)
}

/// As [`fetch`], for up to `limit` points.
pub fn fetch_up_to(points: &[(f64, f64)], limit: usize) -> Result<Vec<f64>, String> {
    let cache = crate::paths::config_dir().map(|d| d.join("elevation-cache"));
    fetch_with(&plan(), cache.as_deref(), points, limit)
}

fn check(points: &[(f64, f64)], limit: usize) -> Result<(), String> {
    if points.is_empty() || points.len() > limit {
        return Err(format!("ask for 1 to {limit} points"));
    }
    if points.iter().any(|(la, lo)| !(la.is_finite() && lo.is_finite() && la.abs() <= 90.0 && lo.abs() <= 180.0)) {
        return Err("a point is not a place on Earth".into());
    }
    Ok(())
}

fn fetch_with(plan: &Plan, cache: Option<&Path>, points: &[(f64, f64)], limit: usize) -> Result<Vec<f64>, String> {
    check(points, limit)?;
    let file = cache.map(|d| d.join(cache_name(points)));
    if let Some(heights) = file.as_deref().and_then(|f| read_cache(f, points.len())) {
        return Ok(heights);
    }
    let mut out = Vec::with_capacity(points.len());
    // the services that have not turned the lookup away yet
    let mut open: Vec<bool> = vec![true; plan.services.len()];
    let mut asked_before = false;
    let mut first_failure: Option<String> = None;
    for chunk in points.chunks(MAX_PER_REQUEST) {
        let mut got = None;
        for (i, service) in plan.services.iter().enumerate() {
            if !open.get(i).copied().unwrap_or(false) {
                continue;
            }
            if asked_before && !service.gap.is_zero() {
                std::thread::sleep(service.gap);
            }
            asked_before = true;
            let address = match service.flavor {
                Flavor::OpenMeteo => query(&service.base, chunk),
                Flavor::OpenTopo => query_topo(&service.base, chunk),
            };
            let result = get_retrying(&address, &plan.waits).and_then(|body| match service.flavor {
                Flavor::OpenMeteo => parse(&body, chunk.len()),
                Flavor::OpenTopo => parse_topo(&body, chunk.len()),
            });
            match result {
                Ok(h) => {
                    got = Some(h);
                    break;
                }
                Err(e) => {
                    log::warn!("elevation: {}: {e}", service.base);
                    if let Some(slot) = open.get_mut(i) {
                        *slot = false;
                    }
                    first_failure.get_or_insert(e);
                }
            }
        }
        match got {
            Some(h) => out.extend(h),
            None => {
                return Err(match first_failure {
                    Some(e) if plan.services.len() > 1 => format!("{e}; the other elevation service did not answer either. Try again in a minute"),
                    Some(e) => e,
                    None => "no elevation service to ask".into(),
                });
            }
        }
    }
    if let Some(f) = file.as_deref() {
        write_cache(f, &out);
    }
    Ok(out)
}

/// A lookup's file name: the first and last point and how many (the same lookup always asks the same points).
fn cache_name(points: &[(f64, f64)]) -> String {
    let (a, b) = (points.first().copied().unwrap_or_default(), points.last().copied().unwrap_or_default());
    format!("{:.4}_{:.4}_{:.4}_{:.4}_{}.json", a.0, a.1, b.0, b.1, points.len())
}

fn read_cache(file: &Path, expected: usize) -> Option<Vec<f64>> {
    let v: Vec<f64> = serde_json::from_slice(&std::fs::read(file).ok()?).ok()?;
    (v.len() == expected && v.iter().all(|h| h.is_finite() && (-12_000.0..=9_000.0).contains(h))).then_some(v)
}

/// Keeps the lookup (best effort), and the folder to the newest few dozen.
fn write_cache(file: &Path, heights: &[f64]) {
    let Some(dir) = file.parent() else { return };
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let Ok(bytes) = serde_json::to_vec(heights) else { return };
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

/// How the services turn a request down for now.
enum Busy {
    /// Too many requests (429, 503): ask again after this, if the service said.
    Limited(Option<Duration>),
    Failed(String),
}

fn get_retrying(address: &str, waits: &[Duration]) -> Result<Vec<u8>, String> {
    let mut waits = waits.iter();
    loop {
        match get(address) {
            Ok(body) => return Ok(body),
            Err(Busy::Failed(e)) => return Err(e),
            Err(Busy::Limited(after)) => match waits.next() {
                Some(w) => std::thread::sleep(after.unwrap_or(*w).min(MAX_WAIT)),
                None => return Err("the elevation service is busy (too many requests)".into()),
            },
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn get(address: &str) -> Result<Vec<u8>, Busy> {
    use crate::net;
    let offline = || Busy::Failed("could not reach the elevation service (is the computer online?)".to_string());
    let r = net::get(address, &[("Accept", "application/json".to_string())], net::Timeouts::secs(8, 20), MAX_BODY).map_err(|e| match e {
        net::NetError::TooBig => Busy::Failed("the elevation service answered too much".into()),
        net::NetError::BadUrl => Busy::Failed("the elevation service address is not valid".into()),
        net::NetError::Unreachable(_) => offline(),
    })?;
    if r.status == 429 || r.status == 503 {
        let after = r.header("Retry-After").and_then(|v| v.trim().parse::<u64>().ok()).map(Duration::from_secs);
        return Err(Busy::Limited(after));
    }
    if r.status != 200 {
        return Err(Busy::Failed(format!("the elevation service answered {}", r.status)));
    }
    Ok(r.body)
}

#[cfg(target_arch = "wasm32")]
fn get(_: &str) -> Result<Vec<u8>, Busy> {
    Err(Busy::Failed("the terrain profile needs the desktop app".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_request_lists_the_points() {
        assert_eq!(query("https://h/e", &[(43.39, -1.79), (43.3, -1.98)]), "https://h/e?latitude=43.39000,43.30000&longitude=-1.79000,-1.98000");
    }

    #[test]
    fn answers_are_read_carefully() {
        assert_eq!(parse(br#"{"elevation":[12.0, 345.5]}"#, 2).unwrap(), vec![12.0, 345.5]);
        assert!(parse(br#"{"elevation":[12.0]}"#, 2).is_err());
        assert!(parse(br#"{"elevation":[12.0, "x"]}"#, 2).is_err());
        assert!(parse(br#"{"elevation":[1e9, 1]}"#, 2).is_err());
        assert!(parse(b"<html>", 1).is_err());
        assert!(parse(br#"{"error":true}"#, 1).is_err());
    }

    #[test]
    fn bad_points_are_refused_before_asking() {
        assert!(fetch(&[]).is_err());
        assert!(fetch(&[(f64::NAN, 0.0)]).is_err());
        assert!(fetch(&vec![(0.0, 0.0); MAX_POINTS + 1]).is_err());
        assert!(fetch_up_to(&vec![(0.0, 0.0); MAX_PANORAMA_POINTS + 1], MAX_PANORAMA_POINTS).is_err());
    }

    use std::io::{BufRead, BufReader, Write};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A server on this machine answering each request with `answer(request line, how many before)`.
    fn serve(answer: impl Fn(&str, usize) -> (u16, String) + Send + Sync + 'static) -> (String, Arc<AtomicUsize>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}/e", listener.local_addr().unwrap());
        let count = Arc::new(AtomicUsize::new(0));
        let seen = count.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut line = String::new();
                let mut reader = BufReader::new(&stream);
                let _ = reader.read_line(&mut line);
                loop {
                    let mut h = String::new();
                    if reader.read_line(&mut h).unwrap_or(0) <= 2 {
                        break;
                    }
                }
                let n = seen.fetch_add(1, Ordering::SeqCst);
                let (status, body) = answer(&line, n);
                let reply = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = (&stream).write_all(reply.as_bytes());
            }
        });
        (base, count)
    }

    /// As many heights as the request has points (every one `h`), in Open-Meteo's shape.
    fn meteo(line: &str, h: f64) -> String {
        let n = line.split("latitude=").nth(1).and_then(|r| r.split('&').next()).map_or(0, |l| l.split(',').count());
        format!("{{\"elevation\":[{}]}}", vec![h.to_string(); n].join(","))
    }

    /// The same in OpenTopoData's shape.
    fn topo(line: &str, h: f64) -> String {
        let n = line.split("locations=").nth(1).and_then(|r| r.split(' ').next()).map_or(0, |l| l.split("%7C").count());
        format!("{{\"results\":[{}],\"status\":\"OK\"}}", vec![format!("{{\"elevation\":{h}}}"); n].join(","))
    }

    fn plan_for(first: &str, second: Option<&str>) -> Plan {
        let svc = |base: &str, flavor| Service { base: base.to_string(), flavor, gap: Duration::ZERO };
        Plan {
            services: std::iter::once(svc(first, Flavor::OpenMeteo)).chain(second.map(|b| svc(b, Flavor::OpenTopo))).collect(),
            waits: vec![Duration::from_millis(1); 2],
        }
    }

    fn places(n: usize) -> Vec<(f64, f64)> {
        (0..n).map(|i| (43.0 + i as f64 * 0.001, -2.0)).collect()
    }

    #[test]
    fn a_busy_answer_is_waited_out_and_asked_again() {
        let (base, count) = serve(|line, n| if n < 2 { (429, String::new()) } else { (200, meteo(line, 812.0)) });
        let h = fetch_with(&plan_for(&base, None), None, &places(5), 10).unwrap();
        assert_eq!(h, vec![812.0; 5]);
        assert_eq!(count.load(Ordering::SeqCst), 3, "two turned away, then answered");
    }

    #[test]
    fn a_service_that_stays_busy_is_left_for_the_second_one() {
        let (busy, asked_busy) = serve(|_, _| (429, String::new()));
        let (other, asked_other) = serve(|line, _| (200, topo(line, 640.0)));
        // 150 points: two requests; the busy service is tried (and its waits spent) once, not once per request
        let h = fetch_with(&plan_for(&busy, Some(&other)), None, &places(150), 200).unwrap();
        assert_eq!(h, vec![640.0; 150]);
        assert_eq!(asked_busy.load(Ordering::SeqCst), 3, "the first try and the two asks again, then left alone");
        assert_eq!(asked_other.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn when_every_service_is_busy_the_answer_says_so() {
        let (busy, _) = serve(|_, _| (429, String::new()));
        let (also, _) = serve(|_, _| (429, String::new()));
        let e = fetch_with(&plan_for(&busy, Some(&also)), None, &places(3), 10).unwrap_err();
        assert!(e.contains("busy") && e.contains("Try again"), "{e}");
    }

    #[test]
    fn a_lookup_is_kept_and_not_asked_again() {
        let dir = std::env::temp_dir().join(format!("lc-elevation-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (base, count) = serve(|line, _| (200, meteo(line, 100.0)));
        let pts = places(7);
        assert_eq!(fetch_with(&plan_for(&base, None), Some(&dir), &pts, 10).unwrap(), vec![100.0; 7]);
        assert_eq!(count.load(Ordering::SeqCst), 1);
        // a plan that points nowhere still answers: from the file
        let nowhere = plan_for("http://127.0.0.1:9/e", None);
        assert_eq!(fetch_with(&nowhere, Some(&dir), &pts, 10).unwrap(), vec![100.0; 7]);
        // another place is another lookup
        assert!(fetch_with(&nowhere, Some(&dir), &places(8), 10).is_err());
        // a damaged file is asked for again
        for e in std::fs::read_dir(&dir).unwrap().flatten() {
            std::fs::write(e.path(), "[1,2").unwrap();
        }
        assert_eq!(fetch_with(&plan_for(&base, None), Some(&dir), &pts, 10).unwrap(), vec![100.0; 7]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_second_services_answers_are_read_carefully() {
        assert_eq!(parse_topo(br#"{"results":[{"elevation":12.5},{"elevation":null}],"status":"OK"}"#, 2).unwrap(), vec![12.5, 0.0]);
        assert!(parse_topo(br#"{"results":[{"elevation":1}]}"#, 2).is_err());
        assert!(parse_topo(br#"{"results":[{"elevation":"x"}]}"#, 1).is_err());
        assert!(parse_topo(br#"{"results":[{}]}"#, 1).is_err());
        assert!(parse_topo(br#"{"error":"x"}"#, 1).is_err());
        assert_eq!(query_topo("https://h/m", &[(43.39, -1.79), (43.3, -1.98)]), "https://h/m?locations=43.39000,-1.79000%7C43.30000,-1.98000");
    }
}
