//! The Sky calendar: the mode menu (Sun, Moon, Night) that heads the side panel's sky tab, and the year of days under
//! it. Each day shows what matters for the mode: how long the day is (Sun), the Moon's disc (Moon), or the Moon's disc
//! inside a ring that fills with the hours of the night without Moon (Night). Hover a day for the figures, click it
//! to plan it.

use light_studio_astro::Span;
use light_studio_astro::moon::principal_phases;
use light_studio_astro::plan::plan_day;
use light_studio_astro::sun::sun_coords;
use light_studio_astro::time::{days_in_month, jd_from_unix};

use super::year::minus;
use super::*;

const WEEKDAYS_MONDAY_FIRST: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const CELL_H: f32 = 44.0;
/// How tall the year's scrolling window is.
const WINDOW_H: f32 = 430.0;

/// What the calendar shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SkyMode {
    Sun,
    Moon,
    Night,
}

pub(super) const MODES: [SkyMode; 3] = [SkyMode::Sun, SkyMode::Moon, SkyMode::Night];

impl SkyMode {
    /// The mode a tab belongs to (the Milky Way calendar lives with the night).
    pub(super) fn of(tab: Tab) -> SkyMode {
        match tab {
            Tab::Moon => SkyMode::Moon,
            Tab::Night | Tab::Calendar => SkyMode::Night,
            _ => SkyMode::Sun,
        }
    }

    pub(super) fn tab(self) -> Tab {
        match self {
            SkyMode::Sun => Tab::Sun,
            SkyMode::Moon => Tab::Moon,
            SkyMode::Night => Tab::Night,
        }
    }

    fn label(self) -> &'static str {
        match self {
            SkyMode::Sun => "Sun",
            SkyMode::Moon => "Moon",
            SkyMode::Night => "Night",
        }
    }
}

fn principal_label(p: Principal) -> &'static str {
    match p {
        Principal::New => "New Moon",
        Principal::FirstQuarter => "First Quarter",
        Principal::Full => "Full Moon",
        Principal::LastQuarter => "Last Quarter",
    }
}

/// Minutes of daylight at latitude `lat` on the day whose local noon is `noon_unix` (the Sun's upper limb at the
/// horizon, refraction included); 0 in the polar night, 1440 in the midnight sun.
pub(super) fn day_length_minutes(lat: f64, noon_unix: f64) -> f64 {
    let dec = sun_coords(jd_from_unix(noon_unix)).dec.to_radians();
    let phi = lat.clamp(-90.0, 90.0).to_radians();
    let cos_h = ((-50.0f64 / 60.0).to_radians().sin() - phi.sin() * dec.sin()) / (phi.cos() * dec.cos());
    if !cos_h.is_finite() {
        return 0.0;
    }
    if cos_h >= 1.0 {
        0.0
    } else if cos_h <= -1.0 {
        1440.0
    } else {
        2.0 * cos_h.acos().to_degrees() * 4.0
    }
}

fn spans_minutes(s: &[Span]) -> f64 {
    s.iter().map(Span::minutes).sum()
}

/// The hours of a night: the astronomical dark (else the whole night) and how much of it has no Moon.
fn night_figures(n: &NightProfile) -> (f64, f64) {
    let dark = spans_minutes(&n.dark);
    if dark > 0.0 {
        (dark, spans_minutes(&minus(&n.dark, &n.moon_up)))
    } else {
        (spans_minutes(&n.night), spans_minutes(&minus(&n.night, &n.moon_up)))
    }
}

impl Page<'_> {
    /// The Sun / Moon / Night menu: a closed row with the mode's icon, its name and a chevron; open, the three choices.
    pub(super) fn sky_menu(&mut self, ui: &mut Ui, st: &mut PlannerState, rt: &mut Runtime) {
        let t = self.t;
        let mode = SkyMode::of(st.tab);
        let w = ui.available_width();
        let (row, resp) = ui.allocate_exact_size(vec2(w, 40.0), Sense::click());
        let p = ui.painter();
        p.rect_filled(row, R_CONTROL + 2.0, t.inset);
        let edge = if rt.sky_menu || resp.hovered() { t.accent } else { t.field_border };
        p.rect_stroke(row, R_CONTROL + 2.0, Stroke::new(1.5, edge), StrokeKind::Inside);
        self.sky_icon(p, pos2(row.left() + 24.0, row.center().y), 11.0, mode);
        p.text(pos2(row.left() + 48.0, row.center().y), Align2::LEFT_CENTER, crate::i18n::tr(mode.label()), t.font(14.0), t.text);
        crate::icons::paint(p, Rect::from_center_size(pos2(row.right() - 18.0, row.center().y), vec2(13.0, 13.0)), Icon::ChevronDown, t.text_dim);
        if resp.clicked() {
            rt.sky_menu = !rt.sky_menu;
        }
        if rt.sky_menu {
            ui.add_space(2.0);
            for m in MODES {
                let (r, resp) = ui.allocate_exact_size(vec2(w, 36.0), Sense::click());
                let p = ui.painter();
                if m == mode {
                    p.rect_filled(r, R_CONTROL, t.tool_active);
                } else if resp.hovered() {
                    p.rect_filled(r, R_CONTROL, t.hover.gamma_multiply(0.6));
                }
                self.sky_icon(p, pos2(r.left() + 24.0, r.center().y), 9.5, m);
                p.text(pos2(r.left() + 48.0, r.center().y), Align2::LEFT_CENTER, crate::i18n::tr(m.label()), t.font(13.0), t.text);
                if resp.clicked() {
                    st.tab = m.tab();
                    rt.sky_menu = false;
                }
            }
        }
        ui.add_space(8.0);
    }

    /// The mode's icon: a sun with rays, the Moon as it stands, or a dark disc with stars.
    fn sky_icon(&self, p: &egui::Painter, c: Pos2, r: f32, mode: SkyMode) {
        let t = self.t;
        match mode {
            SkyMode::Sun => {
                p.circle_filled(c, r * 0.55, self.pal.sun);
                for k in 0..8 {
                    let a = k as f32 * std::f32::consts::FRAC_PI_4;
                    let (s, co) = a.sin_cos();
                    p.line_segment([c + vec2(co, s) * r * 0.8, c + vec2(co, s) * r * 1.05], Stroke::new(1.4, self.pal.sun));
                }
            }
            SkyMode::Moon => {
                let (lit, shade) = moon_colors(true, self.pal, t.inset);
                let phase = moon_phase(self.now);
                moon_disc(p, c, r, phase.elongation, self.obs.lat < 0.0, lit, shade, t.divider);
            }
            SkyMode::Night => {
                p.circle_filled(c, r, Color32::from_rgb(10, 12, 24));
                p.circle_stroke(c, r, Stroke::new(1.0, t.divider));
                for (dx, dy, s) in [(-0.35f32, -0.3f32, 1.3f32), (0.3, -0.05, 1.0), (-0.05, 0.4, 1.1)] {
                    p.circle_filled(c + vec2(dx, dy) * r, s, Color32::from_gray(225));
                }
            }
        }
    }

    /// The year of the day shown: a header (year, paging, back to today) and the months in a scrolling window.
    pub(super) fn sky_calendar(&mut self, ui: &mut Ui, mode: SkyMode, rt: &mut Runtime) {
        let t = self.t;
        let c = self.c;
        let year = c.date.year;
        let today = Date::from_unix(self.now, c.offset);
        let jump_year = |delta: i32| {
            let y = year.saturating_add(delta);
            Date::new(y, c.date.month, c.date.day.min(days_in_month(y, c.date.month)))
        };
        ui.horizontal(|ui| {
            if icon_button(ui, "plannerCalPrev", Icon::ChevronLeft, vec2(26.0, 26.0), false, true, "Previous Year").clicked() {
                self.intents.push(Intent::JumpTo(jump_year(-1)));
            }
            ui.label(egui::RichText::new(year.to_string()).font(t.semibold(14.0)).color(t.text));
            if icon_button(ui, "plannerCalNext", Icon::ChevronRight, vec2(26.0, 26.0), false, true, "Next Year").clicked() {
                self.intents.push(Intent::JumpTo(jump_year(1)));
            }
            if icon_button(ui, "plannerCalToday", Icon::Clock, vec2(26.0, 26.0), false, today != c.date, "Today").clicked() {
                self.intents.push(Intent::JumpTo(today));
            }
        });
        ui.add_space(4.0);

        // the new, quarter and full moons of the year, found once
        let key = (year, c.offset);
        if rt.year_phases.as_ref().is_none_or(|(k, _)| *k != key) {
            let start = Date::new(year, 1, 1).unix_midnight_utc() - f64::from(c.offset) * 60.0;
            let end = Date::new(year.saturating_add(1), 1, 1).unix_midnight_utc() - f64::from(c.offset) * 60.0;
            rt.year_phases = Some((key, principal_phases(start, end)));
        }
        let phases: Vec<(f64, Principal)> = rt.year_phases.as_ref().map(|(_, v)| v.clone()).unwrap_or_default();

        let scroll_key = (year, c.date.month);
        let want_scroll = rt.cal_scrolled != Some(scroll_key);
        egui::ScrollArea::vertical().id_salt(("planner-skycal", year)).max_height(WINDOW_H).auto_shrink([false, true]).show(ui, |ui| {
            for month in 1..=12u32 {
                let month_top = self.sky_month(ui, mode, year, month, today, &phases);
                if want_scroll && month == c.date.month {
                    ui.scroll_to_rect(month_top, Some(Align::TOP));
                }
            }
        });
        rt.cal_scrolled = Some(scroll_key);
        ui.add_space(4.0);
        let help = match mode {
            SkyMode::Sun => "The length of each day; hover a day for sunrise, sunset and the golden hour.",
            SkyMode::Moon => "The Moon at 22:00 and how much of it is lit. A dot marks a new, quarter or full moon.",
            SkyMode::Night => "The Moon as it stands in the night; the ring fills with the hours of the night without Moon.",
        };
        note(ui, crate::i18n::tr(help));
    }

    /// One month: its name, the weekdays and the days. Returns the rectangle it took (to scroll to it).
    fn sky_month(&mut self, ui: &mut Ui, mode: SkyMode, year: i32, month: u32, today: Date, phases: &[(f64, Principal)]) -> Rect {
        let t = self.t;
        let c = self.c;
        let w = ui.available_width();
        let cell_w = w / 7.0;
        let first = Date::new(year, month, 1);
        let lead = ((first.weekday() + 6) % 7) as usize;
        let days = days_in_month(year, month) as usize;
        let rows = (lead + days).div_ceil(7);
        let head_h = 30.0;
        let (grid, _) = ui.allocate_exact_size(vec2(w, head_h + 16.0 + rows as f32 * CELL_H + 6.0), Sense::hover());

        let p = ui.painter().clone();
        let name = crate::i18n::tr_ctx("month", MONTHS.get(month as usize - 1).copied().unwrap_or(""));
        p.text(pos2(grid.center().x, grid.top() + 15.0), Align2::CENTER_CENTER, name, t.semibold(13.0), t.text);
        for (i, wd) in WEEKDAYS_MONDAY_FIRST.iter().enumerate() {
            p.text(
                pos2(grid.left() + cell_w * (i as f32 + 0.5), grid.top() + head_h + 6.0),
                Align2::CENTER_CENTER,
                crate::i18n::tr_ctx("weekday", wd),
                t.font(10.0),
                t.text_dim,
            );
        }
        let (lit, shade) = moon_colors(ui.visuals().dark_mode, self.pal, t.inset);
        for day in 1..=days as u32 {
            let index = lead + day as usize - 1;
            let (col, row) = (index % 7, index / 7);
            let cell =
                Rect::from_min_size(pos2(grid.left() + cell_w * col as f32, grid.top() + head_h + 16.0 + CELL_H * row as f32), vec2(cell_w, CELL_H))
                    .shrink(1.5);
            let date = Date::new(year, month, day);
            let day_start = date.unix_midnight_utc() - f64::from(c.offset) * 60.0;
            let resp = ui.interact(cell, ui.id().with(("planner-skycal", year, month, day)), Sense::click());
            if date == c.date {
                p.rect_filled(cell, R_CONTROL, t.tool_active);
            } else if resp.hovered() {
                p.rect_filled(cell, R_CONTROL, t.hover.gamma_multiply(0.6));
            }
            let number_color = if date == c.date { t.text } else { t.text_label };
            p.text(pos2(cell.center().x, cell.top() + 8.0), Align2::CENTER_CENTER, day.to_string(), t.semibold(10.5), number_color);
            if date == today {
                p.line_segment(
                    [pos2(cell.center().x - 5.0, cell.top() + 15.5), pos2(cell.center().x + 5.0, cell.top() + 15.5)],
                    Stroke::new(1.5, t.accent),
                );
            }
            let at = pos2(cell.center().x, cell.top() + 28.0);
            let mirrored = self.obs.lat < 0.0;
            match mode {
                SkyMode::Sun => {
                    let minutes = day_length_minutes(self.obs.lat, day_start + DAY / 2.0);
                    let bar = Rect::from_center_size(at, vec2((cell_w - 10.0).min(30.0), 6.0));
                    p.rect_filled(bar, 3.0, t.track);
                    let fill = Rect::from_min_size(bar.min, vec2(bar.width() * (minutes / 1440.0).clamp(0.0, 1.0) as f32, bar.height()));
                    p.rect_filled(fill, 3.0, self.pal.sun);
                    p.text(
                        pos2(cell.center().x, cell.bottom() - 4.0),
                        Align2::CENTER_CENTER,
                        format!("{:.1}h", minutes / 60.0),
                        t.font(9.0),
                        t.text_dim,
                    );
                }
                SkyMode::Moon => {
                    let phase = moon_phase(day_start + 22.0 * 3600.0);
                    moon_disc(&p, at, 10.0, phase.elongation, mirrored, lit, shade, t.divider);
                    if phases.iter().any(|(when, _)| *when >= day_start && *when < day_start + DAY) {
                        p.circle_filled(pos2(cell.right() - 6.0, cell.top() + 7.0), 3.0, t.accent);
                    }
                }
                SkyMode::Night => {
                    let phase = moon_phase(day_start + DAY);
                    moon_disc(&p, at, 7.5, phase.elongation, mirrored, lit, shade, t.divider);
                    let (total, moonless) = self.year.get(date.ordinal() as usize - 1).map_or((0.0, 0.0), night_figures);
                    ring(&p, at, 12.0, if total > 0.0 { moonless / total } else { 0.0 }, t.track, self.pal.sun);
                }
            }
            if resp.clicked() {
                self.intents.push(Intent::JumpTo(date));
            }
            if resp.hovered() {
                self.day_tip(&resp, mode, date, day_start, phases);
            }
        }
        grid
    }

    /// What a day says when the pointer rests on it.
    fn day_tip(&self, resp: &egui::Response, mode: SkyMode, date: Date, day_start: f64, phases: &[(f64, Principal)]) {
        let t = self.t;
        let c = self.c;
        let obs = self.obs;
        let (lit, shade) = moon_colors(true, self.pal, t.inset);
        let mirrored = obs.lat < 0.0;
        let night = self.year.get(date.ordinal() as usize - 1).cloned();
        let title = match mode {
            SkyMode::Night => format!("{} {} → {}", crate::i18n::tr("Night of"), short_date(date), short_date(date.add_days(1))),
            _ => short_date(date),
        };
        let accent = t.accent;
        let text = t.text;
        let dim = t.text_dim;
        let divider = t.divider;
        let track = t.track;
        let sun = self.pal.sun;
        let plan = (mode != SkyMode::Night).then(|| plan_day(obs, day_start));
        egui::Tooltip::always_open(resp.ctx.clone(), resp.layer_id, resp.id.with("tip"), egui::PopupAnchor::Pointer).show(|ui| {
            ui.set_max_width(290.0);
            ui.label(egui::RichText::new(title).font(t.semibold(13.0)).color(accent));
            ui.add_space(4.0);
            let fact = |ui: &mut Ui, label: &str, value: String| {
                ui.label(egui::RichText::new(crate::i18n::tr(label)).font(t.font(10.5)).color(dim));
                ui.label(egui::RichText::new(value).font(t.semibold(12.5)).color(text));
                ui.add_space(3.0);
            };
            match mode {
                SkyMode::Sun => {
                    if let Some(plan) = &plan {
                        let minutes = day_length_minutes(obs.lat, day_start + DAY / 2.0);
                        fact(ui, "Sunrise", plan.sun.rise.map_or("--:--".into(), |v| clock(v, c.offset)));
                        fact(ui, "Sunset", plan.sun.set.map_or("--:--".into(), |v| clock(v, c.offset)));
                        fact(ui, "Day length", hm(minutes));
                        let golden =
                            |s: Option<Span>| s.map_or("--:--".into(), |s| format!("{}–{}", clock(s.start, c.offset), clock(s.end, c.offset)));
                        fact(ui, "Golden hour", format!("{}  ·  {}", golden(plan.sun.golden_am), golden(plan.sun.golden_pm)));
                    }
                }
                SkyMode::Moon | SkyMode::Night => {
                    let phase = moon_phase(if mode == SkyMode::Moon { day_start + 22.0 * 3600.0 } else { day_start + DAY });
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(vec2(46.0, 46.0), Sense::hover());
                        moon_disc(ui.painter(), r.center(), 14.0, phase.elongation, mirrored, lit, shade, divider);
                        if mode == SkyMode::Night {
                            let (total, moonless) = night.as_ref().map_or((0.0, 0.0), night_figures);
                            ring(ui.painter(), r.center(), 19.0, if total > 0.0 { moonless / total } else { 0.0 }, track, sun);
                        }
                        ui.vertical(|ui| {
                            fact(ui, "Moon phase", format!("{} {:.1}%", crate::i18n::tr(phase.name.label()), phase.illumination * 100.0));
                            if mode == SkyMode::Night {
                                let (total, moonless) = night.as_ref().map_or((0.0, 0.0), night_figures);
                                let v = if total <= 0.0 {
                                    crate::i18n::tr("No dark sky tonight").to_string()
                                } else if moonless <= 0.5 {
                                    crate::i18n::tr("The Moon is up all night").to_string()
                                } else if moonless >= total - 0.5 {
                                    crate::i18n::tr("No Moon all night").to_string()
                                } else {
                                    format!("{} {}", hm(moonless), crate::i18n::tr("without Moon"))
                                };
                                fact(ui, "Night hours without Moon", v);
                            }
                        });
                    });
                    if let Some(plan) = &plan {
                        fact(ui, "Moonrise", plan.moon.rise.map_or("--:--".into(), |v| clock(v, c.offset)));
                        fact(ui, "Moonset", plan.moon.set.map_or("--:--".into(), |v| clock(v, c.offset)));
                    }
                    if let Some((at, which)) = phases.iter().find(|(when, _)| *when >= day_start && *when < day_start + DAY) {
                        fact(ui, principal_label(*which), clock(*at, c.offset));
                    }
                }
            }
        });
    }
}

/// A ring of radius `r` round `c`: the whole circle faint and the first `fraction` of it (from the top, clockwise) bright.
fn ring(p: &egui::Painter, c: Pos2, r: f32, fraction: f64, track: Color32, color: Color32) {
    p.circle_stroke(c, r, Stroke::new(2.4, track));
    let f = fraction.clamp(0.0, 1.0) as f32;
    if f <= 0.003 {
        return;
    }
    let n = ((f * 64.0).ceil() as usize).max(2);
    let pts: Vec<Pos2> = (0..=n)
        .map(|k| {
            let a = -std::f32::consts::FRAC_PI_2 + f * std::f32::consts::TAU * k as f32 / n as f32;
            c + vec2(a.cos(), a.sin()) * r
        })
        .collect();
    p.add(Shape::line(pts, Stroke::new(2.4, color)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_modes_belong_to_their_tabs() {
        assert_eq!(SkyMode::of(Tab::Sun), SkyMode::Sun);
        assert_eq!(SkyMode::of(Tab::Moon), SkyMode::Moon);
        assert_eq!(SkyMode::of(Tab::Night), SkyMode::Night);
        assert_eq!(SkyMode::of(Tab::Calendar), SkyMode::Night, "the Milky Way calendar lives with the night");
        for m in MODES {
            assert_eq!(SkyMode::of(m.tab()), m);
        }
    }

    #[test]
    fn day_length_follows_the_seasons_and_the_poles() {
        let june = Date::new(2026, 6, 21).unix_midnight_utc() + DAY / 2.0;
        let dec = Date::new(2026, 12, 21).unix_midnight_utc() + DAY / 2.0;
        let (june_len, dec_len) = (day_length_minutes(43.3, june), day_length_minutes(43.3, dec));
        // Donostia: about 15 h 30 in June, 8 h 50 in December
        assert!((900.0..960.0).contains(&june_len), "{june_len}");
        assert!((500.0..560.0).contains(&dec_len), "{dec_len}");
        assert_eq!(day_length_minutes(80.0, june), 1440.0, "midnight sun");
        assert_eq!(day_length_minutes(80.0, dec), 0.0, "polar night");
        assert!((day_length_minutes(0.0, june) - 720.0).abs() < 30.0, "the equator keeps about 12 h");
        assert_eq!(day_length_minutes(f64::NAN, june), 0.0);
    }

    #[test]
    fn principal_phases_have_names() {
        assert_eq!(principal_label(Principal::Full), "Full Moon");
        assert_eq!(principal_label(Principal::New), "New Moon");
    }

    #[test]
    fn night_hours_without_moon_are_what_the_moon_leaves() {
        let span = |a: f64, b: f64| Span { start: a * 3600.0, end: b * 3600.0 };
        let n = NightProfile {
            day_start: 0.0,
            night: vec![span(20.0, 30.0)],
            dark: vec![span(22.0, 28.0)],
            moon_up: vec![span(20.0, 25.0)],
            core: vec![],
        };
        let (total, moonless) = night_figures(&n);
        assert!((total - 360.0).abs() < 1e-6 && (moonless - 180.0).abs() < 1e-6, "{total} {moonless}");
        // no dark sky (summer at high latitude): the whole night counts
        let n = NightProfile { dark: vec![], ..n };
        let (total, moonless) = night_figures(&n);
        assert!((total - 600.0).abs() < 1e-6 && (moonless - 300.0).abs() < 1e-6);
    }
}
