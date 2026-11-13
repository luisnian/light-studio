//! A text field with the behaviour the planner relies on: it tells when an edit was *committed* (Enter, or leaving the
//! field) or *cancelled* (Escape), and whether it was being edited a moment ago, so a value shown formatted is not
//! overwritten while the person types.

use egui::{Context, Id, Response, TextEdit, Ui};

fn editing_id(widget: &str) -> Id {
    Id::new(("text-field-editing", widget))
}

/// True when the field called `widget` had the keyboard focus in the last frame it was shown.
pub fn was_editing(ctx: &Context, widget: &str) -> bool {
    ctx.data(|d| d.get_temp::<bool>(editing_id(widget))).unwrap_or(false)
}

pub struct TextField<'a> {
    widget: String,
    text: &'a mut String,
    multiline: bool,
    hint: Option<String>,
    width: Option<f32>,
    rows: usize,
    frame: bool,
}

/// What a field did in this frame.
pub struct FieldResponse {
    pub response: Response,
    committed: bool,
    cancelled: bool,
    returned: bool,
}

impl FieldResponse {
    /// The edit ended with Enter or by leaving the field.
    pub fn committed(&self) -> bool {
        self.committed
    }

    /// The edit ended with Escape.
    pub fn cancelled(&self) -> bool {
        self.cancelled
    }

    /// The edit ended with the Return key (not by leaving the field).
    pub fn returned(&self) -> bool {
        self.returned
    }

    pub fn changed(&self) -> bool {
        self.response.changed()
    }
}

impl<'a> TextField<'a> {
    pub fn singleline(widget: &str, text: &'a mut String) -> Self {
        TextField { widget: widget.to_string(), text, multiline: false, hint: None, width: None, rows: 1, frame: true }
    }

    pub fn multiline(widget: &str, text: &'a mut String) -> Self {
        TextField { multiline: true, rows: 3, ..TextField::singleline(widget, text) }
    }

    pub fn hint(mut self, hint: impl AsRef<str>) -> Self {
        self.hint = Some(hint.as_ref().to_string());
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    pub fn rows(mut self, rows: usize) -> Self {
        self.rows = rows.max(1);
        self
    }

    pub fn frame(mut self, frame: bool) -> Self {
        self.frame = frame;
        self
    }

    pub fn show(self, ui: &mut Ui) -> FieldResponse {
        let t = crate::theme::Tokens::get(ui.ctx());
        let id = Id::new(("text-field", &self.widget));
        let mut edit = if self.multiline { TextEdit::multiline(self.text).desired_rows(self.rows) } else { TextEdit::singleline(self.text) };
        edit = edit.id(id).font(t.font(13.0));
        if !self.frame {
            edit = edit.frame(egui::Frame::NONE);
        }
        if let Some(hint) = &self.hint {
            edit = edit.hint_text(egui::RichText::new(hint).color(t.text_disabled));
        }
        if let Some(w) = self.width {
            edit = edit.desired_width(w);
        }
        let had_focus = was_editing(ui.ctx(), &self.widget);
        let response = ui.add(edit);
        let focused = response.has_focus();
        let escape = had_focus && ui.input(|i| i.key_pressed(egui::Key::Escape));
        if escape {
            response.surrender_focus();
        }
        ui.ctx().data_mut(|d| d.insert_temp(editing_id(&self.widget), focused && !escape));
        let left = response.lost_focus() && !escape;
        // (a multiline field takes Enter as a new line: it commits only by leaving)
        let committed = left && (self.multiline || had_focus);
        let returned = left && !self.multiline && ui.input(|i| i.key_pressed(egui::Key::Enter));
        FieldResponse { response, committed, cancelled: escape, returned }
    }
}
