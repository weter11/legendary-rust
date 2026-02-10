use crate::app::LegendaryApp;
use crate::worker::WorkerMsg;
use eframe::egui;

pub fn show_library_view(app: &mut LegendaryApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.heading("Library");
        ui.add_space(20.0);
        ui.label("Search:");
        ui.text_edit_singleline(&mut app.search_query);
        if ui.button("Clear").clicked() {
            app.search_query.clear();
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Refresh").clicked() {
                app.installed_games = crate::auth::load_installed_games();
                let _ = app.tx.send(WorkerMsg::RefreshLibrary);

                if !app.config.global.game_paths.is_empty() {
                    let _ = app.tx.send(WorkerMsg::ScanGames {
                        library: app.library.clone(),
                        search_paths: app.config.global.game_paths.clone(),
                    });
                }
            }
        });
    });

    if app.token.is_none() {
        ui.label("You must be logged in to see your library.");
        return;
    }

    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.vertical(|ui| {
            for item in &app.library {
                let local_meta = crate::auth::load_local_metadata(&item.app_name);
                let title = local_meta
                    .as_ref()
                    .map(|m| m.app_title.clone())
                    .or_else(|| {
                        item.metadata
                            .as_ref()
                            .and_then(|m| m.get("title"))
                            .and_then(|t| t.as_str())
                            .map(|s| s.to_string())
                    })
                    .unwrap_or_else(|| item.app_name.clone());

                if !app.search_query.is_empty()
                    && !title
                        .to_lowercase()
                        .contains(&app.search_query.to_lowercase())
                    && !item
                        .app_name
                        .to_lowercase()
                        .contains(&app.search_query.to_lowercase())
                {
                    continue;
                }

                let is_installed = app
                    .installed_games
                    .iter()
                    .any(|g| g.app_name == item.app_name);

                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        let first_img_info = local_meta
                            .as_ref()
                            .and_then(|m| m.metadata.key_images.get(0));
                        if let Some(info) = first_img_info {
                            if let Some(img) = app
                                .images
                                .get(&(item.app_name.clone(), info.image_type.clone()))
                            {
                                let size = img.size_vec2();
                                let ratio = size.x / size.y;
                                img.show_max_size(ui, egui::vec2(100.0 * ratio, 100.0));
                            } else {
                                if !app
                                    .fetching_images
                                    .contains(&(item.app_name.clone(), info.image_type.clone()))
                                {
                                    app.fetching_images.insert((
                                        item.app_name.clone(),
                                        info.image_type.clone(),
                                    ));
                                    let _ = app.tx.send(WorkerMsg::FetchImage {
                                        app_name: item.app_name.clone(),
                                        url: info.url.clone(),
                                        is_installed,
                                        image_type: info.image_type.clone(),
                                    });
                                }
                                ui.allocate_space(egui::vec2(70.0, 100.0));
                            }
                        } else {
                            ui.allocate_space(egui::vec2(70.0, 100.0));
                        }

                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                if is_installed {
                                    ui.label("✅");
                                    if let Some(installed) = app
                                        .installed_games
                                        .iter()
                                        .find(|g| g.app_name == item.app_name)
                                    {
                                        if let Some(asset) = app
                                            .assets
                                            .iter()
                                            .find(|a| a.app_name == item.app_name)
                                        {
                                            if asset.build_version != installed.version {
                                                ui.colored_label(
                                                    egui::Color32::YELLOW,
                                                    "⏫ Update Available",
                                                );
                                            }
                                        }
                                    }
                                }
                                if ui
                                    .button(egui::RichText::new(title).strong().size(18.0))
                                    .clicked()
                                {
                                    app.selected_app_name = Some(item.app_name.clone());
                                    app.unaccepted_eulas.clear();
                                    app.advanced_info = None;
                                    let _ = app.tx.send(WorkerMsg::FetchAssets);
                                    let _ = app.tx.send(WorkerMsg::FetchGameInfo {
                                        app_name: item.app_name.clone(),
                                        namespace: item.namespace.clone(),
                                        catalog_item_id: item.catalog_item_id.clone(),
                                    });

                                    if let Some(installed) = app
                                        .installed_games
                                        .iter()
                                        .find(|g| g.app_name == item.app_name)
                                    {
                                        let _ = app.tx.send(WorkerMsg::FetchAdvancedInfo {
                                            app_name: item.app_name.clone(),
                                            install_path: installed.install_path.clone(),
                                        });
                                    }

                                    app.status_message = "Fetching game info...".to_string();
                                }
                            });
                            ui.horizontal(|ui| {
                                ui.label(format!("ID: {}", item.app_name));
                                if let Some(installed) = app
                                    .installed_games
                                    .iter()
                                    .find(|g| g.app_name == item.app_name)
                                {
                                    ui.label(format!("| v{}", installed.version));
                                }
                            });
                        });
                    });
                });
                ui.add_space(8.0);
            }
        });
    });
}
