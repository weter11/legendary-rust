use crate::app::LegendaryApp;
use crate::config::CompatibilityTool;
use crate::worker::WorkerMsg;
use eframe::egui;

pub fn show_settings_view(app: &mut LegendaryApp, ui: &mut egui::Ui) {
    ui.heading("Global Settings");
    ui.separator();

    if ui.button("Sync with Epic Games Launcher").clicked() {
        let _ = app.tx.send(WorkerMsg::EglSync);
    }
    if ui.button("Check for updates").clicked() {
        let _ = app.tx.send(WorkerMsg::CheckForUpdates);
    }
    ui.add_space(10.0);

    ui.group(|ui| {
        ui.label("Default Compatibility Settings:");
        let mut changed = false;
        if ui
            .checkbox(
                &mut app.config.global.use_custom_pfx,
                "Use Custom WINE/Proton Prefix (PFX)",
            )
            .changed()
        {
            changed = true;
        }
        if app.config.global.use_custom_pfx {
            ui.horizontal(|ui| {
                let mut path_str = app
                    .config
                    .global
                    .custom_pfx_path
                    .as_ref()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default();
                if ui.text_edit_singleline(&mut path_str).changed() {
                    app.config.global.custom_pfx_path = if path_str.is_empty() {
                        None
                    } else {
                        Some(std::path::PathBuf::from(path_str))
                    };
                    changed = true;
                }
                if ui.button("Browse...").clicked() {
                    if let Some(path) = rfd::FileDialog::new().pick_folder() {
                        app.config.global.custom_pfx_path = Some(path);
                        changed = true;
                    }
                }
            });
        }

        ui.separator();
        ui.horizontal(|ui| {
            if ui
                .radio_value(
                    &mut app.config.global.compatibility_tool,
                    Some(CompatibilityTool::SteamProton),
                    "Steam Proton",
                )
                .changed()
            {
                changed = true;
            }
            if ui
                .radio_value(
                    &mut app.config.global.compatibility_tool,
                    Some(CompatibilityTool::CustomProtonWine),
                    "Custom Proton/Wine",
                )
                .changed()
            {
                changed = true;
            }
            if ui
                .radio_value(
                    &mut app.config.global.compatibility_tool,
                    Some(CompatibilityTool::SystemWine),
                    "System Wine",
                )
                .changed()
            {
                changed = true;
            }
            if ui
                .radio_value(
                    &mut app.config.global.compatibility_tool,
                    Some(CompatibilityTool::UmuLauncher),
                    "UMU Launcher",
                )
                .changed()
            {
                changed = true;
            }
        });

        match app.config.global.compatibility_tool {
            Some(CompatibilityTool::SteamProton) => {
                let protons = crate::config::find_steam_protons();
                egui::ComboBox::from_label("Default Proton Version")
                    .selected_text(
                        app.config
                            .global
                            .custom_compatibility_path
                            .as_ref()
                            .map(|p| p.file_name().unwrap_or_default().to_string_lossy())
                            .unwrap_or_else(|| "Select Proton".into()),
                    )
                    .show_ui(ui, |ui| {
                        for p in protons {
                            let name = p
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_string();
                            if ui
                                .selectable_value(
                                    &mut app.config.global.custom_compatibility_path,
                                    Some(p),
                                    name,
                                )
                                .changed()
                            {
                                changed = true;
                            }
                        }
                    });
            }
            Some(CompatibilityTool::CustomProtonWine) => {
                let wines = crate::config::find_custom_wines();
                egui::ComboBox::from_label("Default Wine/Proton Version")
                    .selected_text(
                        app.config
                            .global
                            .custom_compatibility_path
                            .as_ref()
                            .map(|p| p.file_name().unwrap_or_default().to_string_lossy())
                            .unwrap_or_else(|| "Select Tool".into()),
                    )
                    .show_ui(ui, |ui| {
                        for w in wines {
                            let name = w
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_string();
                            if ui
                                .selectable_value(
                                    &mut app.config.global.custom_compatibility_path,
                                    Some(w),
                                    name,
                                )
                                .changed()
                            {
                                changed = true;
                            }
                        }
                        if ui.button("Custom Path...").clicked() {
                            if let Some(path) = rfd::FileDialog::new().pick_folder() {
                                app.config.global.custom_compatibility_path = Some(path);
                                changed = true;
                            }
                        }
                    });
            }
            _ => {}
        }

        ui.add_space(10.0);
        if ui
            .checkbox(
                &mut app.config.global.eos_overlay_enabled,
                "Enable EOS Overlay by default",
            )
            .changed()
        {
            changed = true;
        }

        ui.add_space(10.0);
        ui.collapsing("Advanced Options", |ui| {
            if ui
                .checkbox(
                    &mut app.config.global.proton_prefer_sdl,
                    "Proton Prefer SDL",
                )
                .changed()
            {
                changed = true;
            }

            ui.separator();
            ui.label("Steam Compatibility Overrides:");
            ui.horizontal(|ui| {
                ui.label("STEAM_COMPAT_INSTALL_PATH:");
                let mut path_str = app
                    .config
                    .global
                    .steam_compat_install_path
                    .as_ref()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default();
                if ui.text_edit_singleline(&mut path_str).changed() {
                    app.config.global.steam_compat_install_path = if path_str.is_empty() {
                        None
                    } else {
                        Some(std::path::PathBuf::from(path_str))
                    };
                    changed = true;
                }
            });
            ui.horizontal(|ui| {
                ui.label("STEAM_COMPAT_CLIENT_INSTALL_PATH:");
                let mut path_str = app
                    .config
                    .global
                    .steam_compat_client_install_path
                    .as_ref()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default();
                if ui.text_edit_singleline(&mut path_str).changed() {
                    app.config.global.steam_compat_client_install_path = if path_str.is_empty() {
                        None
                    } else {
                        Some(std::path::PathBuf::from(path_str))
                    };
                    changed = true;
                }
            });
            ui.horizontal(|ui| {
                ui.label("STEAM_COMPAT_DATA_PATH:");
                let mut path_str = app
                    .config
                    .global
                    .steam_compat_data_path
                    .as_ref()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default();
                if ui.text_edit_singleline(&mut path_str).changed() {
                    app.config.global.steam_compat_data_path = if path_str.is_empty() {
                        None
                    } else {
                        Some(std::path::PathBuf::from(path_str))
                    };
                    changed = true;
                }
            });
            ui.horizontal(|ui| {
                ui.label("STEAM_COMPAT_APP_ID:");
                let mut id_str = app
                    .config
                    .global
                    .steam_compat_app_id
                    .clone()
                    .unwrap_or_default();
                if ui.text_edit_singleline(&mut id_str).changed() {
                    app.config.global.steam_compat_app_id = if id_str.is_empty() {
                        None
                    } else {
                        Some(id_str)
                    };
                    changed = true;
                }
            });

            ui.separator();
            ui.label("Other Custom Environment Variables:");
            let mut to_remove = None;
            for (k, v) in &mut app.config.global.env_vars {
                ui.horizontal(|ui| {
                    ui.label(format!("{}: ", k));
                    if ui.text_edit_singleline(v).changed() {
                        changed = true;
                    }
                    if ui.button("🗑").on_hover_text("Remove").clicked() {
                        to_remove = Some(k.clone());
                    }
                });
            }
            if let Some(k) = to_remove {
                app.config.global.env_vars.remove(&k);
                changed = true;
            }

            let new_key = &mut app.new_env_key;
            let new_val = &mut app.new_env_val;
            let global_env_vars = &mut app.config.global.env_vars;
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(new_key).hint_text("Key"));
                ui.add(egui::TextEdit::singleline(new_val).hint_text("Value"));
                if ui.button("Add").clicked() {
                    if !new_key.is_empty() {
                        global_env_vars.insert(new_key.clone(), new_val.clone());
                        new_key.clear();
                        new_val.clear();
                        changed = true;
                    }
                }
            });
        });

        if changed {
            let _ = app.config.save();
        }
    });

    ui.add_space(10.0);

    ui.label("Game Library Paths:");
    let mut to_remove = None;
    for (i, path) in app.config.global.game_paths.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.label(path.to_string_lossy());
            if ui.button("Remove").clicked() {
                to_remove = Some(i);
            }
        });
    }
    if let Some(i) = to_remove {
        app.config.global.game_paths.remove(i);
        let _ = app.config.save();
    }

    if ui.button("Add Path").clicked() {
        if let Some(path) = rfd::FileDialog::new().pick_folder() {
            app.config.global.game_paths.push(path);
            let _ = app.config.save();
        }
    }
}
