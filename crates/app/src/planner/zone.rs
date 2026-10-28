//! The place's time zone: the real offset Open-Meteo gave, used where it is safe, else the estimate.
//!
//! A single answer says what the clock reads at the moment it was asked, not how the place changes it through the year.
//! So it is trusted on days whose summer-time state (as the estimate reckons it) is the same as that day's; for any other
//! day the estimate stands, and the label says so. A place the estimate has wrong all year (a border, a half-hour zone)
//! is fixed on the days it covers, which are the days around the one it was asked on.

use light_studio_astro::time::auto_utc_offset_min;
use serde::{Deserialize, Serialize};

/// What the service said about a place, and when.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ZoneFix {
    pub lat: f64,
    pub lon: f64,
    pub offset_min: i32,
    /// Unix time it was asked.
    pub at: f64,
    pub name: String,
}

impl Default for ZoneFix {
    fn default() -> Self {
        Self { lat: 0.0, lon: 0.0, offset_min: 0, at: 0.0, name: String::new() }
    }
}

/// Where an offset came from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Source {
    Real,
    Estimate,
}

impl ZoneFix {
    /// Brings hostile saved numbers back into range (None when it cannot be trusted).
    pub fn tidy(self) -> Option<Self> {
        let ok = self.lat.is_finite()
            && self.lon.is_finite()
            && self.lat.abs() <= 90.0
            && self.lon.abs() <= 180.0
            && self.at.is_finite()
            && (-720..=840).contains(&self.offset_min);
        ok.then_some(Self { name: self.name.chars().take(64).collect(), ..self })
    }

    /// Whether it was asked about (close to) this place.
    pub fn is_for(&self, lat: f64, lon: f64) -> bool {
        (self.lat - lat).abs() < 0.05 && (self.lon - lon).abs() < 0.05
    }
}

/// The offset in force at `noon` (unix time of local noon on the day) for a place.
pub fn offset_at(lat: f64, lon: f64, noon: f64, fix: Option<&ZoneFix>) -> (i32, Source) {
    let est = auto_utc_offset_min(lat, lon, noon);
    let Some(f) = fix.filter(|f| f.is_for(lat, lon)) else {
        return (est, Source::Estimate);
    };
    let est_then = auto_utc_offset_min(lat, lon, f.at);
    if est_then == est {
        (f.offset_min, Source::Real)
    } else if f.offset_min == est_then {
        // the estimate was right when asked: it is right in the other season too
        (est, Source::Real)
    } else {
        (est, Source::Estimate)
    }
}

/// The task name in the activity list.
const ASKING: &str = "Finding the time zone";

/// Asks the service for the zone of the chosen place, once per place and only while the offset follows the place. A failure is
/// silent (the estimate stands) and not repeated until the place changes.
pub(super) fn ask(app: &mut crate::App, _ctx: &egui::Context) {
    let st = &app.ui.planner;
    if cfg!(test) || !st.placed || st.utc_offset.is_some() || app.tasks.is_running(ASKING) {
        return;
    }
    let (lat, lon) = (st.lat, st.lon);
    if st.zone.as_ref().is_some_and(|z| z.is_for(lat, lon)) {
        return;
    }
    let key = (lat.to_bits(), lon.to_bits());
    if app.planner.zone_asked == Some(key) {
        return;
    }
    app.planner.zone_asked = Some(key);
    // (an answer is only kept if the place is still the one asked about)
    let _ = crate::tasks::spawn(
        app,
        ASKING,
        None,
        move || crate::services::timezone::fetch(lat, lon),
        move |app, _ctx, got| {
            if let Ok(z) = got {
                let st = &mut app.ui.planner;
                if st.lat.to_bits() == key.0 && st.lon.to_bits() == key.1 {
                    st.zone = Some(ZoneFix { lat, lon, offset_min: z.offset_min, at: super::now_unix(), name: z.name }).and_then(ZoneFix::tidy);
                }
            }
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use light_studio_astro::time::Date;

    fn noon(y: i32, m: u32, d: u32) -> f64 {
        Date::new(y, m, d).unix_midnight_utc() + 43_200.0
    }

    fn fix(lat: f64, lon: f64, offset_min: i32, at: f64) -> ZoneFix {
        ZoneFix { lat, lon, offset_min, at, name: String::new() }
    }

    #[test]
    fn without_an_answer_it_is_the_estimate() {
        assert_eq!(offset_at(40.4, -3.7, noon(2026, 7, 1), None), (120, Source::Estimate));
        let elsewhere = fix(10.0, 10.0, 60, noon(2026, 7, 1));
        assert_eq!(offset_at(40.4, -3.7, noon(2026, 7, 1), Some(&elsewhere)).1, Source::Estimate);
    }

    #[test]
    fn a_wrong_estimate_is_fixed_in_the_same_season_only() {
        // a made-up place the estimate puts one hour off all year: the answer wins in July, not in January
        let f = fix(40.4, -3.7, 180, noon(2026, 7, 1));
        assert_eq!(offset_at(40.4, -3.7, noon(2026, 7, 20), Some(&f)), (180, Source::Real));
        assert_eq!(offset_at(40.4, -3.7, noon(2026, 1, 20), Some(&f)), (60, Source::Estimate));
    }

    #[test]
    fn a_right_estimate_is_confirmed_in_every_season() {
        let f = fix(40.4, -3.7, 120, noon(2026, 7, 1));
        assert_eq!(offset_at(40.4, -3.7, noon(2026, 1, 20), Some(&f)), (60, Source::Real));
        assert_eq!(offset_at(40.4, -3.7, noon(2026, 8, 20), Some(&f)), (120, Source::Real));
    }

    #[test]
    fn hostile_saved_numbers_are_dropped() {
        assert!(fix(f64::NAN, 0.0, 0, 0.0).tidy().is_none());
        assert!(fix(0.0, 0.0, 9999, 0.0).tidy().is_none());
        assert!(fix(0.0, 0.0, 60, 1.0).tidy().is_some());
    }

    #[test]
    fn the_planner_uses_the_answer_and_keeps_it_saved() {
        use super::super::{PlannerState, resolve};
        let now = noon(2026, 7, 1);
        let mut st = PlannerState { date: "2026-07-20".into(), placed: true, lat: 40.4, lon: -3.7, ..Default::default() };
        assert_eq!(resolve(&st, now).1, 120);
        st.zone = Some(fix(40.4, -3.7, 180, now));
        assert_eq!(resolve(&st, now).1, 180, "the answer wins on a day of the same season");
        st.utc_offset = Some(60);
        assert_eq!(resolve(&st, now).1, 60, "a hand-set offset wins over everything");
        st.utc_offset = None;
        let json = serde_json::to_string(&st).unwrap();
        let back: PlannerState = serde_json::from_str(&json).unwrap();
        assert_eq!(back.zone, st.zone);
        let mut bad = PlannerState { zone: Some(fix(0.0, 0.0, 99_999, 0.0)), ..Default::default() };
        bad.sane();
        assert!(bad.zone.is_none());
    }
}
