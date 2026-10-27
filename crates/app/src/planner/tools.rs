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
        match optics::timelapse(st.tl_event_h * 3600.0, st.tl_clip, st.tl_fps) {
            Some((photos, interval)) => {
                row(ui, "tl:photos", "Photos", &photos.to_string(), None);
                row(ui, "tl:interval", "Interval", &seconds_text(interval), None);
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
