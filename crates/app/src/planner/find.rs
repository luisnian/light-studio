//! "Find": the days on which the Sun or the Moon stands at a direction (an azimuth, and an altitude if you
//! want one) from the place, within a tolerance, over a stretch of days. It opens in a large card centred
//! over the map, laid out the way the planning apps of the trade do it: a small map to point the direction on,
//! a gauge for the elevation, and a table of the days with the kind of light (and the Moon's phase) on each.
//! A click on a result moves the time bar to it.

use light_studio_astro::moon::moon_phase;
use light_studio_astro::search::{Hit, Target, find_with};
use light_studio_astro::sun::sun_horizontal;

use super::saved::{self, Spec};
use super::skylayers::{ASTRO_LIGHT, BLUE_LIGHT, DAY_LIGHT, GOLDEN_LIGHT, MOONRISE, NAUTICAL_LIGHT, NIGHT_LIGHT, SUNRISE, SUNSET, light_kind};
use super::*;
use crate::map::{Cam, MAX_ZOOM, MIN_ZOOM, MapView, draw_tiles};

/// How many results the table shows.
const MAX_SHOWN: usize = 400;
/// What the table can be sorted by.
const SORT_DATE: u8 = 0;
const SORT_AZ: u8 = 1;
const SORT_ALT: u8 = 2;
const SORT_LIGHT: u8 = 3;
const SORT_PHASE: u8 = 4;
/// The zoom the mini-map starts at.
const START_ZOOM: f64 = 11.0;
/// The kinds of light, from the brightest, with the name that is translated.
const LIGHTS: [(Color32, &str); 6] = [
    (DAY_LIGHT, "Day"),
    (GOLDEN_LIGHT, "Golden hour"),
    (BLUE_LIGHT, "Blue hour"),
    (NAUTICAL_LIGHT, "Nautical"),
    (ASTRO_LIGHT, "Astronomical"),
    (NIGHT_LIGHT, "Night"),
];

/// The name of the kind of light under a Sun at `alt` degrees.
fn light_name(alt: f64) -> &'static str {
    let c = light_kind(alt);
    LIGHTS.iter().find(|(k, _)| *k == c).map_or("Night", |(_, n)| n)
}

/// The body looked for.
fn body_target(st: &PlannerState) -> Target {
    if st.find_core {
        Target::Core
    } else if st.find_sun {
        Target::Sun
    } else {
        Target::Moon
    }
}

/// Why a moment found is interesting, in words (the numbers are the astronomy's, nothing is guessed).
fn explain(h: &Hit, body: &str, moon: bool) -> String {
    let mut s = format!(
        "{body}: {} {:.0}°, {} {:.1}°. {} {:.1}° ({}).",
        crate::i18n::tr("azimuth"),
        h.at.az,
        crate::i18n::tr("elevation"),
        h.at.alt,
        crate::i18n::tr("Sun at"),
        h.sun_alt,
        crate::i18n::tr(light_name(h.sun_alt))
    );
    if moon {
        s.push_str(&format!(" {:.0} % {}.", h.illumination * 100.0, crate::i18n::tr("of the Moon lit")));
    }
    if let Some(m) = h.margin {
        let how = if m >= 0.0 { crate::i18n::tr("above") } else { crate::i18n::tr("behind") };
        s.push_str(&format!(" {:.1}° {how} {}.", m.abs(), crate::i18n::tr("the skyline of the ground heights")));
    }
    s
}

/// The compass bearing (degrees clockwise from north, 0–360) of `p` from `c`.
fn bearing(c: Pos2, p: Pos2) -> f64 {
    let (dx, dy) = (f64::from(p.x - c.x), f64::from(p.y - c.y));
    dx.atan2(-dy).to_degrees().rem_euclid(360.0)
}

/// Where a ray from `c` in direction `dir` leaves `r` (inset a little), for `c` inside `r`.
fn edge_point(r: Rect, c: Pos2, dir: egui::Vec2) -> Pos2 {
    let inset = 6.0;
    let tx = if dir.x > 1e-6 {
        (r.right() - inset - c.x) / dir.x
    } else if dir.x < -1e-6 {
        (r.left() + inset - c.x) / dir.x
    } else {
        f32::INFINITY
    };
    let ty = if dir.y > 1e-6 {
        (r.bottom() - inset - c.y) / dir.y
    } else if dir.y < -1e-6 {
        (r.top() + inset - c.y) / dir.y
    } else {
        f32::INFINITY
    };
    let t = tx.min(ty);
    if t.is_finite() && t > 0.0 { c + dir * t } else { c }
}

/// The corners of a wedge from `c` out to `radius` across `az ± tol` degrees.
fn wedge(c: Pos2, az: f64, tol: f64, radius: f32) -> Vec<Pos2> {
    let mut pts = vec![c];
    let steps = 12;
    for i in 0..=steps {
        let a = (az - tol + 2.0 * tol * f64::from(i) / f64::from(steps)).to_radians();
        pts.push(c + vec2(a.sin() as f32, -a.cos() as f32) * radius);
    }
    pts
}

impl Page<'_> {
    /// The Find card: the search on the left, the dates it found on the right.
    pub(super) fn find_popup(&mut self, ui: &mut Ui, st: &mut PlannerState, rt: &mut Runtime, free: Rect) {
        if !rt.find_open {
            return;
        }
        // a record or the calculations opened in front of it take its place
        if super::popup::shown(st, rt).is_some() || rt.calc_open {
            rt.find_open = false;
            return;
        }
        let t = self.t;
        let r = free.shrink(8.0);
        if r.width() < 200.0 || r.height() < 160.0 {
            return;
        }
        super::layout::surface(ui.painter(), r, &t);
        let _ = ui.interact(r, egui::Id::new("planner-find-bg"), Sense::click_and_drag());
        let painter = ui.painter().with_clip_rect(r);
        painter.text(pos2(r.left() + 20.0, r.top() + 28.0), Align2::LEFT_CENTER, crate::i18n::tr("Find"), t.semibold(16.0), t.text);
        let close = Rect::from_min_size(pos2(r.right() - 16.0 - 28.0, r.top() + 14.0), vec2(28.0, 28.0));
        let mut head = ui.new_child(egui::UiBuilder::new().max_rect(close));
        if icon_button(&mut head, "plannerFindClose", Icon::Close, vec2(28.0, 28.0), false, true, "Close").clicked() {
            rt.find_open = false;
        }
        let body = Rect::from_min_max(pos2(r.left() + 1.0, r.top() + 52.0), r.max - vec2(1.0, 1.0));
        let wide = body.width() >= 700.0;
        let (left, right) = if wide {
            let split = (body.width() - 500.0_f32.min(body.width() * 0.5)).max(body.width() * 0.5);
            (
                Rect::from_min_max(body.min, pos2(body.left() + split, body.bottom())),
                Some(Rect::from_min_max(pos2(body.left() + split, body.top()), body.max)),
            )
        } else {
            (body, None)
        };
        let mut l = ui.new_child(egui::UiBuilder::new().max_rect(left).layout(egui::Layout::top_down(Align::Min)));
        l.set_clip_rect(left);
        super::popup::card_scroll(&mut l, "planner-find-left", left, |ui| {
            egui::Frame::NONE.inner_margin(Margin { left: 20, right: 16, top: 4, bottom: 16 }).show(ui, |ui| {
                ui.set_width(ui.available_width());
                self.find_search(ui, st, rt, left.height());
                if !wide {
                    ui.add_space(10.0);
                    self.find_results(ui, st, rt);
                }
            });
        });
        if let Some(right) = right {
            painter.line_segment([pos2(right.left(), right.top() + 4.0), pos2(right.left(), right.bottom() - 12.0)], Stroke::new(1.0, t.divider));
            let mut rui = ui.new_child(egui::UiBuilder::new().max_rect(right).layout(egui::Layout::top_down(Align::Min)));
            rui.set_clip_rect(right);
            super::popup::card_scroll(&mut rui, "planner-find-right", right, |ui| {
                egui::Frame::NONE.inner_margin(Margin { left: 16, right: 20, top: 4, bottom: 16 }).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    self.find_results(ui, st, rt);
                });
            });
        }
    }

    /// What to look for: the body, the direction on a mini-map, the elevation, the tolerance and the stretch of days.
    fn find_search(&mut self, ui: &mut Ui, st: &mut PlannerState, rt: &mut Runtime, column_h: f32) {
        let t = self.t;
        let items = [("Sun", "sun"), ("Moon", "moon"), ("Milky Way", "core")];
        let chosen = if st.find_core { 2 } else { usize::from(!st.find_sun) };
        if let Some(n) = crate::widgets::segmented(ui, "plannerFindBody", &items, Some(chosen), 3) {
            (st.find_sun, st.find_core) = (n == 0, n == 2);
        }
        ui.add_space(6.0);
        note(ui, crate::i18n::tr("Point the direction on the map and the height on the bar."));
        ui.add_space(4.0);

        // the mini-map, north up, round the place, with the direction as a wedge; the elevation gauge beside it
        let w = ui.available_width();
        let gauge_w = 30.0;
        let (area, _) = ui.allocate_exact_size(vec2(w, (column_h - 250.0).clamp(220.0, 1200.0)), Sense::hover());
        let map = Rect::from_min_max(area.min, pos2(area.right() - gauge_w - 8.0, area.bottom()));
        let gauge = Rect::from_min_max(pos2(area.right() - gauge_w, area.top()), area.max);
        if rt.find_zoom < MIN_ZOOM {
            rt.find_zoom = START_ZOOM;
        }
        let ctx = ui.ctx().clone();
        let now = ctx.input(|i| i.time);
        let resp = ui.interact(map, egui::Id::new("planner-find-map"), Sense::click_and_drag());
        if resp.hovered() {
            // the trackpad's two-finger scroll and its pinch both zoom the map; the card's own scrolling stands aside
            let (scroll, pinch) = ctx.input(|i| (i.smooth_scroll_delta, i.zoom_delta()));
            let dz = f64::from(scroll.y) / 240.0 + f64::from(pinch).log2();
            if dz.abs() > 1e-4 {
                rt.find_zoom = (rt.find_zoom + dz).clamp(MIN_ZOOM, MAX_ZOOM);
            }
            ctx.input_mut(|i| i.smooth_scroll_delta = egui::Vec2::ZERO);
        }
        if (resp.dragged() || resp.clicked())
            && let Some(p) = resp.interact_pointer_pos()
        {
            let c = map.center();
            if (p - c).length() > 4.0 {
                st.find_az = bearing(c, p).round() % 360.0;
            }
        }
        let cam = Cam::new(MapView { lat: st.lat, lon: st.lon, zoom: rt.find_zoom }, map);
        let painter = ui.painter_at(map);
        painter.rect_filled(map, 8.0, t.canvas);
        draw_tiles(&mut *self.tiles, &self.carto, self.satellite, rt.find_zoom, &ctx, &painter, map, cam, now, false);
        let c = map.center();
        let tint = if st.find_sun {
            SUNRISE
        } else if st.find_core {
            self.pal.core
        } else {
            MOONRISE
        };
        // the directions the body never takes in these days are darkened: no use pointing there
        let key = (u8::from(!st.find_sun) + u8::from(st.find_core), st.lat.to_bits(), st.lon.to_bits(), self.c.start.to_bits(), st.find_days as u32);
        if rt.find_reach.as_ref().is_none_or(|(k, _)| *k != key) {
            let target = body_target(st);
            let seen = light_studio_astro::search::reachable_azimuths(target, self.obs, self.c.start, st.find_days as u32);
            rt.find_reach = Some((key, Box::new(seen)));
        }
        if let Some((_, seen)) = &rt.find_reach {
            let far = map.width().hypot(map.height());
            let shade = Color32::from_black_alpha(150);
            let mut i = 0;
            while i < 360 {
                if seen.get(i).copied().unwrap_or(true) {
                    i += 1;
                    continue;
                }
                let from = i;
                while i < 360 && !seen.get(i).copied().unwrap_or(true) {
                    i += 1;
                }
                // bins are whole degrees, centred on their number: a run covers from - 0.5 to i - 0.5
                let (a, b) = (from as f64 - 0.5, i as f64 - 0.5);
                // wedges of at most 60° each: a convex polygon can't be wider than a half turn
                let pieces = (((b - a) / 60.0).ceil() as usize).max(1);
                for piece in 0..pieces {
                    let (pa, pb) = (a + (b - a) * piece as f64 / pieces as f64, a + (b - a) * (piece + 1) as f64 / pieces as f64);
                    let steps = (((pb - pa) / 3.0).ceil() as usize).max(1);
                    let mut pts = vec![c];
                    for k in 0..=steps {
                        let az = (pa + (pb - pa) * k as f64 / steps as f64).to_radians();
                        pts.push(c + vec2(az.sin() as f32, -az.cos() as f32) * far);
                    }
                    painter.add(Shape::convex_polygon(pts, shade, Stroke::NONE));
                }
            }
        }
        // the direction runs out to the map's edge (the map clips it), so it can be placed finely from far away
        let reach = (map.width().hypot(map.height())) + 4.0;
        painter.add(Shape::convex_polygon(wedge(c, st.find_az, st.find_tol.max(1.5), reach), tint.gamma_multiply(0.30), Stroke::new(1.0, tint)));
        let a = st.find_az.to_radians();
        let dir = vec2(a.sin() as f32, -a.cos() as f32);
        painter.line_segment([c, c + dir * reach], Stroke::new(2.0, tint));
        let tip = edge_point(map, c, dir);
        painter.circle_filled(tip, 5.0, tint);
        painter.circle_stroke(tip, 5.0, Stroke::new(1.0, Color32::BLACK));
        painter.circle_filled(c, 5.0, Color32::WHITE);
        painter.circle_stroke(c, 5.0, Stroke::new(1.5, Color32::BLACK));
        painter.text(pos2(map.center().x, map.top() + 10.0), Align2::CENTER_CENTER, "N", t.semibold(11.0), Color32::WHITE);
        painter.rect_stroke(map, 8.0, Stroke::new(1.0, t.divider), StrokeKind::Inside);
        // zoom buttons
        for (k, (label, id, dz)) in [("+", "plannerFindZoomIn", 1.0), ("−", "plannerFindZoomOut", -1.0)].into_iter().enumerate() {
            let b = Rect::from_min_size(pos2(map.right() - 32.0, map.bottom() - 64.0 + 30.0 * k as f32), vec2(26.0, 26.0));
            let rb = ui.interact(b, egui::Id::new(id), Sense::click());
            painter.rect_filled(b, 6.0, if rb.hovered() { t.hover } else { t.chrome });
            painter.text(b.center(), Align2::CENTER_CENTER, label, t.semibold(15.0), t.text);
            if rb.clicked() {
                rt.find_zoom = (rt.find_zoom + dz).clamp(MIN_ZOOM, MAX_ZOOM);
            }
        }
        self.find_gauge(ui, st, gauge);

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.allocate_ui_with_layout(vec2(104.0, 26.0), egui::Layout::left_to_right(Align::Center), |ui| {
                ui.label(egui::RichText::new(crate::i18n::tr("Azimuth (°)")).font(t.font(12.0)).color(t.text_label));
            });
            number(ui, rt, "findAz", &mut st.find_az, (0.0, 360.0), 0, 64.0);
            ui.add_space(8.0);
            if text_button(ui, "plannerFindUseAlt", "Elevation (°)", st.find_use_alt).clicked() {
                st.find_use_alt = !st.find_use_alt;
            }
            ui.add_enabled_ui(st.find_use_alt, |ui| {
                number(ui, rt, "findAlt", &mut st.find_alt, (-90.0, 90.0), 0, 64.0);
            });
        });
        labelled_number(ui, rt, "findTol", "Tolerance (°)", &mut st.find_tol, (0.1, 45.0), 1);
        labelled_number(ui, rt, "findDays", "Days to look through", &mut st.find_days, (1.0, 400.0), 0);
        ui.add_space(6.0);
        self.find_conditions(ui, st, rt);
        ui.add_space(6.0);
        // the skyline is read when the Mountains view is on; a search over it needs it for this place
        let skyline = rt.horizon.as_ref().filter(|h| h.key == super::horizon::key_of(st)).and_then(|h| h.data.as_ref().ok().cloned());
        let waiting = st.find_skyline && skyline.is_none();
        ui.add_enabled_ui(!waiting, |ui| {
            let go = text_button(ui, "plannerFindGo", "Find the Dates", false).clicked() | std::mem::take(&mut rt.find_load);
            if go {
                let start = self.c.start;
                // (the skyline is the one condition a saved search cannot hold)
                let mut spec = Spec::of(&PlannerState { find_skyline: false, ..st.clone() }).unwrap_or_default();
                spec.days = st.find_days;
                let mut conditions = spec.conditions();
                if st.find_skyline {
                    conditions.skyline = skyline.as_deref().map(|s| (s, st.find_margin[0], st.find_margin[1]));
                }
                rt.found_dates = Some(find_with(spec.target(), self.obs, start, st.find_days as u32, st.find_az, spec.alt, st.find_tol, conditions));
                let mut label = spec.label();
                if st.find_skyline {
                    label.push_str(&format!(" · {} {:.1}° … {:.1}°", crate::i18n::tr("over the skyline"), st.find_margin[0], st.find_margin[1]));
                }
                rt.found_label = label;
                rt.found_moon = !st.find_sun && !st.find_core;
                rt.found_core = st.find_core;
                rt.found_sorted_by = None;
                if !rt.found_moon && rt.found_sort == SORT_PHASE {
                    rt.found_sort = SORT_DATE;
                }
            }
        });
        let spec = Spec::of(st);
        let tip = if spec.is_some() {
            "Keep these conditions with the place and see when they next happen"
        } else {
            "A search over the skyline cannot be saved: it needs the terrain"
        };
        let full = st.searches.len() >= saved::MAX_SEARCHES;
        ui.add_enabled_ui(spec.is_some() && !full, |ui| {
            if text_button(ui, "plannerFindSave", "Save the Search", false).on_hover_text(crate::i18n::tr(tip)).clicked()
                && let Some(spec) = spec
            {
                let one = saved::SavedSearch { place: st.place.clone(), lat: st.lat, lon: st.lon, spec };
                if !st.searches.contains(&one) {
                    st.searches.push(one);
                }
            }
        });
    }

    /// The conditions besides the direction: the light, the Moon, the skyline.
    fn find_conditions(&mut self, ui: &mut Ui, st: &mut PlannerState, rt: &mut Runtime) {
        subhead(ui, "Conditions");
        note(ui, crate::i18n::tr("The day's best moment that also meets these, not the best moment checked afterwards."));
        let lights = [("Any light", "0"), ("Golden hour", "1"), ("Blue hour", "2"), ("Dark sky", "3")];
        if let Some(n) = crate::widgets::segmented(ui, "plannerFindLight", &lights, Some(usize::from(st.find_light)), 2) {
            st.find_light = n as u8;
        }
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            if text_button(ui, "plannerFindUseIll", "Moon lit (%)", st.find_use_ill).clicked() {
                st.find_use_ill = !st.find_use_ill;
            }
            ui.add_enabled_ui(st.find_use_ill, |ui| {
                number(ui, rt, "findIllLo", &mut st.find_ill[0], (0.0, 100.0), 0, 56.0);
                number(ui, rt, "findIllHi", &mut st.find_ill[1], (0.0, 100.0), 0, 56.0);
            });
            if text_button(ui, "plannerFindNoMoon", "Moon down", st.find_no_moon).clicked() {
                st.find_no_moon = !st.find_no_moon;
            }
        });
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            if text_button(ui, "plannerFindSkyline", "Over the skyline (°)", st.find_skyline).clicked() {
                st.find_skyline = !st.find_skyline;
            }
            ui.add_enabled_ui(st.find_skyline, |ui| {
                number(ui, rt, "findMarginLo", &mut st.find_margin[0], (-10.0, 10.0), 1, 56.0);
                number(ui, rt, "findMarginHi", &mut st.find_margin[1], (-10.0, 10.0), 1, 56.0);
            });
        });
        if st.find_skyline {
            let loaded = rt.horizon.as_ref().is_some_and(|h| h.key == super::horizon::key_of(st) && h.data.is_ok());
            note(
                ui,
                crate::i18n::tr(if loaded {
                    "From the place's real skyline (ground heights; trees and buildings are not in it). 0° to 1°: just clearing the ridge; −1° to 0°: just behind it."
                } else {
                    "Turn on Mountains once for this place: the skyline is read from the ground heights, and the search needs it."
                }),
            );
        }
    }

    /// The elevation gauge: 0° at the foot to 90° at the top; a click or drag sets the height wanted.
    fn find_gauge(&mut self, ui: &mut Ui, st: &mut PlannerState, gauge: Rect) {
        let t = self.t;
        let resp = ui.interact(gauge, egui::Id::new("planner-find-gauge"), Sense::click_and_drag());
        if (resp.dragged() || resp.clicked())
            && let Some(p) = resp.interact_pointer_pos()
        {
            let f = f64::from(((gauge.bottom() - p.y) / gauge.height()).clamp(0.0, 1.0));
            st.find_alt = (f * 90.0).round();
            st.find_use_alt = true;
        }
        let painter = ui.painter_at(gauge.expand(2.0));
        // the sky from the horizon (bottom) to the zenith (top)
        let rows = 45;
        for i in 0..rows {
            let f0 = i as f32 / rows as f32;
            let y1 = gauge.bottom() - gauge.height() * f0;
            let y0 = gauge.bottom() - gauge.height() * (i + 1) as f32 / rows as f32;
            let k = Color32::from_rgb(40, 52, 90).lerp_to_gamma(Color32::from_rgb(120, 190, 235), f0);
            painter.rect_filled(Rect::from_min_max(pos2(gauge.left(), y0), pos2(gauge.right(), y1 + 0.5)), 0.0, k);
        }
        painter.rect_stroke(gauge, 6.0, Stroke::new(1.0, t.divider), StrokeKind::Inside);
        for deg in [0.0, 30.0, 60.0, 90.0] {
            let y = gauge.bottom() - gauge.height() * (deg / 90.0) as f32;
            painter.line_segment([pos2(gauge.left(), y), pos2(gauge.left() + 6.0, y)], Stroke::new(1.0, Color32::WHITE));
            painter.text(
                pos2(gauge.right() - 3.0, y.clamp(gauge.top() + 6.0, gauge.bottom() - 6.0)),
                Align2::RIGHT_CENTER,
                format!("{deg:.0}"),
                t.font(9.0),
                Color32::WHITE,
            );
        }
        if st.find_use_alt {
            let y = gauge.bottom() - gauge.height() * (st.find_alt.clamp(0.0, 90.0) / 90.0) as f32;
            let tint = if st.find_sun {
                SUNRISE
            } else if st.find_core {
                self.pal.core
            } else {
                MOONRISE
            };
            let band = (gauge.height() * (st.find_tol / 90.0) as f32).max(3.0);
            let b = Rect::from_min_max(pos2(gauge.left(), (y - band).max(gauge.top())), pos2(gauge.right(), (y + band).min(gauge.bottom())));
            painter.rect_filled(b, 0.0, tint.gamma_multiply(0.5));
            painter.line_segment([pos2(gauge.left() - 2.0, y), pos2(gauge.right() + 2.0, y)], Stroke::new(2.0, tint));
        }
    }

    /// The dates found: a table in the way of the planning apps, the right cell coloured by the kind of light.
    fn find_results(&mut self, ui: &mut Ui, st: &mut PlannerState, rt: &mut Runtime) {
        let t = self.t;
        self.saved_list(ui, st, rt);
        let Some(hits) = rt.found_dates.clone() else {
            ui.add_space((ui.available_height() / 3.0).max(40.0));
            ui.vertical_centered(|ui| note(ui, crate::i18n::tr("Choose the direction and press Find the Dates.")));
            return;
        };
        subhead(ui, "Results");
        if !rt.found_label.is_empty() {
            note(ui, &rt.found_label.clone());
        }
        if hits.is_empty() {
            ui.add_space(4.0);
            note(ui, crate::i18n::tr("It is never there in those days. Try a wider tolerance."));
            return;
        }
        let moon = rt.found_moon;
        let shown = hits.len().min(MAX_SHOWN);
        let row_h = if moon { 38.0 } else { 30.0 };
        // the order shown (by date, azimuth, elevation, kind of light or phase), kept until the choice or the search changes
        if rt.found_sorted_by != Some((rt.found_sort, rt.found_desc, hits.len())) {
            let keys: Vec<f64> = hits.iter().map(|h| self.find_key(h, rt.found_sort)).collect();
            let mut order: Vec<usize> = (0..hits.len()).collect();
            order.sort_by(|&a, &b| {
                let (ka, kb) = (keys.get(a).copied().unwrap_or(0.0), keys.get(b).copied().unwrap_or(0.0));
                let o = ka.total_cmp(&kb).then_with(|| {
                    let (ua, ub) = (hits.get(a).map_or(0.0, |h| h.unix), hits.get(b).map_or(0.0, |h| h.unix));
                    ua.total_cmp(&ub)
                });
                if rt.found_desc { o.reverse() } else { o }
            });
            rt.found_order = order;
            rt.found_sorted_by = Some((rt.found_sort, rt.found_desc, hits.len()));
        }
        // the legend of the colours
        ui.add_space(2.0);
        ui.horizontal_wrapped(|ui| {
            for (k, name) in LIGHTS {
                let (r, _) = ui.allocate_exact_size(vec2(10.0, 14.0), Sense::hover());
                ui.painter().rect_filled(Rect::from_center_size(r.center(), vec2(10.0, 10.0)), 3.0, k);
                ui.painter().rect_stroke(Rect::from_center_size(r.center(), vec2(10.0, 10.0)), 3.0, Stroke::new(1.0, t.divider), StrokeKind::Inside);
                ui.label(egui::RichText::new(crate::i18n::tr(name)).font(t.font(10.5)).color(t.text_dim));
                ui.add_space(4.0);
            }
        });
        ui.add_space(4.0);
        let w = ui.available_width();
        let (head, _) = ui.allocate_exact_size(vec2(w, 22.0), Sense::hover());
        let cols = FindCols::new(head.left(), w, moon);
        // the headings sort the table: a click picks the column, a second click turns the order round
        let mut heads = vec![
            (SORT_DATE, cols.date, cols.date + 120.0, "Date", Align2::LEFT_CENTER),
            (SORT_AZ, cols.az - 4.0, cols.az + 38.0, "Azimuth", Align2::RIGHT_CENTER),
            (SORT_ALT, cols.alt - 4.0, cols.alt + 38.0, "Elevation", Align2::RIGHT_CENTER),
            (SORT_LIGHT, cols.sky, cols.sky + cols.sky_w, "Light", Align2::LEFT_CENTER),
        ];
        if moon {
            heads.push((SORT_PHASE, cols.phase, cols.phase + 120.0, "Phase", Align2::LEFT_CENTER));
        }
        for (key, x0, x1, text, align) in heads {
            let hr = Rect::from_min_max(pos2(x0.min(x1) - 2.0, head.top()), pos2(x1 + 2.0, head.bottom()));
            let resp = ui.interact(hr, egui::Id::new(("planner-find-sort", key)), Sense::click());
            let on = rt.found_sort == key;
            let arrow = if !on {
                ""
            } else if rt.found_desc {
                " ↓"
            } else {
                " ↑"
            };
            let color = if on {
                t.text
            } else if resp.hovered() {
                t.text_label
            } else {
                t.text_dim
            };
            let x = if align == Align2::LEFT_CENTER { x0 } else { x1 };
            ui.painter().text(pos2(x, head.center().y), align, format!("{}{arrow}", crate::i18n::tr(text)), t.font(10.5), color);
            if resp.clicked() {
                if on {
                    rt.found_desc = !rt.found_desc;
                } else {
                    rt.found_sort = key;
                    rt.found_desc = false;
                }
            }
        }
        ui.painter().line_segment([pos2(head.left(), head.bottom()), pos2(head.right(), head.bottom())], Stroke::new(1.0, t.divider));
        let (lit, shade) = moon_colors(ui.visuals().dark_mode, self.pal, t.inset);
        let order = rt.found_order.clone();
        for (i, &at) in order.iter().enumerate().take(shown) {
            let Some(h) = hits.get(at) else { continue };
            let (cell, _) = ui.allocate_exact_size(vec2(w, row_h), Sense::hover());
            let resp = ui.interact(cell, egui::Id::new(("planner-find", i)), Sense::click());
            if !ui.is_rect_visible(cell) {
                if resp.clicked() {
                    super::strip::jump_to(st, h.unix, self.c.offset);
                    rt.find_open = false;
                }
                continue;
            }
            if resp.hovered() {
                ui.painter().rect_filled(cell, 6.0, t.hover.gamma_multiply(0.5));
            }
            let body = crate::i18n::tr(if rt.found_core {
                "Milky Way"
            } else if moon {
                "Moon"
            } else {
                "Sun"
            });
            let resp = resp.on_hover_text(explain(h, body, moon));
            self.find_row(ui, st, h, cell, &cols, moon, rt.found_core, (lit, shade));
            if resp.clicked() {
                super::strip::jump_to(st, h.unix, self.c.offset);
                rt.find_open = false;
            }
        }
        if hits.len() > shown {
            note(ui, &format!("{} {shown} / {}.", crate::i18n::tr("Showing the first"), hits.len()));
        }
    }

    /// What a hit is sorted by: the date, the azimuth, the elevation, how bright the light is (day first, night last) or the
    /// Moon's phase (from new, through full, back to new).
    fn find_key(&self, h: &Hit, by: u8) -> f64 {
        match by {
            SORT_AZ => h.at.az,
            SORT_ALT => h.at.alt,
            SORT_LIGHT => {
                let c = light_kind(sun_horizontal(h.unix, self.obs).alt);
                LIGHTS.iter().position(|(k, _)| *k == c).unwrap_or(LIGHTS.len()) as f64
            }
            SORT_PHASE => moon_phase(h.unix).elongation,
            _ => h.unix,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn find_row(
        &self,
        ui: &mut Ui,
        st: &PlannerState,
        h: &Hit,
        cell: Rect,
        cols: &FindCols,
        moon: bool,
        core: bool,
        moon_colors: (Color32, Color32),
    ) {
        let t = self.t;
        let p = ui.painter();
        let y = cell.center().y;
        let d = Date::from_unix(h.unix, self.c.offset);
        p.text(pos2(cols.date, y), Align2::LEFT_CENTER, format!("{} {}", short_date(d), clock(h.unix, self.c.offset)), t.font(12.0), t.text);
        p.text(pos2(cols.az + 38.0, y), Align2::RIGHT_CENTER, format!("{:.0}°", h.at.az), t.font(12.0), t.text);
        p.text(pos2(cols.alt + 38.0, y), Align2::RIGHT_CENTER, format!("{:.0}°", h.at.alt), t.font(12.0), t.text);
        // the colour of the light: the Sun's height decides, whatever body is looked for
        let sun_alt = sun_horizontal(h.unix, self.obs).alt;
        let swatch = Rect::from_min_size(pos2(cols.sky, cell.top() + 3.0), vec2(cols.sky_w, cell.height() - 6.0));
        p.rect_filled(swatch, 6.0, light_kind(sun_alt));
        p.rect_stroke(swatch, 6.0, Stroke::new(1.0, t.divider), StrokeKind::Inside);
        if moon {
            let ph = moon_phase(h.unix);
            moon_disc(p, swatch.center(), 10.0, ph.elongation, st.lat < 0.0, moon_colors.0, moon_colors.1, t.divider);
            let lit_pct = (ph.illumination * 100.0).round();
            p.text(pos2(cols.phase, y - 8.0), Align2::LEFT_CENTER, crate::i18n::tr(ph.name.label()), t.semibold(12.0), t.text);
            let sub = crate::i18n::tr("{n} % lit · {m} % hidden")
                .replace("{n}", &format!("{lit_pct:.0}"))
                .replace("{m}", &format!("{:.0}", 100.0 - lit_pct));
            p.text(pos2(cols.phase, y + 8.0), Align2::LEFT_CENTER, sub, t.font(11.0), t.text_dim);
        } else if core {
            let c = swatch.center();
            p.circle_filled(c, 7.0, self.pal.core);
            p.circle_stroke(c, 7.0, Stroke::new(1.0, Color32::BLACK.gamma_multiply(0.6)));
            p.text(pos2(swatch.right() + 10.0, y), Align2::LEFT_CENTER, crate::i18n::tr(light_name(sun_alt)), t.font(11.5), t.text_dim);
        } else {
            // the Sun: a disc, orange when low and dimmed under the horizon
            let c = swatch.center();
            let col = if h.at.alt < 0.0 {
                SUNRISE.gamma_multiply(0.35)
            } else if h.at.alt < 10.0 {
                SUNSET
            } else {
                SUNRISE
            };
            p.circle_filled(c, 8.0, col);
            p.circle_stroke(c, 8.0, Stroke::new(1.0, Color32::BLACK.gamma_multiply(0.6)));
            p.text(pos2(swatch.right() + 10.0, y), Align2::LEFT_CENTER, crate::i18n::tr(light_name(sun_alt)), t.font(11.5), t.text_dim);
        }
    }
}

/// The x of the table's columns (screen).
struct FindCols {
    date: f32,
    az: f32,
    alt: f32,
    sky: f32,
    sky_w: f32,
    phase: f32,
}

impl FindCols {
    fn new(left: f32, _w: f32, _moon: bool) -> FindCols {
        FindCols { date: left + 4.0, az: left + 128.0, alt: left + 184.0, sky: left + 240.0, sky_w: 56.0, phase: left + 312.0 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_direction_reaches_the_edge_of_the_map() {
        let r = Rect::from_min_max(pos2(0.0, 0.0), pos2(400.0, 200.0));
        let c = r.center();
        // north: up to the top; east: out to the right, well past the height of the map
        let n = edge_point(r, c, vec2(0.0, -1.0));
        assert!((n.y - 6.0).abs() < 0.01 && (n.x - c.x).abs() < 0.01, "{n:?}");
        let e = edge_point(r, c, vec2(1.0, 0.0));
        assert!((e.x - 394.0).abs() < 0.01 && (e.y - c.y).abs() < 0.01, "{e:?}");
        let ne = edge_point(r, c, vec2(std::f32::consts::FRAC_1_SQRT_2, -std::f32::consts::FRAC_1_SQRT_2));
        assert!(r.contains(ne), "{ne:?}");
        assert!(ne.y < 10.0 || ne.x > 390.0, "on the border: {ne:?}");
    }
}
