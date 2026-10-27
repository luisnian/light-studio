//! The map's sky layers, drawn the way the planning apps of the trade draw them: rise and set as thick
//! coloured bands, the position now as a thin line, the Milky Way as a dotted arc with elevation
//! circles, the Sun and Moon as dotted trajectories with hour marks, the light's direction as arrows,
//! meteor showers' radiants, and the whole map tinted by the kind of light (day, golden, blue,
//! twilights, night).
//!
//! The colours are chosen to read on dark and light maps alike.

use crate::services::map_tiles;
use light_studio_astro::sky::milky_way_plane;
use light_studio_astro::sun::{sun_coords, to_horizontal};
use light_studio_astro::time::jd_from_unix;

use super::*;
use crate::map::Cam;

/// Sunrise, sunset, the Sun now, moonrise, moonset and the Moon now.
pub(super) const SUNRISE: Color32 = Color32::from_rgb(250, 219, 60);
pub(super) const SUNSET: Color32 = Color32::from_rgb(252, 80, 17);
pub(super) const SUN_NOW: Color32 = Color32::from_rgb(235, 170, 50);
pub(super) const MOONRISE: Color32 = Color32::from_rgb(66, 197, 228);
pub(super) const MOONSET: Color32 = Color32::from_rgb(61, 90, 193);
pub(super) const MOON_NOW: Color32 = Color32::from_rgb(58, 140, 214);
pub(super) const METEOR: Color32 = Color32::from_rgb(255, 120, 140);

/// The colours of the kinds of light (day, golden hour, blue hour, nautical, astronomical, night).
pub(super) const DAY_LIGHT: Color32 = Color32::from_rgb(66, 197, 228);
pub(super) const GOLDEN_LIGHT: Color32 = Color32::from_rgb(252, 80, 17);
pub(super) const BLUE_LIGHT: Color32 = Color32::from_rgb(30, 136, 255);
pub(super) const NAUTICAL_LIGHT: Color32 = Color32::from_rgb(30, 71, 216);
pub(super) const ASTRO_LIGHT: Color32 = Color32::from_rgb(0, 31, 163);
pub(super) const NIGHT_LIGHT: Color32 = Color32::from_rgb(10, 10, 14);

/// How a body's lines are drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LineStyle {
    /// Rise and set bands, the position now.
    #[default]
    Standard,
    /// Standard, and the path across the sky as a dotted arc with hour marks.
    Trajectory,
    /// Arrows along the way the light travels, by the hour.
    Light,
}

impl LineStyle {
    pub(super) fn next(self) -> LineStyle {
        match self {
            LineStyle::Standard => LineStyle::Trajectory,
            LineStyle::Trajectory => LineStyle::Light,
            LineStyle::Light => LineStyle::Standard,
        }
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            LineStyle::Standard => "Standard",
            LineStyle::Trajectory => "Trajectory",
            LineStyle::Light => "Light direction",
        }
    }
}

/// The kind of light under a Sun at `alt` degrees.
pub(super) fn light_kind(alt: f64) -> Color32 {
    if alt > GOLDEN_TOP {
        DAY_LIGHT
    } else if alt > GOLDEN_BOTTOM {
        GOLDEN_LIGHT
    } else if alt > -6.0 {
        BLUE_LIGHT
    } else if alt > -12.0 {
        NAUTICAL_LIGHT
    } else if alt > -18.0 {
        ASTRO_LIGHT
    } else {
        NIGHT_LIGHT
    }
}

/// A point `az` degrees clockwise from north at the radius that stands for altitude `alt` (the horizon at
/// `reach`, the zenith at the centre).
pub(super) fn polar(origin: Pos2, az: f64, alt: f64, reach: f32) -> Pos2 {
    let a = az.to_radians();
    let r = reach * (1.0 - (alt.clamp(0.0, 90.0) / 90.0) as f32);
    origin + vec2(a.sin() as f32, -a.cos() as f32) * r
}

fn dir(az: f64) -> egui::Vec2 {
    let a = az.to_radians();
    vec2(a.sin() as f32, -a.cos() as f32)
}

/// A thick translucent band from `origin` along an azimuth.
pub(super) fn band(p: &egui::Painter, origin: Pos2, az: f64, length: f32, color: Color32, width: f32) {
    let end = origin + dir(az) * length;
    p.line_segment([origin, end], Stroke::new(width + 2.0, Color32::from_black_alpha(60)));
    p.line_segment([origin, end], Stroke::new(width, color.gamma_multiply(0.88)));
    p.circle_filled(end, width * 0.5 + 1.0, color);
}

/// A dashed line.
pub(super) fn dashed(p: &egui::Painter, a: Pos2, b: Pos2, stroke: Stroke) {
    p.extend(Shape::dashed_line(&[a, b], stroke, 9.0, 6.0));
}

/// A wedge from `origin` as wide as the body's disc (`half` degrees each side), `length` long.
pub(super) fn wedge(p: &egui::Painter, origin: Pos2, az: f64, half: f64, length: f32, color: Color32) {
    let (a, b) = (origin + dir(az - half) * length, origin + dir(az + half) * length);
    p.add(Shape::convex_polygon(vec![origin, a, b], color.gamma_multiply(0.5), Stroke::new(1.0, color)));
}

/// A line along a great circle from a place, as screen points (the world repeats east-west, so each point sits
/// in the copy nearest the place).
pub(super) fn geodesic(cam: &Cam, place_x: f64, lat: f64, lon: f64, az: f64, km: f64) -> Vec<Pos2> {
    let n = (km / 60.0).ceil().clamp(2.0, 120.0) as usize;
    (0..=n)
        .map(|i| {
            let d = km * 1000.0 * i as f64 / n as f64;
            let (la, lo) = light_studio_astro::align::destination(lat, lon, az, d);
            let (x, y) = map_tiles::project(la, lo);
            let mut dx = x - place_x;
            dx -= dx.round();
            cam.screen(place_x + dx, y)
        })
        .collect()
}

impl Page<'_> {
    /// Tints the map by the kind of light the Sun gives at each place, in bands (so zoomed out it shows where the
    /// day, the twilights and the night are round the world).
    pub(super) fn twilight_tint(&self, painter: &egui::Painter, cam: Cam, outer: Rect, at: f64) {
        let jd = jd_from_unix(at);
        let s = sun_coords(jd);
        let cell = 18.0_f32;
        let cols = (outer.width() / cell).ceil().max(1.0) as usize;
        let rows = (outer.height() / cell).ceil().max(1.0) as usize;
        let colour_at = |x: f32, y: f32| -> Option<Color32> {
            let (wx, wy) = cam.world(pos2(x, y));
            let (lat, lon) = map_tiles::unproject(wx.rem_euclid(1.0), wy.clamp(0.0, 1.0));
            let alt = to_horizontal(s.ra, s.dec, jd, Observer::new(lat, lon)).alt;
            let base = light_kind(alt);
            let alpha = if base == NIGHT_LIGHT { 120 } else { 78 };
            Some(Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), alpha))
        };
        let grid: Vec<Option<Color32>> = (0..rows * cols)
            .map(|k| colour_at(outer.left() + (k % cols) as f32 * cell + cell / 2.0, outer.top() + (k / cols) as f32 * cell + cell / 2.0))
            .collect();
        let at = |i: isize, j: isize| -> Option<Color32> {
            if i < 0 || j < 0 || i as usize >= cols || j as usize >= rows {
                None
            } else {
                grid.get(j as usize * cols + i as usize).copied().flatten()
            }
        };
        let mut mesh = egui::epaint::Mesh::default();
        // a cell whose neighbour is another kind of light holds the boundary: it is redrawn in finer pieces, so the
        // line between two kinds is sharp rather than stepped by the cell
        let sub = 6usize;
        for j in 0..rows {
            let y = outer.top() + j as f32 * cell;
            let mut run: Option<(Color32, f32)> = None;
            let flush = |run: &mut Option<(Color32, f32)>, x: f32, mesh: &mut egui::epaint::Mesh| {
                if let Some((c, x0)) = run.take() {
                    mesh.add_colored_rect(Rect::from_min_max(pos2(x0, y), pos2(x.min(outer.right()), (y + cell).min(outer.bottom()))), c);
                }
            };
            for i in 0..=cols {
                let x = outer.left() + i as f32 * cell;
                let (ii, jj) = (i as isize, j as isize);
                let here = if i < cols { at(ii, jj) } else { None };
                let edge = i < cols
                    && [(-1, 0), (1, 0), (0, -1), (0, 1)].iter().any(|&(dx, dy)| {
                        let (ni, nj) = (ii + dx, jj + dy);
                        ni >= 0 && nj >= 0 && (ni as usize) < cols && (nj as usize) < rows && at(ni, nj) != here
                    });
                if edge {
                    flush(&mut run, x, &mut mesh);
                    let f = cell / sub as f32;
                    for v in 0..sub {
                        let mut srun: Option<(Color32, f32)> = None;
                        let sy = y + v as f32 * f;
                        for u in 0..=sub {
                            let sx = x + u as f32 * f;
                            let c = (u < sub).then(|| colour_at(sx + f / 2.0, sy + f / 2.0)).flatten();
                            match (srun, c) {
                                (Some((rc, x0)), Some(n)) if rc == n => srun = Some((rc, x0)),
                                (Some((rc, x0)), n) => {
                                    mesh.add_colored_rect(
                                        Rect::from_min_max(pos2(x0, sy), pos2(sx.min(outer.right()), (sy + f).min(outer.bottom()))),
                                        rc,
                                    );
                                    srun = n.map(|n| (n, sx));
                                }
                                (None, Some(n)) => srun = Some((n, sx)),
                                (None, None) => {}
                            }
                        }
                    }
                    continue;
                }
                match (run, here) {
                    (Some((c, x0)), Some(n)) if c == n => run = Some((c, x0)),
                    (Some(_), n) => {
                        flush(&mut run, x, &mut mesh);
                        run = n.map(|n| (n, x));
                    }
                    (None, Some(n)) => run = Some((n, x)),
                    (None, None) => {}
                }
            }
        }
        painter.add(Shape::mesh(mesh));
    }

    /// Circles for 0°, 10° … 80° of altitude round the place (the horizon outermost).
    pub(super) fn elevation_circles(&self, painter: &egui::Painter, origin: Pos2, reach: f32) {
        let t = self.t;
        for k in 0..9 {
            let alt = f64::from(k) * 10.0;
            let r = reach * (1.0 - (alt / 90.0) as f32);
            let strong = k == 0;
            painter.circle_stroke(origin, r, Stroke::new(if strong { 1.6 } else { 0.8 }, Color32::from_white_alpha(if strong { 120 } else { 55 })));
            if k % 3 == 0 && k > 0 {
                let g = painter.layout_no_wrap(format!("{alt:.0}°"), t.font(9.5), Color32::from_white_alpha(190));
                painter.galley(pos2(origin.x + 3.0, origin.y - r - g.size().y), g, Color32::WHITE);
            }
        }
    }

    /// The Milky Way: the plane of the galaxy as a dotted arc over the sky as it stands at the time shown, the dots
    /// biggest at the core and shrinking away from it, the core
    /// as a big dot with a thick line to it, thin lines where the arc meets the horizon, and bands where the core
    /// comes into view and where it leaves it (light grey, dark grey).
    pub(super) fn milky_way_arc(&self, painter: &egui::Painter, origin: Pos2, reach: f32, at: f64) {
        let plane = milky_way_plane(at, self.obs, 3.0);
        let white = Color32::from_white_alpha(235);
        for p in &plane {
            if p.at.alt >= 0.0 {
                // big at the galactic centre (longitude 0), smaller the farther along the plane
                let away = (p.l + 180.0).rem_euclid(360.0) - 180.0;
                let k = (1.0 - (away.abs() / 180.0) as f32).clamp(0.0, 1.0);
                let pt = polar(origin, p.at.az, p.at.alt, reach);
                painter.circle_filled(pt, 1.2 + 6.3 * k * k, white);
            }
        }
        // where the arc crosses the horizon: thin lines to the edge
        for i in 0..plane.len() {
            let (Some(a), Some(b)) = (plane.get(i), plane.get((i + 1) % plane.len())) else { continue };
            if (a.at.alt >= 0.0) != (b.at.alt >= 0.0) {
                let k = a.at.alt / (a.at.alt - b.at.alt);
                let mut daz = b.at.az - a.at.az;
                daz -= 360.0 * (daz / 360.0).round();
                let az = (a.at.az + daz * k).rem_euclid(360.0);
                painter.line_segment([origin, origin + dir(az) * reach], Stroke::new(1.2, Color32::from_white_alpha(200)));
            }
        }
        // bands where the core starts and stops being seen in a dark sky
        if let Some(span) = self.c.night_core.first() {
            for (time, shade) in [(span.start, Color32::from_gray(205)), (span.end, Color32::from_gray(70))] {
                let h = light_studio_astro::core_horizontal(time, self.obs);
                wedge(painter, origin, h.az, 2.2, reach, shade);
            }
        }
        let core = self.skies.core;
        if core.alt >= 0.0 {
            let tip = polar(origin, core.az, core.alt, reach);
            painter.line_segment([origin, tip], Stroke::new(4.5, Color32::from_black_alpha(90)));
            painter.line_segment([origin, tip], Stroke::new(3.0, Color32::WHITE));
            painter.circle_filled(tip, 7.0, Color32::WHITE);
            painter.circle_stroke(tip, 7.0, Stroke::new(1.5, Color32::from_black_alpha(160)));
        } else {
            let tip = origin + dir(core.az) * reach;
            dashed(painter, origin, tip, Stroke::new(2.0, Color32::from_white_alpha(150)));
        }
    }

    /// A body's path across the sky as a dotted arc with a mark every hour (the day's samples are ten minutes apart).
    pub(super) fn trajectory(&self, painter: &egui::Painter, origin: Pos2, reach: f32, path: &[Horizontal], color: Color32) {
        let t = self.t;
        let per_hour = 6;
        let points: Vec<(usize, Pos2)> =
            path.iter().enumerate().filter(|(_, h)| h.alt >= 0.0).map(|(i, h)| (i, polar(origin, h.az, h.alt, reach))).collect();
        let mut prev: Option<(usize, Pos2)> = None;
        for &(i, pt) in &points {
            if let Some((pi, pp)) = prev
                && i == pi + 1
            {
                painter.add(Shape::dotted_line(&[pp, pt], color, 7.0, 1.6));
            }
            prev = Some((i, pt));
        }
        for &(i, pt) in &points {
            if i % per_hour == 0 {
                painter.circle_filled(pt, 3.6, color);
                painter.circle_stroke(pt, 3.6, Stroke::new(1.0, Color32::from_black_alpha(150)));
                if (i / per_hour) % 2 == 0 {
                    let g = painter.layout_no_wrap(format!("{}", (i / per_hour) % 24), t.semibold(9.5), Color32::WHITE);
                    let pos = pt + vec2(5.0, -g.size().y - 1.0);
                    painter.rect_filled(Rect::from_min_size(pos - vec2(2.0, 0.0), g.size() + vec2(4.0, 0.0)), 3.0, Color32::from_black_alpha(130));
                    painter.galley(pos, g, Color32::WHITE);
                }
            }
        }
    }

    /// Arrows along the way a body's light travels, one for each hour it is up, with the hour.
    pub(super) fn light_arrows(&self, painter: &egui::Painter, origin: Pos2, reach: f32, path: &[Horizontal], color: Color32) {
        let t = self.t;
        for (i, h) in path.iter().enumerate().filter(|(i, _)| i % 6 == 0) {
            if h.alt < 0.0 {
                continue;
            }
            let from = origin + dir(h.az) * (reach * 0.95);
            let to = origin + dir(h.az) * (reach * 0.55);
            painter.line_segment([from, to], Stroke::new(3.0, Color32::from_black_alpha(80)));
            painter.line_segment([from, to], Stroke::new(1.8, color));
            let d = dir(h.az + 180.0);
            let side = vec2(-d.y, d.x);
            let base = to - d * 9.0;
            painter.add(Shape::convex_polygon(vec![to + d * 2.0, base + side * 5.0, base - side * 5.0], color, Stroke::NONE));
            let g = painter.layout_no_wrap(format!("{}", (i / 6) % 24), t.semibold(9.5), Color32::WHITE);
            painter.rect_filled(Rect::from_center_size(from + dir(h.az) * 9.0, g.size() + vec2(5.0, 1.0)), 3.0, Color32::from_black_alpha(130));
            painter.galley(from + dir(h.az) * 9.0 - g.size() / 2.0, g, Color32::WHITE);
        }
    }

    /// A meteor shower's radiant: its path through the night as a dotted arc with hour marks, the radiant now as a
    /// ring, and a line to it.
    /// The shadow of an eclipse over the map: the band of totality (or the annular ring) with the line down its
    /// middle, the limits of the partial eclipse either side, and a label at the greatest eclipse and every ten minutes.
    pub(super) fn eclipse_band(&self, painter: &egui::Painter, cam: Cam, path: &light_studio_astro::eclipse::EclipsePath) {
        let t = self.t;
        let px_per_km = |lat: f64| cam.s / (40_075.0 * lat.to_radians().cos().max(0.05));
        let at = |lat: f64, lon: f64| {
            let (x, y) = map_tiles::project(lat, lon);
            cam.screen(x, y)
        };
        let pts: Vec<(Pos2, &light_studio_astro::eclipse::PathPoint)> = path.points.iter().map(|p| (at(p.lat, p.lon), p)).collect();
        let side = |a: Pos2, b: Pos2| -> egui::Vec2 {
            let d = b - a;
            let n = d.length();
            if n < 0.001 { egui::Vec2::ZERO } else { vec2(-d.y, d.x) / n }
        };
        // a point jumping across the map's seam (the world repeats) must not be joined to its neighbour
        let joined = |a: Pos2, b: Pos2| (b - a).length() < (cam.s as f32) * 0.5;
        let limit = Color32::from_rgb(250, 200, 70);
        let centre = Color32::from_rgb(70, 140, 235);
        let mut north = Vec::new();
        let mut south = Vec::new();
        for (i, (p, pt)) in pts.iter().enumerate() {
            let (a, b) = (pts.get(i.saturating_sub(1)).map_or(*p, |q| q.0), pts.get(i + 1).map_or(*p, |q| q.0));
            let n = side(a, b);
            let w = (pt.penumbra_km * px_per_km(pt.lat)) as f32;
            north.push(*p + n * w);
            south.push(*p - n * w);
        }
        for line in [&north, &south] {
            for w in line.windows(2) {
                if let [a, b] = w
                    && joined(*a, *b)
                {
                    painter.line_segment([*a, *b], Stroke::new(1.5, limit.gamma_multiply(0.85)));
                }
            }
        }
        // the umbra: a strip between the two edges, as quads
        for w in pts.windows(2) {
            let [(a, pa), (b, pb)] = w else { continue };
            if !joined(*a, *b) {
                continue;
            }
            let n = side(*a, *b);
            let (wa, wb) = ((pa.half_width_km.abs() * px_per_km(pa.lat)) as f32, (pb.half_width_km.abs() * px_per_km(pb.lat)) as f32);
            let fill = if pa.half_width_km >= 0.0 {
                Color32::from_rgba_unmultiplied(15, 20, 40, 170)
            } else {
                Color32::from_rgba_unmultiplied(120, 70, 20, 150)
            };
            painter.add(Shape::convex_polygon(vec![*a + n * wa, *b + n * wb, *b - n * wb, *a - n * wa], fill, Stroke::NONE));
            painter.line_segment([*a, *b], Stroke::new(2.0, centre));
        }
        // a label every ten minutes, and at the greatest eclipse
        let mut last = f64::NEG_INFINITY;
        for (i, (p, pt)) in pts.iter().enumerate() {
            let great = i == path.greatest;
            if !great && pt.unix - last < 600.0 {
                continue;
            }
            last = pt.unix;
            if !painter.clip_rect().expand(40.0).contains(*p) {
                continue;
            }
            let text = clock(pt.unix, self.c.offset);
            let g = painter.layout_no_wrap(text, t.semibold(11.0), t.text);
            let r = Rect::from_min_size(*p + vec2(8.0, -g.size().y / 2.0), g.size()).expand2(vec2(5.0, 2.0));
            painter.circle_filled(*p, if great { 6.0 } else { 3.5 }, if great { limit } else { centre });
            painter.rect_filled(r, 6.0, crate::theme::chip_fill(&t));
            painter.galley(r.min + vec2(5.0, 2.0), g, t.text);
        }
    }

    pub(super) fn meteor_layer(&self, painter: &egui::Painter, origin: Pos2, reach: f32, shower: usize) {
        let t = self.t;
        let Some(s) = SHOWERS.get(shower) else { return };
        let path: Vec<Horizontal> = (0..145).map(|i| s.radiant_horizontal(self.c.start + i as f64 * SAMPLE_STEP, self.obs)).collect();
        self.trajectory(painter, origin, reach, &path, METEOR);
        let now = s.radiant_horizontal(self.c.start + self.minute * 60.0, self.obs);
        if now.alt >= 0.0 {
            let pt = polar(origin, now.az, now.alt, reach);
            painter.line_segment([origin, pt], Stroke::new(2.0, METEOR.gamma_multiply(0.9)));
            painter.circle_stroke(pt, 9.0, Stroke::new(2.4, METEOR));
            painter.circle_filled(pt, 3.0, METEOR);
            let g = painter.layout_no_wrap(crate::i18n::tr(s.name).to_string(), t.semibold(10.5), Color32::WHITE);
            let r = Rect::from_min_size(pt + vec2(12.0, -8.0), g.size() + vec2(10.0, 4.0));
            painter.rect_filled(r, 8.0, Color32::from_black_alpha(150));
            painter.galley(r.min + vec2(5.0, 2.0), g, Color32::WHITE);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_zenith_is_the_centre_and_the_horizon_the_edge() {
        let o = pos2(100.0, 100.0);
        assert_eq!(polar(o, 123.0, 90.0, 50.0), o);
        let east = polar(o, 90.0, 0.0, 50.0);
        assert!((east.x - 150.0).abs() < 1e-3 && (east.y - 100.0).abs() < 1e-3, "{east:?}");
        let north = polar(o, 0.0, 45.0, 80.0);
        assert!((north.y - 60.0).abs() < 1e-3, "{north:?}");
        // below the horizon sits on the edge, not outside it
        assert!((polar(o, 0.0, -30.0, 50.0).y - 50.0).abs() < 1e-3);
    }

    #[test]
    fn the_kinds_of_light_follow_the_sun() {
        assert_eq!(light_kind(30.0), DAY_LIGHT);
        assert_eq!(light_kind(2.0), GOLDEN_LIGHT);
        assert_eq!(light_kind(-5.0), BLUE_LIGHT);
        assert_eq!(light_kind(-9.0), NAUTICAL_LIGHT);
        assert_eq!(light_kind(-15.0), ASTRO_LIGHT);
        assert_eq!(light_kind(-40.0), NIGHT_LIGHT);
    }

    #[test]
    fn styles_cycle_back_to_the_start() {
        let s = LineStyle::Standard;
        assert_eq!(s.next().next().next(), s);
        assert_ne!(s.next(), s);
    }
}
