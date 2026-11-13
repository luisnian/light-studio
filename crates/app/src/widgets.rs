//! Buttons and small controls the planner is built from. Each takes an `id` that keeps its state apart from the others,
//! and shows a tooltip from the text translated for the language in use.

use egui::{Align2, Color32, Rect, Response, Sense, Ui, Vec2, vec2};

use crate::icons::{self, Icon};
use crate::theme::{R_CONTROL, Tokens, paint_chosen, paint_pill};

/// An icon button of `size`. `active` paints it as switched on; `enabled` false greys it and ignores clicks.
pub fn icon_button(ui: &mut Ui, id: &str, icon: Icon, size: Vec2, active: bool, enabled: bool, tip: &str) -> Response {
    icon_button_inner(ui, id, icon, size, active, enabled, tip, None, false)
}

/// [`icon_button`] whose icon swells a little under the pointer.
pub fn icon_button_grow(ui: &mut Ui, id: &str, icon: Icon, size: Vec2, active: bool, enabled: bool, tip: &str) -> Response {
    icon_button_inner(ui, id, icon, size, active, enabled, tip, None, true)
}

/// [`icon_button_grow`] with the icon in `tint` (when it is not switched on).
pub fn icon_button_grow_tint(
    ui: &mut Ui,
    id: &str,
    icon: Icon,
    size: Vec2,
    active: bool,
    enabled: bool,
    tip: &str,
    tint: Option<Color32>,
) -> Response {
    icon_button_inner(ui, id, icon, size, active, enabled, tip, tint, true)
}

fn icon_button_inner(
    ui: &mut Ui,
    id: &str,
    icon: Icon,
    size: Vec2,
    active: bool,
    enabled: bool,
    tip: &str,
    tint: Option<Color32>,
    grow: bool,
) -> Response {
    let t = Tokens::get(ui.ctx());
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    let resp = ui.interact(rect, egui::Id::new(("icon-button", id)), sense).union(resp);
    let hovered = enabled && resp.hovered();
    if active {
        paint_chosen(ui.painter(), rect, R_CONTROL, &t);
    } else if hovered {
        ui.painter().rect_filled(rect, R_CONTROL, t.hover);
    }
    let color = if !enabled {
        t.text_disabled
    } else if active {
        t.accent
    } else {
        tint.unwrap_or(if hovered { t.text } else { t.text_label })
    };
    let k = if grow { ui.ctx().animate_bool_with_time(egui::Id::new(("icon-grow", id)), hovered, 0.1) } else { 0.0 };
    let edge = (rect.height().min(rect.width()) * 0.58).clamp(12.0, 22.0) * (1.0 + 0.12 * k);
    icons::paint(ui.painter(), Rect::from_center_size(rect.center(), vec2(edge, edge)), icon, color);
    let tip = crate::i18n::tr(tip);
    if tip.is_empty() { resp } else { resp.on_hover_text(tip) }
}

/// A button with a text label. `active` paints it as switched on.
pub fn text_button(ui: &mut Ui, id: &str, label: &str, active: bool) -> Response {
    let t = Tokens::get(ui.ctx());
    let text = crate::i18n::tr(label);
    let galley = ui.painter().layout_no_wrap(text.to_string(), t.medium(12.5), t.text);
    let size = vec2(galley.size().x + 24.0, 28.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let resp = ui.interact(rect, egui::Id::new(("text-button", id)), Sense::click()).union(resp);
    let enabled = ui.is_enabled();
    if active {
        paint_pill(ui.painter(), rect, rect.height() / 2.0, &t);
    } else {
        let fill = if resp.hovered() && enabled { t.hover } else { t.inset };
        ui.painter().rect_filled(rect, rect.height() / 2.0, fill);
        ui.painter().rect_stroke(rect, rect.height() / 2.0, egui::Stroke::new(1.0, t.card_stroke), egui::StrokeKind::Inside);
    }
    let color = if !enabled {
        t.text_disabled
    } else if active {
        t.on_pill
    } else {
        t.text
    };
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, text, t.medium(12.5), color);
    resp
}

/// A row of `items` (label, key) in `columns` columns, one chosen. Gives the index clicked, if any.
pub fn segmented(ui: &mut Ui, id: &str, items: &[(&str, &str)], chosen: Option<usize>, columns: usize) -> Option<usize> {
    let t = Tokens::get(ui.ctx());
    let gap = 4.0;
    let width = ui.available_width();
    // fewer columns when a label would not fit its button
    let widest = items
        .iter()
        .map(|(label, _)| ui.painter().layout_no_wrap(crate::i18n::tr(label).to_string(), t.font(12.0), Color32::WHITE).size().x)
        .fold(0.0, f32::max);
    let mut columns = columns.clamp(1, items.len().max(1));
    while columns > 1 && (width - gap * (columns as f32 - 1.0)) / (columns as f32) < widest + 16.0 {
        columns -= 1;
    }
    let cell_w = ((width - gap * (columns as f32 - 1.0)) / columns as f32).max(24.0);
    let rows = items.len().div_ceil(columns);
    let (track, _) = ui.allocate_exact_size(vec2(width, rows as f32 * 28.0 + (rows.saturating_sub(1)) as f32 * gap + 6.0), Sense::hover());
    // a recessed track, with the chosen option as a pill set into it
    ui.painter().rect_filled(track, 11.0, t.canvas.gamma_multiply(0.7));
    ui.painter().rect_stroke(track, 11.0, egui::Stroke::new(1.0, t.divider), egui::StrokeKind::Inside);
    let area = track.shrink(3.0);
    let mut clicked = None;
    for (i, (label, key)) in items.iter().enumerate() {
        let (row, col) = (i / columns, i % columns);
        let min = area.min + vec2(col as f32 * (cell_w + gap), row as f32 * (28.0 + gap));
        let cell = Rect::from_min_size(min, vec2(cell_w, 28.0));
        let resp = ui.interact(cell, egui::Id::new(("segment", id, key)), Sense::click());
        let on = chosen == Some(i);
        if on {
            paint_pill(ui.painter(), cell, 8.0, &t);
        } else if resp.hovered() {
            ui.painter().rect_filled(cell, 8.0, t.hover);
        }
        let color = if on { t.on_pill } else { t.text_label };
        ui.painter().with_clip_rect(cell).text(cell.center(), Align2::CENTER_CENTER, crate::i18n::tr(label), t.medium(12.0), color);
        if resp.clicked() {
            clicked = Some(i);
        }
    }
    clicked
}
