//! A tiny stand-in for the window in tests: an app and a context, stepped by hand.

use std::time::Duration;

use crate::App;
use crate::state::UiState;

pub struct Headless {
    pub ctx: egui::Context,
    pub app: App,
}

impl Headless {
    pub fn new() -> Self {
        let ctx = egui::Context::default();
        let app = App::new(ctx.clone(), UiState::default(), false);
        Self { ctx, app }
    }

    /// Runs one pass of the app's background work (tasks, synchronisation), then waits a little.
    pub fn settle(&mut self, pause: Duration) {
        let ctx = self.ctx.clone();
        let mut out = ctx.run_ui(egui::RawInput::default(), |_| {});
        out.textures_delta.clear();
        crate::tasks::poll(&mut self.app, &ctx);
        crate::pdi::logic(&mut self.app, &ctx);
        crate::pdi_pair::logic(&mut self.app, &ctx);
        std::thread::sleep(pause);
    }
}
