//! Light Studio: opens its window on the planner.

// on Windows: a window app, without a console behind it
#![cfg_attr(windows, windows_subsystem = "windows")]

use light_studio::{App, state::UiState, theme};

fn main() -> eframe::Result {
    let _ = env_logger_init();
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../../../assets/icon-512.png")).ok();
    let mut viewport =
        egui::ViewportBuilder::default().with_title("Light Studio").with_inner_size([1360.0, 860.0]).with_min_inner_size([900.0, 600.0]);
    if let Some(icon) = icon {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native(
        "Light Studio",
        options,
        Box::new(|cc| {
            theme::install_fonts(&cc.egui_ctx);
            theme::apply(&cc.egui_ctx);
            Ok(Box::new(App::new(cc.egui_ctx.clone(), UiState::load(), true)))
        }),
    )
}

fn env_logger_init() -> Result<(), ()> {
    Ok(())
}
