//! The strip of information panels along the top of the map: swipe through the Sun and Moon now, rise and
//! set, the twilights, golden and blue hours, the Milky Way, the obstacle (the black pin), shadows, the next
//! eclipse and a meteor shower. A click on a time or a date moves the time bar there.

use light_studio_astro::sky::SHOWERS;

use super::skylayers::{ASTRO_LIGHT, BLUE_LIGHT, GOLDEN_LIGHT, METEOR, MOON_NOW, MOONRISE, MOONSET, NAUTICAL_LIGHT, SUN_NOW, SUNRISE, SUNSET};
use super::*;

/// How many panels the strip pages through.
pub(super) const PAGES: usize = 9;

/// Moves the day and the time bar to a moment (Unix seconds) on the clock of a zone `offset` minutes east of UTC.
pub(super) fn jump_to(st: &mut PlannerState, unix: f64, offset: i32) {
    if !unix.is_finite() {
        return;
    }
    let d = Date::from_unix(unix, offset);
    let midnight = d.unix_midnight_utc() - f64::from(offset) * 60.0;
    st.date = d.iso();
    st.minute = Some(((unix - midnight) / 60.0).clamp(0.0, 1439.0));
}

/// One value in the strip.
struct Cell {
    label: String,
    value: String,
    color: Option<Color32>,
    /// Where a click moves the time bar.
    jump: Option<f64>,
}

fn cell(label: &str, value: String, color: Option<Color32>) -> Cell {
    Cell { label: crate::i18n::tr(label).to_string(), value, color, jump: None }
}

impl Page<'_> {
    fn strip_page(&mut self, st: &PlannerState, rt: &mut Runtime, page: usize) -> (&'static str, Vec<Cell>) {
        let c = self.c;
        let at = c.start + self.minute * 60.0;
        let clk = |t: Option<f64>| t.map_or_else(|| "—".to_string(), |t| clock(t, c.offset));
        let with_az = |t: Option<f64>, az: Option<f64>| match (t, az) {
            (Some(t), Some(az)) => format!("{} · {:.0}°", clock(t, c.offset), az),
            _ => "—".to_string(),
        };
        let span = |s: Option<light_studio_astro::Span>| {
            s.map_or_else(|| "—".to_string(), |s| format!("{}–{}", clock(s.start, c.offset), clock(s.end, c.offset)))
        };
        let jump = |mut cell: Cell, t: Option<f64>| {
            cell.jump = t;
            cell
        };
        let s = &c.plan.sun;
        let m = &c.plan.moon;
        match page {
            0 => {
                let ph = moon_phase(at);
                (
                    "Sun and Moon",
                    vec![
                        cell("Sun", format!("{} · {}", az_text(self.skies.sun.az), alt_text(self.skies.sun.alt)), Some(SUN_NOW)),
                        cell("Moon", format!("{} · {}", az_text(self.skies.moon.az), alt_text(self.skies.moon.alt)), Some(MOON_NOW)),
                        cell(ph.name.label(), format!("{:.0} %", ph.illumination * 100.0), None),
                    ],
                )
            }
            1 => (
                "Rise and set",
                vec![
                    jump(cell("Sunrise", with_az(s.rise, s.rise_az), Some(SUNRISE)), s.rise),
                    jump(cell("Sunset", with_az(s.set, s.set_az), Some(SUNSET)), s.set),
                    jump(cell("Moonrise", with_az(m.rise, m.rise_az), Some(MOONRISE)), m.rise),
                    jump(cell("Moonset", with_az(m.set, m.set_az), Some(MOONSET)), m.set),
                ],
            ),
            2 => (
                "Twilights",
                vec![
                    jump(cell("Civil", format!("{} / {}", clk(s.civil.dawn), clk(s.civil.dusk)), Some(BLUE_LIGHT)), s.civil.dawn),
                    jump(cell("Nautical", format!("{} / {}", clk(s.nautical.dawn), clk(s.nautical.dusk)), Some(NAUTICAL_LIGHT)), s.nautical.dawn),
                    jump(
                        cell("Astronomical", format!("{} / {}", clk(s.astronomical.dawn), clk(s.astronomical.dusk)), Some(ASTRO_LIGHT)),
                        s.astronomical.dawn,
                    ),
                ],
            ),
            3 => (
                "Golden and blue hour",
                vec![
                    jump(cell("Golden Hour (morning)", span(s.golden_am), Some(GOLDEN_LIGHT)), s.golden_am.map(|x| x.start)),
                    jump(cell("Golden Hour (evening)", span(s.golden_pm), Some(GOLDEN_LIGHT)), s.golden_pm.map(|x| x.start)),
                    jump(cell("Blue Hour (morning)", span(s.blue_am), Some(BLUE_LIGHT)), s.blue_am.map(|x| x.start)),
                    jump(cell("Blue Hour (evening)", span(s.blue_pm), Some(BLUE_LIGHT)), s.blue_pm.map(|x| x.start)),
                ],
            ),
            4 => {
                let core = &c.plan.core;
                let dark = c.night_core.first().copied();
                let (az0, az1) = match dark {
                    Some(d) => (
                        az_text(light_studio_astro::core_horizontal(d.start, self.obs).az),
                        az_text(light_studio_astro::core_horizontal(d.end, self.obs).az),
                    ),
                    None => ("—".into(), "—".into()),
                };
                (
                    "Milky Way",
                    vec![
                        jump(cell("Visible", span(dark), Some(self.pal.core)), dark.map(|d| d.start)),
                        cell("Starts at", az0, None),
                        cell("Ends at", az1, None),
                        cell("Core now", format!("{} · {}", az_text(self.skies.core.az), alt_text(self.skies.core.alt)), Some(Color32::WHITE)),
                        jump(
                            cell("Highest", format!("{} · {}", clock(core.transit, c.offset), alt_text(core.transit_alt)), None),
                            Some(core.transit),
                        ),
                    ],
                )
            }
            5 => match st.target {
                None => (
                    "Obstacle",
                    vec![cell(
                        "Black pin",
                        crate::i18n::tr("Right-click the map → Set the Target Here to put the black pin").to_string(),
                        Some(Color32::BLACK),
                    )],
                ),
                Some([la, lo]) => {
                    let sight = light_studio_astro::align::sight(st.lat, st.lon, la, lo, st.target_height);
                    let above = |alt: f64| format!("{:+.1}°", alt - sight.altitude);
                    (
                        "Obstacle",
                        vec![
                            cell(
                                "Distance",
                                if sight.distance < 1000.0 {
                                    format!("{:.0} m", sight.distance)
                                } else {
                                    format!("{:.2} km", sight.distance / 1000.0)
                                },
                                Some(Color32::BLACK),
                            ),
                            cell("Azimuth", az_text(sight.azimuth), None),
                            cell("Elevation angle", alt_text(sight.altitude), None),
                            cell("Sun above the pin", above(self.skies.sun.alt), Some(SUN_NOW)),
                            cell("Moon above the pin", above(self.skies.moon.alt), Some(MOON_NOW)),
                            cell("Core above the pin", above(self.skies.core.alt), Some(self.pal.core)),
                        ],
                    )
                }
            },
            6 => {
                let sun = self.skies.sun;
                let one = light_studio_astro::align::shadow(1.0, sun);
                let own = light_studio_astro::align::shadow(st.target_height.max(0.0), sun);
                (
                    "Shadow",
                    vec![
                        cell("Ratio", one.map_or_else(|| "—".to_string(), |(k, _)| format!("1 : {k:.2}")), Some(Color32::from_gray(125))),
                        cell("Object", format!("{:.1} m", st.target_height.max(0.0)), None),
                        cell(
                            "Shadow",
                            own.map_or_else(
                                || "—".to_string(),
                                |(len, _)| if len > 1000.0 { format!("{:.2} km", len / 1000.0) } else { format!("{len:.1} m") },
                            ),
                            None,
                        ),
                        cell("Direction", one.map_or_else(|| "—".to_string(), |(_, az)| az_text(az)), None),
                    ],
                )
            }
            7 => {
                let list = self.eclipse_rows(st, rt);
                match list.rows.iter().find(|r| r.unix >= at - 3.0 * 3600.0) {
                    Some(r) => (
                        "Eclipse",
                        vec![
                            jump(
                                cell(r.kind, short_date(Date::from_unix(r.unix, c.offset)), Some(if r.solar { SUN_NOW } else { MOON_NOW })),
                                Some(r.unix),
                            ),
                            cell("Greatest eclipse", clock(r.unix, c.offset), None),
                            cell("Magnitude", format!("{:.2}", r.magnitude), None),
                            cell(if r.visible { "Visible from here" } else { "Not visible from here" }, String::new(), None),
                        ],
                    ),
                    None => ("Eclipse", vec![cell("Next eclipse", crate::i18n::tr("None in the next three years").to_string(), None)]),
                }
            }
            _ => {
                let sh = SHOWERS.get(st.shower).unwrap_or(&SHOWERS[0]);
                let peak = sh.peak_date(c.date.year);
                let r = sh.radiant_horizontal(at, self.obs);
                (
                    "Meteor shower",
                    vec![
                        cell("Shower", crate::i18n::tr(sh.name).to_string(), Some(METEOR)),
                        cell("Peak", short_date(peak), None),
                        cell("Radiant", format!("{} · {}", az_text(r.az), alt_text(r.alt)), None),
                        cell("Meteors per hour", format!("{}", sh.zhr), None),
                    ],
                )
            }
        }
    }

    /// The strip itself, over the map's top (between the search and the map's buttons).
    pub(super) fn info_strip(&mut self, ui: &mut Ui, st: &mut PlannerState, rt: &mut Runtime, free: Rect) {
        let t = self.t;
        let left = free.left() + 316.0;
        let right = free.right() - 46.0;
        if right - left < 330.0 {
            return;
        }
        let r = Rect::from_min_max(pos2(left, free.top()), pos2(right, free.top() + 58.0));
        let painter = ui.painter().clone();
        // as see-through as the search, date and time cards at rest, solid under the pointer
        let k = super::layout::card_opacity(ui.ctx(), "strip", r, false);
        painter.rect_filled(r.translate(vec2(0.0, 1.5)), 12.0, Color32::from_black_alpha(50).gamma_multiply(k));
        crate::theme::paint_chip_at(&painter, r, &t, k);
        let _ = ui.interact(r, egui::Id::new("planner-strip-bg"), Sense::click_and_drag());
        let page = st.strip_page.min(PAGES - 1);
        let (title, cells) = self.strip_page(st, rt, page);
        // the chevrons
        for (dx, name, icon, step) in [(0.0, "stripPrev", Icon::ChevronLeft, PAGES - 1), (r.width() - 28.0, "stripNext", Icon::ChevronRight, 1)] {
            let b = Rect::from_min_size(pos2(r.left() + dx, r.top()), vec2(28.0, r.height()));
            let resp = ui.interact(b, egui::Id::new(("planner-strip", name)), Sense::click());
            crate::icons::paint(
                &painter,
                Rect::from_center_size(b.center(), vec2(14.0, 14.0)),
                icon,
                if resp.hovered() { t.text } else { t.text_dim },
            );
            if resp.clicked() {
                st.strip_page = (page + step) % PAGES;
            }
        }
        // the title, the page dots, and the cells
        painter.text(pos2(r.left() + 30.0, r.top() + 11.0), Align2::LEFT_CENTER, crate::i18n::tr(title).to_uppercase(), t.semibold(9.5), t.text_dim);
        for i in 0..PAGES {
            let x = r.right() - 30.0 - (PAGES - 1 - i) as f32 * 8.0;
            painter.circle_filled(pos2(x, r.top() + 11.0), 2.0, if i == page { t.accent } else { t.text_disabled });
        }
        let inner = Rect::from_min_max(pos2(r.left() + 30.0, r.top() + 20.0), pos2(r.right() - 30.0, r.bottom() - 4.0));
        let n = cells.len().max(1) as f32;
        let w = inner.width() / n;
        for (i, c) in cells.iter().enumerate() {
            let b = Rect::from_min_size(pos2(inner.left() + w * i as f32, inner.top()), vec2(w, inner.height()));
            let resp = ui.interact(b, egui::Id::new(("planner-strip-cell", i)), if c.jump.is_some() { Sense::click() } else { Sense::hover() });
            if resp.hovered() && c.jump.is_some() {
                painter.rect_filled(b.shrink2(vec2(2.0, 0.0)), 6.0, t.hover.gamma_multiply(0.5));
            }
            let mut x = b.left() + 6.0;
            if let Some(col) = c.color {
                painter.circle_filled(pos2(x + 2.0, b.top() + 7.0), 3.0, col);
                painter.circle_stroke(pos2(x + 2.0, b.top() + 7.0), 3.0, Stroke::new(0.8, Color32::from_black_alpha(120)));
                x += 10.0;
            }
            painter.with_clip_rect(b).text(pos2(x, b.top() + 7.0), Align2::LEFT_CENTER, &c.label, t.font(10.0), t.text_dim);
            painter.with_clip_rect(b).text(pos2(b.left() + 6.0, b.top() + 22.0), Align2::LEFT_CENTER, &c.value, t.semibold(12.0), t.text);
            if let Some(j) = c.jump
                && resp.clicked()
            {
                jump_to(st, j, self.c.offset);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use light_studio_astro::time::unix_from_civil;

    #[test]
    fn a_jump_lands_on_the_day_and_minute_of_the_places_clock() {
        let mut st = PlannerState::default();
        // 20:30 UTC in a zone two hours east is 22:30 the same day
        jump_to(&mut st, unix_from_civil(2024, 8, 12, 20, 30, 0), 120);
        assert_eq!(st.date, "2024-08-12");
        assert_eq!(st.minute, Some(22.0 * 60.0 + 30.0));
        // 23:00 UTC two hours east is already the next day at 01:00
        jump_to(&mut st, unix_from_civil(2024, 8, 12, 23, 0, 0), 120);
        assert_eq!(st.date, "2024-08-13");
        assert_eq!(st.minute, Some(60.0));
        // a time that is not a number changes nothing
        jump_to(&mut st, f64::NAN, 120);
        assert_eq!(st.date, "2024-08-13");
    }

    #[test]
    fn settings_read_from_a_file_are_brought_into_range() {
        let mut st = PlannerState {
            find_az: f64::NAN,
            strip_page: 99,
            shower: 99,
            meteor: Some(99),
            find_days: 1.0e9,
            to_sensor: 77,
            iso: -5.0,
            ..Default::default()
        };
        st.sane();
        assert!(st.find_az.is_finite() && st.strip_page < PAGES && st.shower < SHOWERS.len());
        assert_eq!(st.meteor, None);
        assert!(st.find_days <= 400.0 && st.to_sensor < SENSORS.len() && st.iso >= 6.0);
    }
}
