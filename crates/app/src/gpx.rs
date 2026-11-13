//! Routes from GPX files (the ones Wikiloc, Komoot, Garmin and most other services export): the whole
//! line of the track, its waypoints, and the numbers a hiker wants (distance, climb), for drawing on a map.
//!
//! A route needs every point, timed or not. What we read: `<trk>` → `<trkseg>` → `<trkpt lat lon>` (with `<ele>`); when the file has no track,
//! the points of `<rte>` → `<rtept>`; and the `<wpt>` waypoints with their `<name>`. The route's name is the
//! first track's, else the first `<rte>`'s, else the file's (`<metadata><name>`). Segments are joined into
//! one line.

use quick_xml::events::Event;

/// Why a GPX file could not be read.
#[derive(Clone, Debug, PartialEq)]
pub enum GpxError {
    Xml(String),
    /// Not a GPX document (no `<gpx>` root).
    NotGpx,
    /// Nesting / size limits exceeded.
    TooComplex,
}

impl std::fmt::Display for GpxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GpxError::Xml(e) => write!(f, "malformed GPX: {e}"),
            GpxError::NotGpx => f.write_str("not a GPX file (no <gpx> element)"),
            GpxError::TooComplex => f.write_str("GPX file too large or too deeply nested"),
        }
    }
}

impl std::error::Error for GpxError {}

/// An `xsd:dateTime` as seconds since the Unix epoch; no zone = UTC (the GPX convention). A bare date is too coarse.
fn parse_time(s: &str) -> Option<f64> {
    use chrono::{DateTime, NaiveDateTime};
    let s = s.trim();
    if !s.contains('T') {
        return None;
    }
    if let Ok(d) = DateTime::parse_from_rfc3339(s) {
        return Some(d.timestamp() as f64 + f64::from(d.timestamp_subsec_millis()) / 1000.0);
    }
    let n = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f").ok()?;
    let d = n.and_utc();
    Some(d.timestamp() as f64 + f64::from(d.timestamp_subsec_millis()) / 1000.0)
}

/// Nesting / size limits (hostile input only; a real route has some thousands of points).
const MAX_DEPTH: usize = 64;
const MAX_POINTS: usize = 1_000_000;
const MAX_WAYPOINTS: usize = 5_000;
/// Mean Earth radius (m).
const EARTH_R: f64 = 6_371_008.8;
/// A change of height smaller than this (m) is GPS noise, not climb.
const CLIMB_STEP: f64 = 3.0;

/// One point of a route.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoutePoint {
    pub latitude: f64,
    pub longitude: f64,
    /// Metres above sea level.
    pub elevation: Option<f64>,
    /// When it was recorded (seconds since the Unix epoch, UTC), if the file says.
    pub time: Option<f64>,
}

/// A named place along the route (a `<wpt>`).
#[derive(Clone, Debug, PartialEq)]
pub struct Waypoint {
    pub latitude: f64,
    pub longitude: f64,
    pub name: String,
}

/// A route read from a GPX file.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Route {
    pub name: String,
    pub points: Vec<RoutePoint>,
    pub waypoints: Vec<Waypoint>,
}

/// Local name of a possibly prefixed XML name (`gpx:trkpt` → `trkpt`).
fn local(name: &[u8]) -> &[u8] {
    name.rsplit(|&b| b == b':').next().unwrap_or(name)
}

#[derive(Default)]
struct Pending {
    lat: Option<f64>,
    lon: Option<f64>,
    ele: Option<f64>,
    time: Option<f64>,
    name: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Track,
    Rte,
    Wpt,
}

fn point_kind(name: &[u8], parent: Option<&[u8]>) -> Option<Kind> {
    match (name, parent) {
        (b"trkpt", Some(b"trkseg")) => Some(Kind::Track),
        (b"rtept", Some(b"rte")) => Some(Kind::Rte),
        (b"wpt", Some(b"gpx")) => Some(Kind::Wpt),
        _ => None,
    }
}

fn valid(lat: f64, lon: f64) -> bool {
    lat.is_finite() && lon.is_finite() && (-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lon)
}

#[derive(Default)]
struct Acc {
    track: Vec<RoutePoint>,
    rte: Vec<RoutePoint>,
    waypoints: Vec<Waypoint>,
    track_name: Option<String>,
    rte_name: Option<String>,
    meta_name: Option<String>,
}

impl Acc {
    fn finish(&mut self, kind: Kind, p: Pending) -> Result<(), GpxError> {
        let (Some(lat), Some(lon)) = (p.lat, p.lon) else { return Ok(()) };
        if !valid(lat, lon) {
            return Ok(());
        }
        let point = RoutePoint { latitude: lat, longitude: lon, elevation: p.ele, time: p.time };
        let line = match kind {
            Kind::Track => &mut self.track,
            Kind::Rte => &mut self.rte,
            Kind::Wpt => {
                if self.waypoints.len() >= MAX_WAYPOINTS {
                    return Ok(());
                }
                self.waypoints.push(Waypoint { latitude: lat, longitude: lon, name: p.name });
                return Ok(());
            }
        };
        if line.len() >= MAX_POINTS {
            return Err(GpxError::TooComplex);
        }
        line.push(point);
        Ok(())
    }
}

/// Parse a GPX document into a [`Route`].
pub fn parse_route(s: &str) -> Result<Route, GpxError> {
    let mut reader = quick_xml::Reader::from_str(s);
    reader.config_mut().check_end_names = true;
    let mut path: Vec<Vec<u8>> = Vec::new();
    let mut seen_gpx = false;
    let mut acc = Acc::default();
    let mut pt: Option<(Kind, Pending)> = None;
    let mut text = String::new();
    loop {
        let ev = reader.read_event().map_err(|e| GpxError::Xml(e.to_string()))?;
        match ev {
            Event::Start(ref e) | Event::Empty(ref e) => {
                let name = local(e.name().as_ref()).to_vec();
                let parent = path.last().map(Vec::as_slice);
                if name == b"gpx" && path.is_empty() {
                    seen_gpx = true;
                }
                if let Some(kind) = point_kind(&name, parent) {
                    let mut p = Pending::default();
                    for a in e.attributes().with_checks(false).flatten() {
                        let v = String::from_utf8_lossy(&a.value);
                        match local(a.key.as_ref()) {
                            b"lat" => p.lat = v.trim().parse().ok(),
                            b"lon" => p.lon = v.trim().parse().ok(),
                            _ => {}
                        }
                    }
                    pt = Some((kind, p));
                }
                text.clear();
                if matches!(ev, Event::Start(_)) {
                    if path.len() >= MAX_DEPTH {
                        return Err(GpxError::TooComplex);
                    }
                    path.push(name);
                } else if point_kind(&name, parent).is_some()
                    && let Some((kind, p)) = pt.take()
                {
                    acc.finish(kind, p)?;
                }
            }
            Event::Text(t) => text.push_str(&t.decode().map_err(|e| GpxError::Xml(e.to_string()))?),
            Event::CData(t) => text.push_str(&String::from_utf8_lossy(&t)),
            Event::End(_) => {
                let Some(name) = path.pop() else { continue };
                let parent = path.last().map(Vec::as_slice);
                let value = text.trim();
                match (name.as_slice(), parent) {
                    (b"ele", Some(b"trkpt" | b"rtept" | b"wpt")) => {
                        if let Some((_, p)) = pt.as_mut() {
                            p.ele = value.parse().ok().filter(|v: &f64| v.is_finite());
                        }
                    }
                    (b"time", Some(b"trkpt" | b"rtept")) => {
                        if let Some((_, p)) = pt.as_mut() {
                            p.time = parse_time(value);
                        }
                    }
                    (b"name", Some(b"wpt")) => {
                        if let Some((_, p)) = pt.as_mut() {
                            p.name = value.to_string();
                        }
                    }
                    (b"name", Some(b"trk")) if acc.track_name.is_none() && !value.is_empty() => acc.track_name = Some(value.to_string()),
                    (b"name", Some(b"rte")) if acc.rte_name.is_none() && !value.is_empty() => acc.rte_name = Some(value.to_string()),
                    (b"name", Some(b"metadata")) if acc.meta_name.is_none() && !value.is_empty() => acc.meta_name = Some(value.to_string()),
                    _ => {
                        if point_kind(&name, parent).is_some()
                            && let Some((kind, p)) = pt.take()
                        {
                            acc.finish(kind, p)?;
                        }
                    }
                }
                text.clear();
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !seen_gpx {
        return Err(GpxError::NotGpx);
    }
    if !path.is_empty() {
        return Err(GpxError::Xml("unclosed GPX elements".into()));
    }
    let points = if acc.track.is_empty() { acc.rte } else { acc.track };
    let name = acc.track_name.or(acc.rte_name).or(acc.meta_name).unwrap_or_default();
    Ok(Route { name, points, waypoints: acc.waypoints })
}

/// Distance between two points along the ground (m), by the haversine formula.
pub fn distance_m(a: (f64, f64), b: (f64, f64)) -> f64 {
    let (la, lb) = (a.0.to_radians(), b.0.to_radians());
    let dlat = lb - la;
    let dlon = (b.1 - a.1).to_radians();
    let h = (dlat / 2.0).sin().powi(2) + la.cos() * lb.cos() * (dlon / 2.0).sin().powi(2);
    2.0 * EARTH_R * h.sqrt().min(1.0).asin()
}

impl Route {
    /// Length of the line (m).
    pub fn distance_m(&self) -> f64 {
        self.points
            .windows(2)
            .map(|w| w.first().zip(w.get(1)).map_or(0.0, |(a, b)| distance_m((a.latitude, a.longitude), (b.latitude, b.longitude))))
            .sum()
    }

    /// Total climb and descent (m), ignoring changes smaller than a few metres (GPS noise). `(0, 0)` without heights.
    pub fn climb(&self) -> (f64, f64) {
        let (mut up, mut down) = (0.0, 0.0);
        let mut reference: Option<f64> = None;
        for e in self.points.iter().filter_map(|p| p.elevation) {
            let r = *reference.get_or_insert(e);
            let d = e - r;
            if d >= CLIMB_STEP {
                up += d;
                reference = Some(e);
            } else if d <= -CLIMB_STEP {
                down -= d;
                reference = Some(e);
            }
        }
        (up, down)
    }

    /// `[min latitude, min longitude, max latitude, max longitude]` of the line and the waypoints.
    pub fn bounds(&self) -> Option<[f64; 4]> {
        let all = self.points.iter().map(|p| (p.latitude, p.longitude)).chain(self.waypoints.iter().map(|w| (w.latitude, w.longitude)));
        all.fold(None, |b, (la, lo)| match b {
            None => Some([la, lo, la, lo]),
            Some([a, b2, c, d]) => Some([a.min(la), b2.min(lo), c.max(la), d.max(lo)]),
        })
    }
}

/// Distance along the line (m) at each point: the first is 0.
pub fn cumulative_m(points: &[RoutePoint]) -> Vec<f64> {
    let mut out = Vec::with_capacity(points.len());
    let mut total = 0.0;
    let mut prev: Option<&RoutePoint> = None;
    for p in points {
        if let Some(q) = prev {
            total += distance_m((q.latitude, q.longitude), (p.latitude, p.longitude));
        }
        out.push(total);
        prev = Some(p);
    }
    out
}

impl Route {
    /// The heights at `samples` evenly spaced distances along the whole line (first point to last), read off the full
    /// line, not a simplified one, and lightly smoothed (a moving average of three, the ends kept) so GPS noise doesn't
    /// spike the chart. Where the file has no height yet (or any more) the nearest one is held. Empty when fewer than two
    /// points carry a height or the line has no length.
    pub fn elevation_profile(&self, samples: usize) -> Vec<f32> {
        let n = samples.clamp(2, 2000);
        let cum = cumulative_m(&self.points);
        let total = cum.last().copied().unwrap_or(0.0);
        let known: Vec<(f64, f64)> = cum.iter().zip(&self.points).filter_map(|(d, p)| p.elevation.map(|e| (*d, e))).collect();
        let (Some(&(d_first, h_first)), Some(&(d_last, h_last))) = (known.first(), known.last()) else { return Vec::new() };
        if known.len() < 2 || total <= 0.0 || !total.is_finite() {
            return Vec::new();
        }
        let mut raw = Vec::with_capacity(n);
        let mut k = 0usize;
        for s in 0..n {
            let d = total * s as f64 / (n - 1) as f64;
            let h = if d <= d_first {
                h_first
            } else if d >= d_last {
                h_last
            } else {
                while known.get(k + 1).is_some_and(|next| next.0 < d) {
                    k += 1;
                }
                match (known.get(k), known.get(k + 1)) {
                    (Some(&(d0, h0)), Some(&(d1, h1))) if d1 > d0 => h0 + (h1 - h0) * ((d - d0) / (d1 - d0)).clamp(0.0, 1.0),
                    (Some(&(_, h0)), _) => h0,
                    _ => h_last,
                }
            };
            raw.push(h);
        }
        let last = raw.len().saturating_sub(1);
        raw.iter()
            .enumerate()
            .map(|(i, &h)| {
                if i == 0 || i == last {
                    return h as f32;
                }
                let (a, b) = (raw.get(i - 1).copied().unwrap_or(h), raw.get(i + 1).copied().unwrap_or(h));
                ((a + h + b) / 3.0) as f32
            })
            .collect()
    }
}

/// Distance (m) from `p` to the segment `a`–`b`, on a flat approximation (fine at route scale).
fn off_line_m(p: &RoutePoint, a: &RoutePoint, b: &RoutePoint) -> f64 {
    let k = ((a.latitude + b.latitude) / 2.0).to_radians().cos();
    let xy = |q: &RoutePoint| ((q.longitude - a.longitude) * k * 111_320.0, (q.latitude - a.latitude) * 110_540.0);
    let (bx, by) = xy(b);
    let (px, py) = xy(p);
    let len2 = bx * bx + by * by;
    let t = if len2 > 0.0 { ((px * bx + py * by) / len2).clamp(0.0, 1.0) } else { 0.0 };
    ((px - t * bx).powi(2) + (py - t * by).powi(2)).sqrt()
}

fn douglas_peucker(p: &[RoutePoint], eps: f64) -> Vec<RoutePoint> {
    let n = p.len();
    if n < 3 {
        return p.to_vec();
    }
    let mut keep = vec![false; n];
    if let Some(k) = keep.first_mut() {
        *k = true;
    }
    if let Some(k) = keep.last_mut() {
        *k = true;
    }
    // an explicit stack: no recursion to blow on a long straight line
    let mut stack = vec![(0usize, n.saturating_sub(1))];
    while let Some((lo, hi)) = stack.pop() {
        if hi <= lo + 1 {
            continue;
        }
        let (Some(a), Some(b)) = (p.get(lo), p.get(hi)) else { continue };
        let mut far = 0.0;
        let mut at = None;
        for (i, q) in p.iter().enumerate().take(hi).skip(lo + 1) {
            let d = off_line_m(q, a, b);
            if d > far {
                far = d;
                at = Some(i);
            }
        }
        if let Some(i) = at.filter(|_| far > eps) {
            if let Some(k) = keep.get_mut(i) {
                *k = true;
            }
            stack.push((lo, i));
            stack.push((i, hi));
        }
    }
    p.iter().zip(&keep).filter(|(_, k)| **k).map(|(q, _)| *q).collect()
}

/// Every `step`-th point, and always the last.
fn thin(p: &[RoutePoint], step: usize) -> Vec<RoutePoint> {
    let mut v: Vec<RoutePoint> = p.iter().step_by(step.max(1)).copied().collect();
    if let Some(last) = p.last()
        && v.last() != Some(last)
    {
        v.push(*last);
    }
    v
}

/// The line with at most about `max_points` points (the ends are kept), dropping those that change its shape least
/// (Douglas–Peucker, tolerance growing until it fits). Heights stay with the points kept.
pub fn simplify(points: &[RoutePoint], max_points: usize) -> Vec<RoutePoint> {
    let max_points = max_points.max(2);
    if points.len() <= max_points {
        return points.to_vec();
    }
    // a cheap first thinning keeps the work bounded on a huge log
    const FIRST: usize = 20_000;
    let pre = if points.len() > FIRST { thin(points, points.len().div_ceil(FIRST)) } else { points.to_vec() };
    let mut eps = 1.0;
    for _ in 0..24 {
        let out = douglas_peucker(&pre, eps);
        if out.len() <= max_points {
            return out;
        }
        eps *= 1.6;
    }
    thin(&pre, pre.len().div_ceil(max_points))
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIKILOC: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<gpx version="1.1" creator="Wikiloc" xmlns="http://www.topografix.com/GPX/1/1">
  <metadata><name>Fichero</name></metadata>
  <wpt lat="43.30" lon="-1.98"><ele>12</ele><name>Cima</name></wpt>
  <trk><name>Ulia por el faro</name>
    <trkseg>
      <trkpt lat="43.3200" lon="-1.9500"><ele>10</ele></trkpt>
      <trkpt lat="43.3210" lon="-1.9500"><ele>30</ele></trkpt>
      <trkpt lat="43.3220" lon="-1.9500"><ele>20</ele></trkpt>
    </trkseg>
  </trk>
</gpx>"#;

    #[test]
    fn reads_a_wikiloc_style_file() {
        let r = parse_route(WIKILOC).unwrap();
        assert_eq!(r.name, "Ulia por el faro");
        assert_eq!(r.points.len(), 3);
        assert_eq!(r.points[1].elevation, Some(30.0));
        assert_eq!(r.waypoints, vec![Waypoint { latitude: 43.30, longitude: -1.98, name: "Cima".into() }]);
        let km = r.distance_m();
        assert!((km - 222.4).abs() < 1.0, "0.002° of latitude is about 222 m, got {km}");
        let (up, down) = r.climb();
        assert_eq!((up, down), (20.0, 10.0));
    }

    #[test]
    fn untimed_points_are_kept() {
        let r = parse_route(WIKILOC).unwrap();
        assert!(r.points.iter().all(|p| p.elevation.is_some()));
        assert_eq!(r.points.len(), 3);
    }

    #[test]
    fn falls_back_to_the_planned_route() {
        let gpx = r#"<gpx version="1.1"><rte><name>Plan</name><rtept lat="1" lon="2"/><rtept lat="1.5" lon="2.5"/></rte></gpx>"#;
        let r = parse_route(gpx).unwrap();
        assert_eq!(r.name, "Plan");
        assert_eq!(r.points.len(), 2);
    }

    #[test]
    fn the_file_name_is_the_last_resort() {
        let gpx = r#"<gpx><metadata><name>Mi ruta</name></metadata><trk><trkseg><trkpt lat="1" lon="1"/></trkseg></trk></gpx>"#;
        assert_eq!(parse_route(gpx).unwrap().name, "Mi ruta");
    }

    #[test]
    fn bad_input_is_an_error_not_a_panic() {
        assert_eq!(parse_route("<html></html>"), Err(GpxError::NotGpx));
        assert!(parse_route("<gpx><trk></gpx>").is_err());
        assert!(parse_route("").is_err());
        // out-of-range and unreadable coordinates are skipped
        let gpx = r#"<gpx><trk><trkseg><trkpt lat="95" lon="0"/><trkpt lat="x" lon="0"/><trkpt lat="1" lon="1"/></trkseg></trk></gpx>"#;
        assert_eq!(parse_route(gpx).unwrap().points.len(), 1);
    }

    #[test]
    fn deep_nesting_is_refused() {
        let s = format!("<gpx>{}{}</gpx>", "<a>".repeat(200), "</a>".repeat(200));
        assert_eq!(parse_route(&s), Err(GpxError::TooComplex));
    }

    #[test]
    fn climb_ignores_small_noise() {
        let mk = |h: &[f64]| Route {
            points: h.iter().map(|e| RoutePoint { latitude: 0.0, longitude: 0.0, elevation: Some(*e), time: None }).collect(),
            ..Default::default()
        };
        assert_eq!(mk(&[100.0, 101.0, 100.0, 102.0, 101.0]).climb(), (0.0, 0.0));
        assert_eq!(mk(&[100.0, 110.0, 105.0]).climb(), (10.0, 5.0));
        assert_eq!(Route::default().climb(), (0.0, 0.0));
    }

    #[test]
    fn bounds_cover_the_line_and_the_waypoints() {
        let r = parse_route(WIKILOC).unwrap();
        let b = r.bounds().unwrap();
        assert!((b[0] - 43.30).abs() < 1e-9 && (b[1] + 1.98).abs() < 1e-9 && (b[2] - 43.322).abs() < 1e-9 && (b[3] + 1.95).abs() < 1e-9);
        assert_eq!(Route::default().bounds(), None);
    }

    #[test]
    fn simplify_keeps_the_ends_and_the_corners() {
        // a long straight line with one corner in the middle
        let mut pts: Vec<RoutePoint> =
            (0..1000).map(|i| RoutePoint { latitude: 43.0, longitude: f64::from(i) * 1e-4, elevation: None, time: None }).collect();
        pts.extend((1..=1000).map(|i| RoutePoint { latitude: 43.0 + f64::from(i) * 1e-4, longitude: 0.0999, elevation: Some(5.0), time: None }));
        let out = simplify(&pts, 50);
        assert!(out.len() <= 50, "{}", out.len());
        assert_eq!(out.first(), pts.first());
        assert_eq!(out.last(), pts.last());
        assert!(out.iter().any(|p| (p.longitude - 0.0999).abs() < 1e-9 && (p.latitude - 43.0).abs() < 1e-9), "the corner survives");
        assert_eq!(simplify(&pts[..10], 50).len(), 10);
        assert_eq!(simplify(&[], 50), Vec::new());
    }

    #[test]
    fn simplify_bounds_the_work_on_a_huge_noisy_line() {
        let pts: Vec<RoutePoint> = (0..200_000)
            .map(|i| RoutePoint { latitude: 43.0 + (f64::from(i) * 0.7).sin() * 1e-3, longitude: f64::from(i) * 1e-6, elevation: None, time: None })
            .collect();
        let out = simplify(&pts, 500);
        assert!(out.len() <= 500, "{}", out.len());
        assert_eq!(out.last(), pts.last());
    }

    fn at(lon: f64, ele: Option<f64>) -> RoutePoint {
        RoutePoint { latitude: 0.0, longitude: lon, elevation: ele, time: None }
    }

    #[test]
    fn the_profile_follows_the_heights_along_the_distance() {
        let r = Route { points: vec![at(0.0, Some(0.0)), at(0.001, Some(10.0)), at(0.002, Some(20.0))], ..Default::default() };
        let p = r.elevation_profile(5);
        assert_eq!(p.len(), 5);
        assert_eq!((p[0], p[4]), (0.0, 20.0), "the ends are kept as they are");
        assert!(p.windows(2).all(|w| w[0] <= w[1]), "{p:?}");
        assert!((p[2] - 10.0).abs() < 0.05, "{p:?}");
    }

    #[test]
    fn the_profile_needs_heights_and_a_length() {
        let none = Route { points: vec![at(0.0, None), at(0.001, None)], ..Default::default() };
        assert!(none.elevation_profile(50).is_empty());
        let one = Route { points: vec![at(0.0, Some(5.0)), at(0.001, None)], ..Default::default() };
        assert!(one.elevation_profile(50).is_empty());
        let still = Route { points: vec![at(0.0, Some(5.0)), at(0.0, Some(9.0))], ..Default::default() };
        assert!(still.elevation_profile(50).is_empty(), "no length, no profile");
        assert!(Route::default().elevation_profile(50).is_empty());
        // an absurd number of samples is bounded
        let r = Route { points: vec![at(0.0, Some(0.0)), at(0.001, Some(10.0))], ..Default::default() };
        assert_eq!(r.elevation_profile(usize::MAX).len(), 2000);
        assert_eq!(r.elevation_profile(0).len(), 2);
    }

    #[test]
    fn the_profile_holds_the_nearest_height_where_the_file_has_none() {
        let r = Route { points: vec![at(0.0, None), at(0.001, Some(10.0)), at(0.002, Some(20.0)), at(0.003, None)], ..Default::default() };
        let p = r.elevation_profile(7);
        assert_eq!(p.first(), Some(&10.0));
        assert_eq!(p.last(), Some(&20.0));
    }

    #[test]
    fn the_profile_keeps_a_peak_that_simplifying_the_line_drops() {
        // a straight line (two points are enough to draw it) over a hill 50 m high
        let pts: Vec<RoutePoint> = (0..8000)
            .map(|i| {
                let x = f64::from(i) - 4000.0;
                at(f64::from(i) * 1e-5, Some(50.0 * (-(x * x) / (2.0 * 400.0 * 400.0)).exp()))
            })
            .collect();
        assert_eq!(simplify(&pts, 500).len(), 2, "the shape of the line alone loses the hill");
        let r = Route { points: pts, ..Default::default() };
        let max = r.elevation_profile(300).into_iter().fold(f32::MIN, f32::max);
        assert!(max > 48.0, "{max}");
    }

    #[test]
    fn cumulative_distance_starts_at_zero_and_grows() {
        let c = cumulative_m(&[at(0.0, None), at(0.001, None), at(0.003, None)]);
        assert_eq!(c.len(), 3);
        assert_eq!(c[0], 0.0);
        assert!((c[1] - 111.2).abs() < 1.0 && (c[2] - 333.6).abs() < 2.0, "{c:?}");
        assert!(cumulative_m(&[]).is_empty());
    }

    #[test]
    fn times_are_read_when_the_file_has_them() {
        let gpx = r#"<gpx><trk><trkseg>
            <trkpt lat="1" lon="1"><time>2026-06-21T18:00:00Z</time></trkpt>
            <trkpt lat="1.001" lon="1"><time>2026-06-21T18:10:30.500+02:00</time></trkpt>
            <trkpt lat="1.002" lon="1"></trkpt>
            <trkpt lat="1.003" lon="1"><time>not a time</time></trkpt>
        </trkseg></trk></gpx>"#;
        let r = parse_route(gpx).unwrap();
        let t: Vec<_> = r.points.iter().map(|p| p.time).collect();
        assert_eq!(t[0], Some(1_782_064_800.0));
        assert_eq!(t[1], Some(1_782_064_800.0 - 7200.0 + 630.5), "a time with a zone keeps it");
        assert_eq!((t[2], t[3]), (None, None));
    }
}
