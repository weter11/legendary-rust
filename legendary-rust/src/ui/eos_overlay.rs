use crate::app::LegendaryApp;
use crate::worker::WorkerMsg;
use crate::utils::get_default_compat_data_path;
use crate::models::{View};
use eframe::egui;

pub fn show_eos_overlay_view(app: &mut LegendaryApp, ui: &mut egui::Ui) {
    ui.heading("EOS Overlay Manager");
    ui.separator();

    ui.horizontal(|ui| {
        ui.label("Manage Prefix:");
        let _path_str = app
            .eos_prefix_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "Default Prefix".to_string());
        if ui
            .selectable_label(app.eos_prefix_path.is_none(), "Default")
            .clicked()
        {
            app.eos_prefix_path = None;
            let _ = app.tx.send(WorkerMsg::QueryEosStatus { prefix: None });
        }
        if ui.button("Browse...").clicked() {
            if let Some(path) = rfd::FileDialog::new().pick_folder() {
                app.eos_prefix_path = Some(path.clone());
                let _ = app
                    .tx
                    .send(WorkerMsg::QueryEosStatus { prefix: Some(path) });
            }
        }
        if let Some(p) = &app.eos_prefix_path {
            ui.label(p.to_string_lossy());
        }
    });

    ui.add_space(10.0);

    ui.group(|ui| {
        ui.heading("Status");
        ui.horizontal(|ui| {
            ui.label("Installed:");
            if app.eos_status.installed {
                ui.colored_label(egui::Color32::GREEN, "YES");
            } else {
                ui.colored_label(egui::Color32::RED, "NO");
            }
        });

        if let Some(path) = &app.eos_status.install_path {
            ui.label(format!("Install Path: {}", path));
        } else {
            if ui.button("Install EOS Overlay").clicked() {
                if let Some(home) = home::home_dir() {
                    let mut p = home;
                    p.push("Games");
                    p.push("eos-overlay");
                    let _ = app.tx.send(WorkerMsg::InstallGame {
                        app_name: crate::eos::EOS_OVERLAY_APP_ID.to_string(),
                        install_path: p,
                        selected_tags: None,
                        platform: "Windows".to_string(),
                    });
                    app.current_view = View::Tasks;
                }
            }
        }

        ui.add_space(5.0);
        ui.horizontal(|ui| {
            ui.label("Registry Status:");
            if let Some(reg_path) = &app.eos_status.registry_path {
                ui.colored_label(egui::Color32::GREEN, format!("Configured ({})", reg_path));
            } else {
                ui.colored_label(egui::Color32::YELLOW, "Not Configured");
            }
        });

        if !app.eos_status.available_paths.is_empty() {
            ui.add_space(5.0);
            ui.label("Other available EOS installs:");
            for path in &app.eos_status.available_paths {
                ui.horizontal(|ui| {
                    ui.label(path);
                    if ui.button("Use this").clicked() {
                        let prefix = app
                            .eos_prefix_path
                            .clone()
                            .or_else(get_default_compat_data_path);
                        if let Some(p) = prefix {
                            let _ = app.tx.send(WorkerMsg::UpdateEosRegistry {
                                overlay_path: path.clone(),
                                prefix: p,
                                enable: true,
                            });
                        }
                    }
                });
            }
        }

        if app.eos_status.installed {
            ui.horizontal(|ui| {
                if ui.button("Enable in Prefix").clicked() {
                    let prefix = app
                        .eos_prefix_path
                        .clone()
                        .or_else(get_default_compat_data_path);
                    if let (Some(path), Some(p)) = (&app.eos_status.install_path, prefix) {
                        let _ = app.tx.send(WorkerMsg::UpdateEosRegistry {
                            overlay_path: path.clone(),
                            prefix: p,
                            enable: true,
                        });
                    }
                }
                if ui.button("Disable in Prefix").clicked() {
                    let prefix = app
                        .eos_prefix_path
                        .clone()
                        .or_else(get_default_compat_data_path);
                    if let Some(p) = prefix {
                        let _ = app.tx.send(WorkerMsg::UpdateEosRegistry {
                            overlay_path: String::new(),
                            prefix: p,
                            enable: false,
                        });
                    }
                }
            });
        }
    });

    ui.add_space(20.0);

    ui.group(|ui| {
        ui.label("Global EOS Overlay Setting:");
        if ui
            .checkbox(
                &mut app.config.global.eos_overlay_enabled,
                "Enable EOS Overlay by default",
            )
            .changed()
        {
            let _ = app.config.save();
        }
    });

    ui.add_space(10.0);
    ui.heading("Per-game EOS Overlay Settings");
    egui::ScrollArea::vertical().show(ui, |ui| {
        for game in &app.installed_games {
            if game.app_name == crate::eos::EOS_OVERLAY_APP_ID {
                continue;
            }
            ui.horizontal(|ui| {
                ui.label(&game.title);
                let settings = app.config.games.entry(game.app_name.clone()).or_default();
                if ui
                    .checkbox(&mut settings.eos_overlay_enabled, "Enabled")
                    .changed()
                {
                    let _ = app.config.save();
                }
            });
        }
    });
}
