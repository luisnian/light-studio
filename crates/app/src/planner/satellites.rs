//! Satellites over the place: the visible passes of the space stations (the satellite lit by the Sun while the sky is dark)
//! and the transits of a satellite across the Sun or the Moon. The orbits come from [`crate::services::satellites`]; the
//! rest is geometry, worked out here for the next few days.

use light_studio_astro::moon::moon_coords;
use light_studio_astro::sun::sun_coords;
use light_studio_astro::time::jd_from_unix;
use light_studio_astro::{Horizontal, Observer, moon_horizontal, sun_horizontal};

use super::*;
use crate::services::satellites::Orbit;

/// How far ahead orbits are trusted: element sets lose accuracy in a few days.
pub(super) const DAYS_AHEAD: f64 = 5.0;
/// A pass counts as visible above this altitude (degrees).
const MIN_ALT: f64 = 10.0;
/// The Sun is low enough for the sky to be dark enough to see a satellite (civil twilight ends).
const DARK_SUN: f64 = -6.0;
/// Seconds between samples while looking for passes.
const PASS_STEP: f64 = 15.0;
/// Seconds between samples while looking for transits (a station crosses a degree in about a second at best).
const TRANSIT_STEP: f64 = 1.0;
/// Closest approach, in degrees, worth refining.
const NEAR_DEG: f64 = 2.5;
const EARTH_KM: f64 = 6378.137;
const SUN_KM: f64 = 696_000.0;
const MOON_KM: f64 = 1737.4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Target {
    Sun,
    Moon,
}

/// One visible pass.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Pass {
    pub name: String,
    pub start: f64,
    pub end: f64,
    pub top: f64,
    pub top_alt: f64,
    pub start_az: f64,
    pub top_az: f64,
    pub end_az: f64,
}

/// One crossing of the Sun's or the Moon's disc.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Transit {
    pub name: String,
    pub target: Target,
    /// The moment of closest approach (Unix seconds).
    pub unix: f64,
    /// Closest approach between centres, degrees.
    pub miss_deg: f64,
    /// The body's radius, degrees.
    pub radius_deg: f64,
    /// How long the satellite is in front of the disc, seconds (0 when it only passes beside).
    pub seconds: f64,
    pub sat: Horizontal,
}

#[derive(Clone, Debug, Default)]
pub(super) struct Report {
    pub passes: Vec<Pass>,
    pub transits: Vec<Transit>,
    /// The age of the oldest element set used at the time of the report, days.
    pub oldest_days: f64,
}

/// Greenwich mean sidereal time, degrees (Meeus 12.4).
fn gmst(jd: f64) -> f64 {
    let t = (jd - 2_451_545.0) / 36_525.0;
    (280.460_618_37 + 360.985_647_366_29 * (jd - 2_451_545.0) + 0.000_387_933 * t * t - t * t * t / 38_710_000.0).rem_euclid(360.0)
}

/// The observer's place in the Earth-fixed frame (km), on the WGS84 ellipsoid at sea level.
fn observer_ecef(obs: Observer) -> [f64; 3] {
    let (lat, lon) = (obs.lat.to_radians(), obs.lon.to_radians());
    let e2 = 0.006_694_379_990_14;
    let n = EARTH_KM / (1.0 - e2 * lat.sin().powi(2)).sqrt();
    [n * lat.cos() * lon.cos(), n * lat.cos() * lon.sin(), n * (1.0 - e2) * lat.sin()]
}

/// Where a satellite at `eci` (km) is in the observer's sky, and its distance (km).
fn look(eci: [f64; 3], jd: f64, obs: Observer) -> (Horizontal, f64) {
    let g = gmst(jd).to_radians();
    let (c, s) = (g.cos(), g.sin());
    let ecef = [c * eci[0] + s * eci[1], -s * eci[0] + c * eci[1], eci[2]];
    let o = observer_ecef(obs);
    let d = [ecef[0] - o[0], ecef[1] - o[1], ecef[2] - o[2]];
    let (lat, lon) = (obs.lat.to_radians(), obs.lon.to_radians());
    let east = -lon.sin() * d[0] + lon.cos() * d[1];
    let north = -lat.sin() * lon.cos() * d[0] - lat.sin() * lon.sin() * d[1] + lat.cos() * d[2];
    let up = lat.cos() * lon.cos() * d[0] + lat.cos() * lon.sin() * d[1] + lat.sin() * d[2];
    let range = (east * east + north * north + up * up).sqrt();
    if range <= 0.0 {
        return (Horizontal { az: 0.0, alt: 90.0 }, 0.0);
    }
    let alt = (up / range).clamp(-1.0, 1.0).asin().to_degrees();
    let az = east.atan2(north).to_degrees().rem_euclid(360.0);
    (Horizontal { az, alt }, range)
}

/// The Sun's direction as a unit vector in the equator-of-date frame.
fn sun_direction(jd: f64) -> [f64; 3] {
    let s = sun_coords(jd);
    let (ra, dec) = (s.ra.to_radians(), s.dec.to_radians());
    [dec.cos() * ra.cos(), dec.cos() * ra.sin(), dec.sin()]
}

/// The satellite is in the Sun's light: outside the Earth's shadow cylinder.
fn sunlit(eci: [f64; 3], sun: [f64; 3]) -> bool {
    let along = eci[0] * sun[0] + eci[1] * sun[1] + eci[2] * sun[2];
    if along >= 0.0 {
        return true;
    }
    let r2 = eci.iter().map(|c| c * c).sum::<f64>();
    (r2 - along * along).max(0.0).sqrt() > EARTH_KM
}

/// The angle between two directions in the sky, degrees.
pub(super) fn separation(a: Horizontal, b: Horizontal) -> f64 {
    let (a1, a2) = (a.alt.to_radians(), b.alt.to_radians());
    let da = (a.az - b.az).to_radians();
    (a1.sin() * a2.sin() + a1.cos() * a2.cos() * da.cos()).clamp(-1.0, 1.0).acos().to_degrees()
}

/// The visible passes of one satellite between `from` and `to`.
fn passes_of(orbit: &Orbit, obs: Observer, from: f64, to: f64) -> Vec<Pass> {
    let mut out: Vec<Pass> = Vec::new();
    let mut open: Option<Pass> = None;
    let mut t = from;
    while t <= to {
        let jd = jd_from_unix(t);
        let visible = orbit.position(t).and_then(|p| {
            let (h, _) = look(p.km, jd, obs);
            let lit = sunlit(p.km, sun_direction(jd));
            (h.alt >= MIN_ALT && lit && sun_horizontal(t, obs).alt <= DARK_SUN).then_some(h)
        });
        match (visible, open.as_mut()) {
            (Some(h), None) => {
                open = Some(Pass { name: orbit.name.clone(), start: t, end: t, top: t, top_alt: h.alt, start_az: h.az, top_az: h.az, end_az: h.az })
            }
            (Some(h), Some(p)) => {
                p.end = t;
                p.end_az = h.az;
                if h.alt > p.top_alt {
                    (p.top, p.top_alt, p.top_az) = (t, h.alt, h.az);
                }
            }
            (None, Some(_)) => out.extend(open.take()),
            (None, None) => {}
        }
        t += PASS_STEP;
    }
    out.extend(open);
    out
}

/// The transits found by walking `from..to`: `sat` gives the satellite's direction (if it can be followed), `body` the
/// body's direction and radius. Closest approaches under [`NEAR_DEG`] are refined, and kept when the centre passes within
/// the radius plus a little.
pub(super) fn transits_between(
    name: &str,
    target: Target,
    from: f64,
    to: f64,
    sat: &dyn Fn(f64) -> Option<Horizontal>,
    body: &dyn Fn(f64) -> (Horizontal, f64),
) -> Vec<Transit> {
    let sep_at = |t: f64| -> Option<f64> {
        let s = sat(t)?;
        let (b, _) = body(t);
        Some(separation(s, b))
    };
    let mut out = Vec::new();
    let mut prev: Option<f64> = None;
    let mut cur: Option<f64> = None;
    let mut t = from;
    while t <= to + TRANSIT_STEP {
        let next = sat(t).filter(|s| s.alt > 0.0).and_then(|_| sep_at(t));
        // a local minimum at the middle sample
        if let (Some(a), Some(b), Some(c)) = (prev, cur, next)
            && b <= a
            && b <= c
            && b < NEAR_DEG
        {
            let (mut lo, mut hi) = (t - 2.0 * TRANSIT_STEP, t);
            for _ in 0..50 {
                let (m1, m2) = (lo + (hi - lo) / 3.0, hi - (hi - lo) / 3.0);
                match (sep_at(m1), sep_at(m2)) {
                    (Some(x), Some(y)) if x < y => hi = m2,
                    (Some(_), Some(_)) => lo = m1,
                    _ => break,
                }
            }
            let tm = (lo + hi) / 2.0;
            if let (Some(miss), Some(h)) = (sep_at(tm), sat(tm)) {
                let (_, radius) = body(tm);
                if miss <= radius * 1.5 && h.alt > 0.0 {
                    // how fast the satellite moves against the body, from the samples a tenth of a second either side
                    let rel = match (sat(tm - 0.1), sat(tm + 0.1)) {
                        (Some(a), Some(b)) => {
                            let (sa, sb) = (relative(a, body(tm - 0.1).0), relative(b, body(tm + 0.1).0));
                            (sb.0 - sa.0).hypot(sb.1 - sa.1) / 0.2
                        }
                        _ => 0.0,
                    };
                    let seconds = if miss < radius && rel > 1e-6 { 2.0 * (radius * radius - miss * miss).sqrt() / rel } else { 0.0 };
                    out.push(Transit { name: name.to_string(), target, unix: tm, miss_deg: miss, radius_deg: radius, seconds, sat: h });
                }
            }
        }
        prev = cur;
        cur = next;
        t += TRANSIT_STEP;
    }
    out
}

/// Where `a` is on the plane tangent to the sky at `b` (degrees east, north): a flat map good for small angles.
fn relative(a: Horizontal, b: Horizontal) -> (f64, f64) {
    let dx = (a.az - b.az + 540.0).rem_euclid(360.0) - 180.0;
    (dx * b.alt.to_radians().cos(), a.alt - b.alt)
}

/// The passes and the transits of every orbit for the next [`DAYS_AHEAD`] days.
pub(super) fn compute(orbits: &[Orbit], obs: Observer, from: f64) -> Report {
    let to = from + DAYS_AHEAD * DAY;
    let mut report = Report::default();
    for o in orbits {
        report.passes.extend(passes_of(o, obs, from, to));
        let sat = |t: f64| o.position(t).map(|p| look(p.km, jd_from_unix(t), obs).0);
        let sun = |t: f64| {
            let jd = jd_from_unix(t);
            let c = sun_coords(jd);
            (sun_horizontal(t, obs), (SUN_KM / c.dist_km).asin().to_degrees())
        };
        let moon = |t: f64| {
            let c = moon_coords(jd_from_unix(t));
            (moon_horizontal(t, obs), (MOON_KM / c.dist_km).asin().to_degrees())
        };
        // the Sun only by day, the Moon whenever it is up: coarse gates keep the walk short
        report.transits.extend(transits_between(&o.name, Target::Sun, from, to, &|t| sat(t).filter(|_| sun_horizontal(t, obs).alt > -1.0), &sun));
        report.transits.extend(transits_between(&o.name, Target::Moon, from, to, &|t| sat(t).filter(|_| moon_horizontal(t, obs).alt > 0.0), &moon));
        report.oldest_days = report.oldest_days.max((from - o.epoch) / DAY);
    }
    report.passes.sort_by(|a, b| a.start.total_cmp(&b.start));
    report.transits.sort_by(|a, b| a.unix.total_cmp(&b.unix));
    report
}

/// The orbits are fetched (and kept six hours) and the report worked out, on one thread, while the section is on screen.
pub(super) fn ask(app: &mut crate::App) {
    let wanted = std::mem::take(&mut app.planner.sats_wanted);
    if cfg!(test) || !wanted || app.planner.sats_pending || !app.ui.planner.placed {
        return;
    }
    let (lat, lon) = (app.ui.planner.lat, app.ui.planner.lon);
    let now = now_unix();
    let key = ((lat * 100.0).round() as i32, (lon * 100.0).round() as i32, (now / 3600.0).floor() as i64);
    if app.planner.sats.as_ref().is_some_and(|(k, _)| *k == key) {
        return;
    }
    // a failure is tried again in ten minutes, the orbits are kept six hours
    let cached = match &app.planner.sats_orbits {
        Some((at, Ok(o))) if now - at < 6.0 * 3600.0 => Some(o.clone()),
        Some((at, Err(_))) if now - at < 600.0 => return,
        _ => None,
    };
    app.planner.sats_pending = true;
    let started = crate::tasks::spawn(
        app,
        "Finding satellite passes",
        None,
        move || {
            let orbits = match cached {
                Some(o) => o,
                None => Arc::new(crate::services::satellites::fetch()?),
            };
            let report = compute(&orbits, Observer::new(lat, lon), now);
            Ok::<_, String>((orbits, report))
        },
        move |app, _ctx, got| {
            app.planner.sats_pending = false;
            match got {
                Ok((orbits, report)) => {
                    app.planner.sats_orbits = Some((now_unix(), Ok(orbits)));
                    app.planner.sats = Some((key, Arc::new(report)));
                }
                Err(e) => {
                    app.planner.sats_orbits = Some((now_unix(), Err(e)));
                    app.planner.sats = None;
                }
            }
        },
    );
    if started.is_err() {
        app.planner.sats_pending = false;
    }
}

impl Page<'_> {
    /// The visible passes and the transits across the Sun and the Moon for the next days.
    pub(super) fn satellites_card(&mut self, ui: &mut Ui, st: &mut PlannerState, rt: &mut Runtime) {
        rt.sats_wanted = true;
        let t = self.t;
        let Some((_, report)) = rt.sats.clone() else {
            let text = match &rt.sats_orbits {
                Some((_, Err(e))) => format!("{} ({e})", crate::i18n::tr("The satellite orbits could not be fetched.")),
                _ => crate::i18n::tr("Working out the satellite passes…").to_string(),
            };
            note(ui, &text);
            return;
        };
        if report.passes.is_empty() && report.transits.is_empty() {
            note(ui, crate::i18n::tr("No visible pass or transit in the next days."));
        }
        let off = self.c.offset;
        for p in report.passes.iter().take(12) {
            let (cell, _) = ui.allocate_exact_size(vec2(ui.available_width(), 38.0), Sense::hover());
            let resp = ui.interact(cell, egui::Id::new(("planner-sat-pass", p.start.to_bits())), Sense::click());
            if resp.hovered() {
                ui.painter().rect_filled(cell, 8.0, t.hover.gamma_multiply(0.5));
            }
            let pt = ui.painter();
            pt.circle_filled(pos2(cell.left() + 10.0, cell.center().y), 4.0, t.caution);
            let when = format!("{} · {}–{}", short_date(Date::from_unix(p.start, off)), clock(p.start, off), clock(p.end, off));
            pt.text(pos2(cell.left() + 26.0, cell.center().y - 8.0), Align2::LEFT_CENTER, &p.name, t.semibold(12.5), t.text);
            pt.text(pos2(cell.left() + 26.0, cell.center().y + 9.0), Align2::LEFT_CENTER, when, t.font(11.5), t.text_dim);
            let path = format!("{} → {} → {}", compass(p.start_az), compass(p.top_az), compass(p.end_az));
            pt.text(pos2(cell.right() - 4.0, cell.center().y - 8.0), Align2::RIGHT_CENTER, format!("{:.0}°", p.top_alt), t.font(11.5), t.text);
            pt.text(pos2(cell.right() - 4.0, cell.center().y + 9.0), Align2::RIGHT_CENTER, path, t.font(11.0), t.text_dim);
            if resp.clicked() {
                super::strip::jump_to(st, p.top, off);
            }
        }
        if !report.transits.is_empty() {
            subhead(ui, "Transits across the Sun and the Moon");
        }
        for tr in report.transits.iter().take(12) {
            let (cell, _) = ui.allocate_exact_size(vec2(ui.available_width(), 38.0), Sense::hover());
            let resp = ui.interact(cell, egui::Id::new(("planner-sat-transit", tr.unix.to_bits())), Sense::click());
            if resp.hovered() {
                ui.painter().rect_filled(cell, 8.0, t.hover.gamma_multiply(0.5));
            }
            let pt = ui.painter();
            let color = if tr.target == Target::Sun { super::skylayers::SUN_NOW } else { super::skylayers::MOON_NOW };
            pt.circle_filled(pos2(cell.left() + 10.0, cell.center().y), 5.0, color);
            let body = crate::i18n::tr(if tr.target == Target::Sun { "Sun" } else { "Moon" });
            let what = if tr.seconds > 0.0 {
                format!("{} {} · {:.1} s", crate::i18n::tr("Transit across the"), body.to_lowercase(), tr.seconds)
            } else {
                format!("{} {}", crate::i18n::tr("Passes beside the"), body.to_lowercase())
            };
            let when = format!("{} · {}", short_date(Date::from_unix(tr.unix, off)), clock_seconds(tr.unix, off));
            pt.text(pos2(cell.left() + 26.0, cell.center().y - 8.0), Align2::LEFT_CENTER, format!("{} · {what}", tr.name), t.semibold(12.0), t.text);
            pt.text(pos2(cell.left() + 26.0, cell.center().y + 9.0), Align2::LEFT_CENTER, when, t.font(11.5), t.text_dim);
            pt.text(
                pos2(cell.right() - 4.0, cell.center().y - 8.0),
                Align2::RIGHT_CENTER,
                format!("{:.0}° {}", tr.sat.alt, compass(tr.sat.az)),
                t.font(11.5),
                t.text,
            );
            pt.text(
                pos2(cell.right() - 4.0, cell.center().y + 9.0),
                Align2::RIGHT_CENTER,
                format!("{} {:.2}°", crate::i18n::tr("miss"), tr.miss_deg),
                t.font(11.0),
                t.text_dim,
            );
            if resp.clicked() {
                super::strip::jump_to(st, tr.unix, off);
            }
        }
        note(
            ui,
            crate::i18n::tr(
                "Orbits from CelesTrak, good for a few days. A transit lasts about a second and its line on the ground is only a few kilometres wide: check it again the day before.",
            ),
        );
    }
}

/// `hh:mm:ss` at the offset.
fn clock_seconds(unix: f64, offset: i32) -> String {
    let s = (unix + f64::from(offset) * 60.0).rem_euclid(86_400.0) as u32;
    format!("{:02}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::satellites::{TEST_TLE, parse};

    fn fixed_body(az: f64, alt: f64, radius: f64) -> impl Fn(f64) -> (Horizontal, f64) {
        move |_| (Horizontal { az, alt }, radius)
    }

    /// A satellite crossing the sky at `speed` degrees a second along a line `offset` degrees above the body's centre.
    fn crossing(offset: f64, speed: f64, alt: f64) -> impl Fn(f64) -> Option<Horizontal> {
        move |t| Some(Horizontal { az: 180.0 + (t - 100.0) * speed / alt.to_radians().cos(), alt: alt + offset })
    }

    #[test]
    fn a_satellite_across_the_disc_is_a_transit_with_its_duration() {
        let found = transits_between("X", Target::Sun, 0.0, 200.0, &crossing(0.1, 0.6, 40.0), &fixed_body(180.0, 40.0, 0.27));
        assert_eq!(found.len(), 1, "{found:?}");
        let t = &found[0];
        assert!((t.unix - 100.0).abs() < 0.05, "{}", t.unix);
        assert!((t.miss_deg - 0.1).abs() < 0.01, "{}", t.miss_deg);
        let want = 2.0 * (0.27f64 * 0.27 - 0.1 * 0.1).sqrt() / 0.6;
        assert!((t.seconds - want).abs() < 0.05, "{} {want}", t.seconds);
    }

    #[test]
    fn a_satellite_that_misses_is_not_a_transit() {
        let none = transits_between("X", Target::Sun, 0.0, 200.0, &crossing(0.8, 0.6, 40.0), &fixed_body(180.0, 40.0, 0.27));
        assert!(none.is_empty(), "{none:?}");
        // near the limb counts as a graze with no time in front
        let graze = transits_between("X", Target::Sun, 0.0, 200.0, &crossing(0.35, 0.6, 40.0), &fixed_body(180.0, 40.0, 0.27));
        assert_eq!(graze.len(), 1);
        assert_eq!(graze[0].seconds, 0.0);
    }

    #[test]
    fn a_satellite_below_the_horizon_is_never_a_transit() {
        let below = |t: f64| Some(Horizontal { az: 180.0 + (t - 100.0) * 0.6, alt: -5.0 });
        let found = transits_between("X", Target::Moon, 0.0, 200.0, &below, &fixed_body(180.0, -5.0, 0.26));
        assert!(found.is_empty());
    }

    #[test]
    fn the_satellite_comes_out_where_the_geometry_says() {
        // directly overhead: a point 400 km above the observer
        let obs = Observer::new(43.3, -2.0);
        let o = observer_ecef(obs);
        let n = o.iter().map(|c| c * c).sum::<f64>().sqrt();
        let jd = 2_460_000.5;
        // the observer's own direction, in the inertial frame, 400 km up
        let g = gmst(jd).to_radians();
        let up = [o[0] / n, o[1] / n, o[2] / n];
        let r = [
            (o[0] + 400.0 * up[0]) * g.cos() - (o[1] + 400.0 * up[1]) * g.sin(),
            (o[0] + 400.0 * up[0]) * g.sin() + (o[1] + 400.0 * up[1]) * g.cos(),
            o[2] + 400.0 * up[2],
        ];
        let (h, range) = look(r, jd, obs);
        assert!(h.alt > 88.0, "{h:?}");
        assert!((range - 400.0).abs() < 25.0, "{range}");
    }

    #[test]
    fn the_shadow_of_the_earth_hides_a_satellite() {
        let sun = [1.0, 0.0, 0.0];
        assert!(sunlit([7000.0, 0.0, 0.0], sun));
        assert!(!sunlit([-7000.0, 0.0, 0.0], sun), "straight behind the Earth");
        assert!(sunlit([-7000.0, 7000.0, 0.0], sun), "far to the side");
    }

    #[test]
    fn the_iss_has_passes_at_a_mid_latitude_and_they_are_sane() {
        let orbits = parse(TEST_TLE).unwrap();
        let obs = Observer::new(43.3, -2.0);
        let from = orbits[0].epoch;
        let passes: Vec<Pass> = passes_of(&orbits[0], obs, from, from + 5.0 * DAY);
        assert!(!passes.is_empty(), "five days at 43 N should include a visible pass");
        for p in &passes {
            assert!(p.top_alt >= MIN_ALT && p.top_alt <= 90.0, "{p:?}");
            assert!(p.start <= p.top && p.top <= p.end);
            assert!(p.end - p.start < 15.0 * 60.0, "a pass is minutes long: {p:?}");
            assert!((0.0..360.0).contains(&p.top_az));
        }
        // visible passes only happen in the dark
        assert!(passes.iter().all(|p| sun_horizontal(p.top, obs).alt <= DARK_SUN));
    }

    #[test]
    fn the_whole_report_runs_and_stays_in_order() {
        let orbits = parse(TEST_TLE).unwrap();
        let r = compute(&orbits, Observer::new(43.3, -2.0), orbits[0].epoch);
        assert!(r.passes.windows(2).all(|w| w[0].start <= w[1].start));
        assert!(r.transits.windows(2).all(|w| w[0].unix <= w[1].unix));
        assert!(r.transits.iter().all(|t| t.miss_deg <= t.radius_deg * 1.5 + 1e-9));
    }
}
