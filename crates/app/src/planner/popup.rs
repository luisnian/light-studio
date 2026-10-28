//! The record of a point of interest or of a plan, in front of the map: choosing a pin (or a line of the list) opens
//! it in a floating glass card with everything kept about it, to read and edit with the map still behind.

use super::plans::ALARMS;
use super::*;

/// Wide enough for the record to sit in two columns.
pub(super) const WIDE: f32 = 700.0;

/// The card's place: centred in the map's free part below the Sun and Moon card. Wider than tall (two columns, so
/// the whole record can be read and filled in at a glance) when the window allows, else the narrow single column.
pub(super) fn card_rect(free: Rect) -> Rect {
    let below = Rect::from_min_max(pos2(free.left(), (free.top() + 80.0).min(free.bottom())), free.right_bottom());
    let avail_w = (free.width() - 24.0).max(0.0);
    let wide = avail_w >= WIDE && below.height() - 12.0 >= 300.0;
    let w = if wide { 880.0_f32.min(avail_w) } else { 410.0_f32.min(avail_w) };
    let top = if wide { 520.0 } else { 640.0 };
    let h = (below.height() - 12.0).clamp(240.0, top).min((free.height() - 24.0).max(0.0));
    let r = Rect::from_center_size(below.center(), vec2(w, h));
    // (a short window: the card moves up rather than out of the free part)
    r.translate(vec2(0.0, (free.bottom() - 6.0 - r.bottom()).min(0.0) + (free.top() + 6.0 - r.top()).max(0.0)))
}

/// The calculations card's place (and Find's): the whole free part of the map, with a small margin.
pub(super) fn calc_rect(free: Rect) -> Rect {
    free.shrink(8.0)
}

/// The sections of the calculations card: name, what is in it.
const CALC_SECTIONS: [(&str, &str); 5] = [
    ("Focus and depth of field", "Hyperfocal, sharp range and tables"),
    ("Exposure", "Exposure value and settings for each light"),
    ("Stars and the Moon", "Star points and trails, the Moon in frame, distances"),
    ("Time-lapse and filters", "Photos and interval, ND filters"),
    ("Panorama", "Rows, columns and shots for a scene"),
];

/// Which part of a record is drawn: all of it (one column) or one of the two columns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Part {
    All,
    Left,
    Right,
}

impl Part {
    pub(super) fn left(self) -> bool {
        self != Part::Right
    }
    pub(super) fn right(self) -> bool {
        self != Part::Left
    }
}

/// What the card shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Shown {
    Site(usize),
    Plan(u64),
}

/// The record that is open, if it still exists (a point of interest wins when both were chosen).
pub(super) fn shown(st: &PlannerState, rt: &Runtime) -> Option<Shown> {
    if let Some(i) = rt.site_open.filter(|i| *i < st.saved.len()) {
        return Some(Shown::Site(i));
    }
    rt.plan_popup.filter(|id| st.plans.iter().any(|p| p.id == *id)).map(Shown::Plan)
}

/// A labelled value of a record in reading mode (nothing when it is empty).
/// A vertical scroll area for a card drawn in the popup layer. egui's own wheel handling asks which layer is under
/// the pointer, and that layer is not an `Area`, so it never answers and the wheel did nothing; this scrolls itself
/// whenever the pointer is over `view`, and takes the wheel so nothing behind the card (the map) also zooms.
pub(super) fn card_scroll<R>(ui: &mut Ui, id: impl std::hash::Hash + std::fmt::Debug, view: Rect, add: impl FnOnce(&mut Ui) -> R) -> R {
    let ctx = ui.ctx().clone();
    let key = egui::Id::new(("planner-card-scroll", id));
    let mut offset: f32 = ctx.data(|d| d.get_temp(key)).unwrap_or(0.0);
    if ctx.input(|i| i.pointer.hover_pos()).is_some_and(|p| view.contains(p)) {
        let dy = ctx.input(|i| i.smooth_scroll_delta.y);
        if dy != 0.0 {
            offset = (offset - dy).max(0.0);
            ctx.input_mut(|i| i.smooth_scroll_delta.y = 0.0);
        }
    }
    let out = egui::ScrollArea::vertical()
        .id_salt(key)
        .auto_shrink([false, false])
        .vertical_scroll_offset(offset)
        .scroll_source(egui::scroll_area::ScrollSource { mouse_wheel: false, ..Default::default() })
        .show(ui, add);
    ctx.data_mut(|d| d.insert_temp(key, out.state.offset.y));
    out.inner
}

pub(super) fn read_field(ui: &mut Ui, t: &Tokens, label: &str, text: &str) {
    if text.trim().is_empty() {
        return;
    }
    ui.label(egui::RichText::new(crate::i18n::tr(label)).font(t.font(11.5)).color(t.text_dim));
    ui.label(egui::RichText::new(text.trim()).font(t.font(13.0)).color(t.text));
    ui.add_space(6.0);
}

impl Page<'_> {
    pub(super) fn record_popup(&mut self, ui: &mut Ui, st: &mut PlannerState, rt: &mut Runtime, free: Rect) {
        let Some(what) = shown(st, rt) else {
            // (a record that was deleted or went out of range leaves nothing open)
            rt.site_open = None;
            rt.plan_popup = None;
            return;
        };
        let t = self.t;
        let r = card_rect(free);
        if r.width() < 120.0 || r.height() < 120.0 {
            return;
        }
        super::layout::surface(ui.painter(), r, &t);
        // swallow clicks on the card's empty parts (the map is under it)
        let _ = ui.interact(r, egui::Id::new("planner-popup-bg"), Sense::click_and_drag());

        // the head: what it is, its name, and the close button
        let (kicker, title) = match what {
            Shown::Site(i) => (crate::i18n::tr("Point of Interest"), st.saved.get(i).map(|p| p.name.clone()).unwrap_or_default()),
            Shown::Plan(id) => (crate::i18n::tr("Plan"), st.plans.iter().find(|p| p.id == id).map(|p| p.name.clone()).unwrap_or_default()),
        };
        let x = r.left() + 20.0;
        let painter = ui.painter().with_clip_rect(r);
        painter.text(pos2(x, r.top() + 20.0), Align2::LEFT_CENTER, kicker, t.font(11.0), t.text_dim);
        let name = painter.layout(title, t.semibold(16.0), t.text, r.width() - 40.0 - 80.0);
        let name_h = name.size().y.min(44.0);
        painter.galley(pos2(x, r.top() + 32.0), name, t.text);
        let close = Rect::from_min_size(pos2(r.right() - 16.0 - 28.0, r.top() + 12.0), vec2(28.0, 28.0));
        let mut head = ui.new_child(egui::UiBuilder::new().max_rect(close));
        let closed = icon_button(&mut head, "plannerPopupClose", Icon::Close, vec2(28.0, 28.0), false, true, "Close").clicked();
        // Edit / Done: a record opens to be read; Edit makes its fields writable
        let editing = rt.edit_of == Some(what);
        let edit_at = Rect::from_min_size(pos2(close.left() - 32.0, close.top()), vec2(28.0, 28.0));
        let mut edit_ui = ui.new_child(egui::UiBuilder::new().max_rect(edit_at));
        let (icon, tip) = if editing { (Icon::Check, "Done") } else { (Icon::Pencil, "Edit") };
        if icon_button(&mut edit_ui, "plannerPopupEdit", icon, vec2(28.0, 28.0), editing, true, tip).clicked() {
            rt.edit_of = if editing { None } else { Some(what) };
        }

        // an edit made in the record is a new version of the point (for the phone's sync, which goes by versions)
        let before = match what {
            Shown::Site(i) => st.saved.get(i).cloned(),
            Shown::Plan(_) => None,
        };
        let body_top = (r.top() + 32.0 + name_h + 10.0).min(r.bottom() - 60.0);
        let pane = Rect::from_min_max(pos2(r.left() + 1.0, body_top), r.max - vec2(1.0, 1.0));
        let mut pane_ui = ui.new_child(egui::UiBuilder::new().max_rect(pane).layout(egui::Layout::top_down(Align::Min)));
        pane_ui.set_clip_rect(pane);
        card_scroll(&mut pane_ui, ("planner-popup", matches!(what, Shown::Site(_))), pane, |ui| {
            egui::Frame::NONE.inner_margin(Margin { left: 20, right: 20, top: 4, bottom: 20 }).show(ui, |ui| {
                ui.set_width(ui.available_width());
                // under the title: where it is and what to do with it
                match what {
                    Shown::Site(i) => self.site_actions(ui, st, rt, i),
                    Shown::Plan(id) => self.plan_actions(ui, st, rt, id),
                }
                ui.add_space(10.0);
                if ui.available_width() >= WIDE - 60.0 {
                    // two columns: what it is on the left, what to write and do on the right
                    ui.columns(2, |cols| match what {
                        Shown::Site(i) => {
                            self.site_record(&mut cols[0], st, rt, i, Part::Left);
                            self.site_record(&mut cols[1], st, rt, i, Part::Right);
                        }
                        Shown::Plan(id) => {
                            self.plan_record(&mut cols[0], st, rt, id, Part::Left);
                            self.plan_record(&mut cols[1], st, rt, id, Part::Right);
                        }
                    });
                } else {
                    match what {
                        Shown::Site(i) => self.site_record(ui, st, rt, i, Part::All),
                        Shown::Plan(id) => self.plan_record(ui, st, rt, id, Part::All),
                    }
                }
            });
        });
        if let (Shown::Site(i), Some(before)) = (what, before)
            && let Some(site) = st.saved.get_mut(i)
            && site.id == before.id
            && !site.same_content(&before)
        {
            site.tidy();
            site.touch();
        }
        if closed {
            rt.site_open = None;
            rt.plan_popup = None;
            rt.edit_of = None;
        }
    }

    /// The calculations (exposure and focus, time-lapse, filters, star trails) in a large card centred over the map.
    pub(super) fn calc_popup(&mut self, ui: &mut Ui, st: &mut PlannerState, rt: &mut Runtime, free: Rect) {
        if !rt.calc_open {
            return;
        }
        // a record opened in front of it takes its place
        if shown(st, rt).is_some() {
            rt.calc_open = false;
            return;
        }
        let t = self.t;
        let r = calc_rect(free);
        if r.width() < 200.0 || r.height() < 160.0 {
            return;
        }
        super::layout::surface(ui.painter(), r, &t);
        // (behind the numbers the map would only be noise)
        ui.painter().rect_filled(r, crate::theme::R_CARD, t.backdrop.gamma_multiply(0.9));
        let _ = ui.interact(r, egui::Id::new("planner-calc-bg"), Sense::click_and_drag());
        let painter = ui.painter().with_clip_rect(r);
        painter.text(pos2(r.left() + 20.0, r.top() + 28.0), Align2::LEFT_CENTER, crate::i18n::tr("Calculations"), t.semibold(16.0), t.text);
        let close = Rect::from_min_size(pos2(r.right() - 16.0 - 28.0, r.top() + 14.0), vec2(28.0, 28.0));
        let mut head = ui.new_child(egui::UiBuilder::new().max_rect(close));
        if icon_button(&mut head, "plannerCalcClose", Icon::Close, vec2(28.0, 28.0), false, true, "Close").clicked() {
            rt.calc_open = false;
        }
        let body = Rect::from_min_max(pos2(r.left() + 1.0, r.top() + 52.0), r.max - vec2(1.0, 1.0));
        let narrow = body.width() < 760.0;
        let nav_w = if narrow { body.width() } else { 300.0 };
        let nav = Rect::from_min_size(body.min, vec2(nav_w, body.height()));
        let mut nav_ui = ui.new_child(egui::UiBuilder::new().max_rect(nav).layout(egui::Layout::top_down(Align::Min)));
        nav_ui.set_clip_rect(nav);
        // the sections and the camera they are for
        card_scroll(&mut nav_ui, "planner-calc-nav", nav, |ui| {
            egui::Frame::NONE.inner_margin(Margin { left: 16, right: 12, top: 4, bottom: 16 }).show(ui, |ui| {
                ui.set_width(ui.available_width());
                for (i, (name, what)) in CALC_SECTIONS.iter().enumerate() {
                    let (cell, _) = ui.allocate_exact_size(vec2(ui.available_width(), 46.0), Sense::hover());
                    let resp = ui.interact(cell, egui::Id::new(("planner-calc-section", i)), Sense::click());
                    let on = rt.calc_section == i;
                    if on {
                        ui.painter().rect_filled(cell, 8.0, t.accent.gamma_multiply(0.18));
                        ui.painter().rect_filled(Rect::from_min_size(cell.min + vec2(0.0, 8.0), vec2(3.0, cell.height() - 16.0)), 1.5, t.accent);
                    } else if resp.hovered() {
                        ui.painter().rect_filled(cell, 8.0, t.hover.gamma_multiply(0.5));
                    }
                    let x = cell.left() + 14.0;
                    ui.painter().text(
                        pos2(x, cell.top() + 15.0),
                        Align2::LEFT_CENTER,
                        crate::i18n::tr(name),
                        t.semibold(13.0),
                        if on { t.text } else { t.text_label },
                    );
                    ui.painter().text(pos2(x, cell.top() + 32.0), Align2::LEFT_CENTER, crate::i18n::tr(what), t.font(10.5), t.text_dim);
                    if resp.clicked() {
                        rt.calc_section = i;
                    }
                }
                ui.add_space(10.0);
                ui.separator();
                subhead(ui, "Camera");
                self.camera_inputs(ui, st, rt);
                if narrow {
                    // a narrow window: the section follows the camera
                    ui.add_space(12.0);
                    self.calc_section(ui, st, rt);
                }
            });
        });
        let content = if narrow { None } else { Some(Rect::from_min_max(pos2(nav.right(), body.top()), body.max)) };
        if let Some(content) = content {
            ui.painter().line_segment(
                [pos2(content.left(), content.top() + 4.0), pos2(content.left(), content.bottom() - 12.0)],
                Stroke::new(1.0, t.divider),
            );
            let mut cui = ui.new_child(egui::UiBuilder::new().max_rect(content).layout(egui::Layout::top_down(Align::Min)));
            cui.set_clip_rect(content);
            card_scroll(&mut cui, "planner-calc-card", content, |ui| {
                egui::Frame::NONE.inner_margin(Margin { left: 20, right: 20, top: 4, bottom: 20 }).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    self.calc_section(ui, st, rt);
                });
            });
        }
    }

    /// What the chosen section shows: its results in two columns when there is room, else one under the other.
    fn calc_section(&mut self, ui: &mut Ui, st: &mut PlannerState, rt: &mut Runtime) {
        let n = rt.calc_section.min(CALC_SECTIONS.len() - 1);
        if let Some((name, _)) = CALC_SECTIONS.get(n) {
            subhead(ui, name);
        }
        let two = ui.available_width() >= 640.0;
        let mut parts = |ui: &mut Ui, half: usize| match (n, half) {
            (0, 0) => {
                self.focus_results(ui, st, rt);
                self.calc_tables(ui, st, rt);
            }
            (0, _) => self.calc_dof(ui, st, rt),
            (1, 0) => self.calc_exposure(ui, st, rt),
            (1, _) => self.calc_by_light(ui, st, rt),
            (2, 0) => {
                self.calc_points(ui, st, rt);
                self.trails_card(ui, st, rt);
            }
            (2, _) => {
                self.calc_moon(ui, st, rt);
                self.calc_subject(ui, st, rt);
                self.calc_equiv(ui, st, rt);
            }
            (3, 0) => self.timelapse_card(ui, st, rt),
            (3, _) => self.nd_card(ui, st, rt),
            (_, 0) => self.panorama_card(ui, st, rt),
            _ => self.panorama_diagram(ui, st),
        };
        if two {
            ui.spacing_mut().item_spacing.x = 32.0;
            ui.columns(2, |cols| {
                parts(&mut cols[0], 0);
                parts(&mut cols[1], 1);
            });
        } else {
            parts(ui, 0);
            ui.add_space(8.0);
            parts(ui, 1);
        }
    }

    /// A plan: its name and notes (editable), when and where, the target, the reminder, and what to do with it.
    fn plan_record(&mut self, ui: &mut Ui, st: &mut PlannerState, rt: &mut Runtime, id: u64, part: Part) {
        let t = self.t;
        let editing = rt.edit_of == Some(Shown::Plan(id));
        let Some(p) = st.plans.iter_mut().find(|p| p.id == id) else { return };
        let label = |ui: &mut Ui, s: &str| {
            ui.label(egui::RichText::new(crate::i18n::tr(s)).font(t.font(11.5)).color(t.text_dim));
        };
        if part.left() {
            if editing {
                label(ui, "Name");
                TextField::singleline("planner:popupPlanName", &mut p.name).width(ui.available_width()).show(ui);
                ui.add_space(8.0);
            }
            let when = Date::parse(&p.date).map_or_else(|| p.date.clone(), short_date);
            let mut out = vec![("popup:day", "Day", when, None), ("popup:time", "Time", clock_minutes(p.minute), None)];
            if !p.place.trim().is_empty() {
                out.push(("popup:place", "Place", p.place.trim().to_string(), None));
            }
            if let Some([la, lo]) = p.target {
                out.push(("popup:target", "Target", format!("{la:.5}, {lo:.5}"), None));
                out.push(("popup:targetHeight", "Target Height", format!("{:.0} m", p.target_height), None));
            }
            tiles(ui, &out);
        }
        if !part.right() {
            p.tidy();
            return;
        }
        if part == Part::All {
            ui.add_space(6.0);
        }
        if editing {
            label(ui, "Notes");
            TextField::multiline("planner:popupPlanNotes", &mut p.notes)
                .hint(crate::i18n::tr("Notes: access, framing, gear…"))
                .width(ui.available_width())
                .rows(if part == Part::All { 5 } else { 4 })
                .show(ui);
        } else {
            read_field(ui, &t, "Notes", &p.notes);
        }
        p.tidy();
        // what to do with the plan, between its notes and its reminder
        ui.add_space(8.0);
        if self.plan_buttons(ui, st, rt, id) {
            return;
        }
        ui.add_space(8.0);
        let Some(p) = st.plans.iter_mut().find(|p| p.id == id) else { return };
        if editing {
            label(ui, "Remind me");
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                for (m, text) in ALARMS {
                    if text_button(ui, &format!("popupAlarm{m}"), text, p.alarm == m).clicked() {
                        p.alarm = m;
                    }
                }
            });
            alarm_custom(ui, &t, id, &mut p.alarm);
        } else {
            row(ui, "popup:alarm", "Remind me", &alarm_text(p.alarm), None);
        }
        p.tidy();
    }

    /// Under a plan's title: where it is.
    fn plan_actions(&mut self, ui: &mut Ui, st: &mut PlannerState, _rt: &mut Runtime, id: u64) {
        let t = self.t;
        let Some(p) = st.plans.iter().find(|p| p.id == id) else { return };
        ui.label(egui::RichText::new(format!("{:.5}, {:.5}", p.lat, p.lon)).font(t.font(12.5)).color(t.text_dim));
    }

    /// What to do with a plan: show it on the map, send it to the calendar, directions, delete.
    /// Returns whether the plan was deleted (nothing more of it is shown).
    fn plan_buttons(&mut self, ui: &mut Ui, st: &mut PlannerState, rt: &mut Runtime, id: u64) -> bool {
        let Some(p) = st.plans.iter().find(|p| p.id == id) else { return true };
        let (lat, lon) = (p.lat, p.lon);
        let mut delete = false;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            if text_button(ui, "popupPlanOpen", "Show in the Planner", true).clicked() {
                self.intents.push(Intent::ShowPlanInPlanner(id));
            }
            if text_button(ui, "popupPlanCalendar", "Add to Calendar", false).clicked() {
                self.intents.push(Intent::SendToCalendar(id));
            }
            if text_button(ui, "plannerPlanDirections", "Directions ↗", false).clicked() {
                self.intents.push(Intent::OpenUrl(format!("https://maps.apple.com/?daddr={lat:.6},{lon:.6}")));
            }
            if text_button(ui, "popupPlanDelete", "Delete", false).clicked() {
                delete = true;
            }
        });
        if delete {
            st.plans.retain(|p| p.id != id);
            if st.open_plan == Some(id) {
                st.open_plan = None;
            }
            rt.plan_popup = None;
        }
        delete
    }
}

/// The reminder in words: a preset's name, or the amount in the largest unit that divides it evenly.
pub(super) fn alarm_text(minutes: u32) -> String {
    if let Some((_, s)) = ALARMS.iter().find(|(m, _)| *m == minutes) {
        return crate::i18n::tr(s).to_string();
    }
    let (n, unit) = alarm_split(minutes);
    crate::i18n::tr_format!("{n} {unit} before", n = n, unit = crate::i18n::tr(unit.label()))
}

/// The units of a free reminder, in minutes each.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum AlarmUnit {
    Minutes,
    Hours,
    Days,
    Weeks,
}

impl AlarmUnit {
    const ALL: [AlarmUnit; 4] = [AlarmUnit::Minutes, AlarmUnit::Hours, AlarmUnit::Days, AlarmUnit::Weeks];
    pub(super) fn minutes(self) -> u32 {
        match self {
            AlarmUnit::Minutes => 1,
            AlarmUnit::Hours => 60,
            AlarmUnit::Days => 1440,
            AlarmUnit::Weeks => 10_080,
        }
    }
    fn label(self) -> &'static str {
        match self {
            AlarmUnit::Minutes => "minutes",
            AlarmUnit::Hours => "hours",
            AlarmUnit::Days => "days",
            AlarmUnit::Weeks => "weeks",
        }
    }
}

/// An amount of minutes as a count of its largest even unit (90 → 90 minutes, 120 → 2 hours, 10080 → 1 week).
pub(super) fn alarm_split(minutes: u32) -> (u32, AlarmUnit) {
    for u in [AlarmUnit::Weeks, AlarmUnit::Days, AlarmUnit::Hours] {
        if minutes >= u.minutes() && minutes.is_multiple_of(u.minutes()) {
            return (minutes / u.minutes(), u);
        }
    }
    (minutes, AlarmUnit::Minutes)
}

/// A reminder of any length: an amount and its unit, beside the presets. 0 is no reminder.
fn alarm_custom(ui: &mut Ui, t: &Tokens, id: u64, alarm: &mut u32) {
    let key = egui::Id::new(("planner-alarm-unit", id));
    let (n0, u0) = alarm_split(*alarm);
    // the unit the user picked sticks (7 days is also 1 week); else the one that fits
    let unit: AlarmUnit = ui.ctx().data(|d| d.get_temp::<(u32, AlarmUnit)>(key)).filter(|(m, _)| *m == *alarm).map_or(u0, |(_, u)| u);
    let mut n = if unit == u0 { n0 } else { *alarm / unit.minutes() };
    let mut unit_now = unit;
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
        ui.label(egui::RichText::new(crate::i18n::tr("Custom")).font(t.font(11.5)).color(t.text_dim));
        let max = 525_600 / unit.minutes();
        let _r = ui.add(egui::DragValue::new(&mut n).range(0..=max).speed(0.2));
        for u in AlarmUnit::ALL {
            if text_button(ui, &format!("popupAlarmUnit{}", u.label()), crate::i18n::tr(u.label()), unit_now == u).clicked() {
                unit_now = u;
            }
        }
        ui.label(egui::RichText::new(crate::i18n::tr("before")).font(t.font(11.5)).color(t.text_dim));
    });
    let minutes = n.saturating_mul(unit_now.minutes()).min(525_600);
    if minutes != *alarm || unit_now != unit {
        *alarm = minutes;
        ui.ctx().data_mut(|d| d.insert_temp(key, (minutes, unit_now)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_card_sits_inside_the_free_part_whatever_its_size() {
        for (w, h) in [(300.0, 200.0), (1100.0, 700.0), (1900.0, 1000.0), (40.0, 30.0)] {
            let free = Rect::from_min_size(pos2(10.0, 10.0), vec2(w, h));
            let r = card_rect(free);
            assert!(free.contains_rect(r), "{w}×{h}: {r:?}");
            assert!(r.width() <= 880.0 && r.height() <= 640.0, "{w}×{h}: {r:?}");
            if r.width() > 410.0 {
                assert!(r.width() > r.height(), "{w}×{h}: wider than tall: {r:?}");
            }
        }
    }

    #[test]
    fn a_reminder_can_be_any_length() {
        assert_eq!(alarm_split(90), (90, AlarmUnit::Minutes));
        assert_eq!(alarm_split(120), (2, AlarmUnit::Hours));
        assert_eq!(alarm_split(2880), (2, AlarmUnit::Days));
        assert_eq!(alarm_split(10_080), (1, AlarmUnit::Weeks));
        assert_eq!(alarm_split(30_240), (3, AlarmUnit::Weeks));
        assert_eq!(alarm_split(0), (0, AlarmUnit::Minutes));
        for m in [1u32, 59, 61, 1439, 4321, 525_600] {
            let (n, u) = alarm_split(m);
            assert_eq!(n * u.minutes(), m, "{m}");
        }
        assert_eq!(alarm_text(60), "1 h", "a preset keeps its name");
        assert!(alarm_text(2880).contains('2'), "{}", alarm_text(2880));
    }

    #[test]
    fn a_record_that_is_gone_is_not_shown() {
        let mut st = PlannerState::default();
        let mut rt = Runtime::default();
        assert_eq!(shown(&st, &rt), None);
        rt.site_open = Some(0);
        assert_eq!(shown(&st, &rt), None, "no site 0");
        st.saved.push(SavedPlace::default());
        assert_eq!(shown(&st, &rt), Some(Shown::Site(0)));
        rt.plan_popup = Some(5);
        assert_eq!(shown(&st, &rt), Some(Shown::Site(0)), "a site wins");
        rt.site_open = None;
        assert_eq!(shown(&st, &rt), None, "no plan 5");
        st.plans.push(Plan { id: 5, ..Plan::default() });
        assert_eq!(shown(&st, &rt), Some(Shown::Plan(5)));
    }
}
