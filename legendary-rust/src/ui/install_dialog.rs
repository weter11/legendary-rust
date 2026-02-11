use crate::app::LegendaryApp;
use crate::models::View;
use crate::worker::WorkerMsg;
use eframe::egui;

pub fn show_install_dialog_view(app: &mut LegendaryApp, ui: &mut egui::Ui) {
    if let Some(info) = &app.install_info {
        ui.heading(format!("Install {}", info.title));
        ui.add_space(10.0);

        egui::Grid::new("install_grid")
            .spacing(egui::vec2(20.0, 10.0))
            .show(ui, |ui| {
                ui.label("Install folder:");
                ui.label(info.install_path.to_string_lossy());
                ui.end_row();

                ui.label("Download size:");
                ui.label(format!(
                    "{:.2} GB",
                    info.download_size as f32 / (1024.0 * 1024.0 * 1024.0)
                ));
                ui.end_row();

                ui.label("Size after install:");
                ui.label(format!(
                    "{:.2} GB",
                    info.install_size as f32 / (1024.0 * 1024.0 * 1024.0)
                ));
                ui.end_row();

                ui.label("Available space:");
                ui.label(format!(
                    "{:.2} GB",
                    info.free_space as f32 / (1024.0 * 1024.0 * 1024.0)
                ));
                ui.end_row();
            });

        if !info.available_tags.is_empty() {
            ui.add_space(20.0);
            ui.heading("Selective Download:");
            egui::ScrollArea::vertical()
                .max_height(200.0)
                .show(ui, |ui| {
                    for tag in &info.available_tags {
                        let mut selected = app.selected_tags.contains(tag);
                        if ui.checkbox(&mut selected, tag).changed() {
                            if selected {
                                app.selected_tags.insert(tag.clone());
                            } else {
                                app.selected_tags.remove(tag);
                            }
                        }
                    }
                });
        }

        ui.add_space(20.0);
        if info.free_space < info.install_size {
            ui.colored_label(egui::Color32::RED, "⚠ Not enough disk space!");
        }

        ui.horizontal(|ui| {
            if ui.button("Install").clicked() {
                let _ = app.tx.send(WorkerMsg::InstallGame {
                    app_name: info.app_name.clone(),
                    install_path: info.install_path.clone(),
                    selected_tags: Some(app.selected_tags.clone()),
                    platform: "Windows".to_string(), // Default to Windows for now
                });
                app.current_view = View::Tasks;
            }

            if ui
                .button("Import existing files (from configured paths)")
                .clicked()
            {
                let catalog_item_id = app
                    .library
                    .iter()
                    .find(|i| i.app_name == info.app_name)
                    .map(|i| i.catalog_item_id.clone())
                    .unwrap_or_default();
                let search_paths = app.config.global.game_paths.clone();
                let _ = app.tx.send(WorkerMsg::ImportGameFromPaths {
                    app_name: info.app_name.clone(),
                    title: info.title.clone(),
                    catalog_item_id,
                    search_paths,
                });
                app.current_view = View::Tasks;
            }
            if ui.button("Cancel").clicked() {
                app.current_view = View::GameDetail;
            }
        });
    }
}
