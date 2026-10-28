//! The page's layout, after the photographer's planning maps (and Luis's own «Mapa solar»): the map fills the
//! view; over it float the place search (top left), the map's buttons (top right), the side panel with its tabs
//! (Sun, Moon, Night, Line Up, Calendar, Sites, Calculations) and, along the bottom, the time bar: the day, the
//! time and a strip with the Sun's and the Moon's height over the day to drag the time along.

use super::*;

/// Space around the floating panels.
const M: f32 = 10.0;
/// The side panel's width at most, and the time bar's height.
const SIDE_W: f32 = 400.0;
/// The width of the map's icon rail, which stays right of the side panel.
pub(super) const RAIL_W: f32 = 36.0;
const TIME_H: f32 = 124.0;
/// The time bar's width unless widened (PhotoPills-like: a compact bar centred under the map).
const TIME_W: f32 = 640.0;

/// The side panel's tabs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Tab {
    #[default]
    Info,
    Sun,
    Moon,
    Night,
    Align,
    Calendar,
    Sites,
    Find,
    Tools,
}

/// Every tab by id (agents and the control channel); the strip shows the Sky group once, led by the Sun/Moon/Night menu.
pub(super) const TABS: [(Tab, &str, &str); 8] = [
    (Tab::Info, "info", "Information"),
    (Tab::Sun, "sun", "Sky"),
    (Tab::Moon, "moon", "Moon"),
    (Tab::Night, "night", "Night"),
    (Tab::Align, "align", "Line Up"),
    (Tab::Calendar, "calendar", "Milky Way"),
    (Tab::Find, "find", "Find"),
    (Tab::Tools, "tools", "Calculations"),
];

/// Where everything goes in `full`: the side panel, the time bar, and the map's free part.
pub(super) struct Areas {
    /// The right edge of the map's icon rail: always the far right, with the side panel opening to its left.
    pub rail_right: f32,
    pub side: Option<Rect>,
    pub time: Rect,
    pub free: Rect,
}

pub(super) fn areas(full: Rect, panel_hidden: bool, time_wide: bool, with_time: bool) -> Areas {
    let side_w = SIDE_W.min((full.width() * 0.5).max(240.0)).min((full.width() - 3.0 * M - RAIL_W).max(0.0));
    let side = (!panel_hidden && side_w > 0.0).then(|| {
        Rect::from_min_max(pos2(full.right() - 2.0 * M - RAIL_W - side_w, full.top() + M), pos2(full.right() - 2.0 * M - RAIL_W, full.bottom() - M))
    });
    let right = side.map_or(full.right() - M, |s| s.left() - M).max(full.left() + M + 160.0);
    let time_h = if with_time { TIME_H.min((full.height() * 0.4).max(0.0)) } else { 0.0 };
    let (left, room) = (full.left() + M, right - full.left() - M);
    let w = if time_wide { room } else { room.min(TIME_W) };
    let x0 = left + (room - w).max(0.0) / 2.0;
    let time = Rect::from_min_max(pos2(x0, full.bottom() - M - time_h), pos2(x0 + w, full.bottom() - M));
    let free_bottom = if with_time { (time.top() - M).max(full.top() + M + 40.0) } else { full.bottom() - M };
    let free = Rect::from_min_max(full.min + vec2(M, M), pos2(right, free_bottom));
    Areas { rail_right: full.right() - M, side, time, free }
}

/// The date card and the time card, stacked under the place search (same width).
pub(crate) fn cards_rects(free: Rect) -> [Rect; 2] {
    let w = (free.width() - 60.0).clamp(120.0, 300.0) + 8.0;
    let date = Rect::from_min_size(pos2(free.left(), free.top() + 42.0), vec2(w, 36.0));
    let time = Rect::from_min_size(pos2(free.left(), date.bottom() + 6.0), vec2(w, 68.0));
    [date, time]
}

/// A floating panel's surface.
pub(super) fn surface(p: &egui::Painter, r: Rect, t: &Tokens) {
    surface_with(p, r, t, 1.0);
}

/// How opaque the search and date/time cards are at rest (they are 60 % see-through so they do not hide the map).
const CARD_REST: f32 = 0.4;
/// The same over the plain (light) map: nearly solid, so the dark cards do not turn into grey patches.
const CARD_REST_PLAIN: f32 = 0.9;

/// The share of opacity of a card over the map: `CARD_REST` at rest, full with the pointer over it (or `keep`),
/// eased over a moment.
pub(super) fn card_opacity(ctx: &egui::Context, id: &str, r: Rect, keep: bool) -> f32 {
    let over = ctx.pointer_hover_pos().is_some_and(|p| r.expand(2.0).contains(p)) || keep;
    let k = ctx.animate_bool_with_time(egui::Id::new(("planner-card-fade", id)), over, 0.12);
    let plain = ctx.data(|d| d.get_temp::<bool>(egui::Id::new("planner-plain-map"))).unwrap_or(false);
    let rest = if plain { CARD_REST_PLAIN } else { CARD_REST };
    rest + (1.0 - rest) * k
}

/// A floating panel's surface at the given opacity (0..1; 1 = as every card).
pub(super) fn surface_with(p: &egui::Painter, r: Rect, t: &Tokens, k: f32) {
    crate::theme::paint_card(p, r, t, k);
}

impl Page<'_> {
    /// The place search, floating over the map's top left corner.
    fn search_is_empty(&self, rt: &Runtime) -> bool {
        rt.search.trim().is_empty()
    }

    pub(super) fn floating_search(&mut self, ui: &mut Ui, rt: &mut Runtime, free: Rect) {
        let t = self.t;
        let w = (free.width() - 60.0).clamp(120.0, 300.0);
        let r = Rect::from_min_size(free.min, vec2(w + 8.0, 36.0));
        let busy = crate::text_field::was_editing(ui.ctx(), "planner:search") || !self.search_is_empty(rt);
        let k = card_opacity(ui.ctx(), "search", r, busy);
        ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new("planner-search-opacity"), k));
        surface_with(ui.painter(), r, &t, k);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(r.shrink(4.0)));
        self.search_field(&mut child, rt, w);
        self.search_results(ui, rt);
    }

    /// The two cards under the place search: the day, and the time with its zone (row 1: previous event, time, next
    /// event, Now, from the left; row 2: the UTC offset).
    pub(super) fn date_time_cards(&mut self, ui: &mut Ui, st: &mut PlannerState, rt: &mut Runtime, free: Rect) {
        let t = self.t;
        let [date, time] = cards_rects(free);
        for (r, id) in [(date, "date"), (time, "time")] {
            let busy = crate::text_field::was_editing(ui.ctx(), "planner:date") && id == "date"
                || crate::text_field::was_editing(ui.ctx(), "planner:time") && id == "time";
            let k = card_opacity(ui.ctx(), id, r, busy);
            surface_with(ui.painter(), r, &t, k);
            let _ = ui.interact(r, egui::Id::new(("planner-card-bg", r.top() as i32)), Sense::click_and_drag());
        }
        let mut row = ui.new_child(egui::UiBuilder::new().max_rect(date.shrink2(vec2(8.0, 4.0))).layout(egui::Layout::left_to_right(Align::Center)));
        row.spacing_mut().item_spacing.x = 6.0;
        self.day_controls(&mut row, st, rt);
        let inner = time.shrink2(vec2(8.0, 4.0));
        let top = Rect::from_min_size(inner.min, vec2(inner.width(), 28.0));
        let bottom = Rect::from_min_size(pos2(inner.left(), top.bottom() + 2.0), vec2(inner.width(), 28.0));
        let mut right = ui.new_child(egui::UiBuilder::new().max_rect(top).layout(egui::Layout::left_to_right(Align::Center)));
        right.spacing_mut().item_spacing.x = 6.0;
        self.time_controls(&mut right, st, rt);
        let mut off = ui.new_child(egui::UiBuilder::new().max_rect(bottom).layout(egui::Layout::left_to_right(Align::Center)));
        off.spacing_mut().item_spacing.x = 6.0;
        self.offset_controls(&mut off, st);
    }

    /// Left to right (under the date): previous event, the time (typed), next event, Now.
    fn time_controls(&mut self, row: &mut Ui, st: &mut PlannerState, rt: &mut Runtime) {
        if icon_button(row, "plannerPrevEvent", Icon::ChevronLeft, vec2(24.0, 26.0), false, true, "Previous Event").clicked() {
            self.step_event(st, -1);
        }
        // the time of day shown
        let mut tbuf = rt.fields.remove("planner:time").unwrap_or_default();
        if !crate::text_field::was_editing(row.ctx(), "planner:time") {
            tbuf = clock_minutes(self.minute);
        }
        let resp = TextField::singleline("planner:time", &mut tbuf).width(64.0).show(row);
        if resp.committed() {
            if let Some(m) = parse_clock(&tbuf) {
                st.minute = Some(m);
            }
            tbuf = clock_minutes(st.minute.unwrap_or(self.minute));
        } else if resp.cancelled() {
            tbuf = clock_minutes(self.minute);
        }
        rt.fields.insert("planner:time".into(), tbuf);
        // the next event of the day (sunrise, a twilight, the golden hour, moonrise…)
        if icon_button(row, "plannerNextEvent", Icon::ChevronRight, vec2(24.0, 26.0), false, true, "Next Event").clicked() {
            self.step_event(st, 1);
        }
        if text_button(row, "plannerNow", "Now", false).on_hover_text(crate::i18n::tr("Today, at this moment")).clicked() {
            st.date.clear();
            st.minute = None;
        }
    }

    // ---- the side panel -----------------------------------------------------------------------

    pub(super) fn side_panel(&mut self, ui: &mut Ui, st: &mut PlannerState, rt: &mut Runtime, r: Rect) {
        let t = self.t;
        surface(ui.painter(), r, &t);
        // swallow clicks on the panel's empty parts (the map is under it)
        let _ = ui.interact(r, egui::Id::new("planner-side-bg"), Sense::click_and_drag());
        let inner = r.shrink2(vec2(16.0, 14.0));
        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::top_down(Align::Min)));
        ui.set_clip_rect(r.shrink(1.0));
        self.side_head(&mut ui, st, rt);
        ui.add_space(8.0);
        self.tabs(&mut ui, rt);
        let pane = Rect::from_min_max(pos2(r.left() + 1.0, ui.cursor().top()), r.max - vec2(1.0, 1.0));
        let mut pane_ui = ui.new_child(egui::UiBuilder::new().max_rect(pane).layout(egui::Layout::top_down(Align::Min)));
        let tab = st.tab;
        egui::ScrollArea::vertical().id_salt(("planner-pane", tab as u8)).auto_shrink([false, false]).show(&mut pane_ui, |ui| {
            egui::Frame::NONE.inner_margin(Margin { left: 15, right: 15, top: 12, bottom: 24 }).show(ui, |ui| {
                ui.set_width(ui.available_width());
                self.pane(ui, st, rt, tab);
            });
        });
    }

    /// The place, its coordinates and where else it can come from.
    fn side_head(&mut self, ui: &mut Ui, st: &mut PlannerState, rt: &mut Runtime) {
        let t = self.t;
        let w = ui.available_width();
        let name = ui.painter().layout(self.place.clone(), t.semibold(16.0), t.text, w - 4.0);
        let (r, _) = ui.allocate_exact_size(vec2(w, name.size().y.min(44.0)), Sense::hover());
        ui.painter().with_clip_rect(r).galley(r.min, name, t.text);
        let line = format!("{} · {}", crate::i18n::date_group_label(&self.c.date.iso(), false), utc_label(self.c.offset));
        ui.label(egui::RichText::new(line).font(t.font(12.0)).color(t.text_dim));
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            let mut lat = st.lat;
            let mut lon = st.lon;
            ui.label(egui::RichText::new(crate::i18n::tr("Lat.")).font(t.font(11.5)).color(t.text_dim));
            let a = number(ui, rt, "lat", &mut lat, (-90.0, 90.0), 4, 70.0);
            ui.label(egui::RichText::new(crate::i18n::tr("Lon.")).font(t.font(11.5)).color(t.text_dim));
            let b = number(ui, rt, "lon", &mut lon, (-180.0, 180.0), 4, 76.0);
            if a || b {
                set_place(st, lat, lon, String::new());
            }
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            let w = (ui.available_width() - 70.0).max(80.0);
            let mut buf = std::mem::take(&mut rt.coords);
            let r =
                TextField::singleline("planner:coords", &mut buf).hint(crate::i18n::tr("Paste coordinates (decimal or degrees)")).width(w).show(ui);
            if r.committed() {
                match super::coords::parse(&buf) {
                    Some((la, lo)) => {
                        set_place(st, la, lo, String::new());
                        buf.clear();
                    }
                    None => rt.search_note = Some(crate::i18n::tr("Those are not coordinates").to_string()),
                }
            }
            rt.coords = buf;
            if text_button(ui, "plannerCopyCoords", "Copy", false).on_hover_text(crate::i18n::tr("Copy the latitude and longitude")).clicked() {
                ui.ctx().copy_text(format!("{:.5}, {:.5}", st.lat, st.lon));
            }
        });
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            if text_button(ui, "plannerMapCentre", "Map Centre", false).on_hover_text(crate::i18n::tr("Use the centre of the map")).clicked() {
                self.intents.push(Intent::UseMapCentre);
            }
        });
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            if text_button(ui, "plannerSavePoi", "Save as POI", false)
                .on_hover_text(crate::i18n::tr("Keep this place as a point of interest"))
                .clicked()
            {
                self.intents.push(Intent::SavePoi);
            }
        });
    }

    /// The tab strip: names with a line under the one shown.
    fn tabs(&mut self, ui: &mut Ui, rt: &mut Runtime) {
        let t = self.t;
        let w = ui.available_width();
        let (row, _) = ui.allocate_exact_size(vec2(w, 34.0), Sense::hover());
        let shown: Vec<&(Tab, &str, &str)> = TABS.iter().filter(|(tab, _, _)| !matches!(tab, Tab::Moon | Tab::Night | Tab::Calendar)).collect();
        let (sky_open, align_open, calc_open, find_open) = (rt.sky_open, rt.align_open, rt.calc_open, rt.find_open);
        let none_open = !(sky_open || align_open || calc_open || find_open);
        let is_on = move |tab: Tab| match tab {
            Tab::Sun => sky_open,
            Tab::Align => align_open,
            Tab::Tools => calc_open,
            Tab::Find => find_open,
            _ => none_open,
        };
        // icons only (the name shows on hover): the strip stays quiet now that a menu picks the map mode
        let cell_w = w / shown.len().max(1) as f32;
        let mut active_x = None;
        let mut x = row.left();
        for (tab, id, label) in shown.iter().map(|t| **t) {
            let cell = Rect::from_min_size(pos2(x, row.top()), vec2(cell_w, row.height()));
            x = cell.right();
            let resp = ui.interact(cell, egui::Id::new(("planner-tab", id)), Sense::click()).on_hover_text(crate::i18n::tr(label));
            let on = is_on(tab);
            let color = if on {
                t.text
            } else if resp.hovered() {
                t.text_label
            } else {
                t.text_dim
            };
            let icon = match tab {
                Tab::Info => Icon::Info,
                Tab::Sun => Icon::Sun,
                Tab::Align => Icon::Linear,
                Tab::Find => Icon::Search,
                _ => Icon::Sliders,
            };
            crate::icons::paint(ui.painter(), Rect::from_center_size(pos2(cell.center().x, cell.center().y - 1.0), vec2(18.0, 18.0)), icon, color);
            if on {
                active_x = Some(cell.center().x);
            }
            if resp.clicked() {
                // Sky, Line Up, Find and the calculations each open in a card of their own, centred over the map
                match tab {
                    Tab::Sun => super::bigcard::toggle(rt, super::bigcard::Card::Sky),
                    Tab::Align => super::bigcard::toggle(rt, super::bigcard::Card::Align),
                    Tab::Tools => super::bigcard::toggle(rt, super::bigcard::Card::Calc),
                    Tab::Find => super::bigcard::toggle(rt, super::bigcard::Card::Find),
                    _ => super::bigcard::toggle_off(rt),
                }
            }
        }
        // the underline slides to the tab shown
        if let Some(ax) = active_x {
            let x = ui.ctx().animate_value_with_time(egui::Id::new("planner-tab-underline"), ax, 0.16);
            let u = Rect::from_min_max(pos2(x - 12.0, row.bottom() - 2.5), pos2(x + 12.0, row.bottom()));
            ui.painter().rect_filled(u, 1.0, t.accent);
        }
        ui.painter().line_segment([pos2(row.left() - 16.0, row.bottom()), pos2(row.right() + 16.0, row.bottom())], Stroke::new(1.0, t.divider));
    }

    fn pane(&mut self, ui: &mut Ui, st: &mut PlannerState, rt: &mut Runtime, tab: Tab) {
        match tab {
            // (the Sky and Line Up cards have their own place over the map now; a saved layout that still names one of
            // their tabs shows Information)
            Tab::Info | Tab::Sun | Tab::Moon | Tab::Night | Tab::Calendar | Tab::Align => self.info_panel(ui, st),
            // (the old POIs tab: records are made from the map's menu now; a saved layout that still names it shows Information)
            Tab::Sites => {
                st.tab = Tab::Info;
                self.info_panel(ui, st);
            }
            // (the search has a card of its own now; a saved layout that still names the tab opens it)
            Tab::Find => {
                st.tab = Tab::Info;
                rt.find_open = true;
                self.info_panel(ui, st);
            }
            // (the calculations have a card of their own now; a saved layout that still names the tab opens it)
            Tab::Tools => {
                st.tab = Tab::Info;
                rt.calc_open = true;
                self.info_panel(ui, st);
            }
        }
        ui.add_space(10.0);
        note(
            ui,
            crate::i18n::tr("Positions are computed on this computer; times are approximate to a minute or two. Meteor figures are typical values."),
        );
    }

    /// Where the Sun, the Moon and the core are at the time shown, side by side; a click hides or shows each on the map
    /// and the strip.
    pub(super) fn now_grid(&mut self, ui: &mut Ui, st: &mut PlannerState) {
        let t = self.t;
        let pal = self.pal;
        let w = ui.available_width();
        let (r, _) = ui.allocate_exact_size(vec2(w, 58.0), Sense::hover());
        ui.painter().rect_filled(r, R_CONTROL + 2.0, t.inset);
        let cw = r.width() / 3.0;
        let names = [("Sun", pal.sun, self.skies.sun), ("Moon", pal.moon, self.skies.moon), ("Milky Way", pal.core, self.skies.core)];
        for (i, (name, color, h)) in names.into_iter().enumerate() {
            let cell = Rect::from_min_size(pos2(r.left() + cw * i as f32, r.top()), vec2(cw, r.height()));
            let resp = ui
                .interact(cell, egui::Id::new(("planner-now", i)), Sense::click())
                .on_hover_text(crate::i18n::tr("Show or hide it on the map and the strip"));
            let on = st.layers[i];
            if i > 0 {
                ui.painter().line_segment([pos2(cell.left(), cell.top() + 8.0), pos2(cell.left(), cell.bottom() - 8.0)], Stroke::new(1.0, t.divider));
            }
            if resp.hovered() {
                ui.painter().rect_filled(cell.shrink(2.0), R_CONTROL, t.hover.gamma_multiply(0.5));
            }
            let p = ui.painter();
            p.circle_filled(pos2(cell.left() + 14.0, cell.top() + 17.0), 4.0, if on { color } else { t.text_disabled });
            p.text(
                pos2(cell.left() + 24.0, cell.top() + 17.0),
                Align2::LEFT_CENTER,
                crate::i18n::tr(name),
                t.font(11.5),
                if on { t.text_dim } else { t.text_disabled },
            );
            p.text(
                pos2(cell.left() + 10.0, cell.top() + 40.0),
                Align2::LEFT_CENTER,
                alt_text(h.alt),
                t.semibold(15.0),
                if on { t.text } else { t.text_disabled },
            );
            p.text(pos2(cell.right() - 8.0, cell.top() + 41.0), Align2::RIGHT_CENTER, az_text(h.az), t.font(11.0), t.text_dim);
            if resp.clicked() {
                st.layers[i] = !st.layers[i];
            }
        }
    }

    // ---- the time bar -------------------------------------------------------------------------

    pub(super) fn time_bar(&mut self, ui: &mut Ui, st: &mut PlannerState, r: Rect) {
        let t = self.t;
        let k = card_opacity(ui.ctx(), "timebar", r, false);
        surface_with(ui.painter(), r, &t, k);
        let _ = ui.interact(r, egui::Id::new("planner-time-bg"), Sense::click_and_drag());
        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(r.shrink2(vec2(14.0, 8.0))).layout(egui::Layout::top_down(Align::Min)));
        ui.set_clip_rect(r.shrink(1.0));
        // the controls float over the map (see `date_time_cards`): the bar is the chart, as tall as it can be
        let h = (ui.available_height() - 18.0).max(20.0);
        self.chart(&mut ui, st, h, true);
        // widen / narrow the bar
        let b = Rect::from_min_size(pos2(r.right() - 30.0, r.top() + 4.0), vec2(24.0, 24.0));
        let mut btn = ui.new_child(egui::UiBuilder::new().max_rect(b).layout(egui::Layout::top_down(Align::Min)));
        let tip = if st.time_wide { "Narrow the time bar" } else { "Widen the time bar" };
        if icon_button(&mut btn, "plannerTimeWide", Icon::Fit, vec2(24.0, 24.0), st.time_wide, true, tip).clicked() {
            st.time_wide = !st.time_wide;
        }
        // fold the bar away until the pointer comes to the bottom edge
        let b2 = b.translate(vec2(-28.0, 0.0));
        let mut btn2 = ui.new_child(egui::UiBuilder::new().max_rect(b2).layout(egui::Layout::top_down(Align::Min)));
        if icon_button(
            &mut btn2,
            "plannerTimeAuto",
            Icon::ChevronDown,
            vec2(24.0, 24.0),
            st.time_auto,
            true,
            "Hide the time bar until the pointer comes to the bottom edge",
        )
        .clicked()
        {
            st.time_auto = !st.time_auto;
        }
    }

    /// Every event of the day shown (Unix seconds, in order): the Sun's and Moon's rises and sets, the twilights, the
    /// golden and blue hours and the Milky Way's rise, dark window and set.
    pub(super) fn day_events(&self) -> Vec<f64> {
        let p = &self.c.plan;
        let mut v: Vec<f64> = Vec::new();
        v.extend([p.sun.rise, p.sun.set, p.moon.rise, p.moon.set, p.core.rise, p.core.set].into_iter().flatten());
        for tw in [&p.sun.civil, &p.sun.nautical, &p.sun.astronomical] {
            v.extend([tw.dawn, tw.dusk].into_iter().flatten());
        }
        for sp in [p.sun.golden_am, p.sun.golden_pm, p.sun.blue_am, p.sun.blue_pm].into_iter().flatten() {
            v.extend([sp.start, sp.end]);
        }
        v.extend(p.core.dark.iter().flat_map(|s| [s.start, s.end]));
        v.retain(|t| t.is_finite() && *t >= p.start && *t < p.start + DAY);
        v.sort_by(f64::total_cmp);
        v.dedup_by(|a, b| (*a - *b).abs() < 60.0);
        v
    }

    /// Moves the time to the next (1) or previous (-1) event; past the last of the day, to the next or previous day.
    pub(super) fn step_event(&self, st: &mut PlannerState, dir: i32) {
        let now = self.c.start + self.minute * 60.0;
        let events = self.day_events();
        let found = if dir > 0 { events.iter().copied().find(|t| *t > now + 30.0) } else { events.iter().rev().copied().find(|t| *t < now - 30.0) };
        match found {
            Some(t) => super::strip::jump_to(st, t, self.c.offset),
            None => {
                st.date = self.c.date.add_days(i64::from(dir)).iso();
                st.minute = Some(if dir > 0 { 0.0 } else { 1439.0 });
            }
        }
    }

    fn day_controls(&mut self, ui: &mut Ui, st: &mut PlannerState, rt: &mut Runtime) {
        let size = vec2(26.0, 26.0);
        if icon_button(ui, "plannerPrev", Icon::ChevronLeft, size, false, true, "Previous Day").clicked() {
            st.date = self.c.date.add_days(-1).iso();
            st.minute = None;
        }
        // the date, typed or picked
        let mut text = rt.fields.remove("planner:date").unwrap_or_default();
        if !crate::text_field::was_editing(ui.ctx(), "planner:date") {
            text = self.c.date.iso();
        }
        let r = TextField::singleline("planner:date", &mut text).width(92.0).show(ui);
        if r.committed() {
            if let Some(d) = Date::parse(&text) {
                st.date = d.iso();
                st.minute = None;
            }
            text = self.c.date.iso();
        } else if r.cancelled() {
            text = self.c.date.iso();
        }
        rt.fields.insert("planner:date".into(), text);
        let mut picked = Some(PickedDate { year: self.c.date.year, month: Some(self.c.date.month as u8), day: Some(self.c.date.day as u8) });
        let today_local = Date::from_unix(self.now, self.c.offset);
        let today = Some(PickedDate { year: today_local.year, month: Some(today_local.month as u8), day: Some(today_local.day as u8) });
        if DatePicker::new("planner", &mut picked).precisions(&[Precision::Day]).today(today).show(ui).changed()
            && let Some(PickedDate { year, month: Some(m), day: Some(d) }) = picked
        {
            st.date = Date::new(year, u32::from(m), u32::from(d)).iso();
            st.minute = None;
        }
        if icon_button(ui, "plannerNext", Icon::ChevronRight, size, false, true, "Next Day").clicked() {
            st.date = self.c.date.add_days(1).iso();
            st.minute = None;
        }
    }

    /// The UTC offset (right to left: Auto, +, the offset, −).
    fn offset_controls(&mut self, ui: &mut Ui, st: &mut PlannerState) {
        let t = self.t;
        let size = vec2(24.0, 24.0);
        let auto = st.utc_offset.is_none();
        // left to right, like the time above it
        if icon_button(ui, "plannerOffsetLess", Icon::Minus, size, false, true, "Earlier Time Zone").clicked() {
            st.utc_offset = Some((self.c.offset - 30).max(-720));
        }
        let source = super::zone::offset_at(st.lat, st.lon, super::noon_of(self.c.date, st.lon), st.zone.as_ref()).1;
        let note = match (auto, source) {
            (false, _) => String::new(),
            (true, super::zone::Source::Real) => format!(" ({})", crate::i18n::tr("auto, from the place's time zone")),
            (true, _) => format!(" ({})", crate::i18n::tr("auto, estimated")),
        };
        let label = format!("{}{}", utc_label(self.c.offset), note);
        ui.label(egui::RichText::new(label).font(t.font(12.0)).color(t.text_dim));
        if icon_button(ui, "plannerOffsetMore", Icon::Plus, size, false, true, "Later Time Zone").clicked() {
            st.utc_offset = Some((self.c.offset + 30).min(840));
        }
        if !auto && text_button(ui, "plannerOffsetAuto", "Auto", false).on_hover_text(crate::i18n::tr("Guess the time zone from the place")).clicked()
        {
            st.utc_offset = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_areas_fit_any_window() {
        for (w, h) in [(280.0, 500.0), (700.0, 900.0), (1600.0, 1000.0), (0.0, 0.0)] {
            let full = Rect::from_min_size(pos2(5.0, 7.0), vec2(w, h));
            for hidden in [false, true] {
                let a = areas(full, hidden, false, true);
                assert!(a.free.width() >= 0.0 && a.free.height() >= 0.0 && a.time.height() >= 0.0, "{w}×{h}");
                if let Some(s) = a.side {
                    assert!(s.width() <= SIDE_W && s.right() <= full.right());
                }
                assert_eq!(a.side.is_none(), hidden || w <= 2.0 * M);
            }
        }
        let a = areas(Rect::from_min_size(Pos2::ZERO, vec2(1600.0, 1000.0)), false, false, true);
        let side = a.side.unwrap();
        assert!(a.free.right() < side.left() && a.time.right() < side.left() && a.free.bottom() < a.time.top());
        // the icon rail stays at the far right whether or not the panel is open: the panel opens to its left
        assert!(side.right() + M <= a.rail_right - RAIL_W);
        assert_eq!(a.rail_right, areas(Rect::from_min_size(Pos2::ZERO, vec2(1600.0, 1000.0)), true, false, true).rail_right);
    }
}
