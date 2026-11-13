//! The Milky Way calendar: a year as twelve rows of days, each night coloured by what the core allows that night —
//! favourable (the core up in a dark sky with no Moon), the Moon in the way (time enough, but the Moon is up), a short
//! window (the core is up in the dark for too little), or no core at all. Under it, the chosen night as a timeline: the
//! dark sky, the Moon and the core hour by hour, with the best window marked. Click or drag a day to go to it.

use light_studio_astro::Span;
use light_studio_astro::time::is_leap;

use super::*;

/// One column every this many days.
const STEP_DAYS: i64 = 1;
/// Minutes of core in a dark, moonless sky from which a night is favourable.
const GOOD_MIN: f64 = 90.0;
/// Minutes of core in a dark sky below which there is nothing to speak of.
const NONE_MIN: f64 = 20.0;

/// What a night is worth for the Milky Way's core.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Verdict {
    /// The core up in a dark sky with the Moon down for [`GOOD_MIN`] or more.
    Good,
    /// The core is up long enough, but the Moon is up for too much of it.
    Moon,
    /// The core is up in the dark, but for a short while only.
    Short,
    /// The core is not up in a dark sky at all (or there is no dark sky).
    None,
}

/// A night rated: the verdict and its numbers.
#[derive(Clone, Debug)]
pub(super) struct Rated {
    pub verdict: Verdict,
    /// Minutes of core in the dark, Moon or not.
    pub core_min: f64,
    /// Of them, the minutes with the Moon down.
    pub ideal_min: f64,
    /// The longest stretch of core in the dark with the Moon down.
    pub best: Option<Span>,
    /// The stretches of core in the dark with the Moon down.
    pub ideal: Vec<Span>,
}

/// The parts of `a` that `b` does not cover.
pub(super) fn minus(a: &[Span], b: &[Span]) -> Vec<Span> {
    let mut out = Vec::new();
    for s in a {
        let mut cut: Vec<Span> = b.iter().copied().filter(|x| x.end > s.start && x.start < s.end).collect();
        cut.sort_by(|p, q| p.start.total_cmp(&q.start));
        let mut at = s.start;
        for x in cut {
            if x.start > at {
                out.push(Span { start: at, end: x.start.min(s.end) });
            }
            at = at.max(x.end);
        }
        if at < s.end {
            out.push(Span { start: at, end: s.end });
        }
    }
    out
}

/// Rates one night.
pub(super) fn rate(n: &NightProfile) -> Rated {
    let core_min: f64 = n.core.iter().map(Span::minutes).sum();
    let ideal = minus(&n.core, &n.moon_up);
    let ideal_min: f64 = ideal.iter().map(Span::minutes).sum();
    let best = ideal.iter().copied().max_by(|a, b| a.minutes().total_cmp(&b.minutes()));
    let verdict = if ideal_min >= GOOD_MIN {
        Verdict::Good
    } else if core_min >= GOOD_MIN {
        Verdict::Moon
    } else if core_min >= NONE_MIN {
        Verdict::Short
    } else {
        Verdict::None
    };
    Rated { verdict, core_min, ideal_min, best, ideal }
}

/// The nights of `year` at a place, one every [`STEP_DAYS`] days. `offset` is the UTC offset set by hand; without it
/// each night uses the guess for its day, so summer time shows as the hour it moves the clock.
pub(super) fn profiles(obs: Observer, year: i32, offset: Option<i32>) -> Vec<NightProfile> {
    let jan1 = Date::new(year, 1, 1);
    let days = if is_leap(year) { 366 } else { 365 };
    (0..days)
        .step_by(STEP_DAYS as usize)
        .map(|i| {
            let d = jan1.add_days(i);
            let off = offset.unwrap_or_else(|| super::zone::offset_at(obs.lat, obs.lon, d.unix_midnight_utc() + DAY / 2.0 - obs.lon * 240.0).0);
            night_profile(obs, d.unix_midnight_utc() - f64::from(off) * 60.0, 900.0)
        })
        .collect()
}

impl Page<'_> {
    /// The colour of a verdict.
    fn verdict_color(&self, v: Verdict) -> Color32 {
        let dark = self.t.text.r() > 128;
        match v {
            Verdict::Good => {
                if dark {
                    Color32::from_rgb(72, 200, 130)
                } else {
                    Color32::from_rgb(28, 150, 86)
                }
            }
            Verdict::Moon => {
                if dark {
                    Color32::from_rgb(112, 156, 236)
                } else {
                    Color32::from_rgb(54, 96, 190)
                }
            }
            Verdict::Short => {
                if dark {
                    Color32::from_rgb(236, 170, 60)
                } else {
                    Color32::from_rgb(196, 124, 10)
                }
            }
            Verdict::None => self.t.track,
        }
    }

    fn verdict_label(v: Verdict) -> &'static str {
        match v {
            Verdict::Good => "Favourable",
            Verdict::Moon => "Moon in the way",
            Verdict::Short => "Short window",
            Verdict::None => "No core",
        }
    }

    /// The year as twelve rows of days, a legend and the chosen night.
    pub(super) fn year_card(&mut self, ui: &mut Ui, st: &mut PlannerState) {
        let t = self.t;
        let year = self.c.date.year;
        let rated: Vec<Rated> = self.year.iter().map(rate).collect();
        let w = ui.available_width();
        let label_w = 30.0;
        let cell_w = ((w - label_w) / 31.0).max(4.0);
        let row_h = 17.0;
        let head_h = 14.0;
        let (outer, resp) = ui.allocate_exact_size(vec2(w, head_h + row_h * 12.0), Sense::click_and_drag());
        let p = ui.painter_at(outer);
        let grid_left = outer.left() + label_w;
        // the day numbers along the top
        for d in [1u32, 5, 10, 15, 20, 25, 30] {
            p.text(
                pos2(grid_left + (d as f32 - 0.5) * cell_w, outer.top() + head_h / 2.0),
                Align2::CENTER_CENTER,
                d.to_string(),
                t.font(9.5),
                t.text_dim,
            );
        }
        let cell = |month: u32, day: u32| {
            Rect::from_min_size(pos2(grid_left + (day - 1) as f32 * cell_w, outer.top() + head_h + (month - 1) as f32 * row_h), vec2(cell_w, row_h))
        };
        let today = Date::from_unix(self.now, self.c.offset);
        for month in 1..=12u32 {
            let label = crate::i18n::tr_ctx("month", MONTHS.get(month as usize - 1).copied().unwrap_or(""));
            p.text(pos2(outer.left() + 2.0, cell(month, 1).center().y), Align2::LEFT_CENTER, label, t.font(10.5), t.text_dim);
            let length = if month == 12 { 31 } else { Date::new(year, month + 1, 1).ordinal() - Date::new(year, month, 1).ordinal() };
            for day in 1..=length {
                let date = Date::new(year, month, day);
                let idx = (date.ordinal() - 1) as usize / STEP_DAYS as usize;
                let verdict = rated.get(idx).map_or(Verdict::None, |r| r.verdict);
                let r = cell(month, day).shrink2(vec2(0.75, 1.25));
                p.rect_filled(r, 2.0, self.verdict_color(verdict).gamma_multiply(if verdict == Verdict::None { 0.55 } else { 0.95 }));
                if date == today {
                    p.circle_filled(r.center(), 1.6, if verdict == Verdict::None { t.text_dim } else { Color32::from_black_alpha(190) });
                }
            }
        }
        let pick = |pos: Pos2| -> Option<Date> {
            let day = ((pos.x - grid_left) / cell_w).floor() as i64 + 1;
            let month = ((pos.y - outer.top() - head_h) / row_h).floor() as i64 + 1;
            if !(1..=31).contains(&day) || !(1..=12).contains(&month) {
                return None;
            }
            let (month, day) = (month as u32, day as u32);
            let length = if month == 12 { 31 } else { Date::new(year, month + 1, 1).ordinal() - Date::new(year, month, 1).ordinal() };
            (day <= length).then(|| Date::new(year, month, day))
        };
        // the chosen day
        let shown = self.c.date;
        let r = cell(shown.month, shown.day).shrink2(vec2(0.0, 0.5));
        p.rect_stroke(r, 3.0, Stroke::new(1.6, t.text), StrokeKind::Outside);
        // a day under the pointer says what it is
        if let Some(pos) = resp.hover_pos()
            && let Some(d) = pick(pos)
            && let Some(n) = rated.get((d.ordinal() - 1) as usize / STEP_DAYS as usize)
        {
            let text = format!("{} · {}", short_date(d), crate::i18n::tr(Self::verdict_label(n.verdict)));
            let g = p.layout_no_wrap(text, t.semibold(11.0), t.text);
            let chip = Rect::from_min_size(
                pos2((pos.x + 10.0).min(outer.right() - g.size().x - 14.0), (pos.y + 14.0).min(outer.bottom() - g.size().y - 8.0)),
                g.size() + vec2(12.0, 6.0),
            );
            crate::theme::paint_chip(&p, chip, &t);
            p.galley(chip.center() - g.size() / 2.0, g, t.text);
        }
        if (resp.clicked() || resp.dragged())
            && let Some(pos) = resp.interact_pointer_pos()
            && let Some(d) = pick(pos)
            && d != self.c.date
        {
            st.date = d.iso();
            st.minute = None;
        }

        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            for v in [Verdict::Good, Verdict::Moon, Verdict::Short, Verdict::None] {
                let (r, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
                ui.painter().rect_filled(r, 2.0, self.verdict_color(v).gamma_multiply(if v == Verdict::None { 0.55 } else { 0.95 }));
                ui.add(
                    egui::Label::new(egui::RichText::new(crate::i18n::tr(Self::verdict_label(v))).font(t.font(10.5)).color(t.text_dim))
                        .wrap_mode(egui::TextWrapMode::Extend),
                );
            }
        });
        ui.add_space(10.0);
        let idx = (self.c.date.ordinal() - 1) as usize / STEP_DAYS as usize;
        if let (Some(n), Some(rating)) = (self.year.get(idx), rated.get(idx)) {
            self.night_detail(ui, n, rating);
        }
    }

    /// The chosen night: its verdict in words and a timeline of the dark sky, the Moon and the core, with the best
    /// window picked out.
    fn night_detail(&self, ui: &mut Ui, n: &NightProfile, r: &Rated) {
        let t = self.t;
        let c = self.c;
        let colour = self.verdict_color(r.verdict);
        let (head, _) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::hover());
        let p = ui.painter();
        p.circle_filled(pos2(head.left() + 7.0, head.center().y), 5.5, colour);
        p.text(
            pos2(head.left() + 20.0, head.center().y),
            Align2::LEFT_CENTER,
            crate::i18n::tr(Self::verdict_label(r.verdict)),
            t.semibold(13.5),
            t.text,
        );
        p.text(pos2(head.right(), head.center().y), Align2::RIGHT_CENTER, short_date(c.date), t.font(12.0), t.text_dim);
        // what it means, in a sentence
        let window = |s: &Span| format!("{}–{}", clock(s.start, c.offset), clock(s.end, c.offset));
        let sentence = match r.verdict {
            Verdict::Good => crate::i18n::tr("The core is up in a dark, moonless sky for {time} ({window}).")
                .replace("{time}", &hm(r.ideal_min))
                .replace("{window}", &r.best.as_ref().map(window).unwrap_or_default()),
            Verdict::Moon => {
                let mid = r.best.map_or(n.day_start + DAY * 0.75, |s| (s.start + s.end) / 2.0);
                let illum = (moon_phase(mid).illumination * 100.0).round();
                crate::i18n::tr("The core is up in the dark for {time}, but the Moon is up for most of it ({illum} % lit).")
                    .replace("{time}", &hm(r.core_min))
                    .replace("{illum}", &illum.to_string())
            }
            Verdict::Short => crate::i18n::tr("The core is up in the dark for only {time}.").replace("{time}", &hm(r.core_min)),
            Verdict::None if n.dark.is_empty() => crate::i18n::tr("There is no astronomical darkness this night.").to_string(),
            Verdict::None => crate::i18n::tr("The core is not up in a dark sky this night.").to_string(),
        };
        note(ui, &sentence);
        ui.add_space(6.0);

        // the timeline: from the first dusk to the last dawn
        let w = ui.available_width();
        let (lo, hi) = n.night.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), s| (a.min(s.start), b.max(s.end)));
        let (lo, hi) = if lo.is_finite() && hi > lo + 3600.0 { (lo, hi) } else { (n.day_start + 18.0 * 3600.0, n.day_start + 30.0 * 3600.0) };
        // whole hours, a little air at each end
        let lo = ((lo - 1800.0) / 3600.0).floor() * 3600.0;
        let hi = ((hi + 1800.0) / 3600.0).ceil() * 3600.0;
        let label_w = 52.0;
        let lane_h = 11.0;
        let gap = 4.0;
        let (area, _) = ui.allocate_exact_size(vec2(w, 3.0 * lane_h + 2.0 * gap + 20.0), Sense::hover());
        let p = ui.painter_at(area);
        let x0 = area.left() + label_w;
        let x_of = |unix: f64| x0 + (w - label_w) * ((unix - lo) / (hi - lo)) as f32;
        let lane = |k: usize| Rect::from_min_size(pos2(x0, area.top() + k as f32 * (lane_h + gap)), vec2(w - label_w, lane_h));
        let lanes = [("Dark", &n.dark, self.pal.blue.gamma_multiply(0.5)), ("Moon", &n.moon_up, self.pal.moon), ("Core", &n.core, self.pal.core)];
        // the best window, behind everything: a column through the three lanes
        let top = lane(0).top() - 3.0;
        let bottom = lane(2).bottom() + 3.0;
        for s in &r.ideal {
            let col = Rect::from_min_max(pos2(x_of(s.start.max(lo)), top), pos2(x_of(s.end.min(hi)), bottom));
            p.rect_filled(col, 3.0, self.verdict_color(Verdict::Good).gamma_multiply(0.28));
            p.rect_stroke(col, 3.0, Stroke::new(1.2, self.verdict_color(Verdict::Good)), StrokeKind::Inside);
        }
        for (k, (name, spans, color)) in lanes.iter().enumerate() {
            let l = lane(k);
            p.text(pos2(area.left() + 2.0, l.center().y), Align2::LEFT_CENTER, crate::i18n::tr(name), t.font(10.5), t.text_dim);
            p.rect_filled(l, 3.0, t.inset);
            for s in spans.iter() {
                let (a, b) = (x_of(s.start.max(lo)), x_of(s.end.min(hi)));
                if b > a {
                    p.rect_filled(Rect::from_min_max(pos2(a, l.top()), pos2(b, l.bottom())), 3.0, *color);
                }
            }
        }
        // the clock along the bottom, every two hours
        let mut at = (lo / 7200.0).ceil() * 7200.0;
        while at <= hi {
            let x = x_of(at);
            p.line_segment([pos2(x, bottom + 1.0), pos2(x, bottom + 4.0)], Stroke::new(1.0, t.text_dim));
            if x + 14.0 <= area.right() {
                p.text(pos2(x, bottom + 5.0), Align2::CENTER_TOP, clock(at, c.offset), t.font(9.5), t.text_dim);
            }
            at += 7200.0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(a: f64, b: f64) -> Span {
        Span { start: a * 3600.0, end: b * 3600.0 }
    }

    fn night(core: Vec<Span>, moon_up: Vec<Span>) -> NightProfile {
        NightProfile { day_start: 0.0, night: vec![span(20.0, 30.0)], dark: vec![span(22.0, 28.0)], moon_up, core, best_core_alt: 30.0 }
    }

    #[test]
    fn a_year_is_a_column_a_day() {
        let v = profiles(Observer::new(40.4, -3.7), 2024, Some(60));
        assert_eq!(v.len(), 366);
        assert!(v.windows(2).all(|w| (w[1].day_start - w[0].day_start - DAY).abs() < 1.0), "evenly spaced with a fixed offset");
        // the core is up in the dark in July but not in January
        let july = v.iter().find(|n| Date::from_unix(n.day_start + 3600.0, 60).month == 7).expect("a July night");
        let january = v.first().expect("a January night");
        assert!(!july.core.is_empty() && january.core.is_empty());
    }

    #[test]
    fn summer_time_moves_the_columns_by_an_hour() {
        // with the offset guessed per day, local midnight in July is an hour earlier in UTC than in January
        let v = profiles(Observer::new(40.4, -3.7), 2024, None);
        let at = |i: usize| (v[i].day_start / DAY).fract() * 24.0;
        let (winter, summer) = (at(4), at(200));
        assert!((winter - summer - 1.0).abs() < 0.01 || (winter - summer + 23.0).abs() < 0.01, "{winter} {summer}");
    }

    #[test]
    fn subtracting_the_moon_leaves_the_dark_moonless_stretches() {
        let left = minus(&[span(22.0, 27.0)], &[span(21.0, 23.0), span(25.0, 26.0)]);
        assert_eq!(left, vec![span(23.0, 25.0), span(26.0, 27.0)]);
        assert_eq!(minus(&[span(22.0, 24.0)], &[span(20.0, 30.0)]), vec![]);
        assert_eq!(minus(&[span(22.0, 24.0)], &[]), vec![span(22.0, 24.0)]);
    }

    #[test]
    fn a_night_is_favourable_moon_spoiled_short_or_empty() {
        // 3 h of core, no Moon
        let good = rate(&night(vec![span(23.0, 26.0)], vec![]));
        assert_eq!(good.verdict, Verdict::Good);
        assert_eq!(good.best, Some(span(23.0, 26.0)));
        // 3 h of core, the Moon up for 2 h 30 of it: 30 min left is not enough
        let moon = rate(&night(vec![span(23.0, 26.0)], vec![span(22.0, 25.5)]));
        assert_eq!(moon.verdict, Verdict::Moon);
        // 45 min of core
        assert_eq!(rate(&night(vec![span(27.0, 27.75)], vec![])).verdict, Verdict::Short);
        // nothing
        assert_eq!(rate(&night(vec![], vec![])).verdict, Verdict::None);
        assert_eq!(rate(&night(vec![span(27.0, 27.1)], vec![])).verdict, Verdict::None);
    }

    #[test]
    fn the_seasons_read_as_they_should_from_madrid() {
        let v = profiles(Observer::new(40.4, -3.7), 2024, Some(120));
        let verdicts: Vec<Verdict> = v.iter().map(|n| rate(n).verdict).collect();
        let at = |month: u32| -> Vec<Verdict> {
            v.iter().zip(&verdicts).filter(|(n, _)| Date::from_unix(n.day_start + 3600.0, 120).month == month).map(|(_, v)| *v).collect()
        };
        assert!(at(1).iter().all(|v| *v == Verdict::None), "no core in January");
        assert!(at(7).contains(&Verdict::Good) && at(7).contains(&Verdict::Moon), "July has both good nights and Moon-spoiled ones");
    }
}
