use crate::app::LegendaryApp;
use crate::models::{SaveSyncStatus, View};
use crate::worker::WorkerMsg;
use eframe::egui;

pub fn show_save_sync_view(app: &mut LegendaryApp, ui: &mut egui::Ui) {
    let status_data = app.save_sync_status.clone();
    if let Some(status) = status_data {
        ui.horizontal(|ui| {
            if ui.button("⬅ Back").clicked() {
                app.save_sync_status = None;
                app.current_view = View::GameDetail;
            }
            ui.heading(format!("Cloud Save Sync: {}", status.app_name));
        });
        ui.add_space(10.0);

        if status.loading {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new());
                ui.label("Fetching save metadata...");
            });
            return;
        }

        if let Some(err) = &status.error {
            ui.group(|ui| {
                ui.colored_label(
                    egui::Color32::LIGHT_RED,
                    egui::RichText::new("Failed to fetch cloud save metadata").strong(),
                );
                ui.label(err);
                if ui.button("Retry").clicked() {
                    if let Some(item) =
                        app.library.iter().find(|i| i.app_name == status.app_name)
                    {
                        let save_path = app
                            .config
                            .games
                            .get(&status.app_name)
                            .and_then(|s| s.save_path.clone());
                        let backup_path = app
                            .advanced_info
                            .as_ref()
                            .and_then(|i| i.backup_path.clone());
                        app.save_sync_status = Some(SaveSyncStatus {
                            app_name: status.app_name.clone(),
                            files: Vec::new(),
                            local_time: None,
                            remote_time: None,
                            backup_time: None,
                            loading: true,
                            error: None,
                        });
                        let _ = app.tx.send(WorkerMsg::SyncCloudSaves {
                            app_name: status.app_name.clone(),
                            namespace: item.namespace.clone(),
                            save_path,
                            backup_path,
                        });
                    }
                }
            });
            return;
        }

        let can_upload = app
            .config
            .games
            .get(&status.app_name)
            .and_then(|s| s.save_path.as_ref())
            .is_some()
            || (app
                .advanced_info
                .as_ref()
                .map(|i| i.app_name == status.app_name && i.save_path.is_some())
                .unwrap_or(false));
        let can_download = !status.files.is_empty();

        ui.label(format!(
            "Cloud manifests discovered: {}",
            status.files.len()
        ));
        if !can_download {
            ui.colored_label(egui::Color32::YELLOW, "Download buttons stay disabled until cloud manifests are available. Try Refresh if you expect cloud saves.");
        }

        egui::ScrollArea::horizontal().show(ui, |ui| {
            ui.horizontal_top(|ui| {
                let box_width =
                    (ui.available_width() - ui.spacing().item_spacing.x * 2.0) / 3.0;
                let box_height = 250.0;

                // Local Box
                let local_frame = egui::Frame::group(ui.style())
                    .rounding(5.0)
                    .stroke(egui::Stroke::new(1.0, egui::Color32::from_gray(60)));

                local_frame.show(ui, |ui| {
                    ui.set_min_size(egui::vec2(box_width, box_height));
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.colored_label(
                                egui::Color32::from_rgb(100, 100, 200),
                                egui::RichText::new(" Local ")
                                    .strong()
                                    .background_color(egui::Color32::from_rgb(40, 40, 80)),
                            );
                        });
                        ui.add_space(10.0);
                        ui.vertical_centered(|ui| {
                            if let Some(t) = status.local_time {
                                ui.label(
                                    t.with_timezone(&chrono::Local)
                                        .format("%Y-%m-%d %H:%M:%S")
                                        .to_string(),
                                );
                            } else {
                                ui.label("No local save found");
                            }

                            let current_save_path = app
                                .config
                                .games
                                .get(&status.app_name)
                                .and_then(|s| s.save_path.clone());
                            if let Some(p) = current_save_path {
                                ui.label(
                                    egui::RichText::new(format!(
                                        "Path: {}",
                                        p.to_string_lossy()
                                    ))
                                    .small(),
                                );
                            } else if let Some(info) = &app.advanced_info {
                                if info.app_name == status.app_name {
                                    if let Some(p) = &info.save_path {
                                        ui.label(
                                            egui::RichText::new(format!(
                                                "Discovered: {}",
                                                p.to_string_lossy()
                                            ))
                                            .small()
                                            .italics(),
                                        );
                                    }
                                }
                            }

                            if ui.button("Change Path...").clicked() {
                                if let Some(path) = rfd::FileDialog::new().pick_folder() {
                                    let game_settings = app
                                        .config
                                        .games
                                        .entry(status.app_name.clone())
                                        .or_default();
                                    game_settings.save_path = Some(path.clone());
                                    let _ = app.config.save();

                                    // Refresh status
                                    if let Some(item) = app
                                        .library
                                        .iter()
                                        .find(|i| i.app_name == status.app_name)
                                    {
                                        let backup_path = app
                                            .advanced_info
                                            .as_ref()
                                            .and_then(|i| i.backup_path.clone());
                                        app.save_sync_status = Some(SaveSyncStatus {
                                            app_name: status.app_name.clone(),
                                            files: Vec::new(),
                                            local_time: None,
                                            remote_time: None,
                                            backup_time: None,
                                            loading: true,
                                            error: None,
                                        });
                                        let _ = app.tx.send(WorkerMsg::SyncCloudSaves {
                                            app_name: status.app_name.clone(),
                                            namespace: item.namespace.clone(),
                                            save_path: Some(path),
                                            backup_path,
                                        });
                                    }
                                }
                            }

                            ui.add_space(10.0);
                            ui.label(egui::RichText::new("🖴").size(80.0));
                            ui.add_space(10.0);

                            ui.with_layout(
                                egui::Layout::bottom_up(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .add_enabled(
                                            can_upload,
                                            egui::Button::new(
                                                egui::RichText::new("Upload Local -> Cloud")
                                                    .strong(),
                                            ),
                                        )
                                        .clicked()
                                    {
                                        let save_path = app
                                            .advanced_info
                                            .as_ref()
                                            .and_then(|i| i.save_path.clone())
                                            .or_else(|| {
                                                app.config
                                                    .games
                                                    .get(&status.app_name)
                                                    .and_then(|s| s.save_path.clone())
                                            });

                                        if let (Some(item), Some(sp)) = (
                                            app.library
                                                .iter()
                                                .find(|i| i.app_name == status.app_name),
                                            save_path,
                                        ) {
                                            let _ = app.tx.send(WorkerMsg::UploadCloudSave {
                                                app_name: status.app_name.clone(),
                                                namespace: item.namespace.clone(),
                                                save_path: sp,
                                            });
                                        }
                                    }
                                },
                            );
                        });
                    });
                });

                // Local Backup Box
                let backup_frame = egui::Frame::group(ui.style())
                    .rounding(5.0)
                    .stroke(egui::Stroke::new(1.0, egui::Color32::from_gray(60)));

                backup_frame.show(ui, |ui| {
                    ui.set_min_size(egui::vec2(box_width, box_height));
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.colored_label(
                                egui::Color32::from_rgb(100, 200, 100),
                                egui::RichText::new(" Local Backup ")
                                    .strong()
                                    .background_color(egui::Color32::from_rgb(40, 80, 40)),
                            );
                        });
                        ui.add_space(10.0);
                        ui.vertical_centered(|ui| {
                            if let Some(t) = status.backup_time {
                                ui.label(
                                    t.with_timezone(&chrono::Local)
                                        .format("%Y-%m-%d %H:%M:%S")
                                        .to_string(),
                                );
                            } else {
                                ui.label("No backup found");
                            }
                            ui.add_space(20.0);
                            ui.label(egui::RichText::new("📦").size(80.0));
                            ui.add_space(20.0);

                            ui.with_layout(
                                egui::Layout::bottom_up(egui::Align::Center),
                                |ui| {
                                    let backup_path = app
                                        .advanced_info
                                        .as_ref()
                                        .and_then(|i| i.backup_path.clone());
                                    if ui
                                        .add_enabled(
                                            status.backup_time.is_some() && can_upload,
                                            egui::Button::new(
                                                egui::RichText::new("Upload Backup -> Cloud")
                                                    .strong(),
                                            ),
                                        )
                                        .clicked()
                                    {
                                        if let (Some(item), Some(bp)) = (
                                            app.library
                                                .iter()
                                                .find(|i| i.app_name == status.app_name),
                                            backup_path,
                                        ) {
                                            let _ = app.tx.send(WorkerMsg::UploadCloudSave {
                                                app_name: status.app_name.clone(),
                                                namespace: item.namespace.clone(),
                                                save_path: bp,
                                            });
                                        }
                                    }
                                },
                            );
                        });
                    });
                });

                // Cloud Box
                let cloud_frame = egui::Frame::group(ui.style())
                    .rounding(5.0)
                    .stroke(egui::Stroke::new(1.0, egui::Color32::from_gray(60)));

                cloud_frame.show(ui, |ui| {
                    ui.set_min_size(egui::vec2(box_width, box_height));
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.colored_label(
                                egui::Color32::from_rgb(100, 100, 200),
                                egui::RichText::new(" Cloud ")
                                    .strong()
                                    .background_color(egui::Color32::from_rgb(40, 40, 80)),
                            );
                        });
                        ui.add_space(10.0);
                        ui.vertical_centered(|ui| {
                            if let Some(t) = status.remote_time {
                                ui.label(
                                    t.with_timezone(&chrono::Local)
                                        .format("%Y-%m-%d %H:%M:%S")
                                        .to_string(),
                                );
                            } else {
                                ui.label("No cloud save found");
                            }
                            ui.add_space(20.0);
                            ui.label(egui::RichText::new("☁").size(80.0));
                            ui.add_space(20.0);

                            ui.with_layout(
                                egui::Layout::bottom_up(egui::Align::Center),
                                |ui| {
                                    ui.vertical(|ui| {
                                        if ui
                                            .add_enabled(
                                                can_download,
                                                egui::Button::new(
                                                    egui::RichText::new(
                                                        "Download Cloud -> Local",
                                                    )
                                                    .strong(),
                                                ),
                                            )
                                            .clicked()
                                        {
                                            let save_path = app
                                                .advanced_info
                                                .as_ref()
                                                .and_then(|i| i.save_path.clone())
                                                .or_else(|| {
                                                    app.config
                                                        .games
                                                        .get(&status.app_name)
                                                        .and_then(|s| s.save_path.clone())
                                                });

                                            if let (Some(item), Some(sp)) = (
                                                app.library
                                                    .iter()
                                                    .find(|i| i.app_name == status.app_name),
                                                save_path,
                                            ) {
                                                let _ = app.tx.send(
                                                    WorkerMsg::DownloadCloudSave {
                                                        app_name: status.app_name.clone(),
                                                        namespace: item.namespace.clone(),
                                                        save_path: sp,
                                                    },
                                                );
                                            }
                                        }
                                        if ui
                                            .add_enabled(
                                                can_download,
                                                egui::Button::new(
                                                    egui::RichText::new(
                                                        "Download Cloud -> Backup",
                                                    )
                                                    .strong(),
                                                ),
                                            )
                                            .clicked()
                                        {
                                            let backup_path = app
                                                .advanced_info
                                                .as_ref()
                                                .and_then(|i| i.backup_path.clone());
                                            if let (Some(item), Some(bp)) = (
                                                app.library
                                                    .iter()
                                                    .find(|i| i.app_name == status.app_name),
                                                backup_path,
                                            ) {
                                                let _ = app.tx.send(
                                                    WorkerMsg::DownloadCloudSave {
                                                        app_name: status.app_name.clone(),
                                                        namespace: item.namespace.clone(),
                                                        save_path: bp,
                                                    },
                                                );
                                            }
                                        }
                                    });
                                },
                            );
                        });
                    });
                });
            });
        });

        ui.add_space(20.0);

        // Comparison message
        if let (Some(l), Some(r)) = (status.local_time, status.remote_time) {
            let diff = (l - r).num_seconds().abs();
            if diff < 2 {
                ui.colored_label(egui::Color32::GREEN, "✔ Both saves are synchronized.");
            } else if l > r {
                ui.colored_label(
                    egui::Color32::YELLOW,
                    format!(
                        "⚠ Local save is newer (by {}).",
                        crate::utils::format_duration(l - r)
                    ),
                );
            } else {
                ui.colored_label(
                    egui::Color32::YELLOW,
                    format!(
                        "⚠ Cloud save is newer (by {}).",
                        crate::utils::format_duration(r - l)
                    ),
                );
            }
        }

        ui.add_space(10.0);

        // Settings Box
        let settings_frame = egui::Frame::group(ui.style())
            .rounding(5.0)
            .stroke(egui::Stroke::new(1.0, egui::Color32::from_gray(60)));

        settings_frame.show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.colored_label(
                        egui::Color32::from_rgb(100, 100, 200),
                        egui::RichText::new(" Settings ")
                            .strong()
                            .background_color(egui::Color32::from_rgb(40, 40, 80)),
                    );
                });
                ui.add_space(10.0);

                let game_settings = app
                    .config
                    .games
                    .entry(status.app_name.clone())
                    .or_default();
                let mut changed = false;

                ui.horizontal(|ui| {
                    ui.label("Enable sync");
                    if ui
                        .checkbox(
                            &mut game_settings.cloud_sync_enabled,
                            "Automatically synchronize saves with the cloud",
                        )
                        .changed()
                    {
                        changed = true;
                    }
                });

                if ui.button("📁 Open Save Folder").clicked() {
                    if let Some(info) = &app.advanced_info {
                        if let Some(p) = &info.save_path {
                            let _ = open::that(p);
                        }
                    }
                }

                if changed {
                    let _ = app.config.save();
                }
            });
        });
    }
}
