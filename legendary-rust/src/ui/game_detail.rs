use crate::app::LegendaryApp;
use crate::models::{SaveSyncStatus, View};
use crate::worker::WorkerMsg;
use crate::config::{CompatibilityTool};
use eframe::egui;

pub fn show_game_detail_view(app: &mut LegendaryApp, ui: &mut egui::Ui) {
    let selected_game = app.selected_game.clone();
    let selected_app_name = app.selected_app_name.clone();

    if let Some(game) = selected_game {
        let app_name = selected_app_name.unwrap_or_else(|| game.id.clone());
        let local_meta = crate::auth::load_local_metadata(&app_name);

        if ui.button("Back").clicked() {
            app.current_view = View::Library;
        }

        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.horizontal(|ui| {
                // Second album art (DieselGameBoxTall or index 1)
                if let Some(meta) = &local_meta {
                    let img_info = meta.metadata.key_images.iter().find(|i| i.image_type == "DieselGameBoxTall")
                        .or_else(|| meta.metadata.key_images.get(1));

                    if let Some(info) = img_info {
                        if let Some(img) = app.images.get(&(app_name.clone(), info.image_type.clone())) {
                            let size = img.size_vec2();
                            let ratio = size.x / size.y;
                            img.show_max_size(ui, egui::vec2(200.0 * ratio, 200.0));
                        } else {
                            if !app.fetching_images.contains(&(app_name.clone(), info.image_type.clone())) {
                                app.fetching_images.insert((app_name.clone(), info.image_type.clone()));
                                let _ = app.tx.send(WorkerMsg::FetchImage {
                                    app_name: app_name.clone(),
                                    url: info.url.clone(),
                                    is_installed: true,
                                    image_type: info.image_type.clone(),
                                });
                            }
                            ui.allocate_space(egui::vec2(150.0, 200.0));
                        }
                    }
                }

                ui.vertical(|ui| {
                    ui.heading(&game.title);
                    if let Some(meta) = &local_meta {
                        if let Some(dev) = &meta.metadata.developer {
                            ui.label(egui::RichText::new(format!("Developer: {}", dev)).italics());
                        }
                    }
                    ui.horizontal(|ui| {
                        ui.label(format!("Application Name: {}", app_name));
                        if let Some(installed) = app.installed_games.iter().find(|g| g.app_name == app_name) {
                            ui.label(format!("(v{})", installed.version));
                        }
                    });

                    if let Some(asset) = app.assets.iter().find(|a| a.catalog_item_id == game.id) {
                        ui.label(format!("Available Version: {}", asset.build_version));
                    }

                    let installed_entry = app.installed_games.iter().find(|g| g.app_name == app_name);
                    if let Some(installed) = installed_entry {
                        let size_gb = installed.install_size as f32 / (1024.0 * 1024.0 * 1024.0);
                        ui.label(format!("Game Size: {:.2} GB", size_gb));
                    } else if let Some(meta) = &local_meta {
                        if let Some(attrs) = &meta.metadata.custom_attributes {
                            if let Some(size) = attrs.get("MaxSizeMB") {
                                ui.label(format!("Game Size: {} MB (estimated)", size.value));
                            }
                        }
                        if let Some(release_info) = &meta.metadata.release_info {
                            let platforms: Vec<_> = release_info.iter().flat_map(|r| &r.platform).collect();
                            ui.label(format!("Platform: {:?}", platforms));
                        }
                    }

                    // Ubisoft Support
                    if let Some(partner) = &game.partner_link_type {
                        if partner.to_lowercase() == "ubisoft" {
                            ui.group(|ui| {
                                ui.colored_label(egui::Color32::LIGHT_BLUE, "ℹ Ubisoft title detected");
                                ui.label("This game requires activation on Ubisoft Connect.");
                                if ui.button("Open Ubisoft Activation Guide").clicked() {
                                    let _ = open::that("https://github.com/derrod/legendary/wiki/Ubisoft-Activation");
                                }
                            });
                        }
                    }

                    // EA/Origin Support
                    let is_ea = local_meta.as_ref().and_then(|m| m.metadata.custom_attributes.as_ref())
                        .and_then(|attrs| attrs.get("ThirdPartyManagedApp"))
                        .map(|a| a.value.to_lowercase().contains("origin") || a.value.to_lowercase().contains("ea app"))
                        .unwrap_or(false);

                    if is_ea {
                        ui.group(|ui| {
                            ui.colored_label(egui::Color32::LIGHT_BLUE, "ℹ EA/Origin title detected");
                            if ui.button(egui::RichText::new("🚀 Launch via Origin/EA App").strong()).clicked() {
                                let _ = app.tx.send(WorkerMsg::LaunchOrigin(app_name.clone()));
                            }
                        });
                    }

                    if let Some(installed) = app.installed_games.iter().find(|g| g.app_name == app_name) {
                        ui.label(format!("Installed at: {}", installed.install_path));
                        if ui.button("☁ Compare local and cloud save files").clicked() {
                            if let Some(item) = app.library.iter().find(|i| i.app_name == app_name) {
                                let save_path = app.advanced_info.as_ref().and_then(|i| i.save_path.clone())
                                    .or_else(|| app.config.games.get(&app_name).and_then(|s| s.save_path.clone()));
                                let backup_path = app.advanced_info.as_ref().and_then(|i| i.backup_path.clone());

                                app.save_sync_status = Some(SaveSyncStatus {
                                    app_name: app_name.clone(),
                                    files: Vec::new(),
                                    local_time: None,
                                    remote_time: None,
                                    backup_time: None,
                                    loading: true,
                                    error: None,
                                });
                                app.current_view = View::SaveSync;

                                let _ = app.tx.send(WorkerMsg::SyncCloudSaves {
                                    app_name: app_name.clone(),
                                    namespace: item.namespace.clone(),
                                    save_path,
                                    backup_path,
                                });
                            }
                        }
                    }

                    let settings = app.config.games.get(&app_name);

                    if let Some(s) = settings {
                        let hours = s.play_time_seconds / 3600;
                        let mins = (s.play_time_seconds % 3600) / 60;
                        ui.label(format!("Time in game: {}h {}m", hours, mins));
                    }

                    if let Some(info) = &app.advanced_info {
                        if let Some(p) = &info.save_path {
                            if ui.button("📁 Open Save Folder").clicked() {
                                let _ = open::that(p);
                            }
                        }
                    }
                });
            });

            if std::env::consts::OS == "linux" {
                ui.add_space(10.0);
                ui.group(|ui| {
                    ui.label("Compatibility Tool:");
                    let mut changed = false;
                    let game_settings = app.config.games.entry(app_name.clone()).or_default();

                    ui.horizontal(|ui| {
                        if ui.checkbox(&mut game_settings.use_custom_pfx, "Custom PFX").changed() {
                            changed = true;
                        }
                        if game_settings.use_custom_pfx {
                            let mut path_str = game_settings.custom_pfx_path.as_ref().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
                            if ui.text_edit_singleline(&mut path_str).changed() {
                                game_settings.custom_pfx_path = if path_str.is_empty() { None } else { Some(std::path::PathBuf::from(path_str)) };
                                changed = true;
                            }
                            if ui.button("Browse...").clicked() {
                                if let Some(path) = rfd::FileDialog::new().pick_folder() {
                                    game_settings.custom_pfx_path = Some(path);
                                    changed = true;
                                }
                            }
                        }
                    });

                    ui.separator();

                    ui.horizontal(|ui| {
                        if ui.radio_value(&mut game_settings.compatibility_tool, Some(CompatibilityTool::SteamProton), "Steam Proton").changed() { changed = true; }
                        if ui.radio_value(&mut game_settings.compatibility_tool, Some(CompatibilityTool::CustomProtonWine), "Custom Proton/Wine").changed() { changed = true; }
                        if ui.radio_value(&mut game_settings.compatibility_tool, Some(CompatibilityTool::SystemWine), "System Wine").changed() { changed = true; }
                        if ui.radio_value(&mut game_settings.compatibility_tool, Some(CompatibilityTool::UmuLauncher), "UMU Launcher").changed() { changed = true; }
                    });

                    match game_settings.compatibility_tool {
                        Some(CompatibilityTool::SteamProton) => {
                            let protons = crate::config::find_steam_protons();
                            egui::ComboBox::from_label("Proton Version")
                                .selected_text(game_settings.custom_compatibility_path.as_ref().map(|p| p.file_name().unwrap_or_default().to_string_lossy()).unwrap_or_else(|| "Select Proton".into()))
                                .show_ui(ui, |ui| {
                                    for p in protons {
                                        let name = p.file_name().unwrap_or_default().to_string_lossy().to_string();
                                        if ui.selectable_value(&mut game_settings.custom_compatibility_path, Some(p), name).changed() { changed = true; }
                                    }
                                });
                        }
                        Some(CompatibilityTool::CustomProtonWine) => {
                            let wines = crate::config::find_custom_wines();
                            egui::ComboBox::from_label("Wine/Proton Version")
                                .selected_text(game_settings.custom_compatibility_path.as_ref().map(|p| p.file_name().unwrap_or_default().to_string_lossy()).unwrap_or_else(|| "Select Tool".into()))
                                .show_ui(ui, |ui| {
                                    for w in wines {
                                        let name = w.file_name().unwrap_or_default().to_string_lossy().to_string();
                                        if ui.selectable_value(&mut game_settings.custom_compatibility_path, Some(w), name).changed() { changed = true; }
                                    }
                                    if ui.button("Custom Path...").clicked() {
                                        if let Some(path) = rfd::FileDialog::new().pick_folder() {
                                            game_settings.custom_compatibility_path = Some(path);
                                            changed = true;
                                        }
                                    }
                                });
                        }
                        _ => {}
                    }
                    if changed {
                        let _ = app.config.save();
                        if let Some(installed) = app.installed_games.iter().find(|g| g.app_name == app_name) {
                            let _ = app.tx.send(WorkerMsg::FetchAdvancedInfo {
                                app_name: app_name.clone(),
                                install_path: installed.install_path.clone(),
                            });
                        }
                    }
                });
            }

            if let Some(meta) = &local_meta {
                if let Some(dlcs) = &meta.metadata.dlc_item_list {
                    if !dlcs.is_empty() {
                        ui.add_space(10.0);
                        ui.collapsing("DLCs", |ui| {
                            for dlc in dlcs {
                                ui.horizontal(|ui| {
                                    ui.label(&dlc.title);
                                    let dlc_installed = app.installed_games.iter().any(|g| g.app_name == dlc.id);
                                    if dlc_installed {
                                        ui.label("✅ Installed");
                                        if ui.button("Uninstall").clicked() {
                                            let _ = app.tx.send(WorkerMsg::UninstallGame(dlc.id.clone()));
                                        }
                                    } else {
                                        if ui.button("Install").clicked() {
                                            // We need to fetch install info for DLC first
                                            let _ = app.tx.send(WorkerMsg::FetchInstallInfo {
                                                app_name: dlc.id.clone(),
                                                title: dlc.title.clone(),
                                            });
                                        }
                                    }
                                });
                            }
                        });
                    }
                }
            }

            ui.separator();
            if !app.unaccepted_eulas.is_empty() {
                ui.add_space(10.0);
                ui.group(|ui| {
                    ui.colored_label(egui::Color32::YELLOW, egui::RichText::new("⚠ Unaccepted EULAs").strong());
                    for eula in app.unaccepted_eulas.clone() {
                        ui.horizontal(|ui| {
                            ui.label(eula["title"].as_str().unwrap_or("Unknown EULA"));
                            if ui.button("View").clicked() {
                                if let Some(url) = eula["url"].as_str() {
                                    let _ = open::that(url);
                                }
                            }
                            if ui.button("Accept").clicked() {
                                let id = eula["key"].as_str().unwrap_or_default().to_string();
                                let version = eula["version"].as_i64().unwrap_or(1) as i32;
                                let _ = app.tx.send(WorkerMsg::AcceptEula { eula_id: id, version });
                                // Remove from list
                                app.unaccepted_eulas.retain(|e| e["key"] != eula["key"]);
                            }
                        });
                    }
                });
            }

            ui.separator();
            if let Some(desc) = &game.description {
                ui.label(desc);
            }

            ui.add_space(10.0);
            ui.collapsing("Advanced Options", |ui| {
                let mut changed = false;
                let game_settings = app.config.games.entry(app_name.clone()).or_default();

                if let Some(info) = &app.advanced_info {
                    if let Some(p) = &info.prefix_path {
                        ui.horizontal(|ui| {
                            ui.label(format!("Current prefix in use: {}", p.to_string_lossy()));
                            if ui.button("Open Folder").clicked() {
                                let _ = open::that(p);
                            }
                        });
                    }
                    if let Some(p) = &info.save_path {
                        ui.horizontal(|ui| {
                            ui.label("📂");
                            ui.label(egui::RichText::new(format!("Local Save Path: {}", p.to_string_lossy())).strong());
                            if ui.button("Open Folder").clicked() {
                                let _ = open::that(p);
                            }
                        });
                    } else {
                        ui.horizontal(|ui| {
                            ui.label("📂");
                            ui.label(egui::RichText::new("Local Save Path: Not discovered").italics());
                        });
                    }
                    if let Some(p) = &info.dlss_path {
                        ui.horizontal(|ui| {
                            ui.label(format!("Discovered DLSS file path: {}", p.to_string_lossy()));
                            if ui.button("Open Folder").clicked() {
                                if let Some(parent) = p.parent() {
                                    let _ = open::that(parent);
                                }
                            }
                        });
                    }
                    if let Some(p) = &info.dlssd_path {
                        ui.horizontal(|ui| {
                            ui.label(format!("Discovered DLSSD file path: {}", p.to_string_lossy()));
                            if ui.button("Open Folder").clicked() {
                                if let Some(parent) = p.parent() {
                                    let _ = open::that(parent);
                                }
                            }
                        });
                    }
                    ui.separator();
                }

                if ui.checkbox(&mut game_settings.play_offline, "Play Offline").changed() {
                    changed = true;
                }

                ui.add_space(5.0);
                ui.group(|ui| {
                    ui.label("Advanced path to save file:");
                    ui.horizontal(|ui| {
                        let mut save_str = game_settings.save_path.as_ref().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
                        if ui.add(egui::TextEdit::singleline(&mut save_str).hint_text("Advanced path to save file (optional)")).changed() {
                            game_settings.save_path = if save_str.is_empty() { None } else { Some(std::path::PathBuf::from(save_str)) };
                            changed = true;
                        }
                        if ui.button("Browse...").clicked() {
                            if let Some(path) = rfd::FileDialog::new().pick_folder() {
                                game_settings.save_path = Some(path);
                                changed = true;
                            }
                        }
                    });
                    ui.separator();
                    ui.label("Local Backup Settings:");
                    if ui.checkbox(&mut game_settings.local_backup_enabled, "Enable Local Backup").changed() {
                        changed = true;
                    }
                    if let Some(info) = &app.advanced_info {
                        if let Some(p) = &info.backup_path {
                            ui.horizontal(|ui| {
                                ui.label(format!("Path: {}", p.to_string_lossy()));
                                if ui.button("Open").clicked() {
                                    let _ = open::that(p);
                                }
                            });
                        }
                    }
                    ui.horizontal(|ui| {
                        let mut backup_str = game_settings.local_backup_path.as_ref().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
                        if ui.add(egui::TextEdit::singleline(&mut backup_str).hint_text("Custom Backup Path (optional)")).changed() {
                            game_settings.local_backup_path = if backup_str.is_empty() { None } else { Some(std::path::PathBuf::from(backup_str)) };
                            changed = true;
                        }
                        if ui.button("Browse...").clicked() {
                            if let Some(path) = rfd::FileDialog::new().pick_folder() {
                                game_settings.local_backup_path = Some(path);
                                changed = true;
                            }
                        }
                    });
                    if let Some(info) = &app.advanced_info {
                        if let (Some(src), Some(dst)) = (&info.save_path, &info.backup_path) {
                            if ui.button("Backup Now (Local -> Backup)").clicked() {
                                let _ = app.tx.send(WorkerMsg::CreateLocalBackup {
                                    app_name: app_name.clone(),
                                    source: src.clone(),
                                    destination: dst.clone(),
                                });
                            }
                        }
                    }
                });

                ui.add_space(5.0);
                ui.label("Custom Executable Path (optional):");
                ui.horizontal(|ui| {
                    let mut exe_str = game_settings.custom_exe_path.as_ref().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
                    if ui.text_edit_singleline(&mut exe_str).changed() {
                        game_settings.custom_exe_path = if exe_str.is_empty() { None } else { Some(std::path::PathBuf::from(exe_str)) };
                        changed = true;
                    }
                    if ui.button("Browse...").clicked() {
                        if let Some(path) = rfd::FileDialog::new().pick_file() {
                            game_settings.custom_exe_path = Some(path);
                            changed = true;
                        }
                    }
                });

                ui.add_space(5.0);
                if ui.checkbox(&mut game_settings.eos_overlay_enabled, "Enable EOS Overlay").changed() {
                    changed = true;
                }

                if ui.checkbox(&mut game_settings.proton_prefer_sdl, "Proton Prefer SDL (PROTON_PREFER_SDL=1)").changed() {
                    changed = true;
                }

                ui.add_space(10.0);
                ui.collapsing("Environment Variables", |ui| {
                    ui.separator();
                    ui.label("Steam Compatibility Overrides:");
                    ui.horizontal(|ui| {
                        ui.label("STEAM_COMPAT_INSTALL_PATH:");
                        let mut path_str = game_settings.steam_compat_install_path.as_ref().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
                        if ui.text_edit_singleline(&mut path_str).changed() {
                            game_settings.steam_compat_install_path = if path_str.is_empty() { None } else { Some(std::path::PathBuf::from(path_str)) };
                            changed = true;
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label("STEAM_COMPAT_CLIENT_INSTALL_PATH:");
                        let mut path_str = game_settings.steam_compat_client_install_path.as_ref().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
                        if ui.text_edit_singleline(&mut path_str).changed() {
                            game_settings.steam_compat_client_install_path = if path_str.is_empty() { None } else { Some(std::path::PathBuf::from(path_str)) };
                            changed = true;
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label("STEAM_COMPAT_DATA_PATH:");
                        let mut path_str = game_settings.steam_compat_data_path.as_ref().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
                        if ui.text_edit_singleline(&mut path_str).changed() {
                            game_settings.steam_compat_data_path = if path_str.is_empty() { None } else { Some(std::path::PathBuf::from(path_str)) };
                            changed = true;
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label("STEAM_COMPAT_APP_ID:");
                        let mut id_str = game_settings.steam_compat_app_id.clone().unwrap_or_default();
                        if ui.text_edit_singleline(&mut id_str).changed() {
                            game_settings.steam_compat_app_id = if id_str.is_empty() { None } else { Some(id_str) };
                            changed = true;
                        }
                    });

                    ui.separator();
                    ui.label("Custom Environment Variables:");
                    let mut to_remove = None;
                    for (k, v) in &mut game_settings.env_vars {
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
                        game_settings.env_vars.remove(&k);
                        changed = true;
                    }

                    let new_key = &mut app.new_env_key;
                    let new_val = &mut app.new_env_val;
                    let game_env_vars = &mut game_settings.env_vars;
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(new_key).hint_text("Key"));
                        ui.add(egui::TextEdit::singleline(new_val).hint_text("Value"));
                        if ui.button("Add").clicked() {
                            if !new_key.is_empty() {
                                game_env_vars.insert(new_key.clone(), new_val.clone());
                                new_key.clear();
                                new_val.clear();
                                changed = true;
                            }
                        }
                    });
                });

                ui.add_space(10.0);
                ui.collapsing("Manifest Files", |ui| {
                    ui.horizontal(|ui| {
                        if ui.button("Fetch/Refresh").clicked() {
                            let catalog_item_id = app.library.iter().find(|i| i.app_name == app_name)
                                .map(|i| i.catalog_item_id.clone())
                                .unwrap_or_default();
                            let _ = app.tx.send(WorkerMsg::ListFiles {
                                app_name: app_name.clone(),
                                catalog_item_id,
                            });
                        }
                        ui.label("Search:");
                        ui.text_edit_singleline(&mut app.manifest_search_query);
                        if ui.button("×").clicked() {
                            app.manifest_search_query.clear();
                        }
                    });

                    if !app.manifest_files.is_empty() {
                        let query = app.manifest_search_query.to_lowercase();
                        egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                            for file in &app.manifest_files {
                                if query.is_empty() || file.to_lowercase().contains(&query) {
                                    ui.label(file);
                                }
                            }
                        });
                    }
                });

                ui.add_space(10.0);
                ui.collapsing("Detailed Info", |ui| {
                    if let Some(meta) = &local_meta {
                        ui.label(format!("ID: {}", meta.metadata.id));
                        ui.label(format!("Namespace: {}", meta.metadata.namespace));
                        if let Some(did) = &meta.metadata.deployment_id {
                            ui.label(format!("Deployment ID: {}", did));
                        }
                        if let Some(attrs) = &meta.metadata.custom_attributes {
                            for (k, v) in attrs {
                                ui.label(format!("{}: {}", k, v.value));
                            }
                        }
                    }
                });

                if changed {
                    let _ = app.config.save();
                }
            });

            ui.add_space(20.0);

            if let Some(task) = app.current_task.clone() {
                ui.group(|ui| {
                    ui.label(format!("Task: {}", task.name));
                    ui.add(egui::ProgressBar::new(task.progress).show_percentage());
                    ui.horizontal(|ui| {
                        if task.is_paused {
                            if ui.button("Resume").clicked() {
                                if let Some(t) = &mut app.current_task { t.is_paused = false; }
                                app.worker_pause.store(false, std::sync::atomic::Ordering::SeqCst);
                                let _ = app.tx.send(WorkerMsg::ResumeTask);
                            }
                        } else {
                            if ui.button("Pause").clicked() {
                                if let Some(t) = &mut app.current_task { t.is_paused = true; }
                                app.worker_pause.store(true, std::sync::atomic::Ordering::SeqCst);
                                let _ = app.tx.send(WorkerMsg::PauseTask);
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            app.worker_cancel.store(true, std::sync::atomic::Ordering::SeqCst);
                            let _ = app.tx.send(WorkerMsg::CancelTask);
                        }
                    });
                });
                ui.add_space(10.0);
            }

            ui.horizontal(|ui| {
                let is_running = app.running_apps.contains(&app_name);
                let button_text = if is_running { "Stop Game" } else { "Start Game" };

                ui.vertical(|ui| {
                    if ui.button(egui::RichText::new(button_text).size(24.0).strong()).clicked() {
                        if is_running {
                            let _ = app.tx.send(WorkerMsg::StopGame(app_name.clone()));
                            app.status_message = format!("Stopping game: {}...", app_name);
                        } else {
                            let offline = app.config.games.get(&app_name).map(|s| s.play_offline).unwrap_or(false);
                            if offline {
                                let can_run_offline = local_meta.as_ref().map(|m| {
                                    m.metadata.custom_attributes.as_ref().and_then(|attrs| {
                                        attrs.get("CanRunOffline").map(|a| a.value == "true")
                                    }).unwrap_or(true)
                                }).unwrap_or(true);

                                if !can_run_offline {
                                    app.status_message = "Warning: Game may not support offline mode.".to_string();
                                }
                            }
                            let _ = app.tx.send(WorkerMsg::LaunchGame { app_name: app_name.clone(), offline });
                        }
                    }

                    let is_installed = app.installed_games.iter().any(|g| g.app_name == app_name);
                    if !is_installed {
                        ui.horizontal(|ui| {
                            if ui.button(egui::RichText::new("Install").size(24.0).strong()).clicked() {
                                let _ = app.tx.send(WorkerMsg::FetchInstallInfo {
                                    app_name: app_name.clone(),
                                    title: game.title.clone(),
                                });
                            }
                            if ui.button(egui::RichText::new("Verify").size(24.0).strong()).clicked() {
                                let catalog_item_id = app.library.iter().find(|i| i.app_name == app_name)
                                    .map(|i| i.catalog_item_id.clone())
                                    .unwrap_or_default();
                                let _ = app.tx.send(WorkerMsg::VerifyGame {
                                    app_name: app_name.clone(),
                                    catalog_item_id
                                });
                                app.current_view = View::Tasks;
                            }
                        });
                    } else {
                        if ui.button("Verify").clicked() {
                            let catalog_item_id = app.library.iter().find(|i| i.app_name == app_name)
                                .map(|i| i.catalog_item_id.clone())
                                .unwrap_or_default();
                            let _ = app.tx.send(WorkerMsg::VerifyGame {
                                app_name: app_name.clone(),
                                catalog_item_id
                            });
                            app.current_view = View::Tasks;
                        }
                        if ui.button("Repair").clicked() {
                            let _ = app.tx.send(WorkerMsg::RepairGame(app_name.clone(), false));
                        }
                        if ui.button("Repair and Update").clicked() {
                            let _ = app.tx.send(WorkerMsg::RepairGame(app_name.clone(), true));
                        }
                        if ui.button("Uninstall").clicked() {
                            let _ = app.tx.send(WorkerMsg::UninstallGame(app_name.clone()));
                        }

                        if let Some(installed) = app.installed_games.iter().find(|g| g.app_name == app_name) {
                            if ui.button("Open Folder").clicked() {
                                let _ = open::that(&installed.install_path);
                            }
                        }
                    }
                });
            });
        });
    }
}
