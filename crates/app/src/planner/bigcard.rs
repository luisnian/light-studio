//! The Sky and Line Up cards: like Find and the calculations, each opens in a card of its own over the map, with room for
//! its parts side by side, instead of squeezed into the side panel.

use super::skycal::SkyMode;
use super::*;

/// The cards that open over the map; one at a time.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Card {
    Sky,
    Align,
    Calc,
    Find,
}

/// Opens (or, if already open, closes) a card and closes the others and any record in front.
pub(super) fn toggle(rt: &mut Runtime, card: Card) {
    let was = match card {
        Card::Sky => rt.sky_open,
        Card::Align => rt.align_open,
        Card::Calc => rt.calc_open,
        Card::Find => rt.find_open,
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
    (rt.sky_open, rt.align_open, rt.calc_open, rt.find_open) = (false, false, false, false);
}

fn open(rt: &mut Runtime, card: Card) {
    match card {
        Card::Sky => rt.sky_open = true,
        Card::Align => rt.align_open = true,
        Card::Calc => rt.calc_open = true,
        Card::Find => rt.find_open = true,
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
    NightCal,
    Year,
    /// A part of the Line Up card (see `align_card`).
    Align(u8),
}

const SKY_TABS: [Tab; 4] = [Tab::Sun, Tab::Moon, Tab::Night, Tab::Calendar];

impl Page<'_> {
    /// The frame of a card: the surface, the title and the close button. Gives the body and whether it was closed.
    fn card_shell(&mut self, ui: &mut Ui, free: Rect, title: &str, id: &str) -> Option<(Rect, bool)> {
        let t = self.t;
        let r = super::popup::calc_rect(free);
        if r.width() < 200.0 || r.height() < 160.0 {
            return None;
        }
        super::layout::surface_big(ui.painter(), r, &t);
        let _ = ui.interact(r, egui::Id::new((id, "bg")), Sense::click_and_drag());
        let painter = ui.painter().with_clip_rect(r);
        painter.text(pos2(r.left() + 20.0, r.top() + 28.0), Align2::LEFT_CENTER, crate::i18n::tr(title), t.semibold(16.0), t.text);
        let close = Rect::from_min_size(pos2(r.right() - 16.0 - 28.0, r.top() + 14.0), vec2(28.0, 28.0));
        let mut head = ui.new_child(egui::UiBuilder::new().max_rect(close));
        let closed = icon_button(&mut head, &format!("{id}Close"), Icon::Close, vec2(28.0, 28.0), false, true, "Close").clicked();
        Some((Rect::from_min_max(pos2(r.left() + 1.0, r.top() + 52.0), r.max - vec2(1.0, 1.0)), closed))
    }

    pub(super) fn sky_popup(&mut self, ui: &mut Ui, st: &mut PlannerState, rt: &mut Runtime, free: Rect) {
        if !rt.sky_open {
            return;
        }
        if super::popup::shown(st, rt).is_some() {
            rt.sky_open = false;
            return;
        }
        let Some((body, closed)) = self.card_shell(ui, free, "Sky", "plannerSky") else { return };
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
            Tab::Night => &[Item::Core, Item::NightCal, Item::Showers, Item::Year, Item::Eclipses],
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
        let Some((body, closed)) = self.card_shell(ui, free, "Line Up", "plannerAlign") else { return };
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

/// The panel's own tab (Information): every card closes.
pub(super) fn toggle_off(rt: &mut Runtime) {
    close_all(rt);
}
