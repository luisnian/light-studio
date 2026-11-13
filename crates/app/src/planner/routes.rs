//! Routes: GPX files (from Wikiloc, Komoot, Garmin, …) kept in the planner and drawn on the map (the **Routes**
//! mode of the map's rail).
//!
//! A route is read with [`crate::gpx::parse_route`], its length and climb worked out on the full line, and
//! the line then thinned to [`MAX_KEPT_POINTS`] points (the shape stays; the settings stay small). The GPX file
//! itself is not kept: importing it again gives the same route. The list lives in the planner's settings, like the
//! plans and the points of interest; which route is selected is not remembered.

use crate::gpx;
use crate::services::map_tiles;

use super::*;
use crate::map::{Cam, MapView, view_at};

/// How many routes are kept.
pub(super) const MAX_ROUTES: usize = 100;
/// How many points of a route are kept (a long hike has tens of thousands in the file).
const MAX_KEPT_POINTS: usize = 1_500;
/// What a route as read back from the settings may hold before it is thrown out as damaged.
const MAX_STORED_POINTS: usize = 10_000;
const MAX_WAYPOINTS: usize = 500;
/// Largest GPX file read (a day-long log at one point per second is a few MB).
const MAX_FILE: u64 = 64 * 1024 * 1024;
const MAX_NAME: usize = 120;
/// Heights kept for the profile (evenly spaced along the route), and what is read back before it is thrown away.
const PROFILE_SAMPLES: usize = 240;
const MAX_PROFILE: usize = 600;
/// How near the route a point of interest has to be to count as on it (m), as the card offers them.
pub(super) const RADII: [f32; 3] = [300.0, 1000.0, 3000.0];
/// Most points of interest listed for a route.
const MAX_NEAR: usize = 200;
/// The profile card's height.
pub(super) const PROFILE_H: f32 = 224.0;

/// The line of a selected route, and of the others.
pub(super) const ROUTE: Color32 = Color32::from_rgb(255, 149, 0);
const START: Color32 = Color32::from_rgb(52, 199, 89);
const FINISH: Color32 = Color32::from_rgb(255, 69, 58);

/// Latitude, longitude (degrees) and height (m, when the file had it) of a point of a route.
pub type RoutePt = (f64, f64, Option<f32>);

/// A route kept from a GPX file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SavedRoute {
    /// Unique among the routes (milliseconds when it was imported).
    pub id: u64,
    pub name: String,
    /// The GPX file it came from (its name only).
    pub file: String,
    pub distance_m: f64,
    pub ascent_m: f64,
    pub descent_m: f64,
    /// `[min latitude, min longitude, max latitude, max longitude]` of the line and the waypoints.
    pub bounds: [f64; 4],
    pub points: Vec<RoutePt>,
    /// Latitude, longitude and name of each waypoint.
    pub waypoints: Vec<(f64, f64, String)>,
    /// Drawn on the map.
    pub visible: bool,
    /// Heights (m) at evenly spaced distances from the start to the end, worked out on the whole line; empty when
    /// the file had no heights.
    pub profile: Vec<f32>,
    /// When each point of `points` was recorded (seconds since the Unix epoch, UTC); empty when the file had no times
    /// (or not on every point).
    pub times: Vec<f64>,
    /// The id the phone knows this route by, once it has been sent there (empty: it is not on the phone).
    pub phone_id: String,
}

/// A point of interest near a route.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Near {
    /// Its place in the planner's list of points of interest.
    pub index: usize,
    /// How far it is from the line (m).
    pub off_m: f32,
    /// How far along the route its nearest point is (0 at the start, 1 at the end).
    pub frac: f32,
}

impl Default for SavedRoute {
    fn default() -> Self {
        SavedRoute {
            id: 0,
            name: String::new(),
            file: String::new(),
            distance_m: 0.0,
            ascent_m: 0.0,
            descent_m: 0.0,
            bounds: [0.0; 4],
            points: Vec::new(),
            waypoints: Vec::new(),
            visible: true,
            profile: Vec::new(),
            times: Vec::new(),
            phone_id: String::new(),
        }
    }
}

fn valid(lat: f64, lon: f64) -> bool {
    lat.is_finite() && lon.is_finite() && lat.abs() <= 90.0 && lon.abs() <= 180.0
}

pub(super) fn elide(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Coordinates to 6 decimals (about 0.1 m): the settings file doesn't need more.
fn round6(v: f64) -> f64 {
    (v * 1e6).round() / 1e6
}

impl SavedRoute {
    /// Whether the route can be drawn (a route read back from a hand-edited or damaged settings file may not).
    pub(super) fn is_sane(&self) -> bool {
        (2..=MAX_STORED_POINTS).contains(&self.points.len())
            && self.points.iter().all(|&(la, lo, _)| valid(la, lo))
            && self.waypoints.iter().all(|(la, lo, _)| valid(*la, *lo))
    }

    /// Trims the texts, bounds the lists and works the numbers that must agree with the points out again.
    pub(super) fn tidy(&mut self) {
        self.name = elide(self.name.trim(), MAX_NAME);
        self.file = elide(self.file.trim(), MAX_NAME);
        self.waypoints.truncate(MAX_WAYPOINTS);
        self.profile.truncate(MAX_PROFILE);
        if self.profile.iter().any(|h| !h.is_finite()) {
            self.profile.clear();
        }
        if self.times.len() != self.points.len() || self.times.iter().any(|t| !t.is_finite()) || self.times.windows(2).any(|w| w[0] > w[1]) {
            self.times.clear();
        }
        for w in &mut self.waypoints {
            w.2 = elide(w.2.trim(), 80);
        }
        for v in [&mut self.distance_m, &mut self.ascent_m, &mut self.descent_m] {
            if !v.is_finite() || *v < 0.0 {
                *v = 0.0;
            }
        }
        let all = self.points.iter().map(|&(la, lo, _)| (la, lo)).chain(self.waypoints.iter().map(|(la, lo, _)| (*la, *lo)));
        if let Some(b) = all.fold(None, |b: Option<[f64; 4]>, (la, lo)| match b {
            None => Some([la, lo, la, lo]),
            Some([a, c, d, e]) => Some([a.min(la), c.min(lo), d.max(la), e.max(lo)]),
        }) {
            self.bounds = b;
        }
    }

    /// Distance along the kept line (m) at each of its points.
    pub(super) fn cumulative(&self) -> Vec<f64> {
        let mut out = Vec::with_capacity(self.points.len());
        let mut total = 0.0;
        let mut prev: Option<(f64, f64)> = None;
        for &(la, lo, _) in &self.points {
            if let Some(q) = prev {
                total += gpx::distance_m(q, (la, lo));
            }
            out.push(total);
            prev = Some((la, lo));
        }
        out
    }

    /// The point `frac` (0 to 1) of the way along the line.
    pub(super) fn point_at(&self, frac: f32) -> Option<(f64, f64)> {
        let cum = self.cumulative();
        let total = *cum.last()?;
        let want = f64::from(frac.clamp(0.0, 1.0)) * total;
        let i = cum.iter().position(|&d| d >= want).unwrap_or(cum.len().saturating_sub(1));
        let (b, d1) = (self.points.get(i)?, *cum.get(i)?);
        let Some((a, d0)) = i.checked_sub(1).and_then(|k| Some((self.points.get(k)?, *cum.get(k)?))) else { return Some((b.0, b.1)) };
        let t = if d1 > d0 { ((want - d0) / (d1 - d0)).clamp(0.0, 1.0) } else { 0.0 };
        Some((a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t))
    }

    /// The points of `places` within `radius` metres of the line, along the route.
    pub(super) fn near_places(&self, places: &[SavedPlace], radius: f32) -> Vec<Near> {
        let cum = self.cumulative();
        let total = cum.last().copied().unwrap_or(0.0);
        if self.points.len() < 2 || total <= 0.0 || !radius.is_finite() {
            return Vec::new();
        }
        let radius = f64::from(radius.max(0.0));
        let [min_la, min_lo, max_la, max_lo] = self.bounds;
        let dla = radius / 110_000.0;
        let dlo = radius / (110_000.0 * ((min_la + max_la) / 2.0).to_radians().cos().abs().max(0.05));
        let mut out = Vec::new();
        for (index, p) in places.iter().enumerate() {
            if !valid(p.lat, p.lon) || p.lat < min_la - dla || p.lat > max_la + dla || p.lon < min_lo - dlo || p.lon > max_lo + dlo {
                continue;
            }
            let Some((off, along)) = self.nearest(&cum, p.lat, p.lon) else { continue };
            if off <= radius {
                out.push(Near { index, off_m: off as f32, frac: (along / total).clamp(0.0, 1.0) as f32 });
            }
        }
        out.sort_by(|a, b| a.frac.total_cmp(&b.frac));
        out.truncate(MAX_NEAR);
        out
    }

    /// How far (m) the point is from the line and how far along the route its nearest spot is (0 to 1).
    pub(super) fn along(&self, lat: f64, lon: f64) -> Option<(f64, f32)> {
        let cum = self.cumulative();
        let total = *cum.last()?;
        let (off, d) = self.nearest(&cum, lat, lon)?;
        (total > 0.0).then(|| (off, (d / total).clamp(0.0, 1.0) as f32))
    }

    /// How far (m) the point is from the line, and the distance along the line (m) of the closest spot.
    pub(super) fn nearest(&self, cum: &[f64], lat: f64, lon: f64) -> Option<(f64, f64)> {
        let (kx, ky) = (111_320.0 * lat.to_radians().cos(), 110_540.0);
        let local = |la: f64, lo: f64| ((lo - lon) * kx, (la - lat) * ky);
        let mut best: Option<(f64, f64)> = None;
        for (w, c) in self.points.windows(2).zip(cum.windows(2)) {
            let (Some(a), Some(b), Some(&d0), Some(&d1)) = (w.first(), w.get(1), c.first(), c.get(1)) else { continue };
            let ((ax, ay), (bx, by)) = (local(a.0, a.1), local(b.0, b.1));
            let (dx, dy) = (bx - ax, by - ay);
            let len2 = dx * dx + dy * dy;
            let t = if len2 > 0.0 { (-(ax * dx + ay * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
            let off = (ax + dx * t).hypot(ay + dy * t);
            if best.is_none_or(|(o, _)| off < o) {
                best = Some((off, d0 + (d1 - d0) * t));
            }
        }
        best
    }

    /// The route as kept: the line thinned, the numbers worked out on the whole of it.
    fn from_gpx(route: &gpx::Route, file: &str, id: u64) -> SavedRoute {
        let (ascent, descent) = route.climb();
        let kept = gpx::simplify(&route.points, MAX_KEPT_POINTS);
        let name = if route.name.trim().is_empty() {
            std::path::Path::new(file).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
        } else {
            route.name.clone()
        };
        let mut r = SavedRoute {
            id,
            name,
            file: file.to_string(),
            distance_m: route.distance_m(),
            ascent_m: ascent,
            descent_m: descent,
            bounds: [0.0; 4],
            points: kept.iter().map(|p| (round6(p.latitude), round6(p.longitude), p.elevation.map(|e| e as f32))).collect(),
            waypoints: route.waypoints.iter().map(|w| (round6(w.latitude), round6(w.longitude), w.name.clone())).collect(),
            visible: true,
            profile: route.elevation_profile(PROFILE_SAMPLES),
            times: Vec::new(),
            phone_id: String::new(),
        };
        // times, when every kept point has one and they never go back
        let times: Vec<f64> = kept.iter().filter_map(|p| p.time).collect();
        if times.len() == kept.len() {
            r.times = times;
        }
        r.tidy();
        r
    }
}

fn new_id(routes: &[SavedRoute]) -> u64 {
    let mut id = (now_unix() * 1000.0).max(1.0) as u64;
    while routes.iter().any(|r| r.id == id) {
        id += 1;
    }
    id
}

/// Reads and parses a GPX file.
fn read_gpx(path: &str) -> Result<gpx::Route, String> {
    let len = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    if len > MAX_FILE {
        return Err(crate::i18n::tr("The file is too large to be a route").to_string());
    }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    gpx::parse_route(&String::from_utf8_lossy(&bytes)).map_err(|e| crate::i18n::tr(&e.to_string()).to_string())
}

/// Keeps the route in the GPX file at `path`; its id.
pub(super) fn import(st: &mut PlannerState, path: &str) -> Result<u64, String> {
    let route = read_gpx(path)?;
    if route.points.len() < 2 {
        return Err(crate::i18n::tr("The file has no route or track").to_string());
    }
    if st.routes.len() >= MAX_ROUTES {
        return Err(crate::i18n::tr("The list of routes is full").to_string());
    }
    let file = std::path::Path::new(path).file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let id = new_id(&st.routes);
    st.routes.push(SavedRoute::from_gpx(&route, &file, id));
    Ok(id)
}

/// The command behind the map's Import button: keeps the route, selects it and has the map show it.
pub fn import_route(app: &mut App, path: &str) -> Result<Value, String> {
    let id = import(&mut app.ui.planner, path)?;
    app.ui.planner.map_mode = MapMode::Routes;
    app.planner.route_sel = Some(id);
    app.planner.route_fit = Some(id);
    let r = app.ui.planner.routes.iter().find(|r| r.id == id);
    Ok(json!({
        "id": id,
        "name": r.map(|r| r.name.clone()),
        "distanceM": r.map(|r| r.distance_m),
        "ascentM": r.map(|r| r.ascent_m),
        "points": r.map(|r| r.points.len()),
        "waypoints": r.map(|r| r.waypoints.len()),
    }))
}

/// A point of interest this close (m) to a waypoint is taken to be it already.
const SAME_SPOT_M: f64 = 25.0;

/// What turning a route's waypoints into points of interest did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Kept {
    pub added: usize,
    /// Waypoints that already have a point of interest on them.
    pub existing: usize,
    /// Waypoints left out because the list of points is full.
    pub full: usize,
}

/// Makes a point of interest of each waypoint of the route that has none within [`SAME_SPOT_M`]; the new ones are ringed on
/// the map of points of interest and go to the phone with the next synchronisation.
pub(super) fn waypoints_to_pois(st: &mut PlannerState, id: u64) -> Result<Kept, String> {
    let route = st.routes.iter().find(|r| r.id == id).ok_or_else(|| format!("there is no route {id}"))?;
    let (waypoints, route_name) = (route.waypoints.clone(), route.name.clone());
    let mut kept = Kept::default();
    for (i, (la, lo, name)) in waypoints.iter().enumerate() {
        if st.saved.iter().any(|p| gpx::distance_m((p.lat, p.lon), (*la, *lo)) <= SAME_SPOT_M) {
            kept.existing += 1;
        } else if st.saved.len() >= super::MAX_SAVED {
            kept.full += 1;
        } else {
            let name = if name.trim().is_empty() { format!("{route_name} · {}", i + 1) } else { name.clone() };
            let description = format!("{}: {route_name}", crate::i18n::tr("Waypoint of the route"));
            let mut site = SavedPlace { name: elide(name.trim(), 80), lat: *la, lon: *lo, description, fresh: true, ..Default::default() };
            site.ensure_identity();
            st.saved.push(site);
            kept.added += 1;
        }
    }
    Ok(kept)
}

/// Where the small «Routes» button goes while the routes card is closed: left of the map's rail.
pub(super) fn panel_rect(rail_right: f32, free: Rect) -> Rect {
    let (w, h) = (92.0, 28.0);
    let right = rail_right - layout::RAIL_W - 8.0;
    Rect::from_min_size(pos2((right - w).max(free.left()), free.top() + 4.0), vec2(w, h))
}

/// The view that shows the whole route in the part of `free` left of `panel` (it is centred in `free`).
pub(super) fn fit_view(st: &PlannerState, id: u64, free: Rect, panel: Rect) -> Option<MapView> {
    let r = st.routes.iter().find(|r| r.id == id)?;
    let [min_la, min_lo, max_la, max_lo] = r.bounds;
    let (x0, y0) = map_tiles::project(max_la, min_lo);
    let (x1, y1) = map_tiles::project(min_la, max_lo);
    let card = if r.profile.len() >= 2 { PROFILE_H + 8.0 } else { 0.0 };
    let room = Rect::from_min_max(free.min, pos2((panel.left() - 8.0).max(free.left() + 64.0), (free.bottom() - card).max(free.top() + 64.0)));
    let v = crate::map::fit(&[(x0, y0), (x1, y1)], room)?;
    // `fit` centres the route in `room`; the map is centred in `free`, so the camera goes a little further east
    let cam = Cam::new(v, free);
    let shift = f64::from(free.center().x - room.center().x) / cam.s;
    Some(view_at(cam.cx + shift, cam.cy, v.zoom))
}

/// Where the profile card of the selected route goes: along the bottom, centred in the free part of the map. None
/// without a profile.
pub(super) fn profile_rect(st: &PlannerState, rt: &Runtime, free: Rect) -> Option<Rect> {
    let route = st.routes.iter().find(|r| Some(r.id) == rt.route_sel)?;
    if route.profile.len() < 2 {
        return None;
    }
    let w = (free.width() - 16.0).min(900.0);
    (w >= 260.0).then(|| Rect::from_center_size(pos2(free.center().x, free.bottom() - PROFILE_H / 2.0), vec2(w, PROFILE_H)))
}

/// The height at `frac` (0 to 1) of the way along the profile.
pub(super) fn sample(profile: &[f32], frac: f32) -> f32 {
    let pos = frac.clamp(0.0, 1.0) * profile.len().saturating_sub(1) as f32;
    let i = pos.floor() as usize;
    match (profile.get(i), profile.get(i + 1)) {
        (Some(&a), Some(&b)) => a + (b - a) * (pos - i as f32),
        (Some(&a), None) => a,
        _ => 0.0,
    }
}

pub(super) fn km(m: f64) -> String {
    format!("{:.1} km", m / 1000.0)
}

impl Page<'_> {
    /// The list of routes (Routes mode): import, an eye to show or hide each, a click to select it and bring it into view.
    /// The profile card of the selected route: heights along the way, the points of interest near it, and what the
    /// pointer is over (the map marks that spot).
    pub(super) fn routes_profile(&mut self, ui: &mut Ui, st: &mut PlannerState, rt: &mut Runtime, r: Rect) {
        let t = self.t;
        let Some(route) = st.routes.iter().find(|x| Some(x.id) == rt.route_sel) else { return };
        let track = if rt.route_light_off { None } else { super::routes_light::track(st, rt, route) };
        let mut date_step = 0_i64;
        let n = route.profile.len();
        if n < 2 {
            return;
        }
        let painter = ui.painter().clone();
        painter.rect_filled(r.translate(vec2(0.0, 1.5)), 12.0, Color32::from_black_alpha(50));
        crate::theme::paint_chip(&painter, r, &t);
        let _ = ui.interact(r, egui::Id::new("planner-route-profile-bg"), Sense::click_and_drag());

        let (lo, hi) = route.profile.iter().fold((f32::MAX, f32::MIN), |(a, b), &h| (a.min(h), b.max(h)));
        let hi = hi.max(lo + 10.0);
        let header = format!("{} · {:.0}–{:.0} m · +{:.0} m · -{:.0} m", km(route.distance_m), lo, hi.max(lo), route.ascent_m, route.descent_m);
        painter.text(pos2(r.left() + 14.0, r.top() + 16.0), Align2::LEFT_CENTER, header, t.semibold(12.0), t.text);

        // the options, from the right: the radius to look within, and whether to look for points of interest
        #[derive(Clone, Copy)]
        enum Chip {
            Radius(usize),
            Pois,
        }
        let mut x = r.right() - 12.0;
        let chips: [(String, bool, Chip); 4] = [
            (crate::i18n::tr("3 km").to_string(), rt.route_radius == 2, Chip::Radius(2)),
            (crate::i18n::tr("1 km").to_string(), rt.route_radius == 1, Chip::Radius(1)),
            (crate::i18n::tr("300 m").to_string(), rt.route_radius == 0, Chip::Radius(0)),
            (crate::i18n::tr("Points of Interest").to_string(), !rt.route_pois_off, Chip::Pois),
        ];
        for (k, (label, on, chip)) in chips.into_iter().enumerate() {
            let on = on && (!matches!(chip, Chip::Radius(_)) || !rt.route_pois_off);
            let g = painter.layout_no_wrap(label, t.font(11.0), if on { t.text } else { t.text_dim });
            let w = g.size().x + 14.0;
            let rect = Rect::from_min_size(pos2(x - w, r.top() + 5.0), vec2(w, 22.0));
            let resp = ui.interact(rect, egui::Id::new(("planner-route-chip", k)), Sense::click());
            if on {
                painter.rect_filled(rect, 11.0, t.tool_active);
            } else if resp.hovered() {
                painter.rect_filled(rect, 11.0, t.hover.gamma_multiply(0.5));
            }
            painter.galley(pos2(rect.left() + 7.0, rect.center().y - g.size().y / 2.0), g, t.text);
            if resp.clicked() {
                match chip {
                    Chip::Radius(i) => rt.route_radius = i,
                    Chip::Pois => rt.route_pois_off = !rt.route_pois_off,
                }
            }
            x = rect.left() - if matches!(chip, Chip::Radius(_)) { 4.0 } else { 8.0 };
        }

        // the chart
        self.light_controls(ui, &painter, r, route, rt, track.as_deref(), &mut date_step);
        let chart = Rect::from_min_max(pos2(r.left() + 50.0, r.top() + 68.0), pos2(r.right() - 16.0, r.bottom() - 48.0));
        let px = |f: f32| chart.left() + chart.width() * f.clamp(0.0, 1.0);
        let py = |h: f32| chart.bottom() - chart.height() * ((h - lo) / (hi - lo)).clamp(0.0, 1.0);
        let top: Vec<Pos2> = route.profile.iter().enumerate().map(|(i, &h)| pos2(px(i as f32 / (n - 1) as f32), py(h))).collect();
        for y in [lo, (lo + hi) / 2.0, hi] {
            let y_px = py(y);
            painter.line_segment([pos2(chart.left(), y_px), pos2(chart.right(), y_px)], Stroke::new(1.0, t.card_stroke.gamma_multiply(0.6)));
            painter.text(pos2(chart.left() - 6.0, y_px), Align2::RIGHT_CENTER, format!("{y:.0}"), t.font(10.5), t.text_dim);
        }
        for pair in top.windows(2) {
            if let [a, b] = pair {
                let quad = vec![*a, *b, pos2(b.x, chart.bottom()), pos2(a.x, chart.bottom())];
                let fill = match &track {
                    Some(tr) => {
                        let mid = (a.x + b.x) / 2.0;
                        super::routes_light::color(tr.light_at((mid - chart.left()) / chart.width().max(1.0))).gamma_multiply(0.3)
                    }
                    None => ROUTE.gamma_multiply(0.28),
                };
                painter.add(Shape::convex_polygon(quad, fill, Stroke::NONE));
            }
        }
        match &track {
            Some(tr) => {
                let at = |i: usize| (i as f32 + 0.5) / (n - 1) as f32;
                for (l, pts) in super::routes_light::coloured(&top, |i| tr.light_at(at(i))) {
                    painter.add(Shape::line(pts, Stroke::new(2.0, super::routes_light::color(l))));
                }
                self.light_strip(ui, &painter, (chart.left(), chart.right()), chart.bottom() + 5.0, tr);
            }
            None => {
                painter.add(Shape::line(top, Stroke::new(2.0, ROUTE)));
            }
        }
        for (f, label, align) in [
            (0.0, "0".to_string(), Align2::LEFT_TOP),
            (0.5, km(route.distance_m / 2.0), Align2::CENTER_TOP),
            (1.0, km(route.distance_m), Align2::RIGHT_TOP),
        ] {
            painter.text(pos2(px(f), chart.bottom() + if track.is_some() { 23.0 } else { 6.0 }), align, label, t.font(10.5), t.text_dim);
        }

        // the points of interest near the route: a dot on the line, a click opens the record
        if !rt.route_pois_off {
            let radius = RADII.get(rt.route_radius).copied().unwrap_or(RADII[0]);
            for near in route.near_places(&st.saved, radius) {
                let Some(p) = st.saved.get(near.index) else { continue };
                let at = pos2(px(near.frac), py(sample(&route.profile, near.frac)));
                let resp = ui.interact(
                    Rect::from_center_size(at, vec2(16.0, 16.0)),
                    egui::Id::new(("planner-route-profile-poi", near.index)),
                    Sense::click(),
                );
                painter.line_segment([at, pos2(at.x, chart.bottom())], Stroke::new(1.0, super::places::POI_RED.gamma_multiply(0.6)));
                painter.circle_filled(at, if resp.hovered() { 6.0 } else { 4.5 }, super::places::POI_RED);
                painter.circle_stroke(at, if resp.hovered() { 6.0 } else { 4.5 }, Stroke::new(1.5, Color32::WHITE));
                if resp.clicked() {
                    self.intents.push(Intent::OpenSite(near.index));
                }
                resp.on_hover_text(format!("{} · {:.0} m {}", p.name, near.off_m, crate::i18n::tr("from the route")));
            }
        }

        // the pointer over the chart
        let over = ui.interact(chart, egui::Id::new("planner-route-profile-chart"), Sense::hover());
        let frac = over.hover_pos().filter(|_| over.hovered()).map(|p| ((p.x - chart.left()) / chart.width().max(1.0)).clamp(0.0, 1.0));
        if frac != rt.route_hover {
            rt.route_hover = frac;
            ui.ctx().request_repaint();
        }
        if let Some(f) = frac {
            let x = px(f);
            let h = sample(&route.profile, f);
            painter.line_segment([pos2(x, chart.top()), pos2(x, chart.bottom())], Stroke::new(1.0, t.text_dim));
            painter.circle_filled(pos2(x, py(h)), 4.5, Color32::WHITE);
            painter.circle_stroke(pos2(x, py(h)), 4.5, Stroke::new(2.0, ROUTE));
            let step = route.distance_m / (n - 1) as f64;
            let i = (f * (n - 1) as f32).round() as usize;
            let (a, b) = (i.saturating_sub(1), (i + 1).min(n - 1));
            let grade = match (route.profile.get(a), route.profile.get(b)) {
                (Some(ha), Some(hb)) if step > 0.0 && b > a => f64::from(hb - ha) / (step * (b - a) as f64) * 100.0,
                _ => 0.0,
            };
            let mut text = format!("{} · {:.0} m · {:+.0} %", km(route.distance_m * f64::from(f)), h, grade);
            if let Some(tr) = &track {
                let secs = tr.secs_at(f);
                if let Some((la, lo)) = route.point_at(f) {
                    let s = light_studio_astro::spot(tr.start + secs, la, lo);
                    text =
                        format!("{text} · {} · {} · {} {:.0}°", tr.clock(secs), crate::i18n::tr(s.light.label()), crate::i18n::tr("Sun"), s.sun.alt);
                }
            }
            let g = painter.layout_no_wrap(text, t.semibold(11.5), t.text);
            let size = g.size() + vec2(14.0, 8.0);
            let left = if x + 10.0 + size.x > chart.right() { x - 10.0 - size.x } else { x + 10.0 };
            let tip = Rect::from_min_size(pos2(left, chart.top() + 2.0), size);
            crate::theme::paint_chip(&painter, tip, &t);
            painter.galley(pos2(tip.left() + 7.0, tip.top() + 4.0), g, t.text);
        }
        if date_step != 0 {
            let (date, _) = super::routes_light::day_of(st, route, now_unix());
            st.date = date.add_days(date_step).iso();
        }
    }

    /// The visible routes as lines, with a dot where each starts and ends; the selected one is heavier and shows its
    /// waypoints (hover for the name).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn draw_routes(
        &mut self,
        ui: &mut Ui,
        painter: &egui::Painter,
        st: &PlannerState,
        rt: &mut Runtime,
        cam: &Cam,
        outer: Rect,
        blocked: &[Rect],
    ) {
        let screen = |la: f64, lo: f64| {
            let (x, y) = map_tiles::project(la, lo);
            cam.screen(x, y)
        };
        for route in st.routes.iter().filter(|r| r.visible) {
            let [min_la, min_lo, max_la, max_lo] = route.bounds;
            if !outer.expand(40.0).intersects(Rect::from_two_pos(screen(min_la, min_lo), screen(max_la, max_lo))) {
                continue;
            }
            let selected = rt.route_sel == Some(route.id);
            let line: Vec<Pos2> = route.points.iter().map(|&(la, lo, _)| screen(la, lo)).collect();
            let track = if selected && !rt.route_light_off { super::routes_light::track(st, rt, route) } else { None };
            let (casing, width) = match (selected, &track) {
                (true, Some(_)) => (Color32::from_black_alpha(170), 4.5),
                (true, None) => (Color32::WHITE.gamma_multiply(0.9), 4.0),
                _ => (Color32::from_black_alpha(90), 3.0),
            };
            painter.add(Shape::line(line.clone(), Stroke::new(width + 2.5, casing)));
            match &track {
                Some(tr) => {
                    for (l, pts) in super::routes_light::coloured(&line, |i| tr.light_of(i)) {
                        painter.add(Shape::line(pts, Stroke::new(width, super::routes_light::color(l))));
                    }
                }
                None => {
                    painter.add(Shape::line(line.clone(), Stroke::new(width, if selected { ROUTE } else { ROUTE.gamma_multiply(0.85) })));
                }
            }
            let ends = |painter: &egui::Painter| {
                for (at, color) in [(line.first(), START), (line.last(), FINISH)] {
                    if let Some(&at) = at {
                        painter.circle_filled(at, 6.0, color);
                        painter.circle_stroke(at, 6.0, Stroke::new(2.0, Color32::WHITE));
                    }
                }
            };
            if !selected {
                ends(painter);
                continue;
            }
            if !rt.route_pois_off {
                let radius = RADII.get(rt.route_radius).copied().unwrap_or(RADII[0]);
                for n in route.near_places(&st.saved, radius) {
                    let Some(p) = st.saved.get(n.index) else { continue };
                    let at = screen(p.lat, p.lon);
                    if !outer.contains(at) || blocked.iter().any(|b| b.contains(at)) {
                        continue;
                    }
                    let id = egui::Id::new(("planner-route-site", n.index));
                    let resp = super::map::pin(ui, painter, at, super::places::POI_RED, Icon::Pin, id, format!("planner:routeSite:{}", n.index));
                    if resp.clicked() {
                        self.intents.push(Intent::OpenSite(n.index));
                    }
                    resp.on_hover_ui_at_pointer(|ui| {
                        ui.label(egui::RichText::new(&p.name).font(self.t.semibold(12.5)).color(self.t.text));
                        let far = format!("{:.0} m {}", n.off_m, crate::i18n::tr("from the route"));
                        ui.label(egui::RichText::new(far).font(self.t.font(11.5)).color(self.t.text_dim));
                    });
                }
            }
            if let Some((la, lo)) = rt.route_hover.and_then(|f| route.point_at(f)) {
                let at = screen(la, lo);
                painter.circle_filled(at, 9.0, ROUTE.gamma_multiply(0.3));
                painter.circle_filled(at, 5.5, Color32::WHITE);
                painter.circle_stroke(at, 5.5, Stroke::new(2.5, ROUTE));
                if let (Some(tr), Some(f)) = (&track, rt.route_hover) {
                    let s = light_studio_astro::spot(tr.start + tr.secs_at(f), la, lo);
                    self.sun_ray(painter, at, s.sun.az, s.sun.alt);
                }
            }
            for (i, (la, lo, name)) in route.waypoints.iter().enumerate() {
                let at = screen(*la, *lo);
                if !outer.contains(at) {
                    continue;
                }
                painter.circle_filled(at, 5.0, Color32::WHITE);
                painter.circle_stroke(at, 5.0, Stroke::new(2.0, ROUTE));
                if !name.is_empty() {
                    let resp =
                        ui.interact(Rect::from_center_size(at, vec2(16.0, 16.0)), egui::Id::new(("planner-route-wpt", route.id, i)), Sense::hover());
                    let mut tip = name.clone();
                    if let (Some(tr), Some((_, f))) = (&track, route.along(*la, *lo)) {
                        let secs = tr.secs_at(f);
                        let s = light_studio_astro::spot(tr.start + secs, *la, *lo);
                        tip = format!(
                            "{name} · {} · {} · {} {:.0}°",
                            tr.clock(secs),
                            crate::i18n::tr(s.light.label()),
                            crate::i18n::tr("Sun"),
                            s.sun.alt
                        );
                    }
                    resp.on_hover_text(tip);
                }
            }
            ends(painter);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GPX: &str = r#"<?xml version="1.0"?>
<gpx version="1.1" creator="Wikiloc" xmlns="http://www.topografix.com/GPX/1/1">
  <wpt lat="43.3205" lon="-1.9505"><name>Mirador</name></wpt>
  <trk><name>Ulia</name><trkseg>
    <trkpt lat="43.3200" lon="-1.9500"><ele>10</ele></trkpt>
    <trkpt lat="43.3210" lon="-1.9510"><ele>40</ele></trkpt>
    <trkpt lat="43.3220" lon="-1.9500"><ele>25</ele></trkpt>
  </trkseg></trk>
</gpx>"#;

    fn file(name: &str, text: &str) -> String {
        // one folder per call: tests run in parallel and write files of the same name
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("ls-routes-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        std::fs::write(&p, text).unwrap();
        p.to_string_lossy().into_owned()
    }

    #[test]
    fn a_gpx_file_becomes_a_route() {
        let mut st = PlannerState::default();
        let id = import(&mut st, &file("ulia.gpx", GPX)).unwrap();
        let r = &st.routes[0];
        assert_eq!((r.id, r.name.as_str(), r.file.as_str()), (id, "Ulia", "ulia.gpx"));
        assert_eq!(r.points.len(), 3);
        assert_eq!(r.waypoints, vec![(43.3205, -1.9505, "Mirador".to_string())]);
        assert!((r.distance_m - 245.0).abs() < 40.0, "{}", r.distance_m);
        assert_eq!((r.ascent_m, r.descent_m), (30.0, 15.0));
        assert!(r.is_sane() && r.visible);
        // [min lat, min lon, max lat, max lon] over the line and the waypoint
        assert_eq!(r.bounds, [43.32, -1.951, 43.322, -1.95]);
        // importing again makes another route with another id
        let again = import(&mut st, &file("ulia.gpx", GPX)).unwrap();
        assert_ne!(again, id);
        assert_eq!(st.routes.len(), 2);
    }

    #[test]
    fn a_file_without_a_name_gives_its_own() {
        let gpx = r#"<gpx><trk><trkseg><trkpt lat="1" lon="1"/><trkpt lat="1.1" lon="1.1"/></trkseg></trk></gpx>"#;
        let mut st = PlannerState::default();
        import(&mut st, &file("sin-nombre.gpx", gpx)).unwrap();
        assert_eq!(st.routes[0].name, "sin-nombre");
    }

    #[test]
    fn bad_files_are_errors_and_change_nothing() {
        let mut st = PlannerState::default();
        assert!(import(&mut st, &file("not.gpx", "<html/>")).is_err());
        assert!(import(&mut st, &file("empty.gpx", "<gpx></gpx>")).is_err());
        let one = r#"<gpx><trk><trkseg><trkpt lat="1" lon="1"/></trkseg></trk></gpx>"#;
        assert!(import(&mut st, &file("one.gpx", one)).is_err());
        assert!(import(&mut st, "/no/such/file.gpx").is_err());
        assert!(st.routes.is_empty());
    }

    #[test]
    fn a_long_log_is_thinned_but_keeps_its_length() {
        let mut body = String::from(r#"<gpx><trk><trkseg>"#);
        for i in 0..8000 {
            body += &format!(
                r#"<trkpt lat="{}" lon="{}"><ele>{}</ele></trkpt>"#,
                43.0 + f64::from(i) * 1e-5,
                -2.0 + (f64::from(i) * 0.05).sin() * 1e-3,
                100 + i / 50
            );
        }
        body += "</trkseg></trk></gpx>";
        let mut st = PlannerState::default();
        import(&mut st, &file("long.gpx", &body)).unwrap();
        let r = &st.routes[0];
        assert!(r.points.len() <= MAX_KEPT_POINTS && r.points.len() > 10, "{}", r.points.len());
        assert!(r.distance_m > 8000.0 * 1.1, "worked out on the whole line, not the thinned one: {}", r.distance_m);
        assert!(r.ascent_m > 100.0);
    }

    #[test]
    fn routes_survive_the_settings_and_old_settings_have_none() {
        let mut st = PlannerState::default();
        import(&mut st, &file("ulia.gpx", GPX)).unwrap();
        let json = serde_json::to_string(&st).unwrap();
        let back: PlannerState = serde_json::from_str(&json).unwrap();
        assert_eq!(back.routes, st.routes);
        let old: PlannerState = serde_json::from_str(r#"{"lat": 43.3}"#).unwrap();
        assert!(old.routes.is_empty());
    }

    #[test]
    fn damaged_routes_are_dropped_when_settings_are_cleaned() {
        let mut st = PlannerState::default();
        import(&mut st, &file("ulia.gpx", GPX)).unwrap();
        st.routes.push(SavedRoute { id: 9, points: vec![(95.0, 0.0, None), (1.0, 1.0, None)], ..Default::default() });
        st.routes.push(SavedRoute { id: 10, points: vec![(1.0, 1.0, None)], ..Default::default() });
        st.routes.push(SavedRoute { id: 11, points: vec![(f64::NAN, 1.0, None), (1.0, 1.0, None)], ..Default::default() });
        st.sane();
        assert_eq!(st.routes.len(), 1);
        assert_eq!(st.routes[0].name, "Ulia");
    }

    #[test]
    fn the_list_of_routes_has_a_limit() {
        let mut st = PlannerState::default();
        for i in 0..MAX_ROUTES as u64 + 5 {
            st.routes.push(SavedRoute { id: i + 1, points: vec![(1.0, 1.0, None), (1.1, 1.1, None)], ..Default::default() });
        }
        assert!(import(&mut st, &file("ulia.gpx", GPX)).is_err());
        st.sane();
        assert_eq!(st.routes.len(), MAX_ROUTES);
    }

    #[test]
    fn the_view_fits_the_whole_route_left_of_the_panel() {
        let mut st = PlannerState::default();
        let id = import(&mut st, &file("ulia.gpx", GPX)).unwrap();
        let free = Rect::from_min_size(pos2(0.0, 0.0), vec2(1400.0, 800.0));
        let panel = panel_rect(1390.0, free);
        let v = fit_view(&st, id, free, panel).unwrap();
        let cam = Cam::new(v, free);
        let r = &st.routes[0];
        for &(la, lo, _) in &r.points {
            let (x, y) = map_tiles::project(la, lo);
            let at = cam.screen(x, y);
            assert!(free.contains(at) && at.x < panel.left(), "{at:?} is off the free part of the map or under the list {panel:?}");
        }
        assert!(fit_view(&st, id + 1, free, panel).is_none());
    }

    const WAYPOINTS: &str = r#"<?xml version="1.0"?>
<gpx version="1.1" creator="Wikiloc" xmlns="http://www.topografix.com/GPX/1/1">
  <metadata><name>Con puntos</name></metadata>
  <wpt lat="43.3200" lon="-1.9500"><name>Salida</name></wpt>
  <wpt lat="43.3210" lon="-1.9510"></wpt>
  <wpt lat="43.3220" lon="-1.9500"><name>Cima</name></wpt>
  <trk><trkseg>
    <trkpt lat="43.3200" lon="-1.9500"><ele>10</ele></trkpt>
    <trkpt lat="43.3220" lon="-1.9500"><ele>40</ele></trkpt>
  </trkseg></trk>
</gpx>"#;

    #[test]
    fn waypoints_become_points_of_interest_once() {
        let mut st = PlannerState::default();
        let id = import(&mut st, &file("wp.gpx", WAYPOINTS)).unwrap();
        let k = waypoints_to_pois(&mut st, id).unwrap();
        assert_eq!(k, Kept { added: 3, existing: 0, full: 0 });
        let names: Vec<_> = st.saved.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["Salida", "Con puntos · 2", "Cima"], "an unnamed waypoint is numbered after the route");
        let p = &st.saved[2];
        assert!(!p.id.is_empty() && p.dirty && p.fresh, "it has an identity, goes to the phone and is ringed on the map");
        assert!(p.description.contains("Con puntos") && (p.lat, p.lon) == (43.322, -1.95));
        // again: nothing new
        assert_eq!(waypoints_to_pois(&mut st, id).unwrap(), Kept { added: 0, existing: 3, full: 0 });
        assert_eq!(st.saved.len(), 3);
        assert!(waypoints_to_pois(&mut st, id + 1).is_err());
    }

    #[test]
    fn a_point_of_interest_close_to_a_waypoint_is_that_waypoint() {
        let mut st = PlannerState::default();
        let id = import(&mut st, &file("wp.gpx", WAYPOINTS)).unwrap();
        // 10 m north of "Salida" (it keeps its own name); 100 m east of "Cima" (does not count)
        st.saved.push(SavedPlace { name: "Mi sitio".into(), lat: 43.3200 + 0.00009, lon: -1.95, ..Default::default() });
        st.saved.push(SavedPlace { name: "Lejos".into(), lat: 43.3220, lon: -1.95 + 0.0013, ..Default::default() });
        let k = waypoints_to_pois(&mut st, id).unwrap();
        assert_eq!(k, Kept { added: 2, existing: 1, full: 0 });
        assert_eq!(st.saved.iter().filter(|p| p.name == "Salida").count(), 0);
        assert_eq!(st.saved[0].name, "Mi sitio");
    }

    #[test]
    fn waypoints_stop_at_the_limit_of_points_of_interest() {
        let mut st = PlannerState::default();
        let id = import(&mut st, &file("wp.gpx", WAYPOINTS)).unwrap();
        for i in 0..super::super::MAX_SAVED - 1 {
            st.saved.push(SavedPlace { name: format!("p{i}"), lat: -30.0, lon: 100.0 + i as f64 * 0.01, ..Default::default() });
        }
        let k = waypoints_to_pois(&mut st, id).unwrap();
        assert_eq!(k, Kept { added: 1, existing: 0, full: 2 });
        assert_eq!(st.saved.len(), super::super::MAX_SAVED);
    }

    /// A straight 1 km track going east along a latitude of 43.3°, climbing 100 m; `lon` of a point `m` metres along.
    const LAT: f64 = 43.3;
    fn lon_at(m: f64) -> f64 {
        -1.95 + m / (111_320.0 * LAT.to_radians().cos())
    }
    fn straight() -> SavedRoute {
        let pts = (0..=10).map(|i| (LAT, round6(lon_at(f64::from(i) * 100.0)), Some(f64::from(i) as f32 * 10.0))).collect();
        let mut r = SavedRoute { id: 1, points: pts, profile: (0..=10).map(|i| i as f32 * 10.0).collect(), ..Default::default() };
        r.distance_m = 1000.0;
        r.tidy();
        r
    }
    fn poi(name: &str, lat: f64, lon: f64) -> SavedPlace {
        SavedPlace { name: name.to_string(), lat, lon, ..Default::default() }
    }

    #[test]
    fn the_profile_is_kept_with_the_route_and_old_routes_have_none() {
        let mut st = PlannerState::default();
        import(&mut st, &file("ulia.gpx", GPX)).unwrap();
        let r = &st.routes[0];
        assert_eq!(r.profile.len(), PROFILE_SAMPLES);
        assert_eq!((r.profile[0], r.profile[PROFILE_SAMPLES - 1]), (10.0, 25.0));
        let back: SavedRoute = serde_json::from_str(&serde_json::to_string(r).unwrap()).unwrap();
        assert_eq!(&back, r);
        let old: SavedRoute = serde_json::from_str(r#"{"id":3,"points":[[1.0,1.0,null],[1.1,1.1,null]]}"#).unwrap();
        assert!(old.profile.is_empty() && old.is_sane());
        // a file without heights has no profile; a damaged profile is dropped
        let flat = GPX.replace("<ele>10</ele>", "").replace("<ele>40</ele>", "").replace("<ele>25</ele>", "");
        import(&mut st, &file("flat.gpx", &flat)).unwrap();
        assert!(st.routes[1].profile.is_empty());
        let mut bad = st.routes[0].clone();
        bad.profile[3] = f32::NAN;
        bad.tidy();
        assert!(bad.profile.is_empty());
        bad.profile = vec![1.0; MAX_PROFILE + 50];
        bad.tidy();
        assert_eq!(bad.profile.len(), MAX_PROFILE);
    }

    #[test]
    fn a_spot_along_the_route_is_found_by_its_share_of_the_distance() {
        let r = straight();
        let (la, lo) = r.point_at(0.5).unwrap();
        assert!((la - LAT).abs() < 1e-6 && (lo - lon_at(500.0)).abs() < 1e-5, "{la} {lo}");
        assert_eq!(r.point_at(0.0).map(|p| p.1), Some(r.points[0].1));
        assert_eq!(r.point_at(7.0).map(|p| p.1), r.points.last().map(|p| p.1), "out of range stays on the line");
        assert!(SavedRoute::default().point_at(0.5).is_none());
    }

    #[test]
    fn points_of_interest_near_the_route_come_in_order_along_it() {
        let r = straight();
        let places = [
            poi("late", LAT + 0.001, lon_at(800.0)),      // 110 m off, 80 % along
            poi("early", LAT - 0.0005, lon_at(100.0)),    // 55 m off, 10 % along
            poi("far", LAT + 0.01, lon_at(500.0)),        // 1.1 km off
            poi("before the start", LAT, lon_at(-200.0)), // 200 m before the start
            poi("nowhere", f64::NAN, 0.0),
            poi("elsewhere", -33.0, 151.0),
        ];
        let near = r.near_places(&places, 300.0);
        assert_eq!(near.iter().map(|n| n.index).collect::<Vec<_>>(), vec![3, 1, 0], "{near:?}");
        let by = |i: usize| near.iter().find(|n| n.index == i).unwrap();
        assert!((by(1).off_m - 55.0).abs() < 3.0 && (by(1).frac - 0.1).abs() < 0.01, "{:?}", by(1));
        assert!((by(0).off_m - 111.0).abs() < 4.0 && (by(0).frac - 0.8).abs() < 0.01, "{:?}", by(0));
        assert_eq!((by(3).frac, by(3).off_m.round()), (0.0, 200.0), "the start is the nearest spot");
        assert_eq!(r.near_places(&places, 100.0).iter().map(|n| n.index).collect::<Vec<_>>(), vec![1]);
        assert_eq!(r.near_places(&places, 3000.0).len(), 4);
        for radius in [f32::NAN, f32::INFINITY, -5.0, 0.0] {
            let _ = r.near_places(&places, radius);
        }
        assert!(SavedRoute::default().near_places(&places, 300.0).is_empty());
    }

    #[test]
    fn the_profile_card_is_centred_at_the_bottom_and_only_with_a_profile() {
        let mut st = PlannerState::default();
        st.routes.push(straight());
        let mut rt = Runtime { route_sel: Some(1), ..Default::default() };
        let free = Rect::from_min_size(pos2(0.0, 0.0), vec2(1400.0, 800.0));
        let card = profile_rect(&st, &rt, free).unwrap();
        assert!(card.bottom() <= free.bottom() && card.left() >= free.left() && card.right() <= free.right(), "{card:?}");
        assert!((card.center().x - free.center().x).abs() < 0.5, "centred: {card:?}");
        assert!(profile_rect(&st, &rt, Rect::from_min_size(pos2(0.0, 0.0), vec2(200.0, 800.0))).is_none(), "too narrow");
        st.routes[0].profile.clear();
        assert!(profile_rect(&st, &rt, free).is_none());
        rt.route_sel = None;
        st.routes[0].profile = vec![1.0, 2.0];
        assert!(profile_rect(&st, &rt, free).is_none());
    }
}
