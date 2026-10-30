//! The application: its state, and the frame loop that draws the planner, the settings window and the messages.

use eframe::egui;

use crate::state::UiState;
use crate::{map, pdi, pdi_pair, planner, settings, tasks};

/// How long a message stays up, seconds.
const TOAST_SECS: f64 = 4.5;
/// How often the state is written when it changed, seconds.
const SAVE_EVERY: f64 = 4.0;

struct Toast {
    text: String,
    error: bool,
    until: f64,
}

pub struct App {
    pub ctx: egui::Context,
    pub ui: UiState,
    pub planner: planner::Runtime,
    pub map: map::Runtime,
    pub pdi: pdi::Pdi,
    pub pdi_pair: pdi_pair::Pair,
    pub tasks: tasks::Tasks,
    /// Running with a window (the synchronisation with the phone starts by itself only then).
    pub styled: bool,
    /// A key just made for the Worker, shown once.
    pub pdi_new_key: Option<String>,
    /// The Worker's key being typed in the settings.
    pub pdi_key_draft: String,
    pub settings_open: bool,
    toasts: Vec<Toast>,
    saved: String,
    saved_at: f64,
    /// When the opening fade began (egui time); `None` before the first frame.
    open_fade_start: Option<f64>,
    /// The opening fade is still running (only in the real app: tests and snapshots stay still).
    open_fade: bool,
}

/// How long the opening fade lasts.
const OPEN_FADE_SECONDS: f64 = 0.4;

impl App {
    pub fn new(ctx: egui::Context, ui: UiState, styled: bool) -> App {
        let language = crate::i18n::Language::parse(&ui.settings.language).unwrap_or_else(crate::i18n::Language::from_system);
        crate::i18n::set_language(language);
        crate::theme::set_red_light(ui.settings.red_light);
        App {
            ctx,
            ui,
            planner: planner::Runtime::default(),
            map: map::Runtime::default(),
            pdi: pdi::Pdi::default(),
            pdi_pair: pdi_pair::Pair::default(),
            tasks: tasks::Tasks::default(),
            styled,
            pdi_new_key: None,
            pdi_key_draft: String::new(),
            settings_open: false,
            toasts: Vec::new(),
            saved: String::new(),
            saved_at: 0.0,
            open_fade_start: None,
            open_fade: styled,
        }
    }

    /// The opening fade: the window shows the dark backdrop and the interface comes up through it in 400 ms
    /// (ease-out). A see-through cover over the whole window does it, so it works the same on every system.
    fn open_fade_overlay(&mut self, ctx: &egui::Context) {
        if !self.open_fade {
            return;
        }
        let now = ctx.input(|i| i.time);
        let start = *self.open_fade_start.get_or_insert(now);
        let p = ((now - start) / OPEN_FADE_SECONDS).clamp(0.0, 1.0);
        if p >= 1.0 {
            self.open_fade = false;
            return;
        }
        let cover = (1.0 - p).powi(3) as f32;
        let colour = ctx.global_style().visuals.panel_fill.gamma_multiply(cover);
        ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("open-fade"))).rect_filled(ctx.content_rect(), 0.0, colour);
        ctx.request_repaint();
    }

    /// A message at the bottom of the window.
    pub fn toast(&mut self, ctx: &egui::Context, text: String) {
        self.push_toast(ctx, text, false);
    }

    /// A message about something that went wrong.
    pub fn toast_error(&mut self, ctx: &egui::Context, text: String) {
        self.push_toast(ctx, text, true);
    }

    fn push_toast(&mut self, ctx: &egui::Context, text: String, error: bool) {
        let now = ctx.input(|i| i.time);
        self.toasts.retain(|t| t.text != text);
        self.toasts.push(Toast { text, error, until: now + TOAST_SECS });
        if self.toasts.len() > 3 {
            self.toasts.remove(0);
        }
        ctx.request_repaint();
    }

    fn show_toasts(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        self.toasts.retain(|t| t.until > now);
        if self.toasts.is_empty() {
            return;
        }
        let t = crate::theme::Tokens::get(ctx);
        egui::Area::new(egui::Id::new("toasts")).order(egui::Order::Tooltip).anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -46.0)).show(
            ctx,
            |ui| {
                for toast in &self.toasts {
                    let galley = ui.painter().layout(toast.text.clone(), t.font(13.0), t.text, 520.0);
                    let (rect, _) = ui.allocate_exact_size(galley.size() + egui::vec2(28.0, 18.0), egui::Sense::hover());
                    crate::theme::paint_card(ui.painter(), rect, &t, 1.0);
                    if toast.error {
                        ui.painter().rect_stroke(rect, crate::theme::R_CARD, egui::Stroke::new(1.5, t.reject), egui::StrokeKind::Inside);
                    }
                    ui.painter().galley(rect.min + egui::vec2(14.0, 9.0), galley, t.text);
                    ui.add_space(6.0);
                }
            },
        );
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
    }

    /// Writes the state when it changed since the last time (at most every few seconds unless `force`).
    pub fn save(&mut self, now: f64, force: bool) {
        if !force && now - self.saved_at < SAVE_EVERY {
            return;
        }
        self.saved_at = now;
        let Ok(text) = self.ui.to_json() else { return };
        if text == self.saved {
            return;
        }
        match crate::state::write(&text) {
            Ok(()) => self.saved = text,
            Err(e) => log::warn!("{e}"),
        }
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        tasks::poll(self, ctx);
        pdi::logic(self, ctx);
        pdi_pair::logic(self, ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        planner::show(self, ui);
        if self.settings_open {
            settings::show(self, &ctx);
        }
        self.show_toasts(&ctx);
        self.open_fade_overlay(&ctx);
        let now = ctx.input(|i| i.time);
        self.save(now, false);
    }

    fn on_exit(&mut self) {
        self.save(0.0, true);
    }
}
