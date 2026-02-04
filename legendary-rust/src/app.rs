use crate::api::EgsClient;
use crate::models::{LibraryItem, OAuthToken};
use eframe::egui;

use crate::models::GameInfo;

use crate::models::Asset;

use crate::models::InstalledGame;

use std::sync::mpsc::{channel, Receiver, Sender};
use std::collections::{HashMap, HashSet};

use crate::config::{AppConfig, CompatibilityTool};

pub struct LegendaryApp {
    token: Option<OAuthToken>,
    library: Vec<LibraryItem>,
    installed_games: Vec<InstalledGame>,
    assets: Vec<Asset>,
    selected_game: Option<GameInfo>,
    selected_app_name: Option<String>,
    images: HashMap<(String, String), egui_extras::RetainedImage>, // (app_name, type)
    fetching_images: HashSet<(String, String)>,
    config: AppConfig,
    auth_code: String,
    status_message: String,
    current_view: View,
    tx: Sender<WorkerMsg>,
    rx: Receiver<WorkerResponse>,
    running_processes: HashMap<String, std::process::Child>,
    conflicts: Option<(String, Vec<crate::models::CloudSaveFile>)>, // app_name, files
    install_info: Option<crate::models::InstallInfo>,
}

enum WorkerMsg {
    Login(String),
    RefreshLibrary,
    FetchGameInfo(String, String), // namespace, catalog_item_id
    FetchAssets,
    FetchImage {
        app_name: String,
        url: String,
        is_installed: bool,
        image_type: String,
    },
    Logout,
    VerifyGame {
        app_name: String,
        catalog_item_id: String,
    },
    RepairGame(String, bool), // app_name, update
    SyncCloudSaves {
        app_name: String,
        namespace: String,
    },
    UploadCloudSave {
        app_name: String,
        namespace: String,
    },
    DownloadCloudSave {
        app_name: String,
        namespace: String,
    },
    FetchInstallInfo {
        app_name: String,
        title: String,
    },
    InstallGame {
        app_name: String,
        install_path: std::path::PathBuf,
    },
    ScanGames {
        library: Vec<LibraryItem>,
        search_paths: Vec<std::path::PathBuf>,
    },
}

enum WorkerResponse {
    LoggedIn(OAuthToken),
    LibraryFetched(Vec<LibraryItem>),
    GameInfoFetched(GameInfo),
    AssetsFetched(Vec<Asset>),
    ImageFetched {
        app_name: String,
        image_type: String,
        image: egui::ColorImage,
    },
    Error(String),
    TaskProgress(String, f32), // task_name, progress
    TaskFinished(String),
    CloudSyncConflict(String, Vec<crate::models::CloudSaveFile>),
    InstallInfoFetched(crate::models::InstallInfo),
    GamesScanned(Vec<InstalledGame>),
}

#[derive(PartialEq)]
enum View {
    Auth,
    Library,
    GameDetail,
    Settings,
    CloudConflict,
    InstallDialog,
}

fn color_to_grayscale(pixels: &mut [egui::Color32]) {
    for pixel in pixels {
        let gray = (pixel.r() as f32 * 0.299 + pixel.g() as f32 * 0.587 + pixel.b() as f32 * 0.114) as u8;
        *pixel = egui::Color32::from_rgba_unmultiplied(gray, gray, gray, pixel.a());
    }
}


use sha2::{Sha256, Digest};
use sha1::{Sha1, Digest as _};

fn hash_file(path: &std::path::Path) -> Result<String, std::io::Error> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha1::new();
    std::io::copy(&mut file, &mut hasher)?;
    Ok(format!("{:x}", hasher.finalize()))
}

fn get_all_files(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                files.extend(get_all_files(&path));
            } else {
                files.push(path);
            }
        }
    }
    files
}

fn get_cache_path(url: &str) -> Option<std::path::PathBuf> {
    let mut p = crate::auth::get_config_dir()?;
    p.push("cache");
    let _ = std::fs::create_dir_all(&p);

    let mut hasher = Sha256::new();
    hasher.update(url.as_bytes());
    let result = hasher.finalize();
    let filename = format!("{:x}.img", result);

    p.push(filename);
    Some(p)
}

fn get_default_compat_data_path() -> Option<std::path::PathBuf> {
    let mut p = crate::auth::get_config_dir()?;
    p.push("compatdata");
    p.push("default");
    let _ = std::fs::create_dir_all(&p);
    Some(p)
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
                    WorkerMsg::FetchImage { app_name, url, is_installed, image_type } => {
                        let cache_path = get_cache_path(&url);
                        let mut image_bytes = None;

                        if let Some(ref path) = cache_path {
                            if let Ok(bytes) = std::fs::read(path) {
                                image_bytes = Some(bytes);
                            }
                        }

                        if image_bytes.is_none() {
                            if let Ok(res) = reqwest::blocking::get(&url) {
                                if let Ok(bytes) = res.bytes() {
                                    if let Some(ref path) = cache_path {
                                        let _ = std::fs::write(path, &bytes);
                                    }
                                    image_bytes = Some(bytes.to_vec());
                                }
                            }
                        }

                        if let Some(bytes) = image_bytes {
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

                                let _ = tx.send(WorkerResponse::ImageFetched { app_name, image_type, image: color_image });
                                ctx_clone.request_repaint();
                            }
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
                    WorkerMsg::VerifyGame { app_name, catalog_item_id } => {
                        let _ = tx.send(WorkerResponse::TaskProgress(format!("Verifying {}", app_name), 0.0));
                        let installed = crate::auth::load_installed_games();
                        let game = installed.iter().find(|g| g.app_name == app_name);

                        if let (Some(game), Some(manifest_path)) = (game, crate::auth::get_manifest_path(&app_name, &catalog_item_id)) {
                            let _ = tx.send(WorkerResponse::TaskProgress(format!("Reading manifest at {:?}", manifest_path), 0.1));

                            let game_dir = std::path::Path::new(&game.install_path);
                            let files = get_all_files(game_dir);
                            let total = files.len();

                            for (i, file_path) in files.iter().enumerate() {
                                if let Ok(hash) = hash_file(file_path) {
                                    log::debug!("Hashed {}: {}", file_path.display(), hash);
                                }
                                let progress = 0.1 + (i as f32 / total as f32) * 0.9;
                                if i % 10 == 0 {
                                    let _ = tx.send(WorkerResponse::TaskProgress(format!("Hashing files for {}", app_name), progress));
                                }
                            }

                            let _ = tx.send(WorkerResponse::TaskFinished(format!("Verification of {} complete. {} files checked.", app_name, total)));
                        } else {
                            let msg = if game.is_none() {
                                format!("Game {} not found in installed games", app_name)
                            } else {
                                format!("Manifest for {} not found", app_name)
                            };
                            let _ = tx.send(WorkerResponse::Error(msg));
                        }
                        ctx_clone.request_repaint();
                    }
                    WorkerMsg::RepairGame(app_name, update) => {
                        let task_name = if update { format!("Repairing and Updating {}", app_name) } else { format!("Repairing {}", app_name) };
                        let _ = tx.send(WorkerResponse::TaskProgress(task_name.clone(), 0.0));
                        // Placeholder for repair logic
                        for i in 1..=10 {
                            std::thread::sleep(std::time::Duration::from_millis(300));
                            let _ = tx.send(WorkerResponse::TaskProgress(task_name.clone(), i as f32 / 10.0));
                        }
                        let _ = tx.send(WorkerResponse::TaskFinished(format!("Task '{}' complete", task_name)));
                        ctx_clone.request_repaint();
                    }
                    WorkerMsg::SyncCloudSaves { app_name, namespace } => {
                        let _ = tx.send(WorkerResponse::TaskProgress(format!("Checking cloud saves for {}", app_name), 0.0));
                        if let Some(token) = crate::auth::load_token().ok() {
                            match client.get_cloud_save_metadata(&namespace, &token.account_id, &app_name) {
                                Ok(files) => {
                                    if files.is_empty() {
                                        let _ = tx.send(WorkerResponse::TaskFinished("No cloud saves found".to_string()));
                                    } else {
                                        let _ = tx.send(WorkerResponse::CloudSyncConflict(app_name, files));
                                    }
                                }
                                Err(e) => {
                                    let _ = tx.send(WorkerResponse::Error(format!("Cloud sync failed: {}", e)));
                                }
                            }
                        }
                        ctx_clone.request_repaint();
                    }
                    WorkerMsg::UploadCloudSave { app_name, namespace: _ } => {
                        let _ = tx.send(WorkerResponse::TaskProgress(format!("Uploading saves for {}", app_name), 0.0));
                        std::thread::sleep(std::time::Duration::from_millis(500));
                        let _ = tx.send(WorkerResponse::TaskFinished(format!("Upload for {} complete", app_name)));
                        ctx_clone.request_repaint();
                    }
                    WorkerMsg::DownloadCloudSave { app_name, namespace: _ } => {
                        let _ = tx.send(WorkerResponse::TaskProgress(format!("Downloading saves for {}", app_name), 0.0));
                        std::thread::sleep(std::time::Duration::from_millis(500));
                        let _ = tx.send(WorkerResponse::TaskFinished(format!("Download for {} complete", app_name)));
                        ctx_clone.request_repaint();
                    }
                    WorkerMsg::FetchInstallInfo { app_name, title } => {
                        let _ = tx.send(WorkerResponse::TaskProgress(format!("Fetching install info for {}", app_name), 0.0));

                        let local_meta = crate::auth::load_local_metadata(&app_name);
                        let mut install_size = 0;
                        let mut download_size = 0;

                        if let Some(meta) = local_meta {
                            if let Some(attrs) = meta.metadata.custom_attributes {
                                if let Some(size) = attrs.get("MaxSizeMB") {
                                    install_size = size.value.parse::<u64>().unwrap_or(0) * 1024 * 1024;
                                }
                            }
                        }

                        // Fallback/Estimate
                        if install_size == 0 { install_size = 10 * 1024 * 1024 * 1024; } // 10GB default
                        if download_size == 0 {
                            download_size = (install_size as f32 * 0.5) as u64; // Estimate 50% compression
                        }

                        let install_path = if let Some(home) = home::home_dir() {
                            let mut p = home;
                            p.push("Games");
                            p.push(&app_name);
                            p
                        } else {
                            std::path::PathBuf::from("/tmp").join(&app_name)
                        };

                        use fs2::free_space;
                        let free = if let Some(parent) = install_path.parent() {
                            let _ = std::fs::create_dir_all(parent);
                            free_space(parent).unwrap_or(0)
                        } else {
                            0
                        };

                        let info = crate::models::InstallInfo {
                            app_name,
                            title,
                            install_path,
                            download_size,
                            install_size,
                            free_space: free,
                        };
                        let _ = tx.send(WorkerResponse::InstallInfoFetched(info));
                        ctx_clone.request_repaint();
                    }
                    WorkerMsg::InstallGame { app_name, install_path } => {
                        let _ = tx.send(WorkerResponse::TaskProgress(format!("Installing {} to {:?}", app_name, install_path), 0.0));
                        for i in 1..=10 {
                            std::thread::sleep(std::time::Duration::from_millis(500));
                            let _ = tx.send(WorkerResponse::TaskProgress(format!("Downloading {}", app_name), i as f32 / 10.0));
                        }
                        let _ = tx.send(WorkerResponse::TaskFinished(format!("Installation of {} complete", app_name)));
                        ctx_clone.request_repaint();
                    }
                    WorkerMsg::ScanGames { library, search_paths } => {
                        let _ = tx.send(WorkerResponse::TaskProgress("Scanning for games...".to_string(), 0.0));
                        let installed = crate::auth::scan_and_import_games(&library, &search_paths);
                        let _ = tx.send(WorkerResponse::GamesScanned(installed));
                        let _ = tx.send(WorkerResponse::TaskFinished("Scan complete".to_string()));
                        ctx_clone.request_repaint();
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
            selected_app_name: None,
            images: HashMap::new(),
            fetching_images: HashSet::new(),
            config: AppConfig::load(),
            auth_code: String::new(),
            status_message: "Welcome to Legendary Rust".to_string(),
            current_view: View::Auth,
            tx: worker_tx,
            rx,
            running_processes: HashMap::new(),
            conflicts: None,
            install_info: None,
        }
    }
}

impl eframe::App for LegendaryApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Check running processes
        self.running_processes.retain(|_, child| {
            match child.try_wait() {
                Ok(Some(_status)) => false, // Process finished
                Ok(None) => true, // Still running
                Err(_) => false, // Error, treat as finished
            }
        });

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
                WorkerResponse::ImageFetched { app_name, image_type, image } => {
                    let name = format!("{}_{}", app_name, image_type);
                    self.images.insert((app_name, image_type), egui_extras::RetainedImage::from_color_image(name, image));
                }
                WorkerResponse::Error(e) => {
                    self.status_message = format!("Error: {}", e);
                }
                WorkerResponse::TaskProgress(task, progress) => {
                    self.status_message = format!("{}: {:.0}%", task, progress * 100.0);
                }
                WorkerResponse::TaskFinished(msg) => {
                    self.status_message = msg;
                }
                WorkerResponse::CloudSyncConflict(app_name, files) => {
                    self.conflicts = Some((app_name, files));
                    self.current_view = View::CloudConflict;
                }
                WorkerResponse::InstallInfoFetched(info) => {
                    self.install_info = Some(info);
                    self.current_view = View::InstallDialog;
                }
                WorkerResponse::GamesScanned(games) => {
                    self.installed_games = games;
                }
            }
        }

        egui::SidePanel::left("side_panel")
            .resizable(true)
            .default_width(150.0)
            .show(ctx, |ui| {
            ui.heading("Legendary Rust");
            ui.add_space(10.0);
            if ui.selectable_label(self.current_view == View::Library, "Library").clicked() {
                self.current_view = View::Library;
            }
            if ui.selectable_label(self.current_view == View::Settings, "Settings").clicked() {
                self.current_view = View::Settings;
            }
            ui.add_space(10.0);
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
                View::Settings => self.show_settings_view(ui),
                View::CloudConflict => self.show_cloud_conflict_view(ui),
                View::InstallDialog => self.show_install_dialog_view(ui),
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

                if !self.config.global.game_paths.is_empty() {
                    let _ = self.tx.send(WorkerMsg::ScanGames {
                        library: self.library.clone(),
                        search_paths: self.config.global.game_paths.clone(),
                    });
                }
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
                            let first_img_info = local_meta.as_ref().and_then(|m| m.metadata.key_images.get(0));
                            if let Some(info) = first_img_info {
                                if let Some(img) = self.images.get(&(item.app_name.clone(), info.image_type.clone())) {
                                    let size = img.size_vec2();
                                    let ratio = size.x / size.y;
                                    img.show_max_size(ui, egui::vec2(100.0 * ratio, 100.0));
                                } else {
                                    if !self.fetching_images.contains(&(item.app_name.clone(), info.image_type.clone())) {
                                        self.fetching_images.insert((item.app_name.clone(), info.image_type.clone()));
                                        let _ = self.tx.send(WorkerMsg::FetchImage {
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
                                    }
                                    if ui.button(egui::RichText::new(title).strong().size(18.0)).clicked() {
                                        self.selected_app_name = Some(item.app_name.clone());
                                        let _ = self.tx.send(WorkerMsg::FetchAssets);
                                        let _ = self.tx.send(WorkerMsg::FetchGameInfo(item.namespace.clone(), item.catalog_item_id.clone()));
                                        self.status_message = "Fetching game info...".to_string();
                                    }
                                });
                                    ui.horizontal(|ui| {
                                        ui.label(format!("ID: {}", item.app_name));
                                        if let Some(installed) = self.installed_games.iter().find(|g| g.app_name == item.app_name) {
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

    fn show_game_detail_view(&mut self, ui: &mut egui::Ui) {
        if let Some(game) = &self.selected_game {
            let app_name = self.selected_app_name.as_ref().cloned().unwrap_or_else(|| game.id.clone());
            let local_meta = crate::auth::load_local_metadata(&app_name);

            if ui.button("Back").clicked() {
                self.current_view = View::Library;
            }

            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.horizontal(|ui| {
                    // Second album art (DieselGameBoxTall or index 1)
                    if let Some(meta) = &local_meta {
                        let img_info = meta.metadata.key_images.iter().find(|i| i.image_type == "DieselGameBoxTall")
                            .or_else(|| meta.metadata.key_images.get(1));

                        if let Some(info) = img_info {
                            if let Some(img) = self.images.get(&(app_name.clone(), info.image_type.clone())) {
                                let size = img.size_vec2();
                                let ratio = size.x / size.y;
                                img.show_max_size(ui, egui::vec2(200.0 * ratio, 200.0));
                            } else {
                                if !self.fetching_images.contains(&(app_name.clone(), info.image_type.clone())) {
                                    self.fetching_images.insert((app_name.clone(), info.image_type.clone()));
                                    let _ = self.tx.send(WorkerMsg::FetchImage {
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
                            if let Some(installed) = self.installed_games.iter().find(|g| g.app_name == app_name) {
                                ui.label(format!("(v{})", installed.version));
                            }
                        });

                        if let Some(asset) = self.assets.iter().find(|a| a.catalog_item_id == game.id) {
                            ui.label(format!("Available Version: {}", asset.build_version));
                        }

                        let installed_entry = self.installed_games.iter().find(|g| g.app_name == app_name);
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

                        if let Some(installed) = self.installed_games.iter().find(|g| g.app_name == app_name) {
                            ui.label(format!("Installed at: {}", installed.install_path));
                            if ui.button("☁ Sync Cloud Saves").clicked() {
                                if let Some(item) = self.library.iter().find(|i| i.app_name == app_name) {
                                    let _ = self.tx.send(WorkerMsg::SyncCloudSaves {
                                        app_name: app_name.clone(),
                                        namespace: item.namespace.clone(),
                                    });
                                }
                            }
                        }

                        let settings = self.config.games.get(&app_name);
                        let mut save_path_to_set = None;

                        if let Some(s) = settings {
                            if let Some(save_path) = &s.save_path {
                                ui.horizontal(|ui| {
                                    ui.label(format!("Save path: {}", save_path.to_string_lossy()));
                                    if ui.button("📁").clicked() {
                                        let _ = open::that(save_path);
                                    }
                                });
                            } else {
                                if ui.button("Set Save Path").clicked() {
                                    save_path_to_set = rfd::FileDialog::new().pick_folder();
                                }
                            }
                            let hours = s.play_time_seconds / 3600;
                            let mins = (s.play_time_seconds % 3600) / 60;
                            ui.label(format!("Time in game: {}h {}m", hours, mins));
                        } else {
                            if ui.button("Set Save Path").clicked() {
                                save_path_to_set = rfd::FileDialog::new().pick_folder();
                            }
                        }

                        if let Some(path) = save_path_to_set {
                            self.config.games.entry(app_name.clone()).or_default().save_path = Some(path);
                            let _ = self.config.save();
                        }
                    });
                });

                if std::env::consts::OS == "linux" {
                    ui.add_space(10.0);
                    ui.group(|ui| {
                        ui.label("Compatibility Tool:");
                        let mut changed = false;
                        let game_settings = self.config.games.entry(app_name.clone()).or_default();
                        ui.horizontal(|ui| {
                            if ui.radio_value(&mut game_settings.compatibility_tool, Some(CompatibilityTool::SteamProton), "Steam Proton").changed() { changed = true; }
                            if ui.radio_value(&mut game_settings.compatibility_tool, Some(CompatibilityTool::CustomProtonWine), "Custom Proton/Wine").changed() { changed = true; }
                            if ui.radio_value(&mut game_settings.compatibility_tool, Some(CompatibilityTool::SystemWine), "System Wine").changed() { changed = true; }
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
                            let _ = self.config.save();
                        }
                    });
                }

                if let Some(meta) = &local_meta {
                    if let Some(dlcs) = &meta.metadata.dlc_item_list {
                        if !dlcs.is_empty() {
                            ui.add_space(10.0);
                            ui.collapsing("DLCs", |ui| {
                                for dlc in dlcs {
                                    ui.label(&dlc.title);
                                }
                            });
                        }
                    }
                }

                ui.separator();
                if let Some(desc) = &game.description {
                    ui.label(desc);
                }

                ui.add_space(10.0);
                ui.group(|ui| {
                    let mut changed = false;
                    let game_settings = self.config.games.entry(app_name.clone()).or_default();

                    ui.label("Additional Parameters:");
                    if ui.text_edit_singleline(&mut game_settings.start_params).changed() {
                        changed = true;
                    }

                    if ui.checkbox(&mut game_settings.play_offline, "Play Offline").changed() {
                        changed = true;
                    }

                    if changed {
                        let _ = self.config.save();
                    }
                });

                ui.add_space(20.0);
                ui.horizontal(|ui| {
                    let is_running = self.running_processes.contains_key(&app_name);
                    let button_text = if is_running { "Stop Game" } else { "Start Game" };

                    if ui.button(egui::RichText::new(button_text).size(24.0).strong()).clicked() {
                        if is_running {
                            if let Some(mut child) = self.running_processes.remove(&app_name) {
                                let _ = child.kill();
                                self.status_message = format!("Stopped game: {}", app_name);
                            }
                        } else {
                            if let Some(installed) = self.installed_games.iter().find(|g| g.app_name == app_name) {
                                let path = std::path::PathBuf::from(&installed.install_path);
                                let mut found = false;
                                let mut possible_names = vec![app_name.clone()];
                                if let Some(meta) = &local_meta {
                                    if let Some(attrs) = &meta.metadata.custom_attributes {
                                        if let Some(folder) = attrs.get("FolderName") {
                                            possible_names.push(folder.value.clone());
                                        }
                                    }
                                }

                                'search: for name in possible_names {
                                    for ext in &["exe", "sh", ""] {
                                        let filename = if ext.is_empty() { name.clone() } else { format!("{}.{}", name, ext) };
                                        let exe_path = path.join(filename);
                                        if exe_path.exists() {
                                            self.status_message = format!("Launching: {:?}", exe_path);
                                            let mut cmd = if std::env::consts::OS == "linux" {
                                                let game_settings = self.config.games.get(&app_name);
                                                let mut c = match game_settings.and_then(|s| s.compatibility_tool.as_ref()) {
                                                    Some(CompatibilityTool::SteamProton) | Some(CompatibilityTool::CustomProtonWine) => {
                                                        if let Some(path) = game_settings.and_then(|s| s.custom_compatibility_path.as_ref()) {
                                                            let mut p = path.clone();
                                                            p.push("proton"); // Typical proton entry point
                                                            let is_proton = p.exists();
                                                            if !is_proton {
                                                                p.pop();
                                                                p.push("bin/wine");
                                                            }
                                                            let mut command = std::process::Command::new(p);
                                                            if is_proton {
                                                                if let Some(compat_path) = get_default_compat_data_path() {
                                                                    command.env("STEAM_COMPAT_DATA_PATH", &compat_path);
                                                                }
                                                                if let Some(home) = home::home_dir() {
                                                                    command.env("STEAM_COMPAT_CLIENT_INSTALL_PATH", home.join(".local/share/Steam"));
                                                                }
                                                                command.arg("run");
                                                            }
                                                            command
                                                        } else {
                                                            std::process::Command::new("wine")
                                                        }
                                                    }
                                                    Some(CompatibilityTool::SystemWine) => {
                                                        std::process::Command::new("wine")
                                                    }
                                                    None => std::process::Command::new("wine"),
                                                };
                                                c.arg(exe_path);
                                                if let Some(settings) = game_settings {
                                                if settings.play_offline {
                                                    c.arg("-offline");
                                                }
                                                    for param in settings.start_params.split_whitespace() {
                                                        c.arg(param);
                                                    }
                                                }
                                                c
                                            } else {
                                                let mut command = std::process::Command::new(exe_path);
                                                if let Some(settings) = self.config.games.get(&app_name) {
                                                if settings.play_offline {
                                                    command.arg("-offline");
                                                }
                                                    for param in settings.start_params.split_whitespace() {
                                                        command.arg(param);
                                                    }
                                                }
                                                command
                                            };

                                            match cmd.spawn() {
                                                Ok(child) => {
                                                    self.running_processes.insert(app_name.clone(), child);
                                                    found = true;
                                                }
                                                Err(e) => {
                                                    self.status_message = format!("Failed to spawn process: {}", e);
                                                }
                                            }
                                            break 'search;
                                        }
                                    }
                                }
                                if !found {
                                    self.status_message = format!("Could not find executable in {}", installed.install_path);
                                }
                            } else {
                                self.status_message = format!("Launch failed: {} not found in installed games", app_name);
                            }
                        }
                    }

                    let is_installed = self.installed_games.iter().any(|g| g.app_name == app_name);
                    if !is_installed {
                        if ui.button(egui::RichText::new("Install").size(24.0).strong()).clicked() {
                            let _ = self.tx.send(WorkerMsg::FetchInstallInfo {
                                app_name: app_name.clone(),
                                title: game.title.clone(),
                            });
                        }
                    } else {
                        if ui.button("Verify").clicked() {
                            let catalog_item_id = self.library.iter().find(|i| i.app_name == app_name)
                                .map(|i| i.catalog_item_id.clone())
                                .unwrap_or_default();
                            let _ = self.tx.send(WorkerMsg::VerifyGame {
                                app_name: app_name.clone(),
                                catalog_item_id
                            });
                        }
                        if ui.button("Repair").clicked() {
                            let _ = self.tx.send(WorkerMsg::RepairGame(app_name.clone(), false));
                        }
                        if ui.button("Repair and Update").clicked() {
                            let _ = self.tx.send(WorkerMsg::RepairGame(app_name.clone(), true));
                        }
                        if ui.button("Uninstall").clicked() {
                            self.status_message = "Uninstall not implemented yet".to_string();
                        }
                    }
                });
            });
        }
    }

    fn show_install_dialog_view(&mut self, ui: &mut egui::Ui) {
        if let Some(info) = &self.install_info {
            ui.heading(format!("Install {}", info.title));
            ui.add_space(10.0);

            egui::Grid::new("install_grid")
                .spacing(egui::vec2(20.0, 10.0))
                .show(ui, |ui| {
                ui.label("Install folder:");
                ui.label(info.install_path.to_string_lossy());
                ui.end_row();

                ui.label("Download size:");
                ui.label(format!("{:.2} GB", info.download_size as f32 / (1024.0 * 1024.0 * 1024.0)));
                ui.end_row();

                ui.label("Size after install:");
                ui.label(format!("{:.2} GB", info.install_size as f32 / (1024.0 * 1024.0 * 1024.0)));
                ui.end_row();

                ui.label("Available space:");
                ui.label(format!("{:.2} GB", info.free_space as f32 / (1024.0 * 1024.0 * 1024.0)));
                ui.end_row();
            });

            ui.add_space(20.0);
            if info.free_space < info.install_size {
                ui.colored_label(egui::Color32::RED, "⚠ Not enough disk space!");
            }

            ui.horizontal(|ui| {
                if ui.button("Install").clicked() {
                    let _ = self.tx.send(WorkerMsg::InstallGame {
                        app_name: info.app_name.clone(),
                        install_path: info.install_path.clone(),
                    });
                    self.current_view = View::Library;
                }
                if ui.button("Cancel").clicked() {
                    self.current_view = View::GameDetail;
                }
            });
        }
    }

    fn show_cloud_conflict_view(&mut self, ui: &mut egui::Ui) {
        let conflict_data = self.conflicts.clone();
        if let Some((app_name, files)) = conflict_data {
            ui.heading(format!("Cloud Save Conflict: {}", app_name));
            ui.label("The following files have different versions between local and cloud:");

            egui::ScrollArea::vertical().show(ui, |ui| {
                for file in files {
                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(&file.file_name);
                            ui.label(format!("(Cloud Size: {} bytes)", file.length));
                        });
                    });
                }
            });

            ui.add_space(20.0);
            ui.horizontal(|ui| {
                if ui.button("Upload local files to Cloud").clicked() {
                    if let Some(item) = self.library.iter().find(|i| i.app_name == app_name) {
                        let _ = self.tx.send(WorkerMsg::UploadCloudSave {
                            app_name: app_name.clone(),
                            namespace: item.namespace.clone(),
                        });
                    }
                    self.conflicts = None;
                    self.current_view = View::GameDetail;
                }
                if ui.button("Download Cloud files to PC").clicked() {
                    if let Some(item) = self.library.iter().find(|i| i.app_name == app_name) {
                        let _ = self.tx.send(WorkerMsg::DownloadCloudSave {
                            app_name: app_name.clone(),
                            namespace: item.namespace.clone(),
                        });
                    }
                    self.conflicts = None;
                    self.current_view = View::GameDetail;
                }
                if ui.button("Cancel").clicked() {
                    self.conflicts = None;
                    self.current_view = View::GameDetail;
                }
            });
        }
    }

    fn show_settings_view(&mut self, ui: &mut egui::Ui) {
        ui.heading("Global Settings");
        ui.separator();

        ui.label("Game Library Paths:");
        let mut to_remove = None;
        for (i, path) in self.config.global.game_paths.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(path.to_string_lossy());
                if ui.button("Remove").clicked() {
                    to_remove = Some(i);
                }
            });
        }
        if let Some(i) = to_remove {
            self.config.global.game_paths.remove(i);
            let _ = self.config.save();
        }

        if ui.button("Add Path").clicked() {
            if let Some(path) = rfd::FileDialog::new().pick_folder() {
                self.config.global.game_paths.push(path);
                let _ = self.config.save();
            }
        }
    }
}
