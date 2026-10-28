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
const SKY_TABS: [Tab; 4] = [Tab::Sun, Tab::Moon, Tab::Night, Tab::Calendar];

impl Page<'_> {
    /// The frame of a card: the surface, the title and the close button. Gives the body and whether it was closed.
    fn card_shell(&mut self, ui: &mut Ui, free: Rect, title: &str, id: &str) -> Option<(Rect, bool)> {
        let t = self.t;
        let r = super::popup::calc_rect(free);
        if r.width() < 200.0 || r.height() < 160.0 {
            return None;
        }
        super::layout::surface(ui.painter(), r, &t);
        // (behind the numbers the map would only be noise)
        ui.painter().rect_filled(r, crate::theme::R_CARD, t.backdrop.gamma_multiply(0.9));
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
        let tab = SKY_TABS.get(chosen).copied().unwrap_or(Tab::Sun);
        let two = ui.available_width() >= 760.0;
        let mut parts = |ui: &mut Ui, half: usize| match (tab, half) {
            (Tab::Sun, 0) => {
                self.now_grid(ui, st);
                let shadow = match light_studio_astro::align::shadow(1.0, self.skies.sun) {
                    Some((k, az)) => crate::i18n::tr("{k}× the object's height, towards {d}")
                        .replace("{k}", &format!("{k:.1}"))
                        .replace("{d}", crate::i18n::tr_ctx("compass", compass(az))),
                    None => crate::i18n::tr("no sun").to_string(),
                };
                ui.add_space(4.0);
                row(ui, "sun:shadow", "Shadows", &shadow, Some(Color32::from_gray(125)));
                ui.add_space(8.0);
                self.sun_card(ui);
                self.sun_facts(ui);
            }
            (Tab::Sun, _) => {
                subhead(ui, "Sun Calendar");
                self.sky_calendar(ui, SkyMode::Sun, rt);
                subhead(ui, "Sky");
                self.dial_card(ui);
            }
            (Tab::Moon, 0) => {
                self.moon_card(ui, st);
                self.moon_facts(ui, rt);
            }
            (Tab::Moon, _) => {
                subhead(ui, "Moon Calendar");
                self.sky_calendar(ui, SkyMode::Moon, rt);
            }
            (Tab::Night, 0) => {
                self.core_card(ui);
                subhead(ui, "Meteor Showers");
                self.showers_card(ui);
                subhead(ui, "Eclipses");
                self.eclipses_card(ui, st, rt);
            }
            (Tab::Night, _) => {
                subhead(ui, "Night Calendar");
                self.sky_calendar(ui, SkyMode::Night, rt);
                subhead(ui, "Milky Way Calendar");
                self.year_card(ui, st);
            }
            (_, 0) => {
                subhead(ui, "Milky Way Calendar");
                self.year_card(ui, st);
            }
            _ => self.core_card(ui),
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
                if ui.available_width() >= 760.0 {
                    ui.spacing_mut().item_spacing.x = 32.0;
                    ui.columns(2, |cols| {
                        self.align_card(&mut cols[0], st, rt, 0);
                        self.align_card(&mut cols[1], st, rt, 1);
                    });
                } else {
                    self.align_card(ui, st, rt, 2);
                }
            });
        });
    }
}

/// The panel's own tab (Information): every card closes.
pub(super) fn toggle_off(rt: &mut Runtime) {
    close_all(rt);
}
