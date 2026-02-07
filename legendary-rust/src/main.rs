mod api;
mod app;
mod utils;
mod auth;
mod config;
mod models;
mod manifest;
mod download;
#[cfg(test)]
mod tests;

use app::LegendaryApp;
use eframe::egui;

fn main() -> Result<(), eframe::Error> {
    env_logger::init();

    // Clear image cache if version is old
    if let Some(mut p) = crate::auth::get_config_dir() {
        p.push("cache");
        let v_path = p.join("version");
        let mut clear = true;
        if let Ok(v) = std::fs::read_to_string(&v_path) {
            if v == "2" {
                clear = false;
            }
        }
        if clear {
            log::info!("Clearing image cache for recreation...");
            let _ = std::fs::remove_dir_all(&p);
            let _ = std::fs::create_dir_all(&p);
            let _ = std::fs::write(v_path, "2");
        }
    }

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
