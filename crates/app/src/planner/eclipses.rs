//! The eclipses coming for the place: the lunar ones and the solar ones that reach it, with their
//! contacts and whether the body is above the horizon, in one list by date.

use light_studio_astro::eclipse::{LunarKind, lunar_eclipses, solar_eclipses};
use light_studio_astro::moon::moon_horizontal;

use super::*;

/// One eclipse in the list.
#[derive(Clone, Debug)]
pub(super) struct Row {
    /// The moment of greatest eclipse (Unix seconds).
    pub unix: f64,
    pub solar: bool,
    pub kind: &'static str,
    pub magnitude: f64,
    /// The body is above the horizon at the greatest eclipse.
    pub visible: bool,
    /// The phases with their times, in order.
    pub times: Vec<(&'static str, f64)>,
    /// For a central eclipse (the shadow reaches the Earth), what it is like on its central line at the greatest
    /// eclipse, wherever that is: the width of the path in km and the seconds the total (or annular) phase lasts there.
    pub central: Option<Central>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Central {
    pub width_km: f64,
    pub duration_s: f64,
}

#[derive(Clone, Debug, Default)]
pub(super) struct Eclipses {
    pub rows: Vec<Row>,
}

/// The eclipses from `from` over about three years.
pub(super) fn compute(obs: Observer, from: f64) -> Eclipses {
    let mut rows = Vec::new();
    for k in 0..3 {
        let a = from + f64::from(k) * 365.0 * DAY;
        let b = a + 365.0 * DAY;
        for e in lunar_eclipses(a, b) {
            let alt = moon_horizontal(e.greatest, obs).alt;
            let kind = match e.kind {
                LunarKind::Total => "Total lunar eclipse",
                LunarKind::Partial => "Partial lunar eclipse",
                LunarKind::Penumbral => "Penumbral lunar eclipse",
            };
            let mut times = Vec::new();
            for (name, t) in [("Penumbra begins", e.p1), ("Partial begins", e.u1), ("Totality begins", e.u2)] {
                times.extend(t.map(|t| (name, t)));
            }
            times.push(("Greatest eclipse", e.greatest));
            for (name, t) in [("Totality ends", e.u3), ("Partial ends", e.u4), ("Penumbra ends", e.p4)] {
                times.extend(t.map(|t| (name, t)));
            }
            rows.push(Row {
                unix: e.greatest,
                solar: false,
                kind,
                magnitude: e.umbral_magnitude.max(e.penumbral_magnitude - 1.0).max(e.umbral_magnitude),
                visible: alt > 0.0,
                times,
                central: None,
            });
        }
        for e in solar_eclipses(a, b, obs) {
            let kind = if e.total {
                "Total solar eclipse"
            } else if e.annular {
                "Annular solar eclipse"
            } else {
                "Partial solar eclipse"
            };
            let mut times = Vec::new();
            for (name, t) in [("Partial begins", e.c1), ("Totality begins", e.c2)] {
                times.extend(t.map(|t| (name, t)));
            }
            times.push(("Greatest eclipse", e.greatest));
            for (name, t) in [("Totality ends", e.c3), ("Partial ends", e.c4)] {
                times.extend(t.map(|t| (name, t)));
            }
            let central = light_studio_astro::besselian::greatest_point(e.new_moon)
                .map(|p| Central { width_km: p.half_width_km.abs() * 2.0, duration_s: p.duration_s });
            rows.push(Row { unix: e.greatest, solar: true, kind, magnitude: e.magnitude, visible: e.sun_alt > 0.0, times, central });
        }
    }
    rows.sort_by(|a, b| a.unix.total_cmp(&b.unix));
    rows.retain(|r| r.unix >= from);
    Eclipses { rows }
}

/// The shadow's track of the first solar eclipse with a central line from a day before `at`, looking ahead about three years.
pub(super) fn next_track(at: f64) -> Option<light_studio_astro::eclipse::EclipsePath> {
    use light_studio_astro::moon::{Principal, next_phase};
    let mut from = at - 86_400.0;
    for _ in 0..40 {
        let nm = next_phase(from, Principal::New)?;
        if let Some(path) = light_studio_astro::eclipse::solar_path(nm) {
            return Some(path);
        }
        from = nm + 86_400.0;
    }
    None
}

impl Page<'_> {
    /// The track of the next central solar eclipse, worked out again when the day moves.
    pub(super) fn eclipse_path(&self, rt: &mut Runtime, at: f64) -> Option<Arc<light_studio_astro::eclipse::EclipsePath>> {
        let day = (at / 86_400.0).floor() as i64;
        if rt.eclipse_track.as_ref().is_none_or(|(d, _)| *d != day) {
            rt.eclipse_track = Some((day, next_track(at).map(Arc::new)));
        }
        rt.eclipse_track.as_ref().and_then(|(_, p)| p.clone())
    }

    /// The eclipse list for the place, worked out again when the place or the month moves.
    pub(super) fn eclipse_rows(&self, st: &PlannerState, rt: &mut Runtime) -> Arc<Eclipses> {
        let bucket = self.c.date.days().div_euclid(30);
        let key: EclipseKey = (st.lat.to_bits(), st.lon.to_bits(), bucket);
        if rt.eclipse_list.as_ref().is_none_or(|(k, _)| *k != key) {
            let from = Date::from_days(bucket * 30).unix_midnight_utc();
            rt.eclipse_list = Some((key, Arc::new(compute(self.obs, from))));
        }
        rt.eclipse_list.as_ref().map(|(_, l)| l.clone()).unwrap_or_default()
    }

    /// The coming eclipses: a click on one moves the time to its greatest moment and opens its phases.
    pub(super) fn eclipses_card(&mut self, ui: &mut Ui, st: &mut PlannerState, rt: &mut Runtime) {
        let t = self.t;
        let list = self.eclipse_rows(st, rt);
        if list.rows.is_empty() {
            note(ui, crate::i18n::tr("No eclipse reaches this place in the next three years."));
            return;
        }
        for (i, r) in list.rows.iter().enumerate() {
            let (cell, _) = ui.allocate_exact_size(vec2(ui.available_width(), 38.0), Sense::hover());
            let resp = ui.interact(cell, egui::Id::new(("planner-eclipse", i)), Sense::click());
            if resp.hovered() {
                ui.painter().rect_filled(cell, 8.0, t.hover.gamma_multiply(0.5));
            }
            let p = ui.painter();
            let color = if r.solar { super::skylayers::SUN_NOW } else { super::skylayers::MOON_NOW };
            p.circle_filled(pos2(cell.left() + 10.0, cell.center().y), 5.0, color);
            if r.solar {
                p.circle_stroke(pos2(cell.left() + 10.0, cell.center().y), 5.0, Stroke::new(1.0, Color32::from_black_alpha(120)));
            }
            let when = format!("{} · {}", short_date(Date::from_unix(r.unix, self.c.offset)), clock(r.unix, self.c.offset));
            p.text(pos2(cell.left() + 26.0, cell.center().y - 8.0), Align2::LEFT_CENTER, crate::i18n::tr(r.kind), t.semibold(12.5), t.text);
            p.text(pos2(cell.left() + 26.0, cell.center().y + 9.0), Align2::LEFT_CENTER, when, t.font(11.5), t.text_dim);
            let (vis, vcolor) =
                if r.visible { ("Visible from here", Color32::from_rgb(90, 200, 120)) } else { ("Not visible from here", t.text_disabled) };
            p.text(
                pos2(cell.right() - 4.0, cell.center().y - 8.0),
                Align2::RIGHT_CENTER,
                format!("{} {:.2}", crate::i18n::tr("Magnitude"), r.magnitude),
                t.font(11.5),
                t.text,
            );
            p.text(pos2(cell.right() - 4.0, cell.center().y + 9.0), Align2::RIGHT_CENTER, crate::i18n::tr(vis), t.font(11.0), vcolor);
            if resp.clicked() {
                super::strip::jump_to(st, r.unix, self.c.offset);
                rt.eclipse_open = if rt.eclipse_open == Some(i) { None } else { Some(i) };
            }
            if rt.eclipse_open == Some(i) {
                for (n, (name, time)) in r.times.iter().enumerate() {
                    row(ui, &format!("eclipse:{i}:{n}"), name, &clock(*time, self.c.offset), None);
                }
                if let Some(c) = r.central {
                    row(ui, &format!("eclipse:{i}:width"), "Width of the path", &format!("{:.0} km", c.width_km), None);
                    let secs = c.duration_s.round().max(0.0) as u32;
                    let long = format!("{} min {:02} s", secs / 60, secs % 60);
                    row(ui, &format!("eclipse:{i}:long"), "Duration on the central line", &long, None);
                }
                ui.add_space(4.0);
            }
        }
        note(ui, crate::i18n::tr("Eclipse times are good to a couple of minutes; magnitudes to a few hundredths."));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use light_studio_astro::time::unix_from_civil;

    #[test]
    fn madrid_sees_the_eclipses_of_august_2026() {
        let obs = Observer::new(40.4168, -3.7038);
        let list = compute(obs, unix_from_civil(2026, 7, 1, 0, 0, 0));
        let solar = list.rows.iter().find(|r| r.solar && (r.unix - unix_from_civil(2026, 8, 12, 18, 30, 0)).abs() < 3600.0);
        let solar = solar.expect("the total eclipse of 12 August is listed");
        assert!(solar.visible && solar.magnitude > 0.95, "{solar:?}");
        assert!(solar.times.iter().any(|(n, _)| *n == "Greatest eclipse"));
        let lunar = list.rows.iter().find(|r| !r.solar && (r.unix - unix_from_civil(2026, 8, 28, 4, 13, 0)).abs() < 3600.0);
        assert!(lunar.is_some(), "the partial lunar eclipse of 28 August is listed");
        // in order, and none before the start
        assert!(list.rows.windows(2).all(|w| w[0].unix <= w[1].unix));
        assert!(list.rows.iter().all(|r| r.unix >= unix_from_civil(2026, 7, 1, 0, 0, 0)));
    }

    #[test]
    fn a_central_eclipse_row_carries_the_width_and_duration_of_its_path() {
        // Madrid sees the partial phase of the eclipse of 2 August 2027, and the row still tells what the path is like
        let obs = Observer::new(40.4168, -3.7038);
        let list = compute(obs, unix_from_civil(2027, 7, 1, 0, 0, 0));
        let row = list.rows.iter().find(|r| r.solar && (r.unix - unix_from_civil(2027, 8, 2, 10, 0, 0)).abs() < 7200.0).expect("listed");
        let c = row.central.expect("a central eclipse");
        assert!((c.width_km - 257.7).abs() < 6.0 && (c.duration_s - 382.6).abs() < 8.0, "{c:?}");
        // a lunar eclipse has none
        assert!(list.rows.iter().filter(|r| !r.solar).all(|r| r.central.is_none()));
    }

    #[test]
    fn the_next_track_from_july_2026_is_the_august_eclipse() {
        let from = light_studio_astro::time::unix_from_civil(2026, 7, 1, 0, 0, 0);
        let path = next_track(from).expect("a central eclipse follows");
        let g = path.points[path.greatest];
        assert!((g.unix - light_studio_astro::time::unix_from_civil(2026, 8, 12, 17, 46, 0)).abs() < 1800.0, "{}", g.unix);
        assert!(g.penumbra_km > 2500.0 && g.penumbra_km < 4500.0, "{}", g.penumbra_km);
    }
}
