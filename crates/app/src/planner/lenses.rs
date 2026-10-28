//! The photographer's own lenses: a name, the focal range, the widest aperture and the nearest focus. Choosing one
//! keeps the camera inputs inside what that lens can really do (a 24 mm f/1.4 cannot be set to 35 mm f/1.0).

use super::*;

/// Most lenses kept, and the longest name.
const MAX_LENSES: usize = 30;
const MAX_NAME: usize = 40;

/// One lens (fixed focal length when the two focal lengths are equal).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Lens {
    pub name: String,
    /// Shortest and longest focal length, millimetres.
    pub focal_min: f64,
    pub focal_max: f64,
    /// The widest aperture (smallest f-number).
    pub aperture_max: f64,
    /// Nearest distance it focuses at, metres.
    pub min_focus: f64,
}

impl Default for Lens {
    fn default() -> Self {
        Lens { name: String::new(), focal_min: 24.0, focal_max: 70.0, aperture_max: 2.8, min_focus: 0.3 }
    }
}

impl Lens {
    /// Brings numbers read from a settings file (or typed) into range; the focal range comes out in order.
    pub fn sane(&mut self) {
        let d = Lens::default();
        let fix = |v: f64, lo: f64, hi: f64, dflt: f64| if v.is_finite() { v.clamp(lo, hi) } else { dflt };
        self.name = self.name.chars().take(MAX_NAME).collect();
        self.focal_min = fix(self.focal_min, 1.0, 2000.0, d.focal_min);
        self.focal_max = fix(self.focal_max, 1.0, 2000.0, d.focal_max);
        if self.focal_min > self.focal_max {
            std::mem::swap(&mut self.focal_min, &mut self.focal_max);
        }
        self.aperture_max = fix(self.aperture_max, 0.7, 64.0, d.aperture_max);
        self.min_focus = fix(self.min_focus, 0.01, 100_000.0, d.min_focus);
    }

    /// The focal length, aperture and focus distance (metres) brought inside what the lens can do.
    pub fn fit(&self, focal: f64, aperture: f64, distance: f64) -> (f64, f64, f64) {
        (focal.clamp(self.focal_min, self.focal_max), aperture.max(self.aperture_max), distance.max(self.min_focus))
    }

    /// "24–70 mm f/2.8".
    pub fn summary(&self) -> String {
        if (self.focal_max - self.focal_min).abs() < 0.5 {
            format!("{:.0} mm f/{}", self.focal_min, self.aperture_max)
        } else {
            format!("{:.0}–{:.0} mm f/{}", self.focal_min, self.focal_max, self.aperture_max)
        }
    }
}

impl PlannerState {
    /// Keeps the lenses list and the chosen one valid.
    pub(super) fn sane_lenses(&mut self) {
        self.lenses.truncate(MAX_LENSES);
        for l in &mut self.lenses {
            l.sane();
        }
        self.lens_draft.sane();
        if self.lens.is_some_and(|n| n >= self.lenses.len()) {
            self.lens = None;
        }
    }

    /// Brings the focal length, aperture and focus distance inside the chosen lens.
    pub(super) fn fit_to_lens(&mut self) {
        if let Some(l) = self.lens.and_then(|n| self.lenses.get(n)) {
            (self.focal, self.aperture, self.distance) = l.fit(self.focal, self.aperture, self.distance);
        }
    }
}

impl Page<'_> {
    /// The lens chips, and the form to add one.
    pub(super) fn lens_picker(&mut self, ui: &mut Ui, st: &mut PlannerState, rt: &mut Runtime) {
        if !st.lenses.is_empty() {
            let labels: Vec<(String, String)> =
                st.lenses.iter().enumerate().map(|(n, l)| (format!("{} · {}", l.name, l.summary()), format!("{n}"))).collect();
            let items: Vec<(&str, &str)> = labels.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
            if let Some(n) = crate::widgets::segmented(ui, "plannerLens", &items, st.lens, 1) {
                st.lens = Some(n);
                st.fit_to_lens();
            }
            if let Some(n) = st.lens {
                ui.horizontal(|ui| {
                    if text_button(ui, "plannerLensFree", "Any lens", false).clicked() {
                        st.lens = None;
                    }
                    if text_button(ui, "plannerLensDelete", "Delete lens", false).clicked() && n < st.lenses.len() {
                        st.lenses.remove(n);
                        st.lens = None;
                    }
                });
            }
        }
        ui.collapsing(crate::i18n::tr("Add a lens"), |ui| {
            TextField::singleline("planner:lensName", &mut st.lens_draft.name).hint(crate::i18n::tr("Name")).width(ui.available_width()).show(ui);
            labelled_number(ui, rt, "lensMin", "Shortest focal length (mm)", &mut st.lens_draft.focal_min, (1.0, 2000.0), 0);
            labelled_number(ui, rt, "lensMax", "Longest focal length (mm)", &mut st.lens_draft.focal_max, (1.0, 2000.0), 0);
            labelled_number(ui, rt, "lensAperture", "Widest aperture (f/)", &mut st.lens_draft.aperture_max, (0.7, 64.0), 1);
            labelled_number(ui, rt, "lensFocus", "Nearest focus (m)", &mut st.lens_draft.min_focus, (0.01, 100_000.0), 2);
            let ready = !st.lens_draft.name.trim().is_empty() && st.lenses.len() < MAX_LENSES;
            ui.add_enabled_ui(ready, |ui| {
                if text_button(ui, "plannerLensAdd", "Add lens", false).clicked() {
                    let mut lens = st.lens_draft.clone();
                    lens.name = lens.name.trim().to_string();
                    lens.sane();
                    st.lenses.push(lens);
                    st.lens = Some(st.lenses.len() - 1);
                    st.fit_to_lens();
                    st.lens_draft.name.clear();
                }
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lens_keeps_the_camera_inside_its_limits() {
        let l = Lens { name: "24-70".into(), focal_min: 24.0, focal_max: 70.0, aperture_max: 2.8, min_focus: 0.38 };
        assert_eq!(l.fit(100.0, 1.4, 0.1), (70.0, 2.8, 0.38));
        assert_eq!(l.fit(10.0, 8.0, 5.0), (24.0, 8.0, 5.0));
        assert_eq!(l.summary(), "24–70 mm f/2.8");
        let prime = Lens { focal_min: 35.0, focal_max: 35.0, aperture_max: 1.4, ..Lens::default() };
        assert_eq!(prime.summary(), "35 mm f/1.4");
    }

    #[test]
    fn hostile_lens_numbers_are_brought_into_range() {
        let mut l = Lens { name: "x".repeat(500), focal_min: 300.0, focal_max: f64::NAN, aperture_max: -1.0, min_focus: f64::INFINITY };
        l.sane();
        assert_eq!(l.name.chars().count(), MAX_NAME);
        assert!(l.focal_min <= l.focal_max && l.focal_min >= 1.0 && l.focal_max <= 2000.0);
        assert!(l.aperture_max >= 0.7 && l.min_focus.is_finite());
        let mut st: PlannerState =
            serde_json::from_str(r#"{"lenses": [{"name": "a", "focalMin": 50, "focalMax": 10}], "lens": 7}"#).unwrap_or_default();
        st.sane();
        assert_eq!((st.lenses.len(), st.lens), (1, None));
        assert!(st.lenses[0].focal_min <= st.lenses[0].focal_max);
    }

    #[test]
    fn old_settings_without_lenses_still_load() {
        let st: PlannerState = serde_json::from_str(r#"{"focal": 35}"#).unwrap_or_default();
        assert!(st.lenses.is_empty() && st.lens.is_none() && st.focal == 35.0);
    }
}
