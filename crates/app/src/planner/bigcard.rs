//! The cards over the map: Information, Sky, Line Up, Find and the calculations share one central card that opens from
//! the middle of the screen, with the buttons that pick which one is shown along its top.

use super::skycal::SkyMode;
use super::*;

/// The cards that open over the map; one at a time.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Card {
    Sky,
    Align,
    Calc,
    Find,
    Info,
}

/// Opens (or, if already open, closes) a card and closes the others and any record in front.
pub(super) fn toggle(rt: &mut Runtime, card: Card) {
    let was = match card {
        Card::Sky => rt.sky_open,
        Card::Align => rt.align_open,
        Card::Calc => rt.calc_open,
        Card::Find => rt.find_open,
        Card::Info => rt.info_card,
    };
    close_all(rt);
    if !was {
        open(rt, card);
    }
}

/// Opens a card (the others close).
pub(super) fn open_only(rt: &mut Runtime, card: Card) {
    close_all(rt);
    open(rt, card);
}

fn close_all(rt: &mut Runtime) {
    (rt.sky_open, rt.align_open, rt.calc_open, rt.find_open, rt.info_card) = (false, false, false, false, false);
}

fn open(rt: &mut Runtime, card: Card) {
    match card {
        Card::Sky => rt.sky_open = true,
        Card::Align => rt.align_open = true,
        Card::Calc => rt.calc_open = true,
        Card::Find => rt.find_open = true,
        Card::Info => rt.info_card = true,
    }
    rt.site_open = None;
    rt.plan_popup = None;
    rt.edit_of = None;
}

/// The mode tabs of the Sky card and the page each shows.
/// A piece of a card.
#[derive(Clone, Copy)]
enum Item {
    Now,
    SunCard,
    SunFacts,
    SunCal,
    Dial,
    MoonCard,
    MoonFacts,
    MoonCal,
    Core,
    Showers,
    Eclipses,
    Satellites,
    NightCal,
    Year,
    /// A part of the Line Up card (see `align_card`).
    Align(u8),
}

const SKY_TABS: [Tab; 4] = [Tab::Sun, Tab::Moon, Tab::Night, Tab::Calendar];

impl Page<'_> {
    /// The frame of a card: the surface (growing from the middle of the screen when it opens), the buttons that pick the
    /// card and the close button. Gives the body and whether it was closed.
    fn card_shell(&mut self, ui: &mut Ui, rt: &mut Runtime, free: Rect, card: Card, id: &str) -> Option<(Rect, bool)> {
        let t = self.t;
        let r = open_rect(ui, &t, free)?;
        let _ = ui.interact(r, egui::Id::new((id, "bg")), Sense::click_and_drag());
        self.card_tabs(ui, rt, r, card);
        let close = Rect::from_min_size(pos2(r.right() - 16.0 - 28.0, r.top() + 14.0), vec2(28.0, 28.0));
        let mut head = ui.new_child(egui::UiBuilder::new().max_rect(close));
        let closed = icon_button(&mut head, &format!("{id}Close"), Icon::Close, vec2(28.0, 28.0), false, true, "Close").clicked();
        Some((Rect::from_min_max(pos2(r.left() + 1.0, r.top() + 52.0), r.max - vec2(1.0, 1.0)), closed))
    }

    /// The buttons along the top of a card that pick which one is shown (Information, Sky, Line Up, Find, Calculations).
    pub(super) fn card_tabs(&mut self, ui: &mut Ui, rt: &mut Runtime, r: Rect, current: Card) {
        let t = self.t;
        let mut x = r.left() + 16.0;
        for (card, id, label) in CARDS {
            let text = crate::i18n::tr(label);
            let galley = ui.painter().layout_no_wrap(text.to_string(), t.font(13.0), t.text);
            let cell = Rect::from_min_size(pos2(x, r.top() + 13.0), vec2(galley.size().x + 24.0, 30.0));
            x = cell.right() + 4.0;
            if cell.right() > r.right() - 56.0 {
                break;
            }
            let resp = ui.interact(cell, egui::Id::new(("planner-card-tab", id)), Sense::click());
            let on = card == current;
            let p = ui.painter().with_clip_rect(r);
            if on {
                p.rect_filled(cell, R_CONTROL, t.tool_active);
            } else if resp.hovered() {
                p.rect_filled(cell, R_CONTROL, t.hover.gamma_multiply(0.6));
            }
            let color = if on {
                t.text
            } else if resp.hovered() {
                t.text_label
            } else {
                t.text_dim
            };
            p.text(cell.center(), Align2::CENTER_CENTER, text, t.semibold(13.0), color);
            if resp.clicked() && !on {
                open_only(rt, card);
            }
        }
    }

    /// The Information card: the place and its coordinates on the left, the facts of the day on the right.
    pub(super) fn info_popup(&mut self, ui: &mut Ui, st: &mut PlannerState, rt: &mut Runtime, free: Rect) {
        if !rt.info_card {
            return;
        }
        if super::popup::shown(st, rt).is_some() {
            rt.info_card = false;
            return;
        }
        let Some((body, closed)) = self.card_shell(ui, rt, free, Card::Info, "plannerInfo") else { return };
        if closed {
            rt.info_card = false;
        }
        let wide = body.width() >= 700.0;
        let left_w = if wide { 360.0 } else { body.width() };
        let left = Rect::from_min_size(body.min, vec2(left_w, if wide { body.height() } else { (body.height() * 0.45).max(200.0) }));
        let mut l = ui.new_child(egui::UiBuilder::new().max_rect(left).layout(egui::Layout::top_down(Align::Min)));
        l.set_clip_rect(left);
        super::popup::card_scroll(&mut l, "planner-info-left", left, |ui| {
            egui::Frame::NONE.inner_margin(Margin { left: 20, right: 16, top: 4, bottom: 20 }).show(ui, |ui| {
                ui.set_width(ui.available_width());
                self.side_head(ui, st, rt);
            });
        });
        let right = if wide {
            Rect::from_min_max(pos2(left.right(), body.top()), body.max)
        } else {
            Rect::from_min_max(pos2(body.left(), left.bottom()), body.max)
        };
        let mut c = ui.new_child(egui::UiBuilder::new().max_rect(right).layout(egui::Layout::top_down(Align::Min)));
        c.set_clip_rect(right);
        super::popup::card_scroll(&mut c, "planner-info-right", right, |ui| {
            egui::Frame::NONE.inner_margin(Margin { left: 16, right: 20, top: 4, bottom: 24 }).show(ui, |ui| {
                ui.set_width(ui.available_width());
                self.pane(ui, st, rt, Tab::Info);
            });
        });
    }

    /// A card that has just been closed shrinks back into the middle of the screen, fading out.
    pub(super) fn card_ghost(&mut self, ui: &mut Ui, open: bool, free: Rect) {
        let k = ui.ctx().data(|d| d.get_temp::<f32>(egui::Id::new("planner-card-grow-k"))).unwrap_or(0.0);
        if open || k <= 0.001 {
            return;
        }
        let (rect, a) = grown(super::popup::calc_rect(free), k);
        super::layout::surface_big_at(ui.painter(), rect, &self.t, a);
    }

    pub(super) fn sky_popup(&mut self, ui: &mut Ui, st: &mut PlannerState, rt: &mut Runtime, free: Rect) {
        if !rt.sky_open {
            return;
        }
        if super::popup::shown(st, rt).is_some() {
            rt.sky_open = false;
            return;
        }
        let Some((body, closed)) = self.card_shell(ui, rt, free, Card::Sky, "plannerSky") else { return };
        if closed {
            rt.sky_open = false;
        }
        let mut cui = ui.new_child(egui::UiBuilder::new().max_rect(body).layout(egui::Layout::top_down(Align::Min)));
        cui.set_clip_rect(body);
        super::popup::card_scroll(&mut cui, "planner-sky-card", body, |ui| {
            egui::Frame::NONE.inner_margin(Margin { left: 20, right: 20, top: 4, bottom: 20 }).show(ui, |ui| {
                ui.set_width(ui.available_width());
                self.sky_body(ui, st, rt);
            });
        });
    }

    fn sky_body(&mut self, ui: &mut Ui, st: &mut PlannerState, rt: &mut Runtime) {
        let items = [("Sun", "sun"), ("Moon", "moon"), ("Night", "night"), ("Milky Way", "core")];
        let chosen =
            SKY_TABS.iter().position(|t| *t == st.tab).unwrap_or_else(|| SKY_TABS.iter().position(|t| *t == SkyMode::of(st.tab).tab()).unwrap_or(0));
        if let Some(n) = crate::widgets::segmented(ui, "plannerSkyMode", &items, Some(chosen), 4)
            && let Some(tab) = SKY_TABS.get(n)
        {
            st.tab = *tab;
        }
        ui.add_space(10.0);
        let list: &[Item] = match SKY_TABS.get(chosen).copied().unwrap_or(Tab::Sun) {
            Tab::Sun => &[Item::Now, Item::SunCard, Item::SunCal, Item::SunFacts, Item::Dial],
            Tab::Moon => &[Item::MoonCard, Item::MoonCal, Item::MoonFacts],
            Tab::Night => &[Item::Core, Item::NightCal, Item::Showers, Item::Year, Item::Eclipses, Item::Satellites],
            _ => &[Item::Year, Item::Core],
        };
        self.flow(ui, st, rt, list);
    }

    /// The pieces laid out in the card: each goes under whichever column is shorter, so there are no gaps.
    fn flow(&mut self, ui: &mut Ui, st: &mut PlannerState, rt: &mut Runtime, list: &[Item]) {
        if ui.available_width() < 760.0 {
            for (i, item) in list.iter().enumerate() {
                if i > 0 {
                    ui.add_space(8.0);
                }
                self.item(ui, st, rt, *item);
            }
            return;
        }
        let gap = 32.0;
        let full = ui.available_rect_before_wrap();
        let w = ((full.width() - gap) / 2.0).floor();
        let mut cols: Vec<Ui> = (0..2)
            .map(|i| {
                let x = full.left() + i as f32 * (w + gap);
                let mut c = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(Rect::from_min_size(pos2(x, full.top()), vec2(w, full.height())))
                        .layout(egui::Layout::top_down(Align::Min)),
                );
                c.set_width(w);
                c
            })
            .collect();
        for item in list {
            let height = |c: &Ui| c.min_rect().height();
            let pick = usize::from(height(&cols[1]) < height(&cols[0]));
            let col = &mut cols[pick];
            col.add_space(if height(col) > 1.0 { 8.0 } else { 0.0 });
            self.item(col, st, rt, *item);
        }
        let used = cols.iter().fold(Rect::from_min_size(full.min, vec2(0.0, 0.0)), |r, c| r.union(c.min_rect()));
        ui.allocate_rect(used, Sense::hover());
    }

    fn item(&mut self, ui: &mut Ui, st: &mut PlannerState, rt: &mut Runtime, item: Item) {
        match item {
            Item::Now => {
                self.now_grid(ui, st);
                let shadow = match light_studio_astro::align::shadow(1.0, self.skies.sun) {
                    Some((k, az)) => crate::i18n::tr("{k}× the object's height, towards {d}")
                        .replace("{k}", &format!("{k:.1}"))
                        .replace("{d}", crate::i18n::tr_ctx("compass", compass(az))),
                    None => crate::i18n::tr("no sun").to_string(),
                };
                ui.add_space(4.0);
                row(ui, "sun:shadow", "Shadows", &shadow, Some(Color32::from_gray(125)));
            }
            Item::SunCard => self.sun_card(ui),
            Item::SunFacts => self.sun_facts(ui),
            Item::SunCal => {
                subhead(ui, "Sun Calendar");
                self.sky_calendar(ui, SkyMode::Sun, rt);
            }
            Item::Dial => {
                subhead(ui, "Sky");
                self.dial_card(ui);
            }
            Item::MoonCard => self.moon_card(ui, st),
            Item::MoonFacts => self.moon_facts(ui, rt),
            Item::MoonCal => {
                subhead(ui, "Moon Calendar");
                self.sky_calendar(ui, SkyMode::Moon, rt);
            }
            Item::Core => self.core_card(ui),
            Item::Showers => {
                subhead(ui, "Meteor Showers");
                self.showers_card(ui);
            }
            Item::Eclipses => {
                subhead(ui, "Eclipses");
                self.eclipses_card(ui, st, rt);
            }
            Item::Satellites => {
                subhead(ui, "Satellites");
                self.satellites_card(ui, st, rt);
            }
            Item::NightCal => {
                subhead(ui, "Night Calendar");
                self.sky_calendar(ui, SkyMode::Night, rt);
            }
            Item::Year => {
                subhead(ui, "Milky Way Calendar");
                self.year_card(ui, st);
            }
            Item::Align(part) => self.align_card(ui, st, rt, part),
        }
    }

    pub(super) fn align_popup(&mut self, ui: &mut Ui, st: &mut PlannerState, rt: &mut Runtime, free: Rect) {
        if !rt.align_open {
            return;
        }
        if super::popup::shown(st, rt).is_some() {
            rt.align_open = false;
            return;
        }
        let Some((body, closed)) = self.card_shell(ui, rt, free, Card::Align, "plannerAlign") else { return };
        if closed {
            rt.align_open = false;
        }
        let mut cui = ui.new_child(egui::UiBuilder::new().max_rect(body).layout(egui::Layout::top_down(Align::Min)));
        cui.set_clip_rect(body);
        super::popup::card_scroll(&mut cui, "planner-align-card", body, |ui| {
            egui::Frame::NONE.inner_margin(Margin { left: 20, right: 20, top: 4, bottom: 20 }).show(ui, |ui| {
                ui.set_width(ui.available_width());
                self.flow(ui, st, rt, &[Item::Align(0), Item::Align(1), Item::Align(3)]);
            });
        });
    }
}

/// The cards the buttons along their top pick from: the card, its id and its name.
const CARDS: [(Card, &str, &str); 5] = [
    (Card::Info, "info", "Information"),
    (Card::Sky, "sky", "Sky"),
    (Card::Align, "align", "Line Up"),
    (Card::Find, "find", "Find"),
    (Card::Calc, "calc", "Calculations"),
];

/// How far open the central card is (0 = a dot in the middle of the screen, 1 = the whole card), eased; one value for all
/// the cards, so changing from one to another does not replay the opening.
pub(super) fn grow(ctx: &egui::Context, open: bool) -> f32 {
    let k = ctx.animate_bool_with_time(egui::Id::new("planner-card-grow"), open, 0.28);
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("planner-card-grow-k"), k));
    k
}

/// The card's rectangle at `k` of its opening, and its opacity: it comes out of the middle like the island.
fn grown(full: Rect, k: f32) -> (Rect, f32) {
    let e = k * k * (3.0 - 2.0 * k);
    let start = Rect::from_center_size(full.center(), vec2(64.0, 40.0));
    (Rect::from_min_max(start.min.lerp(full.min, e), start.max.lerp(full.max, e)), e.clamp(0.0, 1.0))
}

/// The rectangle a big card is drawn in. While it is still growing, only its surface is drawn (and none gives back);
/// too small a room gives none either.
pub(super) fn open_rect(ui: &mut Ui, t: &Tokens, free: Rect) -> Option<Rect> {
    let full = super::popup::calc_rect(free);
    if full.width() < 200.0 || full.height() < 160.0 {
        return None;
    }
    let k = ui.ctx().data(|d| d.get_temp::<f32>(egui::Id::new("planner-card-grow-k"))).unwrap_or(1.0);
    if k < 0.999 {
        let (rect, a) = grown(full, k);
        super::layout::surface_big_at(ui.painter(), rect, t, a);
        let _ = ui.interact(rect, egui::Id::new("planner-card-growing"), Sense::click_and_drag());
        return None;
    }
    super::layout::surface_big(ui.painter(), full, t);
    Some(full)
}
