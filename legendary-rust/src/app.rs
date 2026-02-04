use crate::api::EgsClient;
use crate::models::{LibraryItem, OAuthToken};
use eframe::egui;

use crate::models::GameInfo;

use crate::models::Asset;

use crate::models::InstalledGame;

use std::sync::mpsc::{channel, Receiver, Sender};

use std::collections::HashMap;

pub struct LegendaryApp {
    token: Option<OAuthToken>,
    library: Vec<LibraryItem>,
    installed_games: Vec<InstalledGame>,
    assets: Vec<Asset>,
    selected_game: Option<GameInfo>,
    images: HashMap<String, egui_extras::RetainedImage>,
    auth_code: String,
    status_message: String,
    current_view: View,
    tx: Sender<WorkerMsg>,
    rx: Receiver<WorkerResponse>,
}

enum WorkerMsg {
    Login(String),
    RefreshLibrary,
    FetchGameInfo(String, String), // namespace, catalog_item_id
    FetchAssets,
    FetchImage(String, String, bool), // app_name, url, is_installed
    Logout,
}

enum WorkerResponse {
    LoggedIn(OAuthToken),
    LibraryFetched(Vec<LibraryItem>),
    GameInfoFetched(GameInfo),
    AssetsFetched(Vec<Asset>),
    ImageFetched(String, egui::ColorImage),
    Error(String),
}

#[derive(PartialEq)]
enum View {
    Auth,
    Library,
    GameDetail,
}

fn color_to_grayscale(pixels: &mut [egui::Color32]) {
    for pixel in pixels {
        let gray = (pixel.r() as f32 * 0.299 + pixel.g() as f32 * 0.587 + pixel.b() as f32 * 0.114) as u8;
        *pixel = egui::Color32::from_rgba_unmultiplied(gray, gray, gray, pixel.a());
    }
}

impl LegendaryApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let (tx, rx) = channel();
        let (worker_tx, worker_rx) = channel();

        let ctx_clone = cc.egui_ctx.clone();
        // Spawn worker thread
        std::thread::spawn(move || {
            let mut client = match EgsClient::new() {
                Ok(c) => c,
                Err(e) => {
                    let _ = tx.send(WorkerResponse::Error(format!("Failed to initialize client: {}", e)));
                    return;
                }
            };

            // Try initial load
            if let Ok(saved_token) = crate::auth::load_token() {
                client.set_token(&saved_token.access_token);
                match client.get_library_items() {
                    Ok(items) => {
                        let _ = tx.send(WorkerResponse::LoggedIn(saved_token));
                        let _ = tx.send(WorkerResponse::LibraryFetched(items));
                        ctx_clone.request_repaint();
                    }
                    Err(_) => {
                        if let Some(refresh_token) = &saved_token.refresh_token {
                            if let Ok(new_token) = client.refresh_session(refresh_token) {
                                let _ = crate::auth::save_token(&new_token);
                                if let Ok(items) = client.get_library_items() {
                                    let _ = tx.send(WorkerResponse::LoggedIn(new_token));
                                    let _ = tx.send(WorkerResponse::LibraryFetched(items));
                                    ctx_clone.request_repaint();
                                }
                            }
                        }
                    }
                }
            }

            while let Ok(msg) = worker_rx.recv() {
                match msg {
                    WorkerMsg::Login(code) => {
                        let _ = tx.send(WorkerResponse::Error("Logging in...".to_string()));
                        match client.start_session(&code) {
                            Ok(token) => {
                                let _ = crate::auth::save_token(&token);
                                let _ = tx.send(WorkerResponse::LoggedIn(token));
                                // Fetch library immediately after login
                                if let Ok(items) = client.get_library_items() {
                                    let _ = tx.send(WorkerResponse::LibraryFetched(items));
                                    ctx_clone.request_repaint();
                                }
                            }
                            Err(e) => { let _ = tx.send(WorkerResponse::Error(e.to_string())); }
                        }
                    }
                    WorkerMsg::FetchImage(app_name, url, is_installed) => {
                        match reqwest::blocking::get(url) {
                            Ok(res) => {
                                if let Ok(bytes) = res.bytes() {
                                    if let Ok(img) = image::load_from_memory(&bytes) {
                                        let size = [img.width() as _, img.height() as _];
                                        let pixels = img.to_rgba8();
                                        let pixels: Vec<egui::Color32> = pixels
                                            .chunks_exact(4)
                                            .map(|p| egui::Color32::from_rgba_unmultiplied(p[0], p[1], p[2], p[3]))
                                            .collect();

                                        let mut color_image = egui::ColorImage {
                                            size,
                                            pixels,
                                        };

                                        if !is_installed {
                                            color_to_grayscale(&mut color_image.pixels);
                                        }

                                        let _ = tx.send(WorkerResponse::ImageFetched(app_name, color_image));
                                        ctx_clone.request_repaint();
                                    }
                                }
                            }
                            Err(_) => {}
                        }
                    }
                    WorkerMsg::RefreshLibrary => {
                        match client.get_library_items() {
                            Ok(items) => {
                                let _ = tx.send(WorkerResponse::LibraryFetched(items));
                                ctx_clone.request_repaint();
                            }
                            Err(e) => { let _ = tx.send(WorkerResponse::Error(e.to_string())); }
                        }
                    }
                    WorkerMsg::FetchGameInfo(ns, id) => {
                        match client.get_game_info(&ns, &id) {
                            Ok(info) => {
                                let _ = tx.send(WorkerResponse::GameInfoFetched(info));
                                ctx_clone.request_repaint();
                            }
                            Err(e) => { let _ = tx.send(WorkerResponse::Error(e.to_string())); }
                        }
                    }
                    WorkerMsg::FetchAssets => {
                        match client.get_game_assets("Windows") {
                            Ok(assets) => {
                                let _ = tx.send(WorkerResponse::AssetsFetched(assets));
                                ctx_clone.request_repaint();
                            }
                            Err(e) => { let _ = tx.send(WorkerResponse::Error(e.to_string())); }
                        }
                    }
                    WorkerMsg::Logout => {
                        let _ = crate::auth::get_config_dir().map(|d| {
                            let _ = std::fs::remove_file(d.join("user.json"));
                            let _ = std::fs::remove_file(d.join("token.json"));
                        });
                        // Clear client token too if needed
                    }
                }
            }
        });

        Self {
            token: None,
            library: Vec::new(),
            installed_games: crate::auth::load_installed_games(),
            assets: Vec::new(),
            selected_game: None,
            images: HashMap::new(),
            auth_code: String::new(),
            status_message: "Welcome to Legendary Rust".to_string(),
            current_view: View::Auth,
            tx: worker_tx,
            rx,
        }
    }
}

impl eframe::App for LegendaryApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(res) = self.rx.try_recv() {
            match res {
                WorkerResponse::LoggedIn(token) => {
                    self.token = Some(token);
                    self.current_view = View::Library;
                    self.status_message = "Logged in".to_string();
                }
                WorkerResponse::LibraryFetched(items) => {
                    self.library = items;
                }
                WorkerResponse::GameInfoFetched(info) => {
                    self.selected_game = Some(info);
                    self.current_view = View::GameDetail;
                }
                WorkerResponse::AssetsFetched(assets) => {
                    self.assets = assets;
                }
                WorkerResponse::ImageFetched(app_name, image) => {
                    let name = format!("{}_art", app_name);
                    self.images.insert(app_name, egui_extras::RetainedImage::from_color_image(name, image));
                }
                WorkerResponse::Error(e) => {
                    self.status_message = format!("Error: {}", e);
                }
            }
        }

        egui::SidePanel::left("side_panel").show(ctx, |ui| {
            ui.heading("Legendary Rust");
            if ui.button("Library").clicked() {
                self.current_view = View::Library;
            }
            if self.token.is_none() {
                if ui.button("Login").clicked() {
                    self.current_view = View::Auth;
                }
            } else {
                if ui.button("Logout").clicked() {
                    let _ = self.tx.send(WorkerMsg::Logout);
                    self.token = None;
                    self.library.clear();
                    self.images.clear();
                    self.status_message = "Logged out".to_string();
                    self.current_view = View::Auth;
                }
            }
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            match self.current_view {
                View::Auth => self.show_auth_view(ui),
                View::Library => self.show_library_view(ui),
                View::GameDetail => self.show_game_detail_view(ui),
            }

            ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                ui.horizontal(|ui| {
                    ui.label(&self.status_message);
                });
                ui.separator();
            });
        });
    }
}

impl LegendaryApp {
    fn show_auth_view(&mut self, ui: &mut egui::Ui) {
        ui.heading("Authentication");
        ui.label(&self.status_message);

        ui.separator();
        ui.label("How to log in:");
        ui.label("1. Click the button below to open the Epic Games login page in your browser.");
        ui.label("2. After logging in, you will see a JSON response containing 'authorizationCode'.");
        ui.label("3. Copy that code and paste it into the field below.");
        ui.label("4. Click 'Log In' to complete the process.");

        if ui.button("Open Login URL").clicked() {
            let url = crate::api::EgsClient::get_auth_url();
            let _ = open::that(url);
            self.status_message = "Waiting for authorization code...".to_string();
        }

        ui.horizontal(|ui| {
            ui.label("Authorization Code:");
            ui.text_edit_singleline(&mut self.auth_code);
        });

        if ui.button("Log In").clicked() {
            let _ = self.tx.send(WorkerMsg::Login(self.auth_code.clone()));
            self.status_message = "Logging in...".to_string();
        }
    }

    fn show_library_view(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Library");
            if ui.button("Refresh").clicked() {
                self.installed_games = crate::auth::load_installed_games();
                let _ = self.tx.send(WorkerMsg::RefreshLibrary);
            }
        });

        if self.token.is_none() {
            ui.label("You must be logged in to see your library.");
            return;
        }

        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.vertical(|ui| {
                for item in &self.library {
                    let is_installed = self.installed_games.iter().any(|g| g.app_name == item.app_name);

                    let local_meta = crate::auth::load_local_metadata(&item.app_name);
                    let title = local_meta.as_ref()
                        .map(|m| m.app_title.clone())
                        .or_else(|| item.metadata.as_ref()
                            .and_then(|m| m.get("title"))
                            .and_then(|t| t.as_str())
                            .map(|s| s.to_string()))
                        .unwrap_or_else(|| item.app_name.clone());

                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            if let Some(img) = self.images.get(&item.app_name) {
                                let size = img.size_vec2();
                                let ratio = size.x / size.y;
                                img.show_max_size(ui, egui::vec2(100.0 * ratio, 100.0));
                            } else {
                                // Trigger fetch if not present
                                if let Some(meta) = local_meta {
                                    if let Some(first_img) = meta.metadata.key_images.get(0) {
                                        let _ = self.tx.send(WorkerMsg::FetchImage(item.app_name.clone(), first_img.url.clone(), is_installed));
                                    }
                                }
                                ui.allocate_space(egui::vec2(70.0, 100.0));
                            }

                            ui.vertical(|ui| {
                                ui.horizontal(|ui| {
                                    if is_installed {
                                        ui.label("✅");
                                    }
                                    if ui.button(egui::RichText::new(title).strong().size(18.0)).clicked() {
                                        let _ = self.tx.send(WorkerMsg::FetchAssets);
                                        let _ = self.tx.send(WorkerMsg::FetchGameInfo(item.namespace.clone(), item.catalog_item_id.clone()));
                                        self.status_message = "Fetching game info...".to_string();
                                    }
                                });
                                ui.label(format!("ID: {}", item.app_name));
                            });
                        });
                    });
                    ui.add_space(8.0);
                }
            });
        });
    }

    fn show_game_detail_view(&mut self, ui: &mut egui::Ui) {
        if let Some(game) = &self.selected_game {
            if ui.button("Back").clicked() {
                self.current_view = View::Library;
            }
            ui.heading(&game.title);

            if let Some(asset) = self.assets.iter().find(|a| a.catalog_item_id == game.id) {
                ui.label(format!("Version: {}", asset.build_version));
                ui.label(format!("App Name: {}", asset.app_name));

                if let Some(installed) = self.installed_games.iter().find(|g| g.app_name == asset.app_name) {
                    ui.label(format!("Installed at: {}", installed.install_path));
                }
            }

            ui.label(format!("ID: {}", game.id));
            ui.label(format!("Namespace: {}", game.namespace));
            if let Some(desc) = &game.description {
                ui.label(desc);
            }

            ui.separator();

            ui.horizontal(|ui| {
                if ui.button("Install").clicked() {
                    self.status_message = "Install not implemented yet".to_string();
                }
                if ui.button("Launch").clicked() {
                    if let Some(asset) = self.assets.iter().find(|a| a.catalog_item_id == game.id) {
                        if let Some(installed) = self.installed_games.iter().find(|g| g.app_name == asset.app_name) {
                            let path = std::path::PathBuf::from(&installed.install_path);
                            // Very simplified: look for an .exe or the app_name as binary
                            let mut found = false;
                            for ext in &["exe", "sh", ""] {
                                let exe_path = path.join(format!("{}.{}", asset.app_name, ext));
                                if exe_path.exists() {
                                    self.status_message = format!("Launching: {:?}", exe_path);
                                    let _ = std::process::Command::new(exe_path).spawn();
                                    found = true;
                                    break;
                                }
                            }
                            if !found {
                                self.status_message = format!("Could not find executable in {}", installed.install_path);
                            }
                        } else {
                            self.status_message = "Game not installed according to Legendary config".to_string();
                        }
                    } else {
                        self.status_message = "Launch failed: app name not found".to_string();
                    }
                }
            });
        }
    }
}
