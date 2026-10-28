//! Mountains: the skyline seen from the place all the way round (PeakFinder style), from the real ground heights
//! (Open-Meteo, as the terrain profile), with the Sun, the Moon and the Milky Way's core drawn on it where they are at
//! the time shown, their paths over the day, and the summits that stand out with their height and distance. When it is
//! on it takes the map's place (the whole free part), as a panorama is only of use big.

use light_studio_astro::horizon::{self, Skyline};

use super::*;

/// The place the skyline was read for (latitude and longitude bits).
pub(super) type HorizonKey = (u64, u64);

/// The skyline of a place, or why it could not be had.
#[derive(Clone, Debug)]
pub(super) struct Horizon {
    pub key: HorizonKey,
    pub data: Result<Arc<Skyline>, String>,
    /// The named summits that show on the skyline, highest first.
    pub names: Arc<Vec<Named>>,
    /// Why the summit names are missing (the lookup failed), shown in the credit line.
    pub names_error: Option<String>,
    /// Whether the skyline is the fine one (terrain tiles) and not the coarse web lookup.
    pub fine: bool,
}

/// A named summit as seen from the place.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Named {
    pub name: String,
    pub az: f64,
    /// Degrees above the camera's horizon, metres from the place, metres above the sea.
    pub alt: f64,
    pub dist: f64,
    pub ele: f64,
}

/// The named summits (OpenStreetMap) that stand on or above the skyline from the place.
pub(super) fn visible_names(sky: &Skyline, lat: f64, lon: f64, peaks: &[crate::services::peaks::Peak]) -> Vec<Named> {
    let mut out: Vec<Named> = peaks
        .iter()
        .filter_map(|p| {
            let s = light_studio_astro::align::sight(lat, lon, p.lat, p.lon, p.ele - (sky.ground + light_studio_astro::align::EYE));
            // (a summit under the skyline is hidden by something nearer; the skyline's own samples are a little rough)
            let shows = s.distance >= 300.0 && s.distance <= sky.far * 1.05 && s.altitude >= sky.altitude_at(s.azimuth) - 0.3;
            shows.then(|| Named { name: p.name.clone(), az: s.azimuth, alt: s.altitude, dist: s.distance, ele: p.ele })
        })
        .collect();
    out.sort_by(|a, b| b.ele.total_cmp(&a.ele));
    out.truncate(400);
    out
}

const DEFAULT_FOV: f32 = 110.0;
const MIN_FOV: f32 = 20.0;
const COMPASS_H: f32 = 20.0;

pub(super) fn key_of(st: &PlannerState) -> HorizonKey {
    (st.lat.to_bits(), st.lon.to_bits())
}

/// Where the skyline goes: the map's free part, under the search box and beside the date and time cards (when there is
/// room for it; on a narrow window it runs underneath them).
pub(super) fn view_rect(free: Rect) -> Rect {
    let top = free.top() + 44.0;
    let beside = super::layout::cards_rects(free)[0].right() + 8.0;
    let left = if free.right() - 8.0 - beside >= 420.0 { beside } else { free.left() + 8.0 };
    let r = Rect::from_min_max(pos2(left, top.min(free.bottom())), pos2(free.right() - 8.0, free.bottom() - 8.0));
    if r.width() > 0.0 && r.height() > 0.0 { r } else { Rect::from_min_size(r.min, vec2(0.0, 0.0)) }
}

/// Starts the ground heights round the place when the strip is on and they are not at hand (on a worker thread).
pub(super) fn fetch(app: &mut App) {
    let st = &app.ui.planner;
    if !st.show_horizon {
        return;
    }
    let key = key_of(st);
    if app.planner.horizon.as_ref().is_some_and(|h| h.key == key) || app.planner.horizon_pending == Some(key) {
        return;
    }
    let (lat, lon) = (st.lat, st.lon);
    app.planner.horizon_pending = Some(key);
    let started = crate::tasks::spawn(
        app,
        "Mountains",
        Some("download"),
        move || -> Result<(Arc<Skyline>, Arc<Vec<Named>>, bool, Option<String>), String> {
            // the fine skyline from the terrain tiles; without them (offline, no tiles kept), the coarse one from the web service
            let fine = horizon::Grid::FINE;
            let from_tiles = match crate::services::terrain::heights(&fine.sample_points(lat, lon)) {
                Ok(h) => horizon::skyline_for(fine, &h),
                Err(e) => {
                    log::warn!("mountains: terrain tiles: {e}");
                    None
                }
            };
            let (sky, is_fine) = match from_tiles {
                Some(s) => (s, true),
                None => {
                    let points = horizon::sample_points(lat, lon);
                    let heights = crate::services::elevation::fetch_up_to(&points, crate::services::elevation::MAX_PANORAMA_POINTS)?;
                    (horizon::skyline(&heights).ok_or_else(|| "the ground heights were not what was asked for".to_string())?, false)
                }
            };
            // the names are a bonus: without them the summits keep their height and distance
            let mut names_error: Option<String> = None;
            let names = match crate::services::peaks::near(lat, lon) {
                Ok(p) => visible_names(&sky, lat, lon, &p),
                Err(e) => {
                    log::warn!("mountains: summit names: {e}");
                    names_error = Some(e);
                    Vec::new()
                }
            };
            Ok((Arc::new(sky), Arc::new(names), is_fine, names_error))
        },
        move |app, _ctx, data| {
            if app.planner.horizon_pending == Some(key) {
                app.planner.horizon_pending = None;
            }
            app.planner.horizon = Some(match data {
                Ok((sky, names, fine, names_error)) => Horizon { key, data: Ok(sky), names, fine, names_error },
                Err(e) => Horizon { key, data: Err(e), names: Arc::new(Vec::new()), fine: false, names_error: None },
            });
        },
    );
    if let Err(e) = started {
        app.planner.horizon_pending = None;
        app.planner.horizon = Some(Horizon { key, data: Err(e), names: Arc::new(Vec::new()), fine: false, names_error: None });
    }
}

/// The sky's colour for the Sun at `alt` degrees: night, dusk, day.
fn sky_colours(sun_alt: f64, dark_ui: bool) -> (Color32, Color32) {
    let k = ((sun_alt + 14.0) / 20.0).clamp(0.0, 1.0) as f32;
    let night = (Color32::from_rgb(8, 12, 28), Color32::from_rgb(26, 36, 66));
    let day = (Color32::from_rgb(82, 128, 196), Color32::from_rgb(176, 205, 232));
    let dusk = if dark_ui {
        (Color32::from_rgb(38, 44, 84), Color32::from_rgb(170, 110, 96))
    } else {
        (Color32::from_rgb(70, 84, 140), Color32::from_rgb(220, 160, 130))
    };
    let mix = |a: (Color32, Color32), b: (Color32, Color32), f: f32| (a.0.lerp_to_gamma(b.0, f), a.1.lerp_to_gamma(b.1, f));
    if k < 0.5 { mix(night, dusk, k * 2.0) } else { mix(dusk, day, (k - 0.5) * 2.0) }
}

impl Page<'_> {
    pub(super) fn horizon_strip(&mut self, ui: &mut Ui, st: &mut PlannerState, rt: &mut Runtime, r: Rect) {
        let t = self.t;
        let pal = self.pal;
        if r.width() < 160.0 || r.height() < 90.0 {
            return;
        }
        let shadow = egui::epaint::Shadow { offset: [0, 4], blur: 18, spread: 0, color: t.shadow };
        ui.painter().add(shadow.as_shape(r, egui::CornerRadius::same(R_CARD as u8)));
        let key = key_of(st);
        let sky = rt.horizon.as_ref().filter(|h| h.key == key).cloned();

        // the header: the title, where to look, and the close button
        let head = Rect::from_min_size(r.min, vec2(r.width(), 30.0));
        let body = Rect::from_min_max(pos2(r.left(), head.bottom()), r.max);
        let sun_alt = self.skies.sun.alt;
        let (top, low) = sky_colours(sun_alt, t.backdrop.r() < 128);

        // sky: a gradient from the top down to the horizon (over the ground afterwards)
        let painter = ui.painter_at(r);
        let mut m = egui::Mesh::default();
        m.colored_vertex(body.left_top(), top);
        m.colored_vertex(body.right_top(), top);
        m.colored_vertex(body.right_bottom(), low);
        m.colored_vertex(body.left_bottom(), low);
        m.add_triangle(0, 1, 2);
        m.add_triangle(0, 2, 3);
        // the card's top corners are rounded: the header has the card's colour, the body the sky
        painter.rect_filled(r, R_CARD, t.chrome);
        painter.add(Shape::mesh(m));
        painter.rect_filled(head, egui::CornerRadius { nw: R_CARD as u8, ne: R_CARD as u8, sw: 0, se: 0 }, t.chrome);

        let inner = Rect::from_min_max(pos2(body.left(), body.top()), pos2(body.right(), body.bottom() - COMPASS_H));
        let fov = if rt.pano_fov >= MIN_FOV { rt.pano_fov.min(360.0) } else { DEFAULT_FOV };
        let centre = rt.pano_az.unwrap_or(self.skies.sun.az as f32);
        let ppd = inner.width() / fov;
        let x_of = |az: f64| inner.center().x + (((az as f32 - centre + 540.0).rem_euclid(360.0)) - 180.0) * ppd;
        let az_at = |x: f32| f64::from((centre + (x - inner.center().x) / ppd).rem_euclid(360.0));

        // how tall the sky is drawn: true angles, stretched (at most three times) when the mountains are low
        let data = sky.as_ref().and_then(|h| h.data.as_ref().ok()).cloned();
        let mut max_in_view = data.as_ref().map_or(0.0, |s| {
            (0..=40).map(|k| s.altitude_at(f64::from(centre) - f64::from(fov) / 2.0 + f64::from(fov) * f64::from(k) / 40.0)).fold(0.0, f64::max)
        });
        // the Sun, the Moon and the core stay in the picture when they are in the directions shown
        for (i, h) in [(0usize, self.skies.sun), (1, self.skies.moon), (2, self.skies.core)] {
            let off = ((h.az as f32 - centre + 540.0).rem_euclid(360.0) - 180.0).abs();
            if st.layers[i] && off < fov / 2.0 {
                max_in_view = max_in_view.max(h.alt);
            }
        }
        let avail = inner.height() - 14.0;
        let painter_sky = painter.with_clip_rect(inner);
        let sy = (avail / (max_in_view as f32 + 5.0).max(8.0)).min(ppd * 3.0).max(0.5);
        let y0 =
            inner.bottom() - 6.0 - (-(data.as_ref().map_or(0.0, |s| s.alt.iter().copied().fold(f64::MAX, f64::min).min(0.0)) as f32) * sy).min(14.0);
        let y_of = |alt: f64| y0 - alt as f32 * sy;

        // the ground, a column at a time, paler the farther the ridge
        if let Some(s) = &data {
            let mut mesh = egui::Mesh::default();
            let mut x = inner.left();
            while x < inner.right() {
                let x2 = (x + 1.5).min(inner.right());
                let (a1, a2) = (az_at(x), az_at(x2));
                let near = |az: f64| {
                    let d = s.distance_at(az);
                    let k = ((d / s.near).max(1.0).ln() / (s.far / s.near).ln()).clamp(0.0, 1.0) as f32;
                    Color32::from_rgb(28, 32, 30).lerp_to_gamma(Color32::from_rgb(98, 112, 138), k.powf(0.8))
                };
                let (c1, c2) = (near(a1), near(a2));
                let bottom = |c: Color32| c.gamma_multiply(0.55);
                let (y1, y2) = (y_of(s.altitude_at(a1)), y_of(s.altitude_at(a2)));
                let base = mesh.vertices.len() as u32;
                mesh.colored_vertex(pos2(x, y1), c1);
                mesh.colored_vertex(pos2(x2, y2), c2);
                mesh.colored_vertex(pos2(x2, inner.bottom()), bottom(c2));
                mesh.colored_vertex(pos2(x, inner.bottom()), bottom(c1));
                mesh.add_triangle(base, base + 1, base + 2);
                mesh.add_triangle(base, base + 2, base + 3);
                x = x2;
            }
            painter.add(Shape::mesh(mesh));
        } else {
            painter.rect_filled(Rect::from_min_max(pos2(inner.left(), y0), inner.right_bottom()), 0.0, Color32::from_rgb(30, 34, 32));
        }
        // the true horizon
        painter.line_segment([pos2(inner.left(), y0), pos2(inner.right(), y0)], Stroke::new(1.0, Color32::from_white_alpha(46)));

        // the paths of the Sun and the Moon over the day, dotted
        let sky_p = &painter_sky;
        let path = |f: &dyn Fn(f64) -> Horizontal, color: Color32| {
            let mut prev: Option<Pos2> = None;
            for k in 0..=144 {
                let h = f(self.c.start + f64::from(k) * 600.0);
                if h.alt < -3.0 {
                    prev = None;
                    continue;
                }
                let p = pos2(x_of(h.az), y_of(h.alt));
                if let Some(q) = prev
                    && (p.x - q.x).abs() < inner.width() / 2.0
                    && k % 2 == 0
                {
                    sky_p.line_segment([q, p], Stroke::new(1.5, color.gamma_multiply(0.7)));
                }
                prev = Some(p);
            }
        };
        if st.layers[0] {
            path(&|u| light_studio_astro::sun_horizontal(u, self.obs), pal.sun);
        }
        if st.layers[1] {
            path(&|u| light_studio_astro::moon_horizontal(u, self.obs), pal.moon);
        }

        // the summits: named ones (OpenStreetMap) first, then the ones that only stand out; each in the lowest row where
        // it does not cover another, with a line down to its peak
        if let Some(s) = &data {
            struct Label {
                x: f32,
                y: f32,
                title: String,
                sub: String,
            }
            let names = sky.as_ref().map(|h| h.names.clone()).unwrap_or_default();
            let mut labels: Vec<Label> = Vec::new();
            for n in names.iter() {
                labels.push(Label {
                    x: x_of(n.az),
                    y: y_of(n.alt),
                    title: n.name.clone(),
                    sub: format!("{:.0} m · {}", n.ele, plans::distance_label(n.dist)),
                });
            }
            let diff = |a: f64, b: f64| ((a - b + 540.0).rem_euclid(360.0) - 180.0).abs();
            for p in s.peaks(0.8).into_iter().take(24) {
                if names.iter().any(|n| diff(n.az, p.az) < 1.5) {
                    continue;
                }
                labels.push(Label { x: x_of(p.az), y: y_of(p.alt), title: format!("{:.0} m", p.height), sub: plans::distance_label(p.dist) });
            }
            const ROWS: usize = 5;
            let mut rows: Vec<Vec<(f32, f32)>> = vec![Vec::new(); ROWS];
            let mut placed = 0;
            for l in labels {
                if placed >= 16 {
                    break;
                }
                if l.x < inner.left() + 14.0 || l.x > inner.right() - 14.0 {
                    continue;
                }
                let g1 = painter.layout_no_wrap(l.title, t.semibold(11.0), Color32::WHITE);
                let g2 = painter.layout_no_wrap(l.sub, t.font(10.0), Color32::from_white_alpha(190));
                let w = g1.size().x.max(g2.size().x) + 14.0;
                let span = (l.x - w / 2.0 - 3.0, l.x + w / 2.0 + 3.0);
                let Some(row) = rows.iter().position(|r| r.iter().all(|(a, b)| span.1 < *a || span.0 > *b)) else { continue };
                let cy = l.y - 26.0 - row as f32 * 34.0;
                if cy - 15.0 < body.top() + 2.0 {
                    continue;
                }
                if let Some(r) = rows.get_mut(row) {
                    r.push(span);
                }
                placed += 1;
                let chip = Rect::from_center_size(pos2(l.x, cy), vec2(w, 30.0));
                painter.line_segment([pos2(l.x, l.y - 2.0), pos2(l.x, chip.bottom())], Stroke::new(1.0, Color32::from_white_alpha(120)));
                painter.rect_filled(chip, 6.0, Color32::from_black_alpha(140));
                painter.galley(pos2(chip.center().x - g1.size().x / 2.0, chip.top() + 2.0), g1, Color32::WHITE);
                painter.galley(pos2(chip.center().x - g2.size().x / 2.0, chip.top() + 15.0), g2, Color32::WHITE);
            }
            // the credits
            let fine = sky.as_ref().is_some_and(|h| h.fine);
            let mut credit = String::from(if fine { "Terrain: AWS Terrain Tiles" } else { "Terrain: Copernicus DEM (Open-Meteo)" });
            if !names.is_empty() {
                credit.push_str(" · Names: © OpenStreetMap contributors");
            } else if let Some(why) = sky.as_ref().and_then(|h| h.names_error.as_deref()) {
                credit.push_str(&format!(" · {}: {why}", crate::i18n::tr("Summit names unavailable")));
            }
            let g = painter.layout_no_wrap(credit, t.font(9.5), Color32::from_white_alpha(150));
            painter.galley(pos2(inner.left() + 8.0, inner.bottom() - g.size().y - 3.0), g, Color32::WHITE);
        }

        // the target, if there is one
        if let Some(s) = plans::sight_of(st, None) {
            let x = x_of(s.azimuth);
            if inner.x_range().contains(x) {
                let y = y_of(s.altitude);
                painter.add(Shape::convex_polygon(
                    vec![pos2(x, y), pos2(x - 5.0, y - 9.0), pos2(x + 5.0, y - 9.0)],
                    t.accent,
                    Stroke::new(1.0, Color32::WHITE),
                ));
            }
        }

        // the bodies at the time shown (hollow when the mountains hide them)
        let bodies = [(0usize, self.skies.sun, pal.sun, 8.0f32), (1, self.skies.moon, pal.moon, 7.0), (2, self.skies.core, pal.core, 6.0)];
        for (i, h, color, rad) in bodies {
            if !st.layers[i] || h.alt < -4.0 {
                continue;
            }
            let p = pos2(x_of(h.az), y_of(h.alt));
            if !inner.expand(rad).contains(p) {
                continue;
            }
            let hidden = data.as_ref().is_some_and(|s| h.alt < s.altitude_at(h.az));
            if hidden {
                painter.circle_stroke(p, rad, Stroke::new(1.8, color));
            } else {
                painter.circle_filled(p, rad + 4.0, color.gamma_multiply(0.25));
                painter.circle_filled(p, rad, color);
            }
        }

        // the compass strip
        let cs = Rect::from_min_max(pos2(body.left(), body.bottom() - COMPASS_H), body.right_bottom());
        painter.rect_filled(cs, egui::CornerRadius { nw: 0, ne: 0, sw: R_CARD as u8, se: R_CARD as u8 }, Color32::from_black_alpha(150));
        let first = (f64::from(centre - fov / 2.0) / 15.0).floor() as i32 - 1;
        let last = (f64::from(centre + fov / 2.0) / 15.0).ceil() as i32 + 1;
        for k in first..=last {
            let az = f64::from(k) * 15.0;
            let x = x_of(az);
            if !inner.x_range().contains(x) {
                continue;
            }
            let az360 = az.rem_euclid(360.0);
            let main = (az360 % 45.0).abs() < 1e-6;
            painter.line_segment(
                [pos2(x, cs.top()), pos2(x, cs.top() + if main { 7.0 } else { 4.0 })],
                Stroke::new(1.0, Color32::from_white_alpha(if main { 200 } else { 110 })),
            );
            if main {
                let label = if az360 % 90.0 == 0.0 { crate::i18n::tr_ctx("compass", compass(az360)).to_string() } else { format!("{az360:.0}°") };
                painter.text(pos2(x, cs.bottom() - 6.0), Align2::CENTER_CENTER, label, t.semibold(10.5), Color32::WHITE);
            }
        }

        // interaction: drag to turn, wheel to zoom, a double click to look at the Sun
        let resp = ui.interact(inner, egui::Id::new("planner-horizon"), Sense::click_and_drag());
        let mut new_az = centre;
        let mut new_fov = fov;
        if resp.dragged() {
            new_az = (centre - resp.drag_delta().x / ppd).rem_euclid(360.0);
        }
        if resp.hovered() {
            let (scroll, zoom) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta()));
            new_fov = (fov * (-scroll.y * 0.004).exp() / zoom).clamp(MIN_FOV, 360.0);
            if scroll.x.abs() > 0.0 {
                new_az = (new_az - scroll.x / ppd).rem_euclid(360.0);
            }
        }
        if resp.double_clicked() {
            new_az = self.skies.sun.az as f32;
        }
        rt.pano_az = Some(new_az);
        rt.pano_fov = new_fov;
        // what is under the pointer
        let mut readout = String::new();
        if resp.hovered()
            && let (Some(pos), Some(s)) = (resp.hover_pos(), &data)
        {
            let az = az_at(pos.x);
            let i = s.index_at(az);
            readout = format!(
                "{} {}  ·  {:.1}°  ·  {:.0} m {} {}",
                az_text(az),
                crate::i18n::tr_ctx("compass", compass(az)),
                s.altitude_at(az),
                s.height.get(i).copied().unwrap_or(0.0),
                crate::i18n::tr("at"),
                plans::distance_label(s.dist.get(i).copied().unwrap_or(0.0))
            );
        }

        // the card's frame and header
        painter.rect_stroke(r, R_CARD, Stroke::new(1.0, t.card_stroke), StrokeKind::Inside);
        let mut hui = ui.new_child(egui::UiBuilder::new().max_rect(head.shrink2(vec2(12.0, 3.0))).layout(egui::Layout::left_to_right(Align::Center)));
        hui.spacing_mut().item_spacing.x = 6.0;
        hui.label(egui::RichText::new(crate::i18n::tr("Mountains")).font(t.semibold(13.0)).color(t.text));
        for (id, label, az) in [
            ("plannerHorizonSun", "Sun", self.skies.sun.az),
            ("plannerHorizonMoon", "Moon", self.skies.moon.az),
            ("plannerHorizonCore", "Milky Way", self.skies.core.az),
        ] {
            if text_button(&mut hui, id, label, false).clicked() {
                rt.pano_az = Some(az as f32);
            }
        }
        if let Some(s) = plans::sight_of(st, None)
            && text_button(&mut hui, "plannerHorizonTarget", "Target", false).clicked()
        {
            rt.pano_az = Some(s.azimuth as f32);
        }
        let mut right =
            ui.new_child(egui::UiBuilder::new().max_rect(head.shrink2(vec2(12.0, 3.0))).layout(egui::Layout::right_to_left(Align::Center)));
        right.spacing_mut().item_spacing.x = 6.0;
        if icon_button(&mut right, "plannerHorizonClose", Icon::Minus, vec2(24.0, 24.0), false, true, "Hide the Mountains").clicked() {
            st.show_horizon = false;
        }
        let stretch = if sy > ppd * 1.05 { format!("  ·  ×{:.1}", sy / ppd) } else { String::new() };
        let info = if readout.is_empty() { format!("{fov:.0}°{stretch}") } else { readout };
        right.label(egui::RichText::new(info).font(t.font(11.5)).color(t.text_dim));

        // while it is being looked up, or if it could not be
        let centre_text = match sky.as_ref().map(|h| &h.data) {
            Some(Err(e)) => Some(format!("{}: {e}", crate::i18n::tr("Could not look up the ground heights"))),
            None => Some(crate::i18n::tr("Looking up the ground heights…").to_string()),
            Some(Ok(_)) => None,
        };
        if let Some(msg) = centre_text {
            let chip = Rect::from_center_size(inner.center(), vec2((inner.width() - 40.0).min(520.0), 34.0));
            painter.rect_filled(chip, 10.0, Color32::from_black_alpha(150));
            painter.text(chip.center(), Align2::CENTER_CENTER, msg, t.font(12.0), Color32::WHITE);
            if matches!(sky.as_ref().map(|h| &h.data), Some(Err(_))) {
                let retry = Rect::from_center_size(inner.center() + vec2(0.0, 40.0), vec2(90.0, 28.0));
                let mut rui = ui.new_child(egui::UiBuilder::new().max_rect(retry));
                if text_button(&mut rui, "plannerHorizonRetry", "Try Again", false).clicked() {
                    rt.horizon = None;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_three_new_layers_start_off_are_kept_and_old_settings_still_load() {
        let st = PlannerState::default();
        assert!(!st.show_horizon && !st.show_shadows && !st.show_light);
        let on = PlannerState { show_horizon: true, show_shadows: true, show_light: true, ..Default::default() };
        let back: PlannerState = serde_json::from_str(&serde_json::to_string(&on).unwrap()).unwrap();
        assert!(back.show_horizon && back.show_shadows && back.show_light);
        let old: PlannerState = serde_json::from_str(r#"{"lat":43.0,"lon":-2.0}"#).unwrap();
        assert!(!old.show_horizon && !old.show_shadows && !old.show_light);
    }

    #[test]
    fn the_skyline_fills_the_free_part_beside_the_cards() {
        for (w, h) in [(300.0, 200.0), (1100.0, 700.0), (50.0, 30.0)] {
            let free = Rect::from_min_size(pos2(10.0, 10.0), vec2(w, h));
            let r = view_rect(free);
            assert!(free.contains_rect(r) || r.height() <= 0.0, "{w}×{h}: {r:?}");
            assert!(r.top() >= free.top() + 44.0 - 1e-3 || r.height() <= 0.0, "{w}×{h}: {r:?}");
            assert!(
                r.height() <= 0.0 || ((r.right() - (free.right() - 8.0)).abs() < 1e-3 && (r.bottom() - (free.bottom() - 8.0)).abs() < 1e-3),
                "{w}×{h}: {r:?}"
            );
        }
    }

    #[test]
    fn the_skyline_key_follows_the_place() {
        let a = PlannerState { lat: 43.0, lon: -2.0, ..Default::default() };
        let b = PlannerState { lat: 43.0, lon: -2.1, ..Default::default() };
        assert_eq!(key_of(&a), key_of(&a.clone()));
        assert_ne!(key_of(&a), key_of(&b));
    }
}
