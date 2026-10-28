//! Three small calculators that go with a night out: a time-lapse (how many photos, how often),
//! a neutral density filter (the longer shutter time it asks for) and star trails (how far the
//! stars smear in an exposure). They use the camera set in the Exposure card.

use light_studio_astro::sky::{CORE_DEC_J2000, CORE_RA_J2000, precess};
use light_studio_astro::time::jd_from_unix;

use super::*;

/// A shutter time as photographers write it: `1/125`, `2.5 s`.
pub(super) fn shutter_text(seconds: f64) -> String {
    if !(seconds.is_finite() && seconds > 0.0) {
        return "—".into();
    }
    // fractions read well below a third of a second; above it, tenths
    if seconds >= 0.3 {
        return seconds_text(seconds);
    }
    format!("1/{:.0}", 1.0 / seconds)
}

/// Reads `1/125`, `0.008`, `2.5`, `2.5s` or `30"` as seconds.
pub(super) fn parse_shutter(text: &str) -> Option<f64> {
    let t = text.trim().trim_end_matches(['s', 'S', '"']).trim().replace(',', ".");
    let seconds = match t.split_once('/') {
        Some((a, b)) => a.trim().parse::<f64>().ok()? / b.trim().parse::<f64>().ok()?,
        None => t.parse::<f64>().ok()?,
    };
    (seconds.is_finite() && seconds > 0.0).then_some(seconds)
}

/// A shutter time typed into a field: shown as `1/125`, parsed when the edit ends. True when it changed.
pub(super) fn shutter_field(ui: &mut Ui, rt: &mut Runtime, id: &str, value: &mut f64, width: f32) -> bool {
    let widget = format!("planner:{id}");
    let shown = shutter_text(*value);
    let buf = rt.fields.entry(widget.clone()).or_default();
    if !crate::text_field::was_editing(ui.ctx(), &widget) {
        *buf = shown.clone();
    }
    let r = TextField::singleline(&widget, buf).width(width).show(ui);
    let mut changed = false;
    if r.committed() {
        if let Some(v) = parse_shutter(buf) {
            let v = v.clamp(1.0 / 32_000.0, 3600.0);
            changed = (v - *value).abs() > f64::EPSILON;
            *value = v;
        }
        *buf = shutter_text(*value);
    } else if r.cancelled() {
        *buf = shown;
    }
    changed
}

impl Page<'_> {
    /// The time-lapse: photos and interval for a clip.
    pub(super) fn timelapse_card(&mut self, ui: &mut Ui, st: &mut PlannerState, rt: &mut Runtime) {
        let c = self.c;
        subhead(ui, "Time-lapse");
        labelled_number(ui, rt, "tlEvent", "Event Length (hours)", &mut st.tl_event_h, (0.01, 240.0), 2);
        labelled_number(ui, rt, "tlClip", "Clip Length (s)", &mut st.tl_clip, (0.1, 3600.0), 1);
        labelled_number(ui, rt, "tlFps", "Frame Rate (fps)", &mut st.tl_fps, (1.0, 240.0), 0);
        let t = self.t;
        ui.horizontal(|ui| {
            ui.allocate_ui_with_layout(vec2(150.0, 26.0), egui::Layout::left_to_right(Align::Center), |ui| {
                ui.label(egui::RichText::new(crate::i18n::tr("Exposure of Each Frame")).font(t.font(12.5)).color(t.text_dim));
            });
            shutter_field(ui, rt, "tlExposure", &mut st.tl_exposure, 78.0);
        });
        match optics::timelapse_plan(st.tl_event_h * 3600.0, st.tl_clip, st.tl_fps, st.tl_exposure) {
            Some(p) => {
                row(ui, "tl:photos", "Photos", &p.photos.to_string(), None);
                row(ui, "tl:interval", "Interval", &seconds_text(p.interval), None);
                row(ui, "tl:gap", "Rest between Frames", &if p.gap >= 0.0 { seconds_text(p.gap) } else { "—".to_string() }, None);
                if !p.feasible {
                    note(
                        ui,
                        crate::i18n::tr(
                            "The exposure does not fit in the interval (the camera needs about 1 s to save each frame): shorten the exposure or lengthen the clip.",
                        ),
                    );
                }
            }
            None => note(ui, crate::i18n::tr("The clip is shorter than a frame.")),
        }
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            if let Some(s) = c.plan.sun.golden_pm
                && text_button(ui, "plannerTlGolden", "Golden Hour", false)
                    .on_hover_text(crate::i18n::tr("Use the evening golden hour as the event"))
                    .clicked()
            {
                st.tl_event_h = (s.minutes() / 60.0).max(0.01);
            }
            if let Some(s) = c.night_core.first()
                && text_button(ui, "plannerTlCore", "Core Window", false)
                    .on_hover_text(crate::i18n::tr("Use the night's dark window with the core up as the event"))
                    .clicked()
            {
                st.tl_event_h = (s.minutes() / 60.0).max(0.01);
            }
        });
    }

    /// The grid for a panorama: the scene's size and the overlap against the camera set in the Exposure card.
    pub(super) fn panorama_card(&mut self, ui: &mut Ui, st: &mut PlannerState, rt: &mut Runtime) {
        subhead(ui, "Panorama");
        labelled_number(ui, rt, "panoW", "Scene Width (°)", &mut st.pano_w, (1.0, 360.0), 0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            for w in [90.0, 180.0, 270.0, 360.0] {
                if text_button(ui, &format!("plannerPanoW{w}"), &format!("{w:.0}°"), (st.pano_w - w).abs() < 0.5).clicked() {
                    st.pano_w = w;
                }
            }
        });
        labelled_number(ui, rt, "panoH", "Scene Height (°)", &mut st.pano_h, (1.0, 180.0), 0);
        labelled_number(ui, rt, "panoOverlap", "Overlap (%)", &mut st.pano_overlap, (5.0, 90.0), 0);
        if text_button(ui, "plannerPanoPortrait", "Camera in portrait", st.portrait).clicked() {
            st.portrait = !st.portrait;
        }
        ui.add_space(4.0);
        match self.panorama_plan(st) {
            Some(p) => {
                row(ui, "pano:shots", "Shots", &format!("{} × {} = {}", p.columns, p.rows, p.shots()), None);
                row(ui, "pano:frame", "One Frame Covers", &format!("{:.0}° × {:.0}°", p.fov_w, p.fov_h), None);
                row(ui, "pano:step", "Turn Between Shots", &format!("{:.1}° · {:.1}°", p.step_w, p.step_h), None);
                row(ui, "pano:covered", "The Grid Covers", &format!("{:.0}° × {:.0}°", p.covered_w, p.covered_h), None);
                note(
                    ui,
                    crate::i18n::tr(
                        "Turn the camera about the lens, not the body, or near objects will not line up. With wide lenses and 30 % overlap the stitch usually finds enough to join.",
                    ),
                );
                if p.shots() >= 200 {
                    note(ui, crate::i18n::tr("That is a very large number of shots: try a wider lens or less overlap."));
                }
            }
            None => note(ui, crate::i18n::tr("The camera needs a focal length and a sensor.")),
        }
    }

    /// The grid worked out for the camera of the Exposure card.
    fn panorama_plan(&self, st: &PlannerState) -> Option<optics::PanoramaPlan> {
        let sensor = SENSORS.get(st.sensor).copied().unwrap_or(SENSORS[0]);
        let (w, h) = if st.portrait { (sensor.height, sensor.width) } else { (sensor.width, sensor.height) };
        optics::panorama(w, h, st.focal, st.pano_w, st.pano_h, st.pano_overlap / 100.0)
    }

    /// The frames drawn as they overlap (a sketch of the grid, not of the picture).
    pub(super) fn panorama_diagram(&mut self, ui: &mut Ui, st: &PlannerState) {
        let t = self.t;
        let Some(p) = self.panorama_plan(st) else { return };
        // too many to draw one by one: the sketch would be a smear
        if p.columns > 24 || p.rows > 12 {
            return;
        }
        let w = ui.available_width().clamp(120.0, 520.0);
        let (cols, rows) = (f64::from(p.columns), f64::from(p.rows));
        let span_w = p.fov_w + (cols - 1.0) * p.step_w;
        let span_h = p.fov_h + (rows - 1.0) * p.step_h;
        let scale = f64::from(w) / span_w.max(1e-6);
        let h = ((span_h * scale) as f32).clamp(24.0, 360.0);
        let scale = scale.min(f64::from(h) / span_h.max(1e-6));
        let (area, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
        let origin = area.min;
        for r in 0..p.rows {
            for c in 0..p.columns {
                let x = f64::from(c) * p.step_w * scale;
                let y = f64::from(r) * p.step_h * scale;
                let rect = Rect::from_min_size(origin + vec2(x as f32, y as f32), vec2((p.fov_w * scale) as f32, (p.fov_h * scale) as f32));
                ui.painter().rect_filled(rect, 2.0, t.accent.gamma_multiply(0.14));
                ui.painter().rect_stroke(rect, 2.0, Stroke::new(1.0, t.accent.gamma_multiply(0.8)), StrokeKind::Inside);
            }
        }
    }

    /// Neutral density filters.
    pub(super) fn nd_card(&mut self, ui: &mut Ui, st: &mut PlannerState, rt: &mut Runtime) {
        let t = self.t;
        subhead(ui, "Neutral density");
        ui.horizontal(|ui| {
            ui.allocate_ui_with_layout(vec2(150.0, 26.0), egui::Layout::left_to_right(Align::Center), |ui| {
                ui.label(egui::RichText::new(crate::i18n::tr("Shutter without the Filter")).font(t.font(12.5)).color(t.text_dim));
            });
            shutter_field(ui, rt, "ndBase", &mut st.nd_base, 78.0);
        });
        labelled_number(ui, rt, "ndStops", "Filter (stops)", &mut st.nd_stops, (0.0, 30.0), 1);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            for (stops, label) in [(3.0, "ND8"), (6.0, "ND64"), (10.0, "ND1000"), (13.3, "ND10000"), (16.6, "ND100000")] {
                if text_button(ui, &format!("plannerNd{label}"), label, (st.nd_stops - stops).abs() < 0.05).clicked() {
                    st.nd_stops = stops;
                }
            }
        });
        ui.add_space(4.0);
        match optics::nd_shutter(st.nd_base, st.nd_stops) {
            Some(s) => {
                row(ui, "nd:shutter", "Shutter with the Filter", &shutter_text(s), None);
                row(ui, "nd:factor", "Light Cut", &format!("× {:.0}", 2f64.powf(st.nd_stops)), None);
                if s > optics::CAMERA_MAX_SHUTTER {
                    note(ui, crate::i18n::tr("Longer than 30 s: set Bulb and time it with a remote or the intervalometer."));
                }
                subhead(ui, "Exposure bracket");
                labelled_number(ui, rt, "bracketStep", "Step (stops)", &mut st.bracket_step, (0.1, 4.0), 1);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                    for n in [3_u32, 5, 7, 9] {
                        if text_button(ui, &format!("plannerBracket{n}"), &format!("{n}"), st.bracket_count == n).clicked() {
                            st.bracket_count = n;
                        }
                    }
                });
                if let Some(times) = optics::bracket(s, st.bracket_step, st.bracket_count) {
                    let line = times.iter().map(|x| shutter_text(*x)).collect::<Vec<_>>().join("  ·  ");
                    note(ui, &line);
                    let total: f64 = times.iter().sum();
                    row(ui, "nd:bracketTotal", "Time of the Bracket", &seconds_text(total), None);
                }
            }
            None => note(ui, crate::i18n::tr("That filter is out of range.")),
        }
    }

    /// Star trails by exposure.
    pub(super) fn trails_card(&mut self, ui: &mut Ui, st: &mut PlannerState, rt: &mut Runtime) {
        let c = self.c;
        subhead(ui, "Star trails");
        labelled_number(ui, rt, "trail", "Exposure (s)", &mut st.trail_s, (0.1, 7200.0), 1);
        let sensor = SENSORS.get(st.sensor).copied().unwrap_or(SENSORS[0]);
        let pitch = sensor.pixel_pitch_um(st.megapixels);
        let core_dec = precess(CORE_RA_J2000, CORE_DEC_J2000, jd_from_unix(c.start)).1;
        let trail =
            |dec: f64| optics::star_trail_pixels(st.trail_s, pitch, st.focal, dec).map_or_else(|| "—".to_string(), |px| format!("{px:.1} px"));
        row(ui, "trail:equator", "At the Celestial Equator", &trail(0.0), None);
        row(ui, "trail:core", "At the Milky Way Core", &trail(core_dec), Some(self.pal.core));
        row(ui, "trail:pole", "Near the Pole Star", &trail(80.0), None);
        let sharp = optics::npf_seconds(st.aperture, pitch, st.focal, 0.0).map_or_else(|| "—".to_string(), seconds_text);
        note(
            ui,
            &format!(
                "{} {:.0} mm, {:.2} µm. {} {sharp}.",
                crate::i18n::tr("Camera:"),
                st.focal,
                pitch,
                crate::i18n::tr("Stars stay sharp (about 2 px) up to")
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shutter_times_read_and_write_the_way_photographers_do() {
        assert_eq!(shutter_text(1.0 / 125.0), "1/125");
        assert_eq!(shutter_text(1.0 / 8000.0), "1/8000");
        assert_eq!(shutter_text(2.5), "2.5 s");
        assert_eq!(shutter_text(30.0), "30 s");
        assert_eq!(shutter_text(0.0), "—");
        assert_eq!(shutter_text(f64::NAN), "—");
        assert_eq!(parse_shutter("1/125"), Some(0.008));
        assert_eq!(parse_shutter(" 2,5 s "), Some(2.5));
        assert_eq!(parse_shutter("30\""), Some(30.0));
        assert_eq!(parse_shutter("1/0"), None, "a division by zero is not a time");
        assert_eq!(parse_shutter("0"), None);
        assert_eq!(parse_shutter("-3"), None);
        assert_eq!(parse_shutter("fast"), None);
    }

    #[test]
    fn a_shutter_survives_a_round_trip_through_its_text() {
        for s in [1.0 / 4000.0, 1.0 / 60.0, 0.4, 1.0, 13.0, 90.0] {
            let back = parse_shutter(&shutter_text(s)).expect("reads back");
            assert!((back / s - 1.0).abs() < 0.06, "{s} -> {} -> {back}", shutter_text(s));
        }
    }
}
