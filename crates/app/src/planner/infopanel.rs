//! The information panel: the day's figures in four folding sections (Terrain, Sun and Moon, Natural light, Milky
//! Way), one open at a time, each a small grid of labelled values. A click on a header opens it and closes the one
//! that was open; a click on a time moves the time bar there.

use super::skylayers::{ASTRO_LIGHT, BLUE_LIGHT, GOLDEN_LIGHT, MOON_NOW, MOONRISE, MOONSET, NAUTICAL_LIGHT, SUN_NOW, SUNRISE, SUNSET};
use super::*;

/// The sections, by id (the registry names them `planner:info:<id>`).
pub(super) const SECTIONS: [(&str, &str); 4] =
    [("terrain", "Terrain"), ("sunmoon", "Sun and Moon"), ("light", "Natural light"), ("milkyway", "Milky Way and Galactic Core")];

/// One labelled value of a grid row.
struct Item {
    label: String,
    value: String,
    color: Option<Color32>,
    /// Where a click moves the time bar (Unix seconds).
    jump: Option<f64>,
}

fn item(label: &str, value: String) -> Item {
    Item { label: crate::i18n::tr(label).to_string(), value, color: None, jump: None }
}

enum Row {
    /// Small caption over a value, side by side.
    Cells(Vec<Item>),
    /// A caption line.
    Head(String),
    /// A name with a colour dot and two values (morning, evening).
    Pair(Item, String, Option<f64>, String, Option<f64>),
    /// A line of text.
    Text(String),
}

impl Row {
    fn height(&self) -> f32 {
        match self {
            Row::Cells(_) => 38.0,
            Row::Head(_) => 22.0,
            Row::Pair(..) => 19.0,
            Row::Text(_) => 20.0,
        }
    }
}

/// The light of the moment, from the Sun's height.
fn light_now(alt: f64) -> &'static str {
    if alt >= 6.0 {
        "Day"
    } else if alt >= -4.0 {
        "Golden hour"
    } else if alt >= -8.0 {
        "Blue hour"
    } else if alt >= -18.0 {
        "Twilight"
    } else {
        "Night"
    }
}

fn duration_text(minutes: f64) -> String {
    let m = minutes.max(0.0).round() as u32;
    format!("{} h {:02} min", m / 60, m % 60)
}

impl Page<'_> {
    fn info_rows(&self, st: &PlannerState, id: &str) -> Vec<Row> {
        let c = self.c;
        let at = c.start + self.minute * 60.0;
        let off = c.offset;
        let clk = |t: Option<f64>| t.map_or_else(|| "—".to_string(), |t| clock(t, off));
        let span =
            |s: Option<light_studio_astro::Span>| s.map_or_else(|| "—".to_string(), |s| format!("{}–{}", clock(s.start, off), clock(s.end, off)));
        let s = &c.plan.sun;
        let m = &c.plan.moon;
        let timed = |label: &str, t: Option<f64>, color: Color32| Item {
            label: crate::i18n::tr(label).to_string(),
            value: clk(t),
            color: Some(color),
            jump: t,
        };
        match id {
            "terrain" => match st.target {
                None => vec![Row::Text(crate::i18n::tr("Right-click the map → Set the Target Here to put the black pin").to_string())],
                Some([la, lo]) => {
                    let sight = light_studio_astro::align::sight(st.lat, st.lon, la, lo, st.target_height);
                    let above = |alt: f64| format!("{:+.1}°", alt - sight.altitude);
                    let distance =
                        if sight.distance < 1000.0 { format!("{:.0} m", sight.distance) } else { format!("{:.2} km", sight.distance / 1000.0) };
                    vec![
                        Row::Cells(vec![item("Distance", distance), item("Elevation angle", alt_text(sight.altitude))]),
                        Row::Cells(vec![item("Azimuth", az_text(sight.azimuth)), item("Object", format!("{:.1} m", st.target_height.max(0.0)))]),
                        Row::Cells(vec![
                            item("Sun above the pin", above(self.skies.sun.alt)),
                            item("Moon above the pin", above(self.skies.moon.alt)),
                        ]),
                        Row::Cells(vec![item("Core above the pin", above(self.skies.core.alt))]),
                    ]
                }
            },
            "sunmoon" => {
                let ph = moon_phase(at);
                vec![
                    Row::Cells(vec![timed("Sunrise", s.rise, SUNRISE), timed("Sunset", s.set, SUNSET)]),
                    Row::Cells(vec![timed("Moonrise", m.rise, MOONRISE), timed("Moonset", m.set, MOONSET)]),
                    Row::Head(crate::i18n::tr("Now").to_string()),
                    Row::Cells(vec![
                        Item { color: Some(SUN_NOW), ..item("Sun", az_text(self.skies.sun.az)) },
                        item("Elevation", alt_text(self.skies.sun.alt)),
                    ]),
                    Row::Cells(vec![
                        Item { color: Some(MOON_NOW), ..item("Moon", az_text(self.skies.moon.az)) },
                        item("Elevation", alt_text(self.skies.moon.alt)),
                    ]),
                    Row::Cells(vec![
                        item("Phase", format!("{} · {:.0} %", crate::i18n::tr(ph.name.label()), ph.illumination * 100.0)),
                        item("Light", crate::i18n::tr(light_now(self.skies.sun.alt)).to_string()),
                    ]),
                ]
            }
            "light" => {
                let pair = |name: &str, color: Color32, a: Option<light_studio_astro::Span>, b: Option<light_studio_astro::Span>| {
                    Row::Pair(Item { color: Some(color), ..item(name, String::new()) }, span(a), a.map(|x| x.start), span(b), b.map(|x| x.start))
                };
                let tw = |name: &str, color: Color32, t: &light_studio_astro::Twilight| {
                    Row::Pair(Item { color: Some(color), ..item(name, String::new()) }, clk(t.dawn), t.dawn, clk(t.dusk), t.dusk)
                };
                vec![
                    Row::Head(crate::i18n::tr("Magic hours").to_string()),
                    pair("Blue Hour", BLUE_LIGHT, s.blue_am, s.blue_pm),
                    pair("Golden Hour", GOLDEN_LIGHT, s.golden_am, s.golden_pm),
                    Row::Head(crate::i18n::tr("Twilights").to_string()),
                    tw("Civil", BLUE_LIGHT, &s.civil),
                    tw("Nautical", NAUTICAL_LIGHT, &s.nautical),
                    tw("Astronomical", ASTRO_LIGHT, &s.astronomical),
                ]
            }
            _ => {
                let core = &c.plan.core;
                let dark = c.night_core.first().copied();
                let az_at = |t: f64| az_text(light_studio_astro::core_horizontal(t, self.obs).az);
                let jumpable = |label: &str, t: Option<f64>, color: Color32| Item { color: Some(color), ..timed(label, t, color) };
                vec![
                    Row::Cells(vec![jumpable("Rise", core.rise, self.pal.core), jumpable("Set", core.set, self.pal.core)]),
                    Row::Cells(vec![
                        item("Starts at", dark.map_or_else(|| "—".to_string(), |d| az_at(d.start))),
                        item("Ends at", dark.map_or_else(|| "—".to_string(), |d| az_at(d.end))),
                    ]),
                    Row::Cells(vec![
                        item("Highest", format!("{} · {}", clock(core.transit, off), alt_text(core.transit_alt))),
                        item("Core now", format!("{} · {}", az_text(self.skies.core.az), alt_text(self.skies.core.alt))),
                    ]),
                    Row::Cells(vec![
                        item("Visible", span(dark)),
                        item("Dark window", dark.map_or_else(|| "—".to_string(), |d| duration_text(d.minutes()))),
                    ]),
                ]
            }
        }
    }

    /// The four sections as a column.
    pub(super) fn info_panel(&mut self, ui: &mut Ui, st: &mut PlannerState) {
        let t = self.t;
        for (i, (id, title)) in SECTIONS.iter().enumerate() {
            let open = st.info_open == i;
            let k = ui.ctx().animate_bool_with_time_and_easing(
                egui::Id::new(("planner-info", i)),
                open,
                crate::theme::motion(0.18),
                egui::emath::easing::cubic_out,
            );
            // header
            let w = ui.available_width();
            let (h, _) = ui.allocate_exact_size(vec2(w, 40.0), Sense::hover());
            let resp = ui.interact(h, egui::Id::new(("planner-info-head", i)), Sense::click());
            let p = ui.painter();
            if resp.hovered() {
                p.rect_filled(h.shrink2(vec2(-6.0, 3.0)), R_CONTROL, t.hover.gamma_multiply(0.45));
            }
            let col = if open || resp.hovered() { t.text } else { t.text_dim };
            p.text(pos2(h.left() + 2.0, h.center().y), Align2::LEFT_CENTER, crate::i18n::tr(title), t.semibold(12.5), col);
            let c = pos2(h.right() - 8.0, h.center().y);
            crate::icons::paint(p, Rect::from_center_size(c, vec2(14.0, 14.0)), if open { Icon::Minus } else { Icon::Plus }, col);
            if resp.clicked() {
                st.info_open = if open { usize::MAX } else { i };
            }
            // body
            if k > 0.01 {
                let rows = self.info_rows(st, id);
                let full: f32 = rows.iter().map(Row::height).sum::<f32>() + 8.0;
                let (body, _) = ui.allocate_exact_size(vec2(w, full * k), Sense::hover());
                let p = ui.painter().with_clip_rect(body);
                let mut y = body.top() + 2.0;
                for (ri, row) in rows.iter().enumerate() {
                    let rh = row.height();
                    let band = Rect::from_min_size(pos2(body.left(), y), vec2(w, rh));
                    match row {
                        Row::Cells(items) => {
                            let cw = w / 2.0;
                            for (ci, it) in items.iter().enumerate() {
                                let b = Rect::from_min_size(pos2(band.left() + cw * ci as f32, band.top()), vec2(cw, rh));
                                let sense = if it.jump.is_some() { Sense::click() } else { Sense::hover() };
                                let r = ui.interact(b.intersect(body), egui::Id::new(("planner-info-cell", i, ri, ci)), sense);
                                if r.hovered() && it.jump.is_some() {
                                    p.rect_filled(b.shrink2(vec2(0.0, 1.0)), 6.0, t.hover.gamma_multiply(0.5));
                                }
                                let mut x = b.left() + 2.0;
                                if let Some(col) = it.color {
                                    p.circle_filled(pos2(x + 3.0, b.top() + 9.0), 3.0, col);
                                    x += 11.0;
                                }
                                p.text(pos2(x, b.top() + 9.0), Align2::LEFT_CENTER, &it.label, t.font(10.0), t.text_dim);
                                p.with_clip_rect(b).text(
                                    pos2(b.left() + 2.0, b.top() + 26.0),
                                    Align2::LEFT_CENTER,
                                    &it.value,
                                    t.semibold(12.5),
                                    t.text,
                                );
                                if let Some(j) = it.jump
                                    && r.clicked()
                                {
                                    super::strip::jump_to(st, j, self.c.offset);
                                }
                            }
                        }
                        Row::Head(text) => {
                            p.text(pos2(band.left() + 2.0, band.center().y + 3.0), Align2::LEFT_CENTER, text, t.font(10.0), t.text_dim);
                        }
                        Row::Text(text) => {
                            p.with_clip_rect(band).text(
                                pos2(band.left() + 2.0, band.center().y),
                                Align2::LEFT_CENTER,
                                text,
                                t.font(11.5),
                                t.text_dim,
                            );
                        }
                        Row::Pair(name, a, ja, b, jb) => {
                            let mut x = band.left() + 2.0;
                            if let Some(col) = name.color {
                                p.circle_filled(pos2(x + 3.0, band.center().y), 3.0, col);
                                x += 11.0;
                            }
                            p.text(pos2(x, band.center().y), Align2::LEFT_CENTER, &name.label, t.font(11.5), t.text);
                            for (n, (txt, jump)) in [(a, ja), (b, jb)].into_iter().enumerate() {
                                let colx = band.left() + w * (0.42 + 0.29 * n as f32);
                                let cell = Rect::from_min_size(pos2(colx - 2.0, band.top()), vec2(w * 0.29, rh));
                                let r = ui.interact(
                                    cell.intersect(body),
                                    egui::Id::new(("planner-info-pair", i, ri, n)),
                                    if jump.is_some() { Sense::click() } else { Sense::hover() },
                                );
                                if r.hovered() && jump.is_some() {
                                    p.rect_filled(cell, 5.0, t.hover.gamma_multiply(0.5));
                                }
                                p.with_clip_rect(cell).text(pos2(colx, band.center().y), Align2::LEFT_CENTER, txt, t.font(11.5), t.text_dim);
                                if let Some(j) = *jump
                                    && r.clicked()
                                {
                                    super::strip::jump_to(st, j, self.c.offset);
                                }
                            }
                        }
                    }
                    y += rh;
                }
            }
            // the line between sections
            let y = ui.cursor().top();
            ui.painter().line_segment([pos2(h.left() - 4.0, y), pos2(h.right() + 4.0, y)], Stroke::new(1.0, t.divider.gamma_multiply(0.7)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_light_of_the_moment_follows_the_suns_height() {
        assert_eq!(light_now(30.0), "Day");
        assert_eq!(light_now(0.0), "Golden hour");
        assert_eq!(light_now(-6.0), "Blue hour");
        assert_eq!(light_now(-12.0), "Twilight");
        assert_eq!(light_now(-30.0), "Night");
    }

    #[test]
    fn a_section_number_read_from_a_file_is_brought_into_range() {
        let mut st = PlannerState { info_open: 99, ..Default::default() };
        st.sane();
        assert!(st.info_open >= SECTIONS.len(), "an unknown section leaves them all closed");
        assert_eq!(duration_text(224.0), "3 h 44 min");
    }
}
