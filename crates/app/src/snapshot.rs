//! The interface drawn into an image without a window or a GPU: the app's own frame loop run in an offscreen
//! [`egui::Context`] and rasterized on the CPU ([`crate::softpaint`]). It is how the look of the app is checked on a
//! machine with no screen (see `examples/snapshot.rs`), and what the tests use to look at the interface.

use egui::{Color32, ColorImage, Event, RawInput, Rect, ViewportId, pos2};

use crate::App;
use crate::softpaint::{self, TextureStore};
use crate::state::UiState;

/// What shows where nothing is drawn.
const CLEAR: Color32 = Color32::from_rgb(12, 12, 12);
/// The pretend time between frames, seconds (so the animations are the same on every run).
const FRAME_DT: f64 = 1.0 / 60.0;

/// An app with no window, run frame by frame.
pub struct Session {
    pub ctx: egui::Context,
    pub app: App,
    textures: TextureStore,
    shapes: Vec<egui::epaint::ClippedShape>,
    size: [f32; 2],
    pixels_per_point: f32,
    time: f64,
}

impl Session {
    /// An app in `state`, in a window of `size` points at `pixels_per_point`.
    pub fn new(state: UiState, size: [f32; 2], pixels_per_point: f32) -> Session {
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx);
        crate::theme::apply(&ctx);
        let app = App::new(ctx.clone(), state, false);
        Session { ctx, app, textures: TextureStore::default(), shapes: Vec::new(), size, pixels_per_point, time: 0.0 }
    }

    /// Runs one frame with these events.
    pub fn frame(&mut self, events: Vec<Event>) {
        let rect = Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(self.size[0], self.size[1]));
        let mut raw = RawInput {
            screen_rect: Some(rect),
            time: Some(self.time),
            predicted_dt: FRAME_DT as f32,
            focused: true,
            events,
            max_texture_side: Some(8192),
            ..Default::default()
        };
        let info = raw.viewports.entry(ViewportId::ROOT).or_default();
        info.native_pixels_per_point = Some(self.pixels_per_point);
        info.inner_rect = Some(rect);
        info.focused = Some(true);
        self.time += FRAME_DT;
        let ctx = self.ctx.clone();
        self.app.work(&ctx);
        let app = &mut self.app;
        let mut out = ctx.run_ui(raw, |ui| app.draw(ui));
        self.textures.apply(std::mem::take(&mut out.textures_delta));
        self.shapes = std::mem::take(&mut out.shapes);
        self.pixels_per_point = out.pixels_per_point;
    }

    /// Runs `n` frames, sleeping `pause` between them so the background work (map tiles, calculations) can finish.
    pub fn settle(&mut self, n: usize, pause: std::time::Duration) {
        for _ in 0..n {
            self.frame(Vec::new());
            std::thread::sleep(pause);
        }
    }

    /// The last frame as an image.
    pub fn image(&self) -> ColorImage {
        let size_px = [(self.size[0] * self.pixels_per_point).round() as usize, (self.size[1] * self.pixels_per_point).round() as usize];
        let prims = self.ctx.tessellate(self.shapes.clone(), self.pixels_per_point);
        softpaint::paint(&prims, &self.textures, size_px, self.pixels_per_point, CLEAR)
    }
}
