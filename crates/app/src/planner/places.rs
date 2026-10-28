//! Sites: the spots a person shoots from, each with its record (as in Luis's «Mapa solar»): a status (want to go,
//! visited), the best time, notes and the photos from the phone. They show on the planner's map as dots;
//! a click opens the record.

use super::*;

/// The colour of a point of interest's pin (a plan's is blue).
pub(super) const POI_RED: Color32 = Color32::from_rgb(235, 64, 52);

/// Saving a point again at (almost) the same spot, within about a metre, is the same point: it is renamed, not doubled.
const SAME: f64 = 0.00001;
/// The longest notes and description a site keeps.
pub(super) const MAX_NOTES: usize = 4000;
pub(super) const MAX_DESCRIPTION: usize = 600;

/// Want to go there, or been.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SiteStatus {
    #[default]
    Want,
    Visited,
}

impl SiteStatus {
    pub(super) fn label(self) -> &'static str {
        match self {
            SiteStatus::Want => "Want to Go",
            SiteStatus::Visited => "Visited",
        }
    }
    pub(super) fn color(self) -> Color32 {
        match self {
            SiteStatus::Want => Color32::from_rgb(240, 170, 60),
            SiteStatus::Visited => Color32::from_rgb(70, 190, 120),
        }
    }
}

/// `name` kept at a position: a new point goes last (the others keep their places in the list), unless one is already
/// there, to about a metre (pressing Save twice): that one is renamed and its record stays. Gives its index, or `None`
/// when the list is full.
pub(super) fn keep(saved: &mut Vec<SavedPlace>, name: &str, lat: f64, lon: f64) -> Option<usize> {
    let name: String = name.trim().chars().take(80).collect();
    let name = if name.is_empty() { format!("{lat:.4}°, {lon:.4}°") } else { name };
    if let Some(i) = saved.iter().position(|p| (p.lat - lat).abs() <= SAME && (p.lon - lon).abs() <= SAME) {
        let site = saved.get_mut(i)?;
        if site.name != name {
            site.name = name;
            site.touch();
        }
        return Some(i);
    }
    if saved.len() >= super::MAX_SAVED {
        return None;
    }
    let mut site = SavedPlace { name, lat, lon, ..Default::default() };
    site.ensure_identity();
    saved.push(site);
    Some(saved.len() - 1)
}

impl Page<'_> {
    /// A site's record: name, status, best time, notes, the phone's photos, and what to do with it.
    pub(super) fn site_record(&mut self, ui: &mut Ui, st: &mut PlannerState, rt: &mut Runtime, i: usize, part: super::popup::Part) {
        let t = self.t;
        let editing = rt.edit_of == Some(super::popup::Shown::Site(i));
        let Some(site) = st.saved.get_mut(i) else { return };
        let label = |ui: &mut Ui, s: &str| {
            ui.label(egui::RichText::new(crate::i18n::tr(s)).font(t.font(11.5)).color(t.text_dim));
        };
        if part.left() {
            if editing {
                label(ui, "Name");
                TextField::singleline("planner:siteName", &mut site.name).width(ui.available_width()).show(ui);
                site.name = site.name.chars().take(80).collect();
                ui.add_space(6.0);
                label(ui, "Description");
                TextField::multiline("planner:siteDescription", &mut site.description)
                    .hint(crate::i18n::tr("What this place is, in a few lines"))
                    .width(ui.available_width())
                    .rows(3)
                    .show(ui);
                if site.description.chars().count() > MAX_DESCRIPTION {
                    site.description = site.description.chars().take(MAX_DESCRIPTION).collect();
                }
                ui.add_space(6.0);
                label(ui, "Status");
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    for s in [SiteStatus::Want, SiteStatus::Visited] {
                        if text_button(ui, &format!("plannerSiteStatus{s:?}"), s.label(), site.status == s).clicked() {
                            site.status = s;
                        }
                    }
                });
                ui.add_space(6.0);
                label(ui, "Best Time");
                TextField::singleline("planner:siteBest", &mut site.best)
                    .hint(crate::i18n::tr("Sunrise in March, low tide…"))
                    .width(ui.available_width())
                    .show(ui);
                site.best = site.best.chars().take(200).collect();
                ui.add_space(6.0);
                label(ui, "Notes");
                TextField::multiline("planner:siteNotes", &mut site.notes)
                    .hint(crate::i18n::tr("Access, framing, permits…"))
                    .width(ui.available_width())
                    .rows(4)
                    .show(ui);
                if site.notes.chars().count() > MAX_NOTES {
                    site.notes = site.notes.chars().take(MAX_NOTES).collect();
                }
            } else {
                super::popup::read_field(ui, &t, "Description", &site.description);
                ui.label(egui::RichText::new(crate::i18n::tr("Status")).font(t.font(11.5)).color(t.text_dim));
                ui.label(egui::RichText::new(crate::i18n::tr(site.status.label())).font(t.font(13.0)).color(site.status.color()));
                ui.add_space(6.0);
                super::popup::read_field(ui, &t, "Best Time", &site.best);
                super::popup::read_field(ui, &t, "Notes", &site.notes);
            }
        }
        if !part.right() {
            return;
        }
        // the photos taken with the phone: as many as arrived, the first as the record's picture
        let phone_keys: Vec<String> =
            site.attachments.iter().filter(|a| a.kind.is_empty() || a.kind.starts_with("image/")).map(|a| a.key.clone()).collect();
        if !phone_keys.is_empty() {
            if part.left() {
                ui.add_space(6.0);
            }
            label(ui, "Photos from the Phone");
            let full = ui.available_width();
            let ctx = ui.ctx().clone();
            for (k, key) in phone_keys.iter().enumerate() {
                let tex = rt.phone.get(&ctx, key);
                let h = if k == 0 { (full * 0.66).clamp(90.0, 300.0) } else { (full * 0.5).clamp(70.0, 200.0) };
                let (cell, _) = ui.allocate_exact_size(vec2(full, h), Sense::hover());
                match tex {
                    Some((tex, [w, h])) => {
                        let uv = cover_uv(w as f32, h as f32, cell.width() / cell.height().max(1.0));
                        ui.painter().add(egui::epaint::RectShape::filled(cell, 8.0, Color32::WHITE).with_texture(tex, uv));
                    }
                    None => {
                        ui.painter().rect_filled(cell, 8.0, t.inset);
                    }
                }
                ui.add_space(6.0);
            }
        }
        if part.right() {
            self.site_notebook(ui, st, rt, i, editing);
        }
    }

    /// Under the record's title: where it is and what to do with it (put the camera there, target it, plan it…).
    pub(super) fn site_actions(&mut self, ui: &mut Ui, st: &mut PlannerState, rt: &mut Runtime, i: usize) {
        let t = self.t;
        let Some(site) = st.saved.get(i) else { return };
        let (lat, lon, name) = (site.lat, site.lon, site.name.clone());
        let mut delete = false;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            ui.label(egui::RichText::new(format!("{lat:.5}, {lon:.5}")).font(t.font(12.5)).color(t.text_dim));
            ui.add_space(4.0);
            if text_button(ui, "plannerSitePlan", "Create Plan", false).clicked() {
                self.new_plan_at(st, rt, lat, lon, Some(&name));
                st.map_mode = MapMode::Plans;
            }
            if text_button(ui, "plannerSiteGo", "Put the Camera Here", false).clicked() {
                set_place(st, lat, lon, name.clone());
            }
            if text_button(ui, "plannerSiteTarget", "Use as Target", false).clicked() {
                st.target = Some([lat, lon]);
                st.tab = Tab::Align;
            }
            if text_button(ui, "plannerSiteDirections", "Directions ↗", false).clicked() {
                self.intents.push(Intent::OpenUrl(format!("https://maps.apple.com/?daddr={lat:.6},{lon:.6}")));
            }
            if text_button(ui, "plannerSiteDelete", "Delete", false).clicked() {
                delete = true;
            }
        });
        if delete {
            let gone = st.saved.remove(i);
            // the Worker knows it: tell it at the next synchronisation
            if gone.base > 0 && !st.pdi_deleted.iter().any(|g| g.id == gone.id) {
                st.pdi_deleted.push(super::pois::Gone { id: gone.id, base: gone.base });
            }
            rt.site_open = None;
        }
    }
}

/// The part of a picture that covers a box of the given width / height ratio, centred.
fn cover_uv(w: f32, h: f32, aspect: f32) -> Rect {
    if !(w > 0.0 && h > 0.0 && aspect > 0.0) {
        return Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    }
    let a = w / h;
    if a > aspect {
        let f = aspect / a;
        Rect::from_min_max(pos2((1.0 - f) / 2.0, 0.0), pos2((1.0 + f) / 2.0, 1.0))
    } else {
        let f = a / aspect;
        Rect::from_min_max(pos2(0.0, (1.0 - f) / 2.0), pos2(1.0, (1.0 + f) / 2.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_kept_place_goes_last_and_only_the_same_spot_is_the_same_point() {
        let mut v = Vec::new();
        assert_eq!(keep(&mut v, "Bardenas", 42.18, -1.5), Some(0));
        assert_eq!(keep(&mut v, "  Sierra de Guadarrama ", 40.8, -4.0), Some(1));
        assert_eq!(v.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["Bardenas", "Sierra de Guadarrama"]);
        // the very same spot again: renamed, not doubled, and a new version of the same point
        let id = v[0].id.clone();
        assert_eq!(keep(&mut v, "Bardenas Reales", 42.180001, -1.500001), Some(0));
        assert_eq!((v.len(), v[0].name.as_str(), v[0].id.clone(), v[0].version), (2, "Bardenas Reales", id, 2));
        // a tree thirty metres away is another point (the phone saves them that close)
        assert_eq!(keep(&mut v, "Otro roble", 42.18027, -1.5), Some(2));
        assert_eq!(v.len(), 3);
        assert_eq!(v.iter().map(|p| p.id.clone()).collect::<std::collections::HashSet<_>>().len(), 3, "each has its own id");
    }

    #[test]
    fn saving_a_site_again_keeps_its_record() {
        let mut v = Vec::new();
        keep(&mut v, "Faro", 43.39, -1.79);
        v[0].status = SiteStatus::Visited;
        v[0].notes = "Marea baja".into();
        keep(&mut v, "Otro", 40.0, -3.0);
        keep(&mut v, "Faro de Higer", 43.39, -1.79);
        assert_eq!(v.len(), 2);
        assert_eq!((v[0].name.as_str(), v[0].status, v[0].notes.as_str()), ("Faro de Higer", SiteStatus::Visited, "Marea baja"));
        // old settings without a record read as "want to go"
        let old: SavedPlace = serde_json::from_str(r#"{"name":"x","lat":1,"lon":2}"#).unwrap();
        assert_eq!(old.status, SiteStatus::Want);
    }

    #[test]
    fn an_unnamed_place_is_named_by_its_position_and_a_full_list_says_so() {
        let mut v = Vec::new();
        keep(&mut v, "   ", 40.4168, -3.7038);
        assert_eq!(v[0].name, "40.4168°, -3.7038°");
        let long = "n".repeat(500);
        keep(&mut v, &long, 10.0, 10.0);
        assert_eq!(v[1].name.chars().count(), 80);
        // nothing is dropped silently when the list is full
        let mut full: Vec<SavedPlace> =
            (0..MAX_SAVED).map(|i| SavedPlace { lat: (i % 80) as f64, lon: (i / 80) as f64, ..Default::default() }).collect();
        for p in &mut full {
            p.ensure_identity();
        }
        let first = full[0].id.clone();
        assert_eq!(keep(&mut full, "x", 89.5, 179.5), None);
        assert_eq!((full.len(), full[0].id.clone()), (MAX_SAVED, first));
    }

    fn plan(id: u64, name: &str, lat: f64, lon: f64) -> Plan {
        Plan { id, name: name.into(), lat, lon, date: "2026-11-01".into(), minute: 480.0, ..Default::default() }
    }

    #[test]
    fn points_of_interest_and_plans_are_managed_independently() {
        let mut st = PlannerState::default();
        // a plan and a point of interest at the very same spot
        st.plans.push(plan(1, "Amanecer en el faro", 43.39, -1.79));
        keep(&mut st.saved, "Faro", 43.39, -1.79);
        st.saved[0].description = "Faro sobre acantilado".into();
        st.saved[0].notes = "Marea baja".into();
        // editing the point of interest leaves the plan alone
        st.saved[0].name = "Faro de Higer".into();
        st.saved[0].notes.push_str(" y viento del norte");
        assert_eq!((st.plans[0].name.as_str(), st.plans[0].notes.as_str(), st.plans.len()), ("Amanecer en el faro", "", 1));
        // deleting the point of interest leaves the plan alone
        st.saved.clear();
        assert_eq!(st.plans.len(), 1);
        assert_eq!(st.plans[0].id, 1);
        // and the other way round: a point of interest survives its plan's deletion and edits
        keep(&mut st.saved, "Bardenas", 42.18, -1.5);
        st.plans[0].name = "Otro nombre".into();
        st.plans.clear();
        assert_eq!(st.saved.len(), 1);
        assert_eq!(st.saved[0].name, "Bardenas");
        // keeping a place as a point of interest never makes a plan
        keep(&mut st.saved, "Sierra", 40.8, -4.0);
        assert!(st.plans.is_empty());
    }

    #[test]
    fn points_of_interest_persist_with_their_record_and_old_settings_still_load() {
        let mut st = PlannerState::default();
        keep(&mut st.saved, "Faro", 43.39, -1.79);
        st.saved[0].description = "Faro sobre acantilado".into();
        st.saved[0].notes = "Marea baja".into();
        st.plans.push(plan(9, "Plan", 43.0, -2.0));
        st.map_mode = MapMode::Pois;
        let json = serde_json::to_string(&st).unwrap();
        let back: PlannerState = serde_json::from_str(&json).unwrap();
        assert_eq!(back.saved, st.saved);
        assert_eq!(back.plans, st.plans);
        assert_eq!(back.map_mode, MapMode::Pois);
        // settings from before the points of interest: no description, every layer shown
        let old: PlannerState = serde_json::from_str(r#"{"saved":[{"name":"x","lat":1,"lon":2,"notes":"n"}]}"#).unwrap();
        assert_eq!((old.saved[0].description.as_str(), old.saved[0].notes.as_str()), ("", "n"));
        assert_eq!(old.map_mode, MapMode::Planner, "old settings open on the planner");
    }

    #[test]
    fn points_saved_before_ids_existed_get_unique_ones_and_a_repeated_id_is_made_new() {
        let old: PlannerState = serde_json::from_str(r#"{"saved":[{"name":"a","lat":1,"lon":2},{"name":"b","lat":3,"lon":4}]}"#).unwrap();
        let mut st = old;
        st.saved.push(SavedPlace { id: "same".into(), lat: 5.0, lon: 5.0, ..Default::default() });
        st.saved.push(SavedPlace { id: "same".into(), lat: 6.0, lon: 6.0, ..Default::default() });
        st.sane();
        let ids: std::collections::HashSet<_> = st.saved.iter().map(|p| p.id.clone()).collect();
        assert_eq!(ids.len(), 4);
        assert!(st.saved.iter().all(|p| !p.id.is_empty() && p.version >= 1 && !p.created_at.is_empty() && p.origin == "desktop"));
        // and what was filled in is what is saved: the same ids after a round trip
        let back: PlannerState = serde_json::from_str(&serde_json::to_string(&st).unwrap()).unwrap();
        assert_eq!(back.saved, st.saved);
    }

    #[test]
    fn a_long_description_and_a_big_library_of_points_are_kept_within_bounds() {
        let mut st = PlannerState {
            saved: (0..MAX_SAVED + 30).map(|i| SavedPlace { lat: (i % 80) as f64, lon: (i / 80 % 170) as f64, ..Default::default() }).collect(),
            ..Default::default()
        };
        st.saved[0].description = "d".repeat(5000);
        st.sane();
        assert_eq!(st.saved.len(), MAX_SAVED);
        assert_eq!(st.saved[0].description.chars().count(), MAX_DESCRIPTION);
    }
}
