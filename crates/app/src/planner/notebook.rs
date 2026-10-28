//! The location notebook: what a photographer writes down about a place beyond its record (how to get there, what to
//! carry, the conditions that make it work, the reference photos) and the shots planned there, each with its framing
//! (kept from the viewfinder), whether it has been taken and where the result is. It belongs to the point of interest
//! and stays on this computer: the phone's sync format has no room for it, so a sync never touches it.

use super::framing::FrameShot;
use super::*;

pub(super) const MAX_SHOTS: usize = 30;
const MAX_TEXT: usize = 600;
const MAX_REFERENCES: usize = 1000;
const MAX_TITLE: usize = 80;
const MAX_RESULT: usize = 300;
/// Plans within this distance of the place count as being for it (degrees, about 150 m).
pub(super) const NEAR_DEGREES: f64 = 0.0015;

/// A shot planned at a place.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Shot {
    pub title: String,
    /// The idea: subject, mood, light wanted, exposure.
    pub idea: String,
    /// Where the camera looks and with what lens, when it was kept from the viewfinder.
    pub frame: Option<FrameShot>,
    /// The day aimed at (`YYYY-MM-DD`), free to leave empty.
    pub planned: String,
    pub done: bool,
    /// The day it was taken (`YYYY-MM-DD`) and where the picture is (a file name, a link, a note).
    pub done_on: String,
    pub result: String,
}

/// The notebook of one place.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Notebook {
    /// How to get there: road, parking, walk, permits, hazards.
    pub access: String,
    /// What to take.
    pub gear: String,
    /// The conditions that make the place work (tide, wind, season, light).
    pub conditions: String,
    /// Reference photos: file names or links, one a line.
    pub references: String,
    pub shots: Vec<Shot>,
}

/// A text cut to `n` characters (never inside one).
fn cut(s: &mut String, n: usize) {
    if s.chars().count() > n {
        *s = s.chars().take(n).collect();
    }
}

impl Notebook {
    /// Keeps every field within its limits (also for what was read from a settings file).
    pub fn tidy(&mut self) {
        cut(&mut self.access, MAX_TEXT);
        cut(&mut self.gear, MAX_TEXT);
        cut(&mut self.conditions, MAX_TEXT);
        cut(&mut self.references, MAX_REFERENCES);
        self.shots.truncate(MAX_SHOTS);
        for s in &mut self.shots {
            cut(&mut s.title, MAX_TITLE);
            cut(&mut s.idea, MAX_TEXT);
            cut(&mut s.planned, 10);
            cut(&mut s.done_on, 10);
            cut(&mut s.result, MAX_RESULT);
            if let Some(f) = &mut s.frame {
                // the numbers of a framing read from a file are brought into range
                let mut st = PlannerState::default();
                f.apply(&mut st);
                *f = FrameShot::of(&st);
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        *self == Notebook::default()
    }

    /// The shots still to take, and those taken.
    pub fn counts(&self) -> (usize, usize) {
        let done = self.shots.iter().filter(|s| s.done).count();
        (self.shots.len() - done, done)
    }

    /// The reference photos listed, one per non-empty line.
    pub fn reference_list(&self) -> Vec<&str> {
        self.references.lines().map(str::trim).filter(|l| !l.is_empty()).collect()
    }
}

/// "285° · 24 mm · landscape".
fn frame_summary(f: &FrameShot) -> String {
    format!("{:.0}° · {:.0} mm · {}", f.az, f.focal, crate::i18n::tr(if f.portrait { "portrait" } else { "landscape" }))
}

/// What the notebook UI asks the page to do once the record's borrow is over.
enum Act {
    /// Put the camera at the place with this framing.
    Show(FrameShot),
    OpenPlan(u64),
}

impl Page<'_> {
    /// The notebook in a site's record: read, or written when the record is being edited.
    pub(super) fn site_notebook(&mut self, ui: &mut Ui, st: &mut PlannerState, rt: &mut Runtime, i: usize, editing: bool) {
        let t = self.t;
        let here = self.c.date.iso();
        let now_frame = FrameShot::of(st);
        let Some(site) = st.saved.get(i) else { return };
        let (lat, lon, name) = (site.lat, site.lon, site.name.clone());
        // the plans kept for this place (the pending ones)
        let plans: Vec<(u64, String, String)> = st
            .plans
            .iter()
            .filter(|p| (p.lat - lat).abs() <= NEAR_DEGREES && (p.lon - lon).abs() <= NEAR_DEGREES)
            .map(|p| (p.id, p.name.clone(), p.date.clone()))
            .collect();
        let Some(site) = st.saved.get_mut(i) else { return };
        let mut act: Option<Act> = None;
        ui.add_space(8.0);
        subhead(ui, "Notebook");
        let nb = &mut site.notebook;
        let id = |n: &str| format!("planner:nb:{i}:{n}");
        if editing {
            for (key, label, hint) in [
                ("access", "Access", "Road, parking, walk, permits, hazards…"),
                ("gear", "Gear", "Lenses, filters, tripod, headlamp…"),
                ("conditions", "Ideal conditions", "Tide, wind, season, light…"),
            ] {
                ui.label(egui::RichText::new(crate::i18n::tr(label)).font(t.font(11.5)).color(t.text_dim));
                let text = match key {
                    "access" => &mut nb.access,
                    "gear" => &mut nb.gear,
                    _ => &mut nb.conditions,
                };
                TextField::multiline(&id(key), text).hint(crate::i18n::tr(hint)).width(ui.available_width()).rows(2).show(ui);
                ui.add_space(4.0);
            }
            ui.label(egui::RichText::new(crate::i18n::tr("Reference photos")).font(t.font(11.5)).color(t.text_dim));
            TextField::multiline(&id("refs"), &mut nb.references)
                .hint(crate::i18n::tr("File names or links, one a line"))
                .width(ui.available_width())
                .rows(2)
                .show(ui);
        } else {
            super::popup::read_field(ui, &t, "Access", &nb.access);
            super::popup::read_field(ui, &t, "Gear", &nb.gear);
            super::popup::read_field(ui, &t, "Ideal conditions", &nb.conditions);
            let refs = nb.reference_list().join("\n");
            super::popup::read_field(ui, &t, "Reference photos", &refs);
        }

        // the shots
        ui.add_space(6.0);
        let (pending, done) = nb.counts();
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(crate::i18n::tr("Shots")).font(t.font(11.5)).color(t.text_dim));
            ui.label(
                egui::RichText::new(format!("{pending} {} · {done} {}", crate::i18n::tr("pending"), crate::i18n::tr("taken")))
                    .font(t.font(11.5))
                    .color(t.text_label),
            );
        });
        let mut remove: Option<usize> = None;
        for (n, shot) in nb.shots.iter_mut().enumerate() {
            egui::Frame::NONE.fill(t.inset).corner_radius(8.0).inner_margin(Margin::same(8)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                if editing {
                    TextField::singleline(&id(&format!("shot{n}")), &mut shot.title)
                        .hint(crate::i18n::tr("Title"))
                        .width(ui.available_width())
                        .show(ui);
                    TextField::multiline(&id(&format!("idea{n}")), &mut shot.idea)
                        .hint(crate::i18n::tr("The idea"))
                        .width(ui.available_width())
                        .rows(2)
                        .show(ui);
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(crate::i18n::tr("Planned")).font(t.font(11.0)).color(t.text_dim));
                        TextField::singleline(&id(&format!("planned{n}")), &mut shot.planned).hint("YYYY-MM-DD").width(96.0).show(ui);
                    });
                } else {
                    let title = if shot.title.trim().is_empty() { crate::i18n::tr("Untitled shot") } else { shot.title.trim() };
                    ui.label(egui::RichText::new(format!("{} {title}", if shot.done { "☑" } else { "☐" })).font(t.semibold(13.0)).color(t.text));
                    if !shot.idea.trim().is_empty() {
                        ui.label(egui::RichText::new(shot.idea.trim()).font(t.font(12.0)).color(t.text_label));
                    }
                    if !shot.planned.is_empty() && !shot.done {
                        ui.label(
                            egui::RichText::new(format!("{} {}", crate::i18n::tr("Planned"), shot.planned)).font(t.font(11.5)).color(t.text_dim),
                        );
                    }
                }
                if let Some(f) = shot.frame {
                    ui.label(egui::RichText::new(frame_summary(&f)).font(t.font(11.5)).color(t.text_dim));
                }
                if shot.done {
                    if editing {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(crate::i18n::tr("Taken on")).font(t.font(11.0)).color(t.text_dim));
                            TextField::singleline(&id(&format!("doneon{n}")), &mut shot.done_on).hint("YYYY-MM-DD").width(96.0).show(ui);
                        });
                        TextField::singleline(&id(&format!("result{n}")), &mut shot.result)
                            .hint(crate::i18n::tr("Where the picture is"))
                            .width(ui.available_width())
                            .show(ui);
                    } else if !shot.result.trim().is_empty() || !shot.done_on.is_empty() {
                        ui.label(
                            egui::RichText::new(format!("{} {} · {}", crate::i18n::tr("Taken on"), shot.done_on, shot.result.trim()))
                                .font(t.font(11.5))
                                .color(t.text_dim),
                        );
                    }
                }
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(6.0, 4.0);
                    if let Some(f) = shot.frame
                        && text_button(ui, &format!("plannerNbShow{n}"), "Show Framing", false).clicked()
                    {
                        act = Some(Act::Show(f));
                    }
                    if editing {
                        if text_button(ui, &format!("plannerNbFrame{n}"), "Keep Current Framing", false).clicked() {
                            shot.frame = Some(now_frame);
                        }
                        let label = if shot.done { "Not Taken" } else { "Mark as Taken" };
                        if text_button(ui, &format!("plannerNbDone{n}"), label, shot.done).clicked() {
                            shot.done = !shot.done;
                            shot.done_on = if shot.done { here.clone() } else { String::new() };
                        }
                        if text_button(ui, &format!("plannerNbRemove{n}"), "Remove", false).clicked() {
                            remove = Some(n);
                        }
                    }
                });
            });
            ui.add_space(4.0);
        }
        if let Some(n) = remove {
            nb.shots.remove(n);
        }
        if editing && nb.shots.len() < MAX_SHOTS && text_button(ui, "plannerNbAdd", "Add a Shot", false).clicked() {
            nb.shots.push(Shot { frame: Some(now_frame), ..Default::default() });
        }
        nb.tidy();

        if !plans.is_empty() {
            ui.add_space(6.0);
            ui.label(egui::RichText::new(crate::i18n::tr("Plans for this place")).font(t.font(11.5)).color(t.text_dim));
            for (pid, pname, pdate) in plans {
                if text_button(ui, &format!("plannerNbPlan{pid}"), &format!("{pname} · {pdate}"), false).clicked() {
                    act = Some(Act::OpenPlan(pid));
                }
            }
        }
        match act {
            Some(Act::Show(f)) => {
                set_place(st, lat, lon, name);
                f.apply(st);
            }
            Some(Act::OpenPlan(id)) => {
                rt.plan_popup = Some(id);
                rt.site_open = None;
                rt.edit_of = None;
            }
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hostile_notebooks_are_brought_into_range() {
        let mut nb = Notebook {
            access: "a".repeat(5000),
            references: "r".repeat(5000),
            shots: (0..100)
                .map(|n| Shot {
                    title: "t".repeat(500),
                    done_on: "9999999999999".into(),
                    frame: Some(FrameShot { az: f64::NAN, focal: -1.0, portrait: false, sensor: 77 }),
                    result: format!("{n}").repeat(500),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };
        nb.tidy();
        assert_eq!((nb.access.chars().count(), nb.references.chars().count(), nb.shots.len()), (MAX_TEXT, MAX_REFERENCES, MAX_SHOTS));
        let f = nb.shots[0].frame.expect("kept");
        assert!(f.az.is_finite() && f.focal >= 1.0 && f.sensor < SENSORS.len());
        assert!(nb.shots.iter().all(|s| s.title.chars().count() <= MAX_TITLE && s.done_on.chars().count() <= 10));
    }

    #[test]
    fn counts_and_references() {
        let nb = Notebook {
            references: " IMG_0012.CR2 \n\n https://example.org/a \n".into(),
            shots: vec![Shot { done: true, ..Default::default() }, Shot::default(), Shot::default()],
            ..Default::default()
        };
        assert_eq!(nb.counts(), (2, 1));
        assert_eq!(nb.reference_list(), ["IMG_0012.CR2", "https://example.org/a"]);
        assert!(Notebook::default().is_empty() && !nb.is_empty());
    }

    #[test]
    fn a_site_without_a_notebook_still_loads() {
        let old: SavedPlace = serde_json::from_str(r#"{"name":"x","lat":1,"lon":2}"#).expect("reads");
        assert!(old.notebook.is_empty());
        let with: SavedPlace = serde_json::from_str(r#"{"name":"x","notebook":{"gear":"tripod","shots":[{"title":"Moon"}]}}"#).expect("reads");
        assert_eq!((with.notebook.gear.as_str(), with.notebook.shots.len()), ("tripod", 1));
    }
}
