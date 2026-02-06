mod api;
mod app;
mod auth;
mod config;
mod models;
mod manifest;
#[cfg(test)]
mod tests;

use app::LegendaryApp;
use eframe::egui;

fn main() -> Result<(), eframe::Error> {
    env_logger::init();

    let options = eframe::NativeOptions {
        initial_window_size: Some(egui::vec2(800.0, 600.0)),
        ..Default::default()
    };

    eframe::run_native(
        "Legendary Rust",
        options,
        Box::new(|cc| Box::new(LegendaryApp::new(cc))),
    )
}
