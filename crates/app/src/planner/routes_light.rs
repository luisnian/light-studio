//! The light along a route: pick a start time and a pace, and each point of the route gets the time you reach it and
//! the kind of light there (day, golden hour, blue hour, twilight, night). The profile card shows it as a strip under
//! the chart, the map colours the line by it, and hovering the profile points the Sun's direction out on the map.
//!
//! The sums are in [`light_studio_astro::light`]; the day and zone come from the planner (its date, its zone if set,
//! else the zone of the route's start).

use crate::gpx::distance_m;
use light_studio_astro::{Light, Observer, light_at, plan_day, walk_seconds};

use super::routes::SavedRoute;
use super::*;

/// Paces offered: label and the factor on Tobler's walking speed.
pub(super) const PACES: [(&str, f64); 3] = [("Relaxed", 0.75), ("Normal", 1.0), ("Brisk", 1.3)];
/// The pace chosen until another is (Normal).
pub(super) const DEFAULT_PACE: usize = 1;
/// The start time chosen until another is (minutes after midnight).
pub(super) const DEFAULT_START: u32 = 9 * 60;
/// The start moves by this many minutes at a time.
const STEP_MIN: u32 = 15;

pub(super) fn color(l: Light) -> Color32 {
    match l {
        Light::Day => Color32::from_rgb(236, 236, 242),
        Light::Golden => Color32::from_rgb(255, 196, 0),
        Light::Blue => Color32::from_rgb(64, 156, 255),
        Light::Twilight => Color32::from_rgb(170, 110, 235),
        Light::Night => Color32::from_rgb(98, 96, 205),
    }
}

/// The key a cached [`Track`] was worked out for: route, number of points, start minute, pace, day, zone.
pub(super) type Key = (u64, usize, u32, usize, i64, i32);

/// Where a walk of a route is at each of its kept points: how far along, how long after the start, and the light there.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Track {
    /// Unix time of the start (seconds, UTC).
    pub start: f64,
    pub date: Date,
    /// The zone's offset (minutes east of UTC).
    pub offset: i32,
    frac: Vec<f32>,
    secs: Vec<f64>,
    light: Vec<Light>,
}

impl Track {
    /// The walk of `route` starting `minute` minutes after midnight (local) on `date`, at `pace` times Tobler's speed.
    /// None for a route with no length.
    pub(super) fn new(route: &SavedRoute, date: Date, offset: i32, minute: u32, pace: f64) -> Option<Track> {
        let mut cum = Vec::with_capacity(route.points.len());
        let mut legs = Vec::with_capacity(route.points.len());
        let (mut total, mut prev, mut height): (f64, Option<(f64, f64)>, Option<f32>) = (0.0, None, None);
        for &(la, lo, h) in &route.points {
            let rise = match (height, h) {
                (Some(a), Some(b)) => f64::from(b - a),
                _ => 0.0,
            };
            if let Some(q) = prev {
                let run = distance_m(q, (la, lo));
                total += run;
                legs.push((run, rise));
            }
            cum.push(total);
            prev = Some((la, lo));
            height = h.or(height);
        }
        if total <= 0.0 || !total.is_finite() || cum.len() < 2 {
            return None;
        }
        let secs = walk_seconds(&legs, pace);
        let start = date.unix_midnight_utc() - f64::from(offset) * 60.0 + f64::from(minute.min(1439)) * 60.0;
        let light = route.points.iter().zip(&secs).map(|(&(la, lo, _), s)| light_at(start + s, la, lo)).collect();
        Some(Track { start, date, offset, frac: cum.iter().map(|d| (d / total) as f32).collect(), secs, light })
    }

    /// How long the walk takes (s).
    pub(super) fn duration(&self) -> f64 {
        self.secs.last().copied().unwrap_or(0.0)
    }

    /// Seconds after the start at `frac` (0 to 1) of the way along.
    pub(super) fn secs_at(&self, frac: f32) -> f64 {
        let f = frac.clamp(0.0, 1.0);
        let i = self.frac.partition_point(|&x| x < f);
        let (Some(&f1), Some(&s1)) = (self.frac.get(i), self.secs.get(i)) else { return self.duration() };
        let Some((&f0, &s0)) = i.checked_sub(1).and_then(|k| Some((self.frac.get(k)?, self.secs.get(k)?))) else { return s1 };
        if f1 > f0 { s0 + (s1 - s0) * f64::from((f - f0) / (f1 - f0)) } else { s1 }
    }

    /// The kind of light at `frac` of the way along.
    pub(super) fn light_at(&self, frac: f32) -> Light {
        let i = self.frac.partition_point(|&x| x <= frac.clamp(0.0, 1.0)).saturating_sub(1);
        self.light.get(i).copied().unwrap_or(Light::Day)
    }

    /// The light of the line from kept point `i` to the next.
    pub(super) fn light_of(&self, i: usize) -> Light {
        self.light.get(i).copied().unwrap_or(Light::Day)
    }

    /// Stretches of the route in one kind of light: from, to (0 to 1) and which.
    pub(super) fn runs(&self) -> Vec<(f32, f32, Light)> {
        let mut out: Vec<(f32, f32, Light)> = Vec::new();
        for (i, pair) in self.frac.windows(2).enumerate() {
            let (Some(&a), Some(&b)) = (pair.first(), pair.get(1)) else { continue };
            let l = self.light_of(i);
            match out.last_mut() {
                Some(last) if last.2 == l => last.1 = b,
                _ => out.push((a, b, l)),
            }
        }
        out
    }

    /// The local time `secs` after the start, `HH:MM` (`+1` etc. when it is a later day).
    pub(super) fn clock(&self, secs: f64) -> String {
        let local = (self.start - self.date.unix_midnight_utc()) / 60.0 + f64::from(self.offset) + secs / 60.0;
        let minutes = local.round() as i64;
        let (days, m) = (minutes.div_euclid(1440), minutes.rem_euclid(1440));
        let mut s = format!("{:02}:{:02}", m / 60, m % 60);
        if days != 0 {
            s.push_str(&format!("{days:+}"));
        }
        s
    }
}

/// The day and zone a walk of `route` is looked at: the planner's date (else today) and zone (else the start's).
pub(super) fn day_of(st: &PlannerState, route: &SavedRoute, now: f64) -> (Date, i32) {
    let (la, lo) = route.points.first().map_or((st.lat, st.lon), |p| (p.0, p.1));
    let guess = st.utc_offset.unwrap_or_else(|| zone::offset_at(la, lo, now).0);
    let date = Date::parse(&st.date).unwrap_or_else(|| Date::from_unix(now, guess));
    let offset = st.utc_offset.unwrap_or_else(|| zone::offset_at(la, lo, noon_of(date, lo)).0);
    (date, offset)
}

/// The walk the card and the map show: cached while nothing it depends on changes.
pub(super) fn track(st: &PlannerState, rt: &mut Runtime, route: &SavedRoute) -> Option<Arc<Track>> {
    if route.points.len() < 2 {
        return None;
    }
    let (date, offset) = day_of(st, route, now_unix());
    let minute = rt.route_start.unwrap_or(DEFAULT_START).min(1439);
    let pace = rt.route_pace.unwrap_or(DEFAULT_PACE).min(PACES.len() - 1);
    let key = (route.id, route.points.len(), minute, pace, date.days(), offset);
    if let Some((k, t)) = &rt.route_track
        && *k == key
    {
        return Some(t.clone());
    }
    let t = Arc::new(Track::new(route, date, offset, minute, PACES.get(pace).map_or(1.0, |p| p.1))?);
    rt.route_track = Some((key, t.clone()));
    Some(t)
}

/// Sunrise and sunset (unix seconds) on the day of `track`, at the middle of the route.
fn sun_times(route: &SavedRoute, date: Date, offset: i32) -> (Option<f64>, Option<f64>) {
    let [min_la, min_lo, max_la, max_lo] = route.bounds;
    let obs = Observer::new((min_la + max_la) / 2.0, (min_lo + max_lo) / 2.0);
    let day = plan_day(obs, date.unix_midnight_utc() - f64::from(offset) * 60.0);
    (day.sun.rise, day.sun.set)
}

/// The line's points split into stretches of one kind of light (each stretch repeats the point it starts at, so the
/// pieces join up).
pub(super) fn coloured(points: &[Pos2], light_of: impl Fn(usize) -> Light) -> Vec<(Light, Vec<Pos2>)> {
    let mut out: Vec<(Light, Vec<Pos2>)> = Vec::new();
    for (i, pair) in points.windows(2).enumerate() {
        let (Some(&a), Some(&b)) = (pair.first(), pair.get(1)) else { continue };
        let l = light_of(i);
        match out.last_mut() {
            Some((last, pts)) if *last == l => pts.push(b),
            _ => out.push((l, vec![a, b])),
        }
    }
    out
}

/// A row of controls laid out from the left; what doesn't fit before `right` is left out.
struct Row {
    x: f32,
    y: f32,
    right: f32,
}

impl Row {
    fn take(&mut self, w: f32, gap: f32) -> Option<Rect> {
        let r = Rect::from_min_size(pos2(self.x, self.y), vec2(w, 22.0));
        self.x += w + gap;
        (r.right() <= self.right).then_some(r)
    }
}

fn width(painter: &egui::Painter, t: &Tokens, text: &str) -> f32 {
    painter.layout_no_wrap(text.to_string(), t.font(11.0), t.text).size().x
}

/// A small pill button; `on` fills it.
fn pill(ui: &mut Ui, painter: &egui::Painter, t: &Tokens, rect: Rect, id: &str, label: &str, on: bool) -> bool {
    let resp = ui.interact(rect, egui::Id::new(("planner-route-light", id)), Sense::click());
    if on {
        painter.rect_filled(rect, 11.0, t.tool_active);
    } else if resp.hovered() {
        painter.rect_filled(rect, 11.0, t.hover.gamma_multiply(0.5));
    }
    painter.text(rect.center(), Align2::CENTER_CENTER, label, t.font(11.0), if on { t.text } else { t.text_dim });
    resp.clicked()
}

fn dim_text(painter: &egui::Painter, t: &Tokens, rect: Rect, text: &str) {
    painter.text(pos2(rect.left(), rect.center().y), Align2::LEFT_CENTER, text, t.font(11.0), t.text_dim);
}

impl Page<'_> {
    /// The second row of the profile card: start time, day, pace, arrival, sunrise/sunset shortcuts, and the switch that
    /// turns the light off. `date_step` collects a change of day (the planner's date is changed by the caller).
    pub(super) fn light_controls(
        &self,
        ui: &mut Ui,
        painter: &egui::Painter,
        r: Rect,
        route: &SavedRoute,
        rt: &mut Runtime,
        track: Option<&Track>,
        date_step: &mut i64,
    ) {
        let t = self.t;
        let mut row = Row { x: r.left() + 14.0, y: r.top() + 33.0, right: r.right() - 76.0 };
        let tr = |s: &str| crate::i18n::tr(s).to_string();
        let off = rt.route_light_off;
        if let (Some(trk), false) = (track, off) {
            let minute = rt.route_start.unwrap_or(DEFAULT_START).min(1439);
            let label = tr("Start");
            if let Some(b) = row.take(width(painter, &t, &label), 6.0) {
                dim_text(painter, &t, b, &label);
            }
            if let Some(b) = row.take(22.0, 0.0)
                && pill(ui, painter, &t, b, "startDown", "−", false)
            {
                rt.route_start = Some(minute.saturating_sub(STEP_MIN));
            }
            if let Some(b) = row.take(44.0, 0.0) {
                painter.text(b.center(), Align2::CENTER_CENTER, format!("{:02}:{:02}", minute / 60, minute % 60), t.semibold(12.0), t.text);
            }
            if let Some(b) = row.take(22.0, 14.0)
                && pill(ui, painter, &t, b, "startUp", "+", false)
            {
                rt.route_start = Some((minute + STEP_MIN).min(1439));
            }
            if let Some(b) = row.take(22.0, 0.0)
                && pill(ui, painter, &t, b, "dayDown", "‹", false)
            {
                *date_step = -1;
            }
            if let Some(b) = row.take(78.0, 0.0) {
                painter.text(b.center(), Align2::CENTER_CENTER, trk.date.iso(), t.font(11.5), t.text);
            }
            if let Some(b) = row.take(22.0, 14.0)
                && pill(ui, painter, &t, b, "dayUp", "›", false)
            {
                *date_step = 1;
            }
            let pace = rt.route_pace.unwrap_or(DEFAULT_PACE).min(PACES.len() - 1);
            for (i, (name, _)) in PACES.iter().enumerate() {
                let label = tr(name);
                if let Some(b) = row.take(width(painter, &t, &label) + 14.0, 3.0)
                    && pill(ui, painter, &t, b, &format!("pace{i}"), &label, pace == i)
                {
                    rt.route_pace = Some(i);
                }
            }
            row.x += 11.0;
            let arrive = format!("{} {}", tr("Arrival"), trk.clock(trk.duration()));
            if let Some(b) = row.take(width(painter, &t, &arrive), 14.0) {
                dim_text(painter, &t, b, &arrive);
            }
            let (rise, set) = sun_times(route, trk.date, trk.offset);
            let midnight = trk.date.unix_midnight_utc() - f64::from(trk.offset) * 60.0;
            let minute_of = |unix: f64| (((unix - midnight) / 60.0).round().max(0.0) as u32).min(1439);
            let (up, down) = (tr("Start at sunrise"), tr("Arrive at sunset"));
            if let Some(b) = row.take(width(painter, &t, &up) + 14.0, 3.0)
                && pill(ui, painter, &t, b, "sunrise", &up, false)
                && let Some(rise) = rise
            {
                rt.route_start = Some(minute_of(rise));
            }
            if let Some(b) = row.take(width(painter, &t, &down) + 14.0, 3.0)
                && pill(ui, painter, &t, b, "sunset", &down, false)
                && let Some(set) = set
            {
                rt.route_start = Some(minute_of(set - trk.duration()));
            }
        }
        // the switch, at the right end
        let label = tr("Light");
        let w = width(painter, &t, &label) + 14.0;
        let b = Rect::from_min_size(pos2(r.right() - 12.0 - w, r.top() + 33.0), vec2(w, 22.0));
        if pill(ui, painter, &t, b, "toggle", &label, !off) {
            rt.route_light_off = !off;
        }
    }

    /// The strip under the chart: one block per stretch of the same light, named and timed when there is room.
    pub(super) fn light_strip(&self, ui: &mut Ui, painter: &egui::Painter, chart_x: (f32, f32), y: f32, track: &Track) {
        let t = self.t;
        let px = |f: f32| chart_x.0 + (chart_x.1 - chart_x.0) * f.clamp(0.0, 1.0);
        let span = track.duration();
        for (i, (a, b, l)) in track.runs().into_iter().enumerate() {
            let rect = Rect::from_min_max(pos2(px(a), y), pos2(px(b).max(px(a) + 1.0), y + 14.0));
            painter.rect_filled(rect, 3.0, color(l).gamma_multiply(0.85));
            let from = track.clock(track.secs_at(a));
            let to = track.clock(track.secs_at(b));
            let name = crate::i18n::tr(l.label()).to_string();
            let g = painter.layout_no_wrap(format!("{name} {from}"), t.font(10.5), Color32::from_black_alpha(220));
            if g.size().x + 8.0 <= rect.width() {
                painter.galley(pos2(rect.left() + 4.0, rect.center().y - g.size().y / 2.0), g, Color32::BLACK);
            }
            let resp = ui.interact(rect, egui::Id::new(("planner-route-light-run", i)), Sense::hover());
            if span > 0.0 {
                resp.on_hover_text(format!("{name} · {from}–{to}"));
            }
        }
    }

    /// The Sun's direction on the map from the spot the pointer is over on the profile.
    pub(super) fn sun_ray(&self, painter: &egui::Painter, at: Pos2, az: f64, alt: f64) {
        let dir = vec2(az.to_radians().sin() as f32, -(az.to_radians().cos() as f32));
        let k = if alt > 0.0 {
            1.0
        } else if alt > -6.0 {
            0.6
        } else {
            0.3
        };
        let c = color(Light::Golden).gamma_multiply(k);
        let end = at + dir * 52.0;
        painter.line_segment([at + dir * 10.0, end], Stroke::new(2.5, c));
        painter.circle_filled(end, 7.0, c);
        painter.circle_stroke(end, 7.0, Stroke::new(1.5, Color32::from_black_alpha(160)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LAT: f64 = 43.3;

    fn lon_at(m: f64) -> f64 {
        -1.95 + m / (111_320.0 * LAT.to_radians().cos())
    }

    /// A straight track going east, a point every 100 m.
    fn straight(metres: u32, climb: f32) -> SavedRoute {
        let n = metres / 100;
        let points = (0..=n).map(|i| (LAT, lon_at(f64::from(i) * 100.0), Some(climb * i as f32 / n.max(1) as f32))).collect();
        let profile = (0..60).map(|i| climb * i as f32 / 59.0).collect();
        let mut r = SavedRoute { id: 1, name: "Recta".into(), points, profile, ..Default::default() };
        r.distance_m = f64::from(metres);
        r.tidy();
        r
    }

    fn midsummer() -> Date {
        Date::new(2026, 6, 21)
    }

    #[test]
    fn a_walk_from_day_into_golden_hour_has_two_stretches() {
        // 4.5 km on the flat at 21:00 local (UTC+2): about 54 minutes, the Sun goes from 10° to just above the horizon
        let r = straight(4500, 0.0);
        let t = Track::new(&r, midsummer(), 120, 21 * 60, 1.0).unwrap();
        let mins = t.duration() / 60.0;
        assert!((mins - 53.6).abs() < 1.0, "{mins}");
        assert_eq!(t.clock(0.0), "21:00");
        let arrival = t.clock(t.duration());
        assert!(("21:50"..="21:58").contains(&arrival.as_str()), "{arrival}");
        let runs = t.runs();
        assert_eq!(runs.len(), 2, "{runs:?}");
        assert_eq!((runs[0].2, runs[1].2), (Light::Day, Light::Golden));
        assert_eq!((runs[0].0, runs[1].1), (0.0, 1.0));
        assert_eq!(runs[0].1, runs[1].0, "the stretches join up");
        assert_eq!((t.light_at(0.0), t.light_at(1.0), t.light_at(0.1), t.light_at(0.9)), (Light::Day, Light::Golden, Light::Day, Light::Golden));
        // a slower pace takes longer, and the same walk on a climb takes longer than on the flat
        let slow = Track::new(&r, midsummer(), 120, 21 * 60, 0.75).unwrap();
        assert!((slow.duration() / t.duration() - 1.0 / 0.75).abs() < 1e-9);
        let hill = Track::new(&straight(4500, 400.0), midsummer(), 120, 21 * 60, 1.0).unwrap();
        assert!(hill.duration() > t.duration() * 1.3, "{} {}", hill.duration(), t.duration());
    }

    #[test]
    fn times_are_interpolated_and_late_ones_name_the_next_day() {
        let t = Track::new(&straight(4500, 0.0), midsummer(), 120, 23 * 60 + 50, 1.0).unwrap();
        assert_eq!(t.clock(0.0), "23:50");
        assert_eq!(t.clock(t.duration()), "00:44+1");
        assert_eq!(t.secs_at(0.0), 0.0);
        assert!((t.secs_at(0.5) - t.duration() / 2.0).abs() < 1.0, "even on the flat");
        assert_eq!((t.secs_at(-3.0), t.secs_at(9.0)), (0.0, t.duration()));
        assert!(t.secs_at(0.3) < t.secs_at(0.4));
        // a different zone moves the clock, not the walk
        let utc = Track::new(&straight(4500, 0.0), midsummer(), 0, 21 * 60, 1.0).unwrap();
        assert_eq!(utc.clock(0.0), "21:00");
        assert!(utc.start > Track::new(&straight(4500, 0.0), midsummer(), 120, 21 * 60, 1.0).unwrap().start);
    }

    #[test]
    fn a_route_without_length_has_no_walk() {
        assert!(Track::new(&SavedRoute::default(), midsummer(), 0, 0, 1.0).is_none());
        let still = SavedRoute { points: vec![(LAT, -1.9, None), (LAT, -1.9, None)], ..Default::default() };
        assert!(Track::new(&still, midsummer(), 0, 0, 1.0).is_none());
    }

    #[test]
    fn the_line_is_split_where_the_light_changes() {
        let pts = [pos2(0.0, 0.0), pos2(1.0, 0.0), pos2(2.0, 0.0), pos2(3.0, 0.0)];
        let lights = [Light::Day, Light::Day, Light::Golden];
        let parts = coloured(&pts, |i| lights[i]);
        assert_eq!(parts.len(), 2);
        assert_eq!((parts[0].0, parts[0].1.len()), (Light::Day, 3));
        assert_eq!((parts[1].0, parts[1].1.clone()), (Light::Golden, vec![pts[2], pts[3]]));
        assert!(coloured(&pts[..1], |_| Light::Day).is_empty());
    }
}
