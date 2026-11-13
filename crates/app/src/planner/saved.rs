//! Saved searches: the conditions of the Find card (body, direction, light, Moon, skyline) kept with their place, and the
//! next date each one is met, worked out in the background every day. A search over the skyline keeps the skyline it was
//! made with (one altitude per direction, to a tenth of a degree), as the background check cannot read the terrain.

use std::hash::{Hash, Hasher};

use light_studio_astro::Observer;
use light_studio_astro::horizon::Skyline;
use light_studio_astro::search::{Conditions, Hit, Target, find_with};
use light_studio_astro::time::{DAY, Date};
use serde::{Deserialize, Serialize};

use super::*;

/// Most searches kept.
pub const MAX_SEARCHES: usize = 40;
/// How many days ahead the background check looks (the search itself may cover more).
const CHECK_DAYS: u32 = 120;
/// A date within this many days is flagged.
pub const SOON_DAYS: f64 = 14.0;

/// What the Find card asks, without the place.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Spec {
    pub sun: bool,
    pub core: bool,
    pub az: f64,
    pub alt: Option<f64>,
    pub tol: f64,
    pub days: f64,
    pub light: u8,
    pub ill: Option<[f64; 2]>,
    pub no_moon: bool,
    /// Over the skyline: the body must clear it by this many degrees (low, high), and the skyline it was made with.
    pub margin: Option<[f64; 2]>,
    pub skyline: Option<Vec<f32>>,
}

impl Default for Spec {
    fn default() -> Self {
        Self { sun: true, core: false, az: 90.0, alt: None, tol: 2.0, days: 365.0, light: 0, ill: None, no_moon: false, margin: None, skyline: None }
    }
}

impl Spec {
    /// The card's current conditions; `None` when they use the skyline (see [`Spec::with_skyline`]).
    pub fn of(st: &PlannerState) -> Option<Self> {
        Self::with_skyline(st, None)
    }

    /// The card's current conditions, with the skyline of the place when the search uses it (`None` if it does and the
    /// terrain has not been read).
    pub fn with_skyline(st: &PlannerState, skyline: Option<&Skyline>) -> Option<Self> {
        let (margin, kept) = if st.find_skyline {
            let sky = skyline?;
            (Some(st.find_margin), Some(sky.alt.iter().map(|a| (a * 10.0).round() as f32 / 10.0).collect()))
        } else {
            (None, None)
        };
        Some(Self {
            sun: st.find_sun && !st.find_core,
            core: st.find_core,
            az: st.find_az,
            alt: st.find_use_alt.then_some(st.find_alt),
            tol: st.find_tol,
            days: st.find_days,
            light: st.find_light,
            ill: st.find_use_ill.then_some(st.find_ill),
            no_moon: st.find_no_moon,
            margin,
            skyline: kept,
        })
    }

    /// The skyline kept with the search, as far as a search needs it (the altitude in each direction).
    fn kept_skyline(&self) -> Option<Skyline> {
        let alt: Vec<f64> = self.skyline.as_ref()?.iter().map(|a| f64::from(*a)).collect();
        Some(Skyline { near: 0.0, far: 0.0, ground: 0.0, dist: vec![0.0; alt.len()], height: vec![0.0; alt.len()], alt })
    }

    /// Puts these conditions on the card.
    pub fn apply(&self, st: &mut PlannerState) {
        st.find_sun = self.sun && !self.core;
        st.find_core = self.core;
        st.find_az = self.az;
        st.find_use_alt = self.alt.is_some();
        if let Some(a) = self.alt {
            st.find_alt = a;
        }
        st.find_tol = self.tol;
        st.find_days = self.days;
        st.find_light = self.light;
        st.find_use_ill = self.ill.is_some();
        if let Some(i) = self.ill {
            st.find_ill = i;
        }
        st.find_no_moon = self.no_moon;
        st.find_skyline = self.margin.is_some() && self.skyline.is_some();
        if let Some(m) = self.margin {
            st.find_margin = m;
        }
    }

    pub fn target(&self) -> Target {
        if self.core {
            Target::Core
        } else if self.sun {
            Target::Sun
        } else {
            Target::Moon
        }
    }

    pub fn conditions(&self) -> Conditions<'static> {
        Conditions {
            sun_alt: match self.light {
                1 => Some((-4.0, 6.0)),
                2 => Some((-6.0, -4.0)),
                3 => Some((-90.0, -18.0)),
                _ => None,
            },
            moon_illumination: self.ill.map(|i| (i[0] / 100.0, i[1] / 100.0)),
            moon_alt_max: self.no_moon.then_some(-1.0),
            skyline: None,
        }
    }

    /// The conditions in words (the body is the first part).
    pub fn label(&self) -> String {
        let tr = crate::i18n::tr;
        let body = tr(if self.core {
            "Milky Way"
        } else if self.sun {
            "Sun"
        } else {
            "Moon"
        });
        let mut label = format!("{body} · {} {:.0}° ± {:.1}°", tr("Azimuth"), self.az, self.tol);
        if let Some(a) = self.alt {
            label.push_str(&format!(" · {} {a:.0}° ± {:.1}°", tr("Elevation"), self.tol));
        }
        if let Some(name) = [None, Some("Golden hour"), Some("Blue hour"), Some("Dark sky")].get(usize::from(self.light)).copied().flatten() {
            label.push_str(&format!(" · {}", tr(name)));
        }
        if let Some(i) = self.ill {
            label.push_str(&format!(" · {:.0}–{:.0} % {}", i[0], i[1], tr("lit")));
        }
        if self.no_moon {
            label.push_str(&format!(" · {}", tr("Moon down")));
        }
        if let (Some(m), Some(_)) = (self.margin, &self.skyline) {
            label.push_str(&format!(" · {} {:.1}° … {:.1}°", tr("over the skyline"), m[0], m[1]));
        }
        label
    }

    /// Brings hostile saved numbers back into range.
    fn sane(&mut self) {
        let fix = |v: f64, lo: f64, hi: f64, d: f64| if v.is_finite() { v.clamp(lo, hi) } else { d };
        self.az = fix(self.az, 0.0, 360.0, 90.0);
        self.alt = self.alt.map(|a| fix(a, -90.0, 90.0, 10.0));
        self.tol = fix(self.tol, 0.1, 45.0, 2.0);
        self.days = fix(self.days, 1.0, 400.0, 365.0).round();
        self.light = self.light.min(3);
        self.ill = self.ill.map(|i| {
            let (a, b) = (fix(i[0], 0.0, 100.0, 0.0), fix(i[1], 0.0, 100.0, 100.0));
            [a.min(b), a.max(b)]
        });
        if self.core {
            self.sun = false;
        }
        // a skyline needs a margin and a usable list of directions; otherwise the search is the plain one
        let usable = self.skyline.as_ref().is_some_and(|k| (8..=720).contains(&k.len()) && k.iter().all(|a| a.is_finite()));
        match (self.margin, usable) {
            (Some(m), true) => self.margin = Some([fix(m[0], -90.0, 90.0, 0.0), fix(m[1], -90.0, 90.0, 1.0)]),
            _ => {
                self.margin = None;
                self.skyline = None;
            }
        }
        if let Some(k) = &mut self.skyline {
            for a in k {
                *a = a.clamp(-90.0, 90.0);
            }
        }
    }

    /// The hits from `now` on, over at most `days` days, at a place.
    pub fn upcoming(&self, lat: f64, lon: f64, now: f64, days: u32) -> Vec<Hit> {
        if !(now.is_finite() && lat.is_finite() && lon.is_finite() && lat.abs() <= 90.0 && lon.abs() <= 180.0) {
            return Vec::new();
        }
        let off = super::zone::offset_at(lat, lon, now).0;
        let start = Date::from_unix(now, off).unix_midnight_utc() - f64::from(off) * 60.0;
        let kept = self.kept_skyline();
        let mut conditions = self.conditions();
        conditions.skyline = kept.as_ref().zip(self.margin).map(|(s, m)| (s, m[0], m[1]));
        let mut hits =
            find_with(self.target(), Observer::new(lat, lon), start, days.min(self.days as u32).max(1), self.az, self.alt, self.tol, conditions);
        hits.retain(|h| h.unix >= now);
        hits
    }
}

/// A search kept with its place.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SavedSearch {
    pub place: String,
    pub lat: f64,
    pub lon: f64,
    pub spec: Spec,
}

impl Default for SavedSearch {
    fn default() -> Self {
        Self { place: String::new(), lat: 0.0, lon: 0.0, spec: Spec::default() }
    }
}

impl PlannerState {
    pub(crate) fn sane_searches(&mut self) {
        self.searches.retain(|s| s.lat.is_finite() && s.lon.is_finite() && s.lat.abs() <= 90.0 && s.lon.abs() <= 180.0);
        self.searches.truncate(MAX_SEARCHES);
        for s in &mut self.searches {
            s.place = s.place.chars().take(80).collect();
            s.spec.sane();
        }
    }
}

/// Identifies the set of searches, so the check is made again when one changes.
fn fingerprint(searches: &[SavedSearch], today: i32) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    format!("{searches:?}").hash(&mut h);
    today.hash(&mut h);
    h.finish()
}

/// The first date of each search, `None` when it is not met within the days checked.
pub fn next_dates(searches: &[SavedSearch], now: f64) -> Vec<Option<f64>> {
    searches.iter().map(|s| s.spec.upcoming(s.lat, s.lon, now, CHECK_DAYS).first().map(|h| h.unix)).collect()
}

const CHECKING: &str = "Checking saved searches";

/// Works the next dates out in the background when the searches or the day changed.
pub(super) fn refresh(app: &mut crate::App) {
    if cfg!(test) || app.planner.saved_next_pending || app.tasks.is_running(CHECKING) {
        return;
    }
    let now = now_unix();
    let print = fingerprint(&app.ui.planner.searches, (now / DAY) as i32);
    if app.planner.saved_next.as_ref().is_some_and(|(p, _)| *p == print) {
        return;
    }
    let searches = app.ui.planner.searches.clone();
    app.planner.saved_next_pending = true;
    let started = crate::tasks::spawn(
        app,
        CHECKING,
        None,
        move || next_dates(&searches, now),
        move |app, _ctx, dates| {
            app.planner.saved_next_pending = false;
            app.planner.saved_next = Some((print, dates));
        },
    );
    if started.is_err() {
        app.planner.saved_next_pending = false;
    }
}

impl Page<'_> {
    /// The saved searches, above the results: the date each is next met, to open it, and a way to load or remove it.
    pub(super) fn saved_list(&mut self, ui: &mut Ui, st: &mut PlannerState, rt: &mut Runtime) {
        if st.searches.is_empty() {
            return;
        }
        let t = self.t;
        subhead(ui, "Saved searches");
        let dates = rt.saved_next.as_ref().map(|(_, d)| d.clone());
        let now = now_unix();
        let mut remove = None;
        let mut load = None;
        egui::ScrollArea::vertical().id_salt("planner-saved").max_height(132.0).show(ui, |ui| {
            for (i, s) in st.searches.iter().enumerate() {
                ui.horizontal(|ui| {
                    let next = dates.as_ref().and_then(|d| d.get(i).copied());
                    let (text, soon) = match next {
                        None => (crate::i18n::tr("checking…").to_string(), false),
                        Some(None) => (format!("{} {CHECK_DAYS} {}", crate::i18n::tr("none in the next"), crate::i18n::tr("days")), false),
                        Some(Some(u)) => {
                            let off = super::zone::offset_at(s.lat, s.lon, u).0;
                            (format!("{} {}", short_date(Date::from_unix(u, off)), clock(u, off)), u - now <= SOON_DAYS * DAY)
                        }
                    };
                    let name = if s.place.is_empty() { format!("{:.3}, {:.3}", s.lat, s.lon) } else { s.place.clone() };
                    let resp = ui.add(
                        egui::Label::new(egui::RichText::new(format!("{}  ·  {}", text, name)).font(t.font(12.0)).color(if soon {
                            t.text
                        } else {
                            t.text_dim
                        }))
                        .truncate()
                        .sense(Sense::click()),
                    );
                    let resp = resp.on_hover_text(s.spec.label());
                    if resp.clicked() {
                        load = Some(i);
                    }
                    if icon_button(ui, &format!("plannerSavedRemove{i}"), Icon::Close, vec2(20.0, 20.0), false, true, "Remove").clicked() {
                        remove = Some(i);
                    }
                });
            }
        });
        if let Some(i) = load.and_then(|i| st.searches.get(i).cloned()) {
            super::set_place(st, i.lat, i.lon, i.place.clone());
            i.spec.apply(st);
            rt.find_load = true;
        }
        if let Some(i) = remove.filter(|i| *i < st.searches.len()) {
            st.searches.remove(i);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> f64 {
        Date::new(2026, 3, 1).unix_midnight_utc() + 43_200.0
    }

    #[test]
    fn a_search_round_trips_through_the_card() {
        let mut st = PlannerState {
            find_sun: false,
            find_core: true,
            find_az: 170.0,
            find_use_alt: true,
            find_alt: 12.0,
            find_light: 3,
            find_use_ill: true,
            find_ill: [0.0, 20.0],
            find_no_moon: true,
            ..Default::default()
        };
        let spec = Spec::of(&st).unwrap();
        assert_eq!(spec.target(), Target::Core);
        let mut other = PlannerState::default();
        spec.apply(&mut other);
        assert_eq!(Spec::of(&other).unwrap(), spec);
        st.find_skyline = true;
        assert!(Spec::of(&st).is_none(), "a search over the skyline needs the terrain to be saved");
    }

    fn ridge(alt: f64) -> Skyline {
        Skyline { near: 0.0, far: 0.0, ground: 0.0, alt: vec![alt; 120], dist: vec![0.0; 120], height: vec![0.0; 120] }
    }

    #[test]
    fn a_search_over_the_skyline_keeps_it() {
        let st = PlannerState { find_skyline: true, find_margin: [0.0, 1.0], find_az: 90.0, find_tol: 30.0, ..Default::default() };
        let spec = Spec::with_skyline(&st, Some(&ridge(5.04))).unwrap();
        assert_eq!(spec.margin, Some([0.0, 1.0]));
        assert_eq!(spec.skyline.as_ref().map(Vec::len), Some(120));
        let mut other = PlannerState::default();
        spec.apply(&mut other);
        assert!(other.find_skyline && other.find_margin == [0.0, 1.0]);
        // the date it gives is the one the same search over that skyline gives, and differs from the flat one
        let (lat, lon, now) = (40.4, -3.7, now());
        let over = spec.upcoming(lat, lon, now, 60);
        let flat = Spec { margin: None, skyline: None, ..spec.clone() }.upcoming(lat, lon, now, 60);
        assert!(over.iter().all(|h| h.margin.is_some_and(|m| (0.0..=1.0).contains(&m))), "every hit clears the ridge by 0° to 1°");
        assert!(!flat.is_empty());
        // hostile saved numbers fall back to the plain search
        let mut bad = spec.clone();
        bad.skyline = Some(vec![f32::NAN; 120]);
        bad.sane();
        assert!(bad.margin.is_none() && bad.skyline.is_none());
        let mut few = spec;
        few.skyline = Some(vec![1.0; 3]);
        few.sane();
        assert!(few.skyline.is_none());
    }

    #[test]
    fn upcoming_dates_start_now_and_meet_the_conditions() {
        // the Sun rising near the east in the next weeks, golden hour
        let spec = Spec { az: 90.0, tol: 6.0, days: 60.0, light: 1, ..Spec::default() };
        let hits = spec.upcoming(40.4, -3.7, now(), 60);
        assert!(!hits.is_empty());
        assert!(hits.iter().all(|h| h.unix >= now() && h.sun_alt > -4.1 && h.sun_alt < 6.1));
        assert!(hits.windows(2).all(|w| w[0].unix <= w[1].unix));
        let next = next_dates(&[SavedSearch { lat: 40.4, lon: -3.7, spec, ..Default::default() }], now());
        assert_eq!(next, vec![hits.first().map(|h| h.unix)]);
    }

    #[test]
    fn hostile_searches_are_tamed() {
        let mut st = PlannerState {
            searches: vec![
                SavedSearch { lat: f64::NAN, ..Default::default() },
                SavedSearch {
                    lat: 10.0,
                    lon: 10.0,
                    place: "x".repeat(500),
                    spec: Spec {
                        az: f64::NAN,
                        tol: -5.0,
                        days: 1e9,
                        light: 99,
                        ill: Some([90.0, 10.0]),
                        alt: Some(f64::INFINITY),
                        ..Spec::default()
                    },
                },
            ],
            ..Default::default()
        };
        st.sane_searches();
        assert_eq!(st.searches.len(), 1);
        let s = &st.searches[0];
        assert_eq!((s.place.chars().count(), s.spec.az, s.spec.tol, s.spec.days, s.spec.light), (80, 90.0, 0.1, 400.0, 3));
        assert_eq!(s.spec.ill, Some([10.0, 90.0]));
        assert!(s.spec.alt.is_some_and(f64::is_finite));
        assert!(s.spec.upcoming(f64::NAN, 0.0, now(), 10).is_empty() && s.spec.upcoming(0.0, 0.0, f64::NAN, 10).is_empty());
        st.searches = vec![SavedSearch::default(); 100];
        st.sane_searches();
        assert_eq!(st.searches.len(), MAX_SEARCHES);
    }
}
