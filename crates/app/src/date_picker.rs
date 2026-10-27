//! A calendar that opens under a button: a month at a time, weeks starting on Monday, a day chosen with a click.

use egui::{Align2, Area, Id, Order, Rect, Response, Sense, Ui, pos2, vec2};
use light_studio_astro::time::{Date, days_in_month};

use crate::icons::Icon;
use crate::theme::{R_CARD, R_CONTROL, Tokens};

/// A day, month or year chosen; `month` and `day` are `None` when the choice is coarser.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PickedDate {
    pub year: i32,
    pub month: Option<u8>,
    pub day: Option<u8>,
}

/// How exact a choice must be. The planner only picks days.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Precision {
    Day,
}

pub struct DatePicker<'a> {
    id: String,
    value: &'a mut Option<PickedDate>,
    today: Option<PickedDate>,
}

impl<'a> DatePicker<'a> {
    pub fn new(id: &str, value: &'a mut Option<PickedDate>) -> Self {
        DatePicker { id: id.to_string(), value, today: None }
    }

    pub fn precisions(self, _allowed: &[Precision]) -> Self {
        self
    }

    pub fn today(mut self, today: Option<PickedDate>) -> Self {
        self.today = today;
        self
    }

    /// The button; `changed()` on the answer says a day was chosen.
    pub fn show(self, ui: &mut Ui) -> Response {
        let open_id = Id::new(("date-picker-open", &self.id));
        let shown_id = Id::new(("date-picker-month", &self.id));
        let mut response =
            crate::widgets::icon_button(ui, &format!("datePicker-{}", self.id), Icon::Calendar, vec2(24.0, 24.0), false, true, "Choose a date");
        let mut open = ui.ctx().data(|d| d.get_temp::<bool>(open_id)).unwrap_or(false);
        if response.clicked() {
            open = !open;
            if open {
                // the month of the day now chosen
                let start = self.value.or(self.today).map(|p| (p.year, u32::from(p.month.unwrap_or(1))));
                ui.ctx().data_mut(|d| d.insert_temp(shown_id, start.unwrap_or((2026, 1))));
            }
        }
        if open {
            let (year, month) = ui.ctx().data(|d| d.get_temp::<(i32, u32)>(shown_id)).unwrap_or((2026, 1));
            let anchor = response.rect.left_bottom() + vec2(0.0, 4.0);
            let mut show = (year, month);
            let mut picked: Option<u32> = None;
            let area = Area::new(Id::new(("date-picker-area", &self.id))).order(Order::Foreground).fixed_pos(anchor).show(ui.ctx(), |ui| {
                let t = Tokens::get(ui.ctx());
                let (frame, _) = ui.allocate_exact_size(vec2(224.0, 222.0), Sense::click());
                crate::theme::paint_card(ui.painter(), frame, &t, 1.0);
                let head = Rect::from_min_size(frame.min + vec2(8.0, 8.0), vec2(208.0, 26.0));
                let mut top = ui.new_child(egui::UiBuilder::new().max_rect(head).layout(egui::Layout::left_to_right(egui::Align::Center)));
                if crate::widgets::icon_button(&mut top, "dpPrev", Icon::ChevronLeft, vec2(24.0, 24.0), false, true, "Previous Month").clicked() {
                    show = if month == 1 { (year - 1, 12) } else { (year, month - 1) };
                }
                let title = format!("{} {}", crate::i18n::tr_ctx("month", MONTHS[(month as usize - 1).min(11)]), year);
                let (label, _) = top.allocate_exact_size(vec2(152.0, 24.0), Sense::hover());
                top.painter().text(label.center(), Align2::CENTER_CENTER, title, t.semibold(13.0), t.text);
                if crate::widgets::icon_button(&mut top, "dpNext", Icon::ChevronRight, vec2(24.0, 24.0), false, true, "Next Month").clicked() {
                    show = if month == 12 { (year + 1, 1) } else { (year, month + 1) };
                }
                let grid = Rect::from_min_size(frame.min + vec2(8.0, 40.0), vec2(208.0, 174.0));
                let cell = vec2(208.0 / 7.0, 174.0 / 7.0);
                for (i, wd) in WEEKDAYS_MONDAY_FIRST.iter().enumerate() {
                    let p = pos2(grid.left() + (i as f32 + 0.5) * cell.x, grid.top() + cell.y / 2.0);
                    ui.painter().text(p, Align2::CENTER_CENTER, crate::i18n::tr_ctx("weekday", wd), t.font(10.5), t.text_dim);
                }
                let first = Date::new(year, month, 1);
                let lead = (first.weekday() + 6) % 7;
                for day in 1..=days_in_month(year, month) {
                    let slot = lead + day - 1;
                    let (col, row) = (slot % 7, slot / 7 + 1);
                    let r = Rect::from_min_size(pos2(grid.left() + col as f32 * cell.x, grid.top() + row as f32 * cell.y), cell).shrink(1.0);
                    let resp = ui.interact(r, Id::new(("dp-day", &self.id, day)), Sense::click());
                    let is = |p: Option<PickedDate>| p.is_some_and(|p| p.year == year && p.month == Some(month as u8) && p.day == Some(day as u8));
                    let chosen = is(*self.value);
                    if chosen {
                        ui.painter().rect_filled(r, R_CONTROL, t.tool_active);
                    } else if resp.hovered() {
                        ui.painter().rect_filled(r, R_CONTROL, t.hover);
                    }
                    if is(self.today) {
                        ui.painter().rect_stroke(r, R_CONTROL, egui::Stroke::new(1.0, t.accent), egui::StrokeKind::Inside);
                    }
                    ui.painter().text(
                        r.center(),
                        Align2::CENTER_CENTER,
                        day.to_string(),
                        t.font(12.0),
                        if chosen && t.dark { egui::Color32::WHITE } else { t.text },
                    );
                    if resp.clicked() {
                        picked = Some(day);
                    }
                }
                let _ = R_CARD;
                frame
            });
            ui.ctx().data_mut(|d| d.insert_temp(shown_id, show));
            if let Some(day) = picked {
                *self.value = Some(PickedDate { year, month: Some(month as u8), day: Some(day as u8) });
                response.mark_changed();
                open = false;
            } else if ui.input(|i| i.pointer.any_click()) && !response.clicked() {
                // a click outside the calendar closes it
                let inside = ui.input(|i| i.pointer.interact_pos()).is_some_and(|p| area.response.rect.contains(p));
                if !inside {
                    open = false;
                }
            } else if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                open = false;
            }
        }
        ui.ctx().data_mut(|d| d.insert_temp(open_id, open));
        response
    }
}

const WEEKDAYS_MONDAY_FIRST: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
