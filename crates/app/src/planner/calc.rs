//! The rest of the photographer's calculators: exposure (the exposure value and the settings that give
//! the same light), depth of field with a circle of confusion worked out from the print, the inverse
//! (what range do I want sharp?), the depth-of-field and hyperfocal tables, stars as points, how far to
//! stand from a subject, and what a focal length is on another sensor.

use light_studio_astro::optics::{self, LIGHTS, SENSORS};

use super::tools::{shutter_field, shutter_text};
use super::*;

/// A table: a heading row and rows of text, the first column the row's name.
fn grid(ui: &mut Ui, _id: &str, head: &[String], rows: &[(String, Vec<String>)]) {
    let t = Tokens::get(ui.ctx());
    let cols = head.len() + 1;
    let w = ui.available_width();
    let first = 52.0_f32.min(w / 3.0);
    let cw = (w - first) / head.len().max(1) as f32;
    let (hr, _) = ui.allocate_exact_size(vec2(w, 20.0), Sense::hover());
    for (i, h) in head.iter().enumerate() {
        ui.painter().text(pos2(hr.left() + first + cw * (i as f32 + 0.5), hr.center().y), Align2::CENTER_CENTER, h, t.semibold(10.5), t.text_label);
    }
    ui.painter().line_segment([hr.left_bottom(), hr.right_bottom()], Stroke::new(1.0, t.divider));
    for (n, (name, cells)) in rows.iter().enumerate() {
        let (r, _) = ui.allocate_exact_size(vec2(w, 20.0), Sense::hover());
        if n % 2 == 1 {
            ui.painter().rect_filled(r, 3.0, t.inset.gamma_multiply(0.6));
        }
        ui.painter().text(pos2(r.left() + 3.0, r.center().y), Align2::LEFT_CENTER, name, t.semibold(10.5), t.text_dim);
        for (i, text) in cells.iter().enumerate().take(cols - 1) {
            ui.painter().text(pos2(r.left() + first + cw * (i as f32 + 0.5), r.center().y), Align2::CENTER_CENTER, text, t.font(10.0), t.text);
        }
    }
}

/// Metres in the shortest useful form.
fn metres(mm: f64) -> String {
    if !mm.is_finite() {
        return "∞".into();
    }
    let m = mm / 1000.0;
    if m < 10.0 {
        format!("{m:.2}")
    } else if m < 100.0 {
        format!("{m:.1}")
    } else {
        format!("{m:.0}")
    }
}

impl Page<'_> {
    /// The exposure value and the same light at other settings.
    pub(super) fn calc_exposure(&mut self, ui: &mut Ui, st: &mut PlannerState, rt: &mut Runtime) {
        let t = self.t;
        // ---- exposure -----------------------------------------------------------------------
        subhead(ui, "Exposure");
        ui.horizontal(|ui| {
            ui.allocate_ui_with_layout(vec2(150.0, 26.0), egui::Layout::left_to_right(Align::Center), |ui| {
                ui.label(egui::RichText::new(crate::i18n::tr("Shutter")).font(t.font(12.5)).color(t.text_dim));
            });
            shutter_field(ui, rt, "exShutter", &mut st.nd_base, 78.0);
        });
        labelled_number(ui, rt, "exIso", "ISO", &mut st.iso, (6.0, 6_400_000.0), 0);
        note(ui, &format!("{} f/{:.1}", crate::i18n::tr("Aperture:"), st.aperture));
        match optics::exposure_value(st.aperture, st.nd_base, st.iso) {
            Some(ev) => {
                tiles(ui, &[("ex:ev", "Exposure Value (ISO 100)", format!("{ev:.1}"), None)]);
                let stops = [1.4, 2.0, 2.8, 4.0, 5.6, 8.0, 11.0, 16.0];
                let rows: Vec<(String, Vec<String>)> = stops
                    .iter()
                    .map(|&f| {
                        let cells = [100.0, 400.0, 1600.0, 6400.0]
                            .iter()
                            .map(|&iso| optics::shutter_for_ev(f, iso, ev).map_or_else(|| "—".to_string(), shutter_text))
                            .collect();
                        (format!("f/{f}"), cells)
                    })
                    .collect();
                ui.add_space(4.0);
                note(ui, crate::i18n::tr("The same light at other settings (shutter by aperture and ISO):"));
                grid(ui, "exgrid", &["100".into(), "400".into(), "1600".into(), "6400".into()], &rows);
            }
            None => note(ui, crate::i18n::tr("Those numbers do not make an exposure.")),
        }
    }

    /// What each kind of light asks for, at this aperture and ISO.
    pub(super) fn calc_by_light(&mut self, ui: &mut Ui, st: &mut PlannerState, _rt: &mut Runtime) {
        let t = self.t;
        subhead(ui, "Exposure by light");
        let rows: Vec<(String, Vec<String>)> = LIGHTS
            .iter()
            .map(|(name, lev)| {
                (
                    String::new(),
                    vec![
                        crate::i18n::tr(name).to_string(),
                        format!("{lev:+.0}"),
                        optics::shutter_for_ev(st.aperture, st.iso, *lev).map_or_else(|| "—".to_string(), shutter_text),
                    ],
                )
            })
            .collect();
        for (_, cells) in rows.iter() {
            if let [name, lev, shutter] = &cells[..] {
                let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::hover());
                ui.painter().text(pos2(r.left() + 2.0, r.center().y), Align2::LEFT_CENTER, name, t.font(11.5), t.text_dim);
                ui.painter().text(pos2(r.right() - 80.0, r.center().y), Align2::RIGHT_CENTER, format!("EV {lev}"), t.font(11.0), t.text_label);
                ui.painter().text(pos2(r.right(), r.center().y), Align2::RIGHT_CENTER, shutter, t.font(11.5), t.text);
            }
        }
    }

    /// Depth of field from the print, and its inverse.
    pub(super) fn calc_dof(&mut self, ui: &mut Ui, st: &mut PlannerState, rt: &mut Runtime) {
        let sensor = SENSORS.get(st.sensor).copied().unwrap_or(SENSORS[0]);
        subhead(ui, "Depth of field from the print");
        labelled_number(ui, rt, "printCm", "Print diagonal (cm)", &mut st.print_cm, (2.0, 2000.0), 0);
        labelled_number(ui, rt, "viewCm", "Viewing distance (cm)", &mut st.view_cm, (5.0, 10_000.0), 0);
        let diag = sensor.width.hypot(sensor.height);
        match optics::coc_for_print(diag, st.print_cm * 10.0, st.view_cm * 10.0, 1.0) {
            Some(coc) => {
                let mut out = vec![("dof:coc", "Circle of Confusion", format!("{:.1} µm", coc * 1000.0), None)];
                match optics::depth_of_field(st.focal, st.aperture, coc, st.distance * 1000.0) {
                    Some(d) => {
                        out.push(("dof:hyper", "Hyperfocal Distance", distance_text(d.hyperfocal), None));
                        out.push(("dof:near", "Sharp From", distance_text(d.near), None));
                        out.push(("dof:far", "Sharp To", d.far.map_or_else(|| "∞".to_string(), distance_text), None));
                        let front = st.distance * 1000.0 - d.near;
                        let behind = d.far.map(|f| f - st.distance * 1000.0);
                        if let Some(total) = d.total().filter(|t| *t > 0.0) {
                            out.push(("dof:front", "In Front", format!("{} ({:.0} %)", distance_text(front), 100.0 * front / total), None));
                            out.push((
                                "dof:behind",
                                "Behind",
                                behind.map_or_else(|| "∞".to_string(), |b| format!("{} ({:.0} %)", distance_text(b), 100.0 * b / total)),
                                None,
                            ));
                        }
                        tiles(ui, &out);
                    }
                    None => note(ui, crate::i18n::tr("Focus must be farther than the focal length.")),
                }
            }
            None => note(ui, crate::i18n::tr("Those sizes do not make a circle of confusion.")),
        }

        subhead(ui, "Inverse: what range must be sharp?");
        labelled_number(ui, rt, "dofNear", "Nearest sharp (m)", &mut st.dof_near, (0.1, 100_000.0), 2);
        labelled_number(ui, rt, "dofFar", "Farthest sharp (m)", &mut st.dof_far, (0.2, 1_000_000.0), 2);
        match optics::inverse_depth_of_field(st.focal, sensor.coc, st.dof_near * 1000.0, st.dof_far * 1000.0) {
            Some((n, d)) => {
                tiles(ui, &[("inv:aperture", "Aperture", format!("f/{n:.1}"), None), ("inv:distance", "Focus At", distance_text(d), None)]);
                if ui.is_enabled() && text_button(ui, "plannerInvUse", "Use These Settings", false).clicked() {
                    st.aperture = n.clamp(0.7, 64.0);
                    st.distance = (d / 1000.0).clamp(0.05, 100_000.0);
                }
            }
            None => note(ui, crate::i18n::tr("The far limit must be beyond the near one.")),
        }
    }

    /// The depth-of-field and hyperfocal tables.
    pub(super) fn calc_tables(&mut self, ui: &mut Ui, st: &mut PlannerState, _rt: &mut Runtime) {
        let sensor = SENSORS.get(st.sensor).copied().unwrap_or(SENSORS[0]);
        subhead(ui, "Depth of field table (metres)");
        let aps = [2.8, 4.0, 5.6, 8.0, 11.0];
        let dists = [0.5, 1.0, 2.0, 3.0, 5.0, 10.0, 20.0];
        let rows: Vec<(String, Vec<String>)> = dists
            .iter()
            .map(|&d| {
                let cells = aps
                    .iter()
                    .map(|&f| match optics::depth_of_field(st.focal, f, sensor.coc, d * 1000.0) {
                        Some(x) => format!("{}–{}", metres(x.near), x.far.map_or_else(|| "∞".to_string(), metres)),
                        None => "—".to_string(),
                    })
                    .collect();
                (format!("{d} m"), cells)
            })
            .collect();
        grid(ui, "dofgrid", &aps.iter().map(|f| format!("f/{f}")).collect::<Vec<_>>(), &rows);
        note(ui, &format!("{} {:.0} mm.", crate::i18n::tr("For the focal length"), st.focal));

        subhead(ui, "Hyperfocal table (metres)");
        let focals = [14.0, 20.0, 24.0, 35.0, 50.0, 85.0];
        let rows: Vec<(String, Vec<String>)> = focals
            .iter()
            .map(|&fl| {
                let cells = aps.iter().map(|&f| optics::hyperfocal(fl, f, sensor.coc).map_or_else(|| "—".to_string(), metres)).collect();
                (format!("{fl} mm"), cells)
            })
            .collect();
        grid(ui, "hypgrid", &aps.iter().map(|f| format!("f/{f}")).collect::<Vec<_>>(), &rows);
    }

    /// Stars as points.
    pub(super) fn calc_points(&mut self, ui: &mut Ui, st: &mut PlannerState, rt: &mut Runtime) {
        let sensor = SENSORS.get(st.sensor).copied().unwrap_or(SENSORS[0]);
        let pitch = sensor.pixel_pitch_um(st.megapixels);
        subhead(ui, "Stars as points");
        labelled_number(ui, rt, "starsPx", "Smear allowed (px)", &mut st.stars_px, (0.1, 50.0), 1);
        let dec_core = light_studio_astro::sky::precess(
            light_studio_astro::sky::CORE_RA_J2000,
            light_studio_astro::sky::CORE_DEC_J2000,
            light_studio_astro::time::jd_from_unix(self.c.start),
        )
        .1;
        let mut out: Vec<(&str, &str, String, Option<Color32>)> = Vec::new();
        for (id, name, dec) in
            [("eq", "At the Celestial Equator", 0.0), ("core", "At the Milky Way Core", dec_core), ("pole", "Near the Pole Star", 80.0)]
        {
            let s = optics::stars_as_points_seconds(pitch, st.focal, dec, st.stars_px);
            out.push((id, name, s.map_or_else(|| "—".to_string(), seconds_text), None));
        }
        let ids: Vec<String> = out.iter().map(|(id, ..)| format!("points:{id}")).collect();
        let items: Vec<(&str, &str, String, Option<Color32>)> =
            out.iter().zip(&ids).map(|((_, n, v, d), id)| (id.as_str(), *n, v.clone(), *d)).collect();
        tiles(ui, &items);
    }

    /// How far to stand from a subject.
    pub(super) fn calc_subject(&mut self, ui: &mut Ui, st: &mut PlannerState, rt: &mut Runtime) {
        let sensor = SENSORS.get(st.sensor).copied().unwrap_or(SENSORS[0]);
        subhead(ui, "Distance to a subject");
        labelled_number(ui, rt, "subjectM", "Subject size (m)", &mut st.subject_m, (0.001, 10_000.0), 2);
        let dist = |dim: f64| optics::subject_distance(st.focal, st.subject_m * 1000.0, dim).map_or_else(|| "—".to_string(), distance_text);
        tiles(
            ui,
            &[
                ("subject:long", "Filling the Long Side", dist(sensor.width), None),
                ("subject:short", "Filling the Short Side", dist(sensor.height), None),
            ],
        );
    }

    /// The Moon against a subject.
    pub(super) fn calc_moon(&mut self, ui: &mut Ui, st: &mut PlannerState, _rt: &mut Runtime) {
        let sensor = SENSORS.get(st.sensor).copied().unwrap_or(SENSORS[0]);
        subhead(ui, "The Moon and a subject");
        let (moon_deg, _) = light_studio_astro::info::moon_size(self.c.start + self.minute * 60.0);
        let vfov = optics::field_of_view(if st.portrait { sensor.width } else { sensor.height }, st.focal).unwrap_or(0.0);
        if vfov > 0.0 {
            tiles(
                ui,
                &[(
                    "moon:frame",
                    "Moon in the Frame",
                    format!("{:.1} % {}", 100.0 * moon_deg / vfov, crate::i18n::tr("of the height")),
                    Some(self.pal.moon),
                )],
            );
        }
        let d = st.subject_m / moon_deg.to_radians().tan().max(1e-9);
        tiles(ui, &[("moon:distance", "Subject as Big as the Moon at", distance_text(d * 1000.0), None)]);
        note(ui, crate::i18n::tr("The distance at which the subject above covers the same angle as the Moon."));
    }

    /// A focal length on another sensor.
    pub(super) fn calc_equiv(&mut self, ui: &mut Ui, st: &mut PlannerState, _rt: &mut Runtime) {
        let sensor = SENSORS.get(st.sensor).copied().unwrap_or(SENSORS[0]);
        subhead(ui, "Focal length on another sensor");
        let names: Vec<(&str, String)> = SENSORS.iter().enumerate().map(|(n, s)| (s.name, format!("{n}"))).collect();
        let items: Vec<(&str, &str)> = names.iter().map(|(label, key)| (*label, key.as_str())).collect();
        if let Some(n) = crate::widgets::segmented(ui, "plannerToSensor", &items, Some(st.to_sensor), 3) {
            st.to_sensor = n;
        }
        let to = SENSORS.get(st.to_sensor).copied().unwrap_or(SENSORS[0]);
        tiles(
            ui,
            &[
                (
                    "equiv:focal",
                    "Equivalent Focal Length",
                    optics::equivalent_focal(st.focal, &sensor, &to).map_or_else(|| "—".to_string(), |f| format!("{f:.0} mm")),
                    None,
                ),
                ("equiv:aperture", "Equivalent Aperture", format!("f/{:.1}", st.aperture * to.crop() / sensor.crop()), None),
            ],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::metres;

    #[test]
    fn distances_are_written_in_metres_with_the_digits_that_matter() {
        assert_eq!(metres(1500.0), "1.50");
        assert_eq!(metres(9990.0), "9.99");
        assert_eq!(metres(25_000.0), "25.0");
        assert_eq!(metres(250_000.0), "250");
        assert_eq!(metres(f64::INFINITY), "∞");
        assert_eq!(metres(f64::NAN), "∞");
    }
}
