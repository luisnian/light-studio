//! Line Up: a target on the map (a lighthouse, a peak, a chapel), the sight line from the place to it with the real
//! ground heights (Open-Meteo, as Luis's «Mapa solar»), the terrain profile between them (does a hill hide it?), the
//! target's shadow at the time shown, and the days the Sun or the Moon stand right behind it.

use light_studio_astro::align::{self, Alignment, Body, Profile};

use super::*;

/// How far ahead the alignment search looks (days), and how many ground samples the profile has.
const ALIGN_DAYS: u32 = 365;
pub(super) const PROFILE_SAMPLES: usize = 91;

/// Which camera and target the ground heights belong to (latitude and longitude bits of both).
pub(super) type TerrainKey = (u64, u64, u64, u64);

/// The ground between the place and the target: `PROFILE_SAMPLES` heights from the camera to the target.
#[derive(Clone, Debug)]
pub(super) struct Terrain {
    pub key: TerrainKey,
    pub ground: Result<Vec<f64>, String>,
}

pub(super) fn terrain_key(st: &PlannerState) -> Option<TerrainKey> {
    st.target.map(|[la, lo]| (st.lat.to_bits(), st.lon.to_bits(), la.to_bits(), lo.to_bits()))
}

/// The points the profile samples, from the place to the target.
pub(super) fn profile_points(st: &PlannerState) -> Vec<(f64, f64)> {
    let Some([la, lo]) = st.target else { return Vec::new() };
    let mut dlon = lo - st.lon;
    dlon -= (dlon / 360.0).round() * 360.0;
    (0..PROFILE_SAMPLES)
        .map(|i| {
            let k = i as f64 / (PROFILE_SAMPLES - 1) as f64;
            (st.lat + (la - st.lat) * k, (st.lon + dlon * k + 540.0).rem_euclid(360.0) - 180.0)
        })
        .collect()
}

/// The ground heights of the place and the target, when known for this pair.
pub(super) fn ground_ends(st: &PlannerState, rt: &Runtime) -> Option<(f64, f64)> {
    let key = terrain_key(st)?;
    let t = rt.terrain.as_ref().filter(|t| t.key == key)?;
    let g = t.ground.as_ref().ok()?;
    Some((*g.first()?, *g.last()?))
}

/// Starts the ground heights for the place and the target when they changed (on a worker thread; the answer
/// lands in `app.planner.terrain`).
pub(super) fn fetch_terrain(app: &mut App) {
    let st = &app.ui.planner;
    let Some(key) = terrain_key(st) else { return };
    if app.planner.terrain.as_ref().is_some_and(|t| t.key == key) || app.planner.terrain_pending == Some(key) {
        return;
    }
    let points = profile_points(st);
    app.planner.terrain_pending = Some(key);
    let started = crate::tasks::spawn(
        app,
        "Terrain profile",
        Some("download"),
        move || crate::services::elevation::fetch(&points),
        move |app, _ctx, ground| {
            if app.planner.terrain_pending == Some(key) {
                app.planner.terrain_pending = None;
            }
            app.planner.terrain = Some(Terrain { key, ground });
        },
    );
    if let Err(e) = started {
        app.planner.terrain_pending = None;
        app.planner.terrain = Some(Terrain { key, ground: Err(e) });
    }
}

impl Page<'_> {
    pub(super) fn align_card(&mut self, ui: &mut Ui, st: &mut PlannerState, rt: &mut Runtime) {
        let t = self.t;
        let pal = self.pal;
        let c = self.c;
        let ground = ground_ends(st, rt);
        let Some(s) = plans::sight_of(st, ground) else {
            note(
                ui,
                crate::i18n::tr(
                    "Right-click the map (or turn on the target button and click) to choose what to line the Sun or the Moon up with: a lighthouse, a peak, a tower.",
                ),
            );
            return;
        };
        let key = terrain_key(st);
        let pending = key.is_some() && rt.terrain_pending == key;
        let terrain = rt.terrain.as_ref().filter(|t| Some(t.key) == key).cloned();
        let height_text = |h: Option<f64>| match h {
            Some(h) => format!("{h:.0} m"),
            None if pending => crate::i18n::tr("looking up…").to_string(),
            None => "—".to_string(),
        };
        subhead(ui, "From the Camera to the Target");
        row(ui, "align:distance", "Distance", &plans::distance_label(s.distance), None);
        row(ui, "align:azimuth", "Direction", &az_text(s.azimuth), None);
        row(ui, "align:groundCamera", "Ground at the camera", &height_text(ground.map(|g| g.0)), None);
        row(ui, "align:groundTarget", "Ground at the target", &height_text(ground.map(|g| g.1)), None);
        if labelled_number(ui, rt, "targetHeight", "Object height (m)", &mut st.target_height, (0.0, 9000.0), 0) {
            rt.align = None;
        }
        row(ui, "align:altitude", "Angle to the top", &alt_text(s.altitude), None);
        // the target's shadow at the time shown
        let at = c.start + self.minute * 60.0;
        let sun = light_studio_astro::sun_horizontal(at, self.obs);
        let shadow = match align::shadow(st.target_height, sun) {
            Some((len, az)) => format!("{} → {}", plans::distance_label(len), crate::i18n::tr_ctx("compass", compass(az))),
            None if st.target_height <= 0.0 => crate::i18n::tr("give the object a height").to_string(),
            None => crate::i18n::tr("no sun").to_string(),
        };
        row(ui, "align:shadow", "Object's shadow", &shadow, Some(Color32::from_gray(125)));
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            if text_button(ui, "plannerSwapTarget", "Swap Camera and Target", false).clicked()
                && let Some([la, lo]) = st.target
            {
                let (cl, co) = (st.lat, st.lon);
                let keep = st.map;
                set_place(st, la, lo, String::new());
                st.map = keep;
                st.target = Some([cl, co]);
                rt.align = None;
            }
            if text_button(ui, "plannerClearTarget", "Clear Target", false).clicked() {
                st.target = None;
                rt.align = None;
            }
        });

        // the ground between
        subhead(ui, "Terrain Profile");
        match terrain.as_ref().map(|t| &t.ground) {
            Some(Ok(g)) => match align::profile(s.distance, g, st.target_height) {
                Some(p) => self.profile_chart(ui, &p, s.distance),
                None => note(ui, crate::i18n::tr("The place and the target are the same point.")),
            },
            Some(Err(e)) => {
                note(ui, &format!("{}: {e}", crate::i18n::tr("Could not look up the ground heights")));
                if text_button(ui, "plannerTerrainRetry", "Try Again", false).clicked() {
                    rt.terrain = None;
                }
            }
            None => note(
                ui,
                crate::i18n::tr(if pending {
                    "Looking up the ground heights…"
                } else {
                    "The ground heights are looked up when there is a target."
                }),
            ),
        }

        // where the bodies are now, and when they line up
        subhead(ui, "At the Time Shown");
        let off = |h: Horizontal| {
            let d = (h.az - s.azimuth + 540.0).rem_euclid(360.0) - 180.0;
            let alt = h.alt + align::refraction(h.alt);
            let side = if d < 0.0 { crate::i18n::tr("left") } else { crate::i18n::tr("right") };
            let up = alt - s.altitude;
            let vert = if up >= 0.0 { crate::i18n::tr("above") } else { crate::i18n::tr("below") };
            format!("{:.1}° {side} · {:.1}° {vert}", d.abs(), up.abs())
        };
        row(ui, "align:sunNow", "Sun now", &off(sun), Some(pal.sun));
        row(ui, "align:moonNow", "Moon now", &off(light_studio_astro::moon_horizontal(at, self.obs)), Some(pal.moon));
        subhead(ui, "Find the Alignment");
        let key = (st.lat.to_bits(), st.lon.to_bits(), s.azimuth.to_bits(), s.altitude.to_bits(), c.date.days());
        if text_button(ui, "plannerFindAlignments", "Find Dates", false).clicked() {
            let found = align::alignments(self.obs, s, c.start, ALIGN_DAYS);
            rt.align = Some((key, found));
        }
        let Some((k, found)) = rt.align.as_ref() else {
            note(ui, crate::i18n::tr("Find the days of the coming year when the Sun or the Moon stand right behind the target."));
            return;
        };
        if *k != key {
            note(ui, crate::i18n::tr("The target or the place moved: find the dates again."));
            return;
        }
        ui.add_space(4.0);
        if found.is_empty() {
            note(ui, crate::i18n::tr("Neither the Sun nor the Moon lines up with it in the coming year."));
            return;
        }
        let mut jump: Option<(Date, f64)> = None;
        for a in found.iter().take(60) {
            let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::click());
            if resp.hovered() {
                ui.painter().rect_filled(r, R_CONTROL, t.hover.gamma_multiply(0.5));
            }
            let col = if a.body == Body::Sun { pal.sun } else { pal.moon };
            ui.painter().circle_filled(pos2(r.left() + 8.0, r.center().y), 4.0, col);
            let d = Date::from_unix(a.unix, c.offset);
            ui.painter().text(
                pos2(r.left() + 20.0, r.center().y),
                Align2::LEFT_CENTER,
                format!("{}  {}", short_date(d), clock(a.unix, c.offset)),
                t.font(12.0),
                t.text,
            );
            ui.painter().text(pos2(r.right() - 4.0, r.center().y), Align2::RIGHT_CENTER, alignment_label(a), t.font(11.5), t.text_dim);
            if resp.clicked() {
                let minute = (a.unix - d.unix_midnight_utc() + f64::from(c.offset) * 60.0) / 60.0;
                jump = Some((d, minute));
            }
        }
        if let Some((d, m)) = jump {
            self.intents.push(Intent::JumpToTime(d, m));
        }
    }

    /// The ground (filled), the sight line from the eye to the target's top (dashed) and, if a hill is in the way,
    /// where.
    fn profile_chart(&mut self, ui: &mut Ui, p: &Profile, distance: f64) {
        let t = self.t;
        let w = ui.available_width();
        let (r, _) = ui.allocate_exact_size(vec2(w, 130.0), Sense::hover());
        let painter = ui.painter_at(r);
        painter.rect_filled(r, R_CONTROL, t.inset);
        let heights = p.points.iter().map(|q| q.1).chain([p.eye, p.top]);
        let (lo, hi) = heights.fold((f64::MAX, f64::MIN), |(a, b), h| (a.min(h), b.max(h)));
        let pad = ((hi - lo) * 0.12).max(5.0);
        let (lo, hi) = (lo - pad, hi + pad);
        let x = |d: f64| r.left() + (d / distance) as f32 * r.width();
        let y = |h: f64| r.bottom() - ((h - lo) / (hi - lo)) as f32 * r.height();
        // the ground, a column per sample
        let ground = Color32::from_rgb(104, 112, 86);
        for pair in p.points.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            painter.add(Shape::convex_polygon(
                vec![pos2(x(a.0), r.bottom()), pos2(x(a.0), y(a.1)), pos2(x(b.0), y(b.1)), pos2(x(b.0), r.bottom())],
                ground,
                Stroke::NONE,
            ));
        }
        let line: Vec<Pos2> = p.points.iter().map(|q| pos2(x(q.0), y(q.1))).collect();
        painter.add(Shape::line(line, Stroke::new(1.2, Color32::from_rgb(160, 168, 130))));
        painter.extend(Shape::dashed_line(&[pos2(x(0.0), y(p.eye)), pos2(x(distance), y(p.top))], Stroke::new(1.4, t.text), 5.0, 4.0));
        painter.circle_filled(pos2(x(0.0), y(p.eye)), 3.5, t.accent);
        painter.circle_filled(pos2(x(distance), y(p.top)), 3.5, t.text);
        if let Some((d, _)) = p.block {
            let h = p.points.iter().min_by(|a, b| (a.0 - d).abs().total_cmp(&(b.0 - d).abs())).map_or(p.eye, |q| q.1);
            painter.circle_filled(pos2(x(d), y(h)), 4.5, t.reject);
        }
        ui.horizontal(|ui| {
            let small = |s: String| egui::RichText::new(s).font(t.font(11.0)).color(t.text_dim);
            let first = p.points.first().map_or(0.0, |q| q.1);
            let last = p.points.last().map_or(0.0, |q| q.1);
            ui.label(small(format!("{} {first:.0} m", crate::i18n::tr("Camera"))));
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                ui.label(small(format!("{} {last:.0} m", crate::i18n::tr("Target"))));
                ui.add_space(12.0);
                ui.label(small(plans::distance_label(distance)));
            });
        });
        let verdict = match p.block {
            Some((d, over)) => crate::i18n::tr("The view is blocked {d} from the camera: the ground rises about {m} m above the sight line.")
                .replace("{d}", &plans::distance_label(d))
                .replace("{m}", &format!("{over:.0}")),
            None => crate::i18n::tr("Clear view: the ground does not cross the sight line to the target's top.").to_string(),
        };
        let color = if p.block.is_some() { t.reject } else { Color32::from_rgb(70, 190, 120) };
        ui.label(egui::RichText::new(verdict).font(t.semibold(12.0)).color(color));
        note(ui, crate::i18n::tr("With the Earth's curve; trees and buildings are not in the heights (about 90 m apart)."));
    }
}

fn alignment_label(a: &Alignment) -> String {
    let body =
        if a.body == Body::Sun { crate::i18n::tr("Sun").to_string() } else { format!("{} {:.0} %", crate::i18n::tr("Moon"), a.illumination * 100.0) };
    let pos = if a.above.abs() < 0.3 {
        crate::i18n::tr("on the top").to_string()
    } else if a.above > 0.0 {
        format!("{:.1}° {}", a.above, crate::i18n::tr("above"))
    } else {
        format!("{:.1}° {}", -a.above, crate::i18n::tr("below"))
    };
    format!("{body} · {pos}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_profile_samples_run_from_the_place_to_the_target_even_across_the_date_line() {
        let st = PlannerState { lat: 10.0, lon: 179.9, target: Some([10.2, -179.9]), ..Default::default() };
        let pts = profile_points(&st);
        assert_eq!(pts.len(), PROFILE_SAMPLES);
        assert!((pts[0].0 - 10.0).abs() < 1e-9 && (pts[0].1 - 179.9).abs() < 1e-9, "{:?}", pts[0]);
        let last = pts[PROFILE_SAMPLES - 1];
        assert!((last.0 - 10.2).abs() < 1e-9 && (last.1 + 179.9).abs() < 1e-9, "{last:?}");
        // the middle is on the date line, not on the far side of the world
        assert!(pts[45].1.abs() > 179.0, "{:?}", pts[45]);
        assert!(profile_points(&PlannerState::default()).is_empty());
    }

    #[test]
    fn ground_heights_count_only_for_their_own_place_and_target() {
        let mut st = PlannerState { lat: 43.0, lon: -2.0, target: Some([43.1, -2.1]), target_height: 20.0, ..Default::default() };
        let mut rt = Runtime::default();
        assert_eq!(ground_ends(&st, &rt), None);
        rt.terrain = Some(Terrain { key: terrain_key(&st).unwrap(), ground: Ok(vec![50.0, 10.0, 300.0]) });
        assert_eq!(ground_ends(&st, &rt), Some((50.0, 300.0)));
        // a 300 m hill with a 20 m tower seen from 50 m up is higher than the same tower on level ground
        let raised = plans::sight_of(&st, ground_ends(&st, &rt)).unwrap();
        let level = plans::sight_of(&st, None).unwrap();
        assert!(raised.altitude > level.altitude + 1.0);
        st.lat = 43.5;
        assert_eq!(ground_ends(&st, &rt), None, "the place moved");
    }
}
