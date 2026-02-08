use crate::api::EgsClient;
use crate::models::{LibraryItem, OAuthToken};
use eframe::egui;

use crate::models::GameInfo;

use crate::models::Asset;

use crate::models::InstalledGame;

use std::sync::mpsc::{channel, Receiver, Sender};
use std::collections::{HashMap, HashSet};
use chrono::{DateTime, Utc};

use crate::config::{AppConfig, CompatibilityTool};

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

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
    search_query: String,
    status_message: String,
    current_view: View,
    tx: Sender<WorkerMsg>,
    rx: Receiver<WorkerResponse>,
    worker_cancel: Arc<AtomicBool>,
    worker_pause: Arc<AtomicBool>,
    running_processes: HashMap<String, std::process::Child>,
    save_sync_status: Option<SaveSyncStatus>,
    install_info: Option<crate::models::InstallInfo>,
    selected_tags: HashSet<String>,
    current_task: Option<TaskStatus>,
    manifest_files: Vec<String>,
    manifest_search_query: String,
    eos_status: crate::eos::EosOverlayStatus,
}

#[derive(Clone)]
pub struct TaskStatus {
    pub name: String,
    pub progress: f32,
    pub is_paused: bool,
    pub speed: String,
    pub eta: String,
}

#[derive(Clone)]
pub struct SaveSyncStatus {
    pub app_name: String,
    pub files: Vec<crate::models::CloudSaveFile>,
    pub local_time: Option<DateTime<Utc>>,
    pub remote_time: Option<DateTime<Utc>>,
    pub loading: bool,
    pub error: Option<String>,
}

pub(crate) enum WorkerMsg {
    Login(String),
    LoginSid(String),
    RefreshLibrary,
    FetchGameInfo {
        app_name: String,
        namespace: String,
        catalog_item_id: String,
    },
    FetchAssets,
    FetchImage {
        app_name: String,
        url: String,
        is_installed: bool,
        image_type: String,
    },
    Logout,
    CancelTask,
    PauseTask,
    ResumeTask,
    VerifyGame {
        app_name: String,
        catalog_item_id: String,
    },
    RepairGame(String, bool), // app_name, update
    SyncCloudSaves {
        app_name: String,
        namespace: String,
        save_path: Option<std::path::PathBuf>,
    },
    UploadCloudSave {
        app_name: String,
        namespace: String,
        save_path: std::path::PathBuf,
    },
    DownloadCloudSave {
        app_name: String,
        namespace: String,
        save_path: std::path::PathBuf,
    },
    FetchInstallInfo {
        app_name: String,
        title: String,
    },
    InstallGame {
        app_name: String,
        install_path: std::path::PathBuf,
        selected_tags: Option<HashSet<String>>,
        platform: String,
    },
    UninstallGame(String),
    ScanGames {
        library: Vec<LibraryItem>,
        search_paths: Vec<std::path::PathBuf>,
    },
    EglSync,
    ListFiles {
        app_name: String,
        catalog_item_id: String,
    },
    LaunchGame {
        app_name: String,
        offline: bool,
    },
    QueryEosStatus {
        prefix: Option<std::path::PathBuf>,
    },
    UpdateEosRegistry {
        overlay_path: String,
        prefix: std::path::PathBuf,
        enable: bool,
    },
}

pub(crate) enum WorkerResponse {
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
    TaskProgress {
        task_name: String,
        progress: f32,
        is_paused: bool,
        speed: Option<String>,
        eta: Option<String>,
    },
    TaskFinished(String),
    SaveSyncStatusFetched {
        app_name: String,
        files: Vec<crate::models::CloudSaveFile>,
        local_time: Option<DateTime<Utc>>,
        remote_time: Option<DateTime<Utc>>,
        error: Option<String>,
    },
    InstallInfoFetched(crate::models::InstallInfo),
    GamesScanned(Vec<InstalledGame>),
    FilesListed(Vec<String>),
    GameLaunched {
        app_name: String,
        child: std::process::Child,
    },
    EosStatusFetched(crate::eos::EosOverlayStatus),
}

#[derive(PartialEq)]
enum View {
    Auth,
    Library,
    GameDetail,
    Settings,
    SaveSync,
    InstallDialog,
    Tasks,
    Account,
    EosOverlay,
}

fn color_to_grayscale(pixels: &mut [egui::Color32]) {
    for pixel in pixels {
        let gray = (pixel.r() as f32 * 0.299 + pixel.g() as f32 * 0.587 + pixel.b() as f32 * 0.114) as u8;
        *pixel = egui::Color32::from_rgba_unmultiplied(gray, gray, gray, pixel.a());
    }
}


use sha2::{Sha256, Digest};

fn get_latest_local_save_time(path: &std::path::Path) -> Option<DateTime<Utc>> {
    let mut latest: Option<DateTime<Utc>> = None;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                if let Some(t) = get_latest_local_save_time(&p) {
                    if latest.is_none() || t > latest.unwrap() {
                        latest = Some(t);
                    }
                }
            } else if let Ok(meta) = std::fs::metadata(p) {
                if let Ok(modified) = meta.modified() {
                    let dt: DateTime<Utc> = modified.into();
                    if latest.is_none() || dt > latest.unwrap() {
                        latest = Some(dt);
                    }
                }
            }
        }
    }
    latest
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


fn construct_manifest_url(manifest_node: &serde_json::Value) -> Option<String> {
    let uri = manifest_node["uri"].as_str()?;
    if let Some(params) = manifest_node["queryParams"].as_array() {
        if params.is_empty() {
            return Some(uri.to_string());
        }
        let mut url = uri.to_string();
        url.push('?');
        for (i, param) in params.iter().enumerate() {
            if i > 0 { url.push('&'); }
            let name = param["name"].as_str().unwrap_or("");
            let value = param["value"].as_str().unwrap_or("");
            url.push_str(&format!("{}={}", urlencoding::encode(name), urlencoding::encode(value)));
        }
        Some(url)
    } else {
        Some(uri.to_string())
    }
}

fn find_manifest_url(asset_manifest: &serde_json::Value) -> Option<String> {
    let elements = asset_manifest["elements"].as_array()?;
    for element in elements {
        if let Some(manifests) = element["manifests"].as_array() {
            for manifest in manifests {
                if let Some(url) = construct_manifest_url(manifest) {
                    return Some(url);
                }
            }
        }
    }
    None
}

fn extract_deployment_id(manifest_info: &serde_json::Value) -> Option<String> {
    manifest_info["elements"].as_array()?
        .get(0)?
        .get("sidecar")?
        .get("config")?
        .as_str()
        .and_then(|config_str| serde_json::from_str::<serde_json::Value>(config_str).ok())
        .and_then(|config_json| config_json["deploymentId"].as_str().map(|s| s.to_string()))
}

fn get_cache_path(url: &str) -> Option<std::path::PathBuf> {
    let mut p = crate::auth::get_config_dir()?;
    p.push("cache");
    let _ = std::fs::create_dir_all(&p);

    let mut hasher = Sha256::new();
    hasher.update(url.as_bytes());
    hasher.update(b"v2"); // Force recreation for resized images
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
        let worker_cancel = Arc::new(AtomicBool::new(false));
        let worker_pause = Arc::new(AtomicBool::new(false));

        let worker_cancel_clone = worker_cancel.clone();
        let worker_pause_clone = worker_pause.clone();

        let ctx_clone = cc.egui_ctx.clone();
        // Spawn worker thread
        std::thread::spawn(move || {
            let cancel = worker_cancel_clone;
            let pause = worker_pause_clone;
            let mut cached_library_items: Vec<LibraryItem> = Vec::new();

            let check_status = || {
                while pause.load(Ordering::SeqCst) {
                    if cancel.load(Ordering::SeqCst) { return true; }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
                cancel.load(Ordering::SeqCst)
            };

            let mut client = match EgsClient::new() {
                Ok(mut c) => {
                    if let Some(config_dir) = crate::auth::get_config_dir() {
                        c.set_save_token_path(config_dir.join("user.json"));
                    }
                    c
                }
                Err(e) => {
                    let _ = tx.send(WorkerResponse::Error(format!("Failed to initialize client: {}", e)));
                    return;
                }
            };

            // Try initial load
            if let Ok(saved_token) = crate::auth::load_token() {
                client.set_token(&saved_token);
                match client.get_library_items() {
                    Ok(items) => {
                        cached_library_items = items.clone();
                        let _ = tx.send(WorkerResponse::LoggedIn(saved_token));
                        let _ = tx.send(WorkerResponse::LibraryFetched(items));
                        ctx_clone.request_repaint();
                    }
                    Err(e) => {
                        log::error!("Initial library fetch failed: {}", e);
                        // refresh_if_needed is called inside get_library_items, so if it still fails here, it might be fatal or need re-login
                    }
                }
            }

            while let Ok(msg) = worker_rx.recv() {
                cancel.store(false, Ordering::SeqCst);
                pause.store(false, Ordering::SeqCst);
                match msg {
                    WorkerMsg::CancelTask => {
                        continue;
                    }
                    WorkerMsg::PauseTask => {
                        continue;
                    }
                    WorkerMsg::ResumeTask => {
                        continue;
                    }
                    WorkerMsg::Login(code) => {
                        let _ = tx.send(WorkerResponse::Error("Logging in...".to_string()));
                        match client.start_session(&code) {
                            Ok(token) => {
                                let _ = crate::auth::save_token(&token);
                                let _ = tx.send(WorkerResponse::LoggedIn(token));
                                // Fetch library immediately after login
                                if let Ok(items) = client.get_library_items() {
                                    cached_library_items = items.clone();
                                    let _ = tx.send(WorkerResponse::LibraryFetched(items));
                                    ctx_clone.request_repaint();
                                }
                            }
                            Err(e) => { let _ = tx.send(WorkerResponse::Error(e.to_string())); }
                        }
                    }
                    WorkerMsg::LoginSid(sid) => {
                        let _ = tx.send(WorkerResponse::Error("Logging in with SID...".to_string()));
                        match client.start_session_with_sid(&sid) {
                            Ok(token) => {
                                let _ = crate::auth::save_token(&token);
                                let _ = tx.send(WorkerResponse::LoggedIn(token));
                                // Fetch library immediately after login
                                if let Ok(items) = client.get_library_items() {
                                    cached_library_items = items.clone();
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
                                    // Resize by 2x before caching
                                    if let Ok(img) = image::load_from_memory(&bytes) {
                                        let resized = img.resize(img.width() / 2, img.height() / 2, image::imageops::FilterType::Triangle);
                                        let mut buf = std::io::Cursor::new(Vec::new());
                                        if resized.write_to(&mut buf, image::ImageFormat::Png).is_ok() {
                                            let resized_bytes = buf.into_inner();
                                            if let Some(ref path) = cache_path {
                                                let _ = std::fs::write(path, &resized_bytes);
                                            }
                                            image_bytes = Some(resized_bytes);
                                        }
                                    }

                                    if image_bytes.is_none() {
                                        if let Some(ref path) = cache_path {
                                            let _ = std::fs::write(path, &bytes);
                                        }
                                        image_bytes = Some(bytes.to_vec());
                                    }
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
                                cached_library_items = items.clone();
                                let _ = tx.send(WorkerResponse::LibraryFetched(items));
                                ctx_clone.request_repaint();
                            }
                            Err(e) => { let _ = tx.send(WorkerResponse::Error(e.to_string())); }
                        }
                    }
                    WorkerMsg::FetchGameInfo { app_name, namespace, catalog_item_id } => {
                        match client.get_game_info(&namespace, &catalog_item_id) {
                            Ok(info) => {
                                // Save metadata
                                let meta = crate::models::LocalGameMetadata {
                                    app_name: app_name.clone(),
                                    app_title: info.title.clone(),
                                    metadata: crate::models::LocalMetadataDetails {
                                        id: info.id.clone(),
                                        namespace: info.namespace.clone(),
                                        deployment_id: None, // Will be filled by FetchInstallInfo/Verify
                                        developer: None, // Not available in GameInfo
                                        key_images: info.key_images.clone(),
                                        dlc_item_list: None,
                                        custom_attributes: info.custom_attributes.clone(),
                                        release_info: None,
                                    }
                                };
                                let _ = crate::auth::save_local_metadata(&app_name, &meta);

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
                        let _ = client.invalidate_session();
                        let _ = crate::auth::get_config_dir().map(|d| {
                            let _ = std::fs::remove_file(d.join("user.json"));
                            let _ = std::fs::remove_file(d.join("token.json"));
                        });
                    }
                    WorkerMsg::VerifyGame { app_name, catalog_item_id } => {
                        let _ = tx.send(WorkerResponse::TaskProgress { task_name: format!("Verifying {}", app_name), progress: 0.0, is_paused: false, speed: None, eta: None });
                        let mut installed = crate::auth::load_installed_games();
                        let game_idx = installed.iter().position(|g| g.app_name == app_name);

                        if let Some(idx) = game_idx {
                            let game = installed[idx].clone();
                            let mut manifest_opt = None;
                            let mut manifest_path_opt = game.manifest_path.as_ref().map(std::path::PathBuf::from);

                            if manifest_path_opt.as_ref().map(|p| !p.exists()).unwrap_or(true) {
                                manifest_path_opt = crate::auth::get_manifest_path(&app_name, &catalog_item_id);
                            }

                            if let Some(manifest_path) = manifest_path_opt {
                                let _ = tx.send(WorkerResponse::TaskProgress { task_name: format!("Reading manifest at {:?}", manifest_path), progress: 0.0, is_paused: false, speed: None, eta: None });
                                match std::fs::read(&manifest_path) {
                                    Ok(data) => {
                                        match crate::manifest::parse_manifest(&data) {
                                            Ok(m) => manifest_opt = Some(m),
                                            Err(e) => log::error!("Failed to parse manifest at {:?}: {}", manifest_path, e),
                                        }
                                    }
                                    Err(e) => log::error!("Failed to read manifest at {:?}: {}", manifest_path, e),
                                }
                            }

                            if manifest_opt.is_none() {
                                // Try to download manifest
                                let _ = tx.send(WorkerResponse::TaskProgress { task_name: format!("Manifest not found, fetching for {}", app_name), progress: 0.0, is_paused: false, speed: None, eta: None });
                                match client.get_game_assets(&game.platform) {
                                    Ok(assets) => {
                                        if let Some(asset) = assets.iter().find(|a| a.app_name == app_name || a.catalog_item_id == catalog_item_id) {
                                            match client.get_asset_manifest(&game.platform, &asset.namespace, &asset.catalog_item_id, &asset.app_name, &asset.label_name) {
                                                Ok(manifest_info) => {
                                                    // Update deployment_id in metadata if possible
                                                    if let Some(did) = extract_deployment_id(&manifest_info) {
                                                        if let Some(mut meta) = crate::auth::load_local_metadata(&app_name) {
                                                            meta.metadata.deployment_id = Some(did);
                                                            let _ = crate::auth::save_local_metadata(&app_name, &meta);
                                                        }
                                                    }

                                                    if let Some(url) = find_manifest_url(&manifest_info) {
                                                        match client.download_manifest(&url, Some(app_name.as_str())) {
                                                            Ok(manifest_data) => {
                                                                // Save manifest
                                                                let mut manifest_path_saved = None;
                                                                if let Some(mut p) = crate::auth::get_config_dir() {
                                                                    p.push("manifests");
                                                                    let _ = std::fs::create_dir_all(&p);
                                                                    let manifest_path = p.join(format!("{}.manifest", app_name));
                                                                    if std::fs::write(&manifest_path, &manifest_data).is_ok() {
                                                                        manifest_path_saved = Some(manifest_path.to_string_lossy().to_string());
                                                                    }
                                                                }
                                                                if let Some(mps) = manifest_path_saved {
                                                                    installed[idx].manifest_path = Some(mps);
                                                                    let _ = crate::auth::save_installed_games(&installed);
                                                                }

                                                                match crate::manifest::parse_manifest(&manifest_data) {
                                                                    Ok(m) => manifest_opt = Some(m),
                                                                    Err(e) => log::error!("Failed to parse downloaded manifest for {}: {}", app_name, e),
                                                                }
                                                            }
                                                            Err(e) => log::error!("Failed to download manifest for {}: {}", app_name, e),
                                                        }
                                                    } else {
                                                        log::error!("Manifest URL not found in asset manifest for {}", app_name);
                                                    }
                                                }
                                                Err(e) => log::error!("Failed to fetch asset manifest for {}: {}", app_name, e),
                                            }
                                        } else {
                                            log::error!("Asset not found for {} on platform {}", app_name, game.platform);
                                        }
                                    }
                                    Err(e) => log::error!("Failed to fetch assets for platform {}: {}", game.platform, e),
                                }
                            }

                            let game_dir = std::path::Path::new(&game.install_path);

                            if let Some(manifest) = manifest_opt {
                                let total_files = manifest.files.len();
                                let mut verified = 0;
                                let mut mismatches = 0;
                                let mut missing = 0;

                                for (i, (filename, info)) in manifest.files.iter().enumerate() {
                                    if check_status() {
                                        let _ = tx.send(WorkerResponse::TaskFinished("Verification cancelled".to_string()));
                                        break;
                                    }

                                    let file_path = game_dir.join(filename);
                                    if file_path.exists() {
                                        if let Ok(actual_hash) = crate::utils::hash_file(&file_path) {
                                            let expected_hash = hex::encode(&info.hash);
                                            if actual_hash == expected_hash {
                                                verified += 1;
                                            } else {
                                                mismatches += 1;
                                                log::warn!("Hash mismatch for {}: expected {}, got {}", filename, expected_hash, actual_hash);
                                            }
                                        } else {
                                            mismatches += 1;
                                        }
                                    } else {
                                        missing += 1;
                                        log::warn!("File missing: {}", filename);
                                    }

                                    if i % 10 == 0 {
                                        let progress = i as f32 / total_files as f32;
                                        let _ = tx.send(WorkerResponse::TaskProgress {
                                            task_name: format!("Verifying: {}", filename),
                                            progress,
                                            is_paused: pause.load(Ordering::SeqCst),
                                            speed: None,
                                            eta: None,
                                        });
                                    }
                                }
                                let _ = tx.send(WorkerResponse::TaskFinished(format!("Verification of {} complete. {} verified, {} mismatches, {} missing.", app_name, verified, mismatches, missing)));
                            } else {
                                let _ = tx.send(WorkerResponse::Error(format!("Manifest for {} not found and could not be downloaded", app_name)));
                            }
                        } else {
                            let _ = tx.send(WorkerResponse::Error(format!("Game {} not found in installed games", app_name)));
                        }
                        ctx_clone.request_repaint();
                    }
                    WorkerMsg::RepairGame(app_name, update) => {
                        let task_name = if update { format!("Repairing and Updating {}", app_name) } else { format!("Repairing {}", app_name) };
                        let _ = tx.send(WorkerResponse::TaskProgress { task_name: task_name.clone(), progress: 0.0, is_paused: false, speed: None, eta: None });
                        // Placeholder for repair logic
                        for i in 1..=10 {
                            if check_status() {
                                let _ = tx.send(WorkerResponse::TaskFinished(format!("Task '{}' cancelled", task_name)));
                                break;
                            }
                            std::thread::sleep(std::time::Duration::from_millis(300));
                            let _ = tx.send(WorkerResponse::TaskProgress { task_name: task_name.clone(), progress: i as f32 / 10.0, is_paused: pause.load(Ordering::SeqCst), speed: None, eta: None });
                        }
                        let _ = tx.send(WorkerResponse::TaskFinished(format!("Task '{}' complete", task_name)));
                        ctx_clone.request_repaint();
                    }
                    WorkerMsg::SyncCloudSaves { app_name, namespace, save_path } => {
                        let _ = tx.send(WorkerResponse::TaskProgress { task_name: format!("Checking cloud saves for {}", app_name), progress: 0.0, is_paused: false, speed: None, eta: None });
                        let local_time = save_path.as_ref().and_then(|p| get_latest_local_save_time(p));

                        if let Some(token) = crate::auth::load_token().ok() {
                            match client.get_cloud_save_metadata(&namespace, &token.account_id, &app_name) {
                                Ok(files) => {
                                    let mut remote_time = None;
                                    for file in &files {
                                        if let Ok(dt) = DateTime::parse_from_rfc3339(&file.last_modified) {
                                            let dt_utc = dt.with_timezone(&Utc);
                                            if remote_time.is_none() || dt_utc > remote_time.unwrap() {
                                                remote_time = Some(dt_utc);
                                            }
                                        }
                                    }

                                    let _ = tx.send(WorkerResponse::SaveSyncStatusFetched {
                                        app_name,
                                        files,
                                        local_time,
                                        remote_time,
                                        error: None,
                                    });
                                }
                                Err(e) => {
                                    let _ = tx.send(WorkerResponse::SaveSyncStatusFetched {
                                        app_name,
                                        files: Vec::new(),
                                        local_time,
                                        remote_time: None,
                                        error: Some(e.to_string()),
                                    });
                                }
                            }
                        } else {
                            let _ = tx.send(WorkerResponse::SaveSyncStatusFetched {
                                app_name,
                                files: Vec::new(),
                                local_time,
                                remote_time: None,
                                error: Some("No authentication token found".to_string()),
                            });
                        }
                        ctx_clone.request_repaint();
                    }
                    WorkerMsg::UploadCloudSave { app_name, namespace, save_path } => {
                        let _ = tx.send(WorkerResponse::TaskProgress { task_name: format!("Uploading saves for {}", app_name), progress: 0.0, is_paused: false, speed: None, eta: None });
                        if let Ok(token) = crate::auth::load_token() {
                            let files = get_all_files(&save_path);
                            let total = files.len();
                            let mut success = true;
                            for (i, file_path) in files.iter().enumerate() {
                                if let Ok(data) = std::fs::read(file_path) {
                                    let rel_path = file_path.strip_prefix(&save_path).unwrap_or(file_path);
                                    let filename = rel_path.to_string_lossy().to_string();
                                    match client.upload_cloud_file(&namespace, &token.account_id, &app_name, &filename, data) {
                                        Ok(_) => {},
                                        Err(e) => {
                                            let _ = tx.send(WorkerResponse::Error(format!("Failed to upload {}: {}", filename, e)));
                                            success = false;
                                            break;
                                        }
                                    }
                                }
                                let _ = tx.send(WorkerResponse::TaskProgress { task_name: format!("Uploading saves for {}", app_name), progress: (i + 1) as f32 / total as f32, is_paused: false, speed: None, eta: None });
                            }
                            if success {
                                let _ = tx.send(WorkerResponse::TaskFinished(format!("Upload for {} complete. {} files uploaded.", app_name, total)));
                            }
                        } else {
                            let _ = tx.send(WorkerResponse::Error("Not logged in".to_string()));
                        }
                        ctx_clone.request_repaint();
                    }
                    WorkerMsg::DownloadCloudSave { app_name, namespace, save_path } => {
                        let _ = tx.send(WorkerResponse::TaskProgress { task_name: format!("Downloading saves for {}", app_name), progress: 0.0, is_paused: false, speed: None, eta: None });
                        if let Ok(token) = crate::auth::load_token() {
                            match client.get_cloud_save_metadata(&namespace, &token.account_id, &app_name) {
                                Ok(files) => {
                                    let total = files.len();
                                    let mut success = true;
                                    for (i, file) in files.iter().enumerate() {
                                        match client.download_cloud_file(&namespace, &token.account_id, &app_name, &file.file_name) {
                                            Ok(data) => {
                                                let target_path = save_path.join(&file.file_name);
                                                if let Some(parent) = target_path.parent() {
                                                    let _ = std::fs::create_dir_all(parent);
                                                }
                                                if let Err(e) = std::fs::write(&target_path, data) {
                                                    let _ = tx.send(WorkerResponse::Error(format!("Failed to write {}: {}", file.file_name, e)));
                                                    success = false;
                                                    break;
                                                }
                                            }
                                            Err(e) => {
                                                let _ = tx.send(WorkerResponse::Error(format!("Failed to download {}: {}", file.file_name, e)));
                                                success = false;
                                                break;
                                            }
                                        }
                                        let _ = tx.send(WorkerResponse::TaskProgress { task_name: format!("Downloading saves for {}", app_name), progress: (i + 1) as f32 / total as f32, is_paused: false, speed: None, eta: None });
                                    }
                                    if success {
                                        let _ = tx.send(WorkerResponse::TaskFinished(format!("Download for {} complete. {} files downloaded.", app_name, total)));
                                    }
                                }
                                Err(e) => {
                                    let _ = tx.send(WorkerResponse::Error(format!("Failed to get cloud metadata: {}", e)));
                                }
                            }
                        } else {
                            let _ = tx.send(WorkerResponse::Error("Not logged in".to_string()));
                        }
                        ctx_clone.request_repaint();
                    }
                    WorkerMsg::FetchInstallInfo { app_name, title } => {
                        let _ = tx.send(WorkerResponse::TaskProgress { task_name: format!("Fetching install info for {}", app_name), progress: 0.0, is_paused: false, speed: None, eta: None });

                        let mut available_tags = Vec::new();

                        let asset_info = if app_name == crate::eos::EOS_OVERLAY_APP_ID {
                            Some(("Windows".to_string(), crate::eos::EOS_OVERLAY_NAMESPACE.to_string(), crate::eos::EOS_OVERLAY_CATALOG_ID.to_string(), app_name.clone(), "Live".to_string()))
                        } else {
                            if let Ok(assets) = client.get_game_assets("Windows") {
                                assets.iter().find(|a| a.app_name == app_name).map(|asset| {
                                    ("Windows".to_string(), asset.namespace.clone(), asset.catalog_item_id.clone(), asset.app_name.clone(), asset.label_name.clone())
                                })
                            } else { None }
                        };

                        if let Some((plat, namespace, catalog_id, app, label)) = asset_info {
                            if let Ok(manifest_info) = client.get_asset_manifest(&plat, &namespace, &catalog_id, &app, &label) {
                                // Update deployment_id in metadata if possible
                                if let Some(did) = extract_deployment_id(&manifest_info) {
                                    if let Some(mut meta) = crate::auth::load_local_metadata(&app_name) {
                                        meta.metadata.deployment_id = Some(did);
                                        let _ = crate::auth::save_local_metadata(&app_name, &meta);
                                    }
                                }

                                if let Some(url) = find_manifest_url(&manifest_info) {
                                    if let Ok(manifest_data) = client.download_manifest(&url, Some(app_name.as_str())) {
                                        if let Ok(manifest) = crate::manifest::parse_manifest(&manifest_data) {
                                            let mut tags = HashSet::new();
                                            for file in manifest.files.values() {
                                                for tag in &file.install_tags {
                                                    tags.insert(tag.clone());
                                                }
                                            }
                                            available_tags = tags.into_iter().collect();
                                            available_tags.sort();
                                        }
                                    }
                                }
                            }
                        }

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
                            available_tags,
                        };
                        let _ = tx.send(WorkerResponse::InstallInfoFetched(info));
                        ctx_clone.request_repaint();
                    }
                    WorkerMsg::InstallGame { app_name, install_path, selected_tags, platform } => {
                        let _ = tx.send(WorkerResponse::TaskProgress { task_name: format!("Preparing installation for {}", app_name), progress: 0.0, is_paused: false, speed: None, eta: None });

                        // Actual implementation: Create directory
                        if let Err(e) = std::fs::create_dir_all(&install_path) {
                            let _ = tx.send(WorkerResponse::Error(format!("Failed to create directory: {}", e)));
                            continue;
                        }

                        // Try to get manifest URL
                        let _ = tx.send(WorkerResponse::TaskProgress { task_name: format!("Fetching manifest for {}", app_name), progress: 0.05, is_paused: false, speed: None, eta: None });

                        let mut manifest_data_opt = None;
                        let mut base_url_opt = None;
                        let mut manifest_path_saved = None;
                        let mut version = "1.0.0".to_string();
                        let mut install_size = 0u64;
                        let mut download_size = 0u64;

                        let asset_info = if app_name == crate::eos::EOS_OVERLAY_APP_ID {
                            Some(("Windows".to_string(), crate::eos::EOS_OVERLAY_NAMESPACE.to_string(), crate::eos::EOS_OVERLAY_CATALOG_ID.to_string(), app_name.clone(), "Live".to_string()))
                        } else {
                            match client.get_game_assets(&platform) {
                                Ok(assets) => {
                                    assets.iter().find(|a| a.app_name == app_name).map(|asset| {
                                        version = asset.build_version.clone();
                                        (platform.clone(), asset.namespace.clone(), asset.catalog_item_id.clone(), asset.app_name.clone(), asset.label_name.clone())
                                    })
                                }
                                Err(e) => {
                                    log::error!("Failed to fetch assets for platform {}: {}", platform, e);
                                    None
                                }
                            }
                        };

                        if let Some((plat, namespace, catalog_id, app, label)) = asset_info {
                            match client.get_asset_manifest(&plat, &namespace, &catalog_id, &app, &label) {
                                Ok(manifest_info) => {
                                    // Update deployment_id in metadata if possible
                                    if let Some(did) = extract_deployment_id(&manifest_info) {
                                        if let Some(mut meta) = crate::auth::load_local_metadata(&app_name) {
                                            meta.metadata.deployment_id = Some(did);
                                            let _ = crate::auth::save_local_metadata(&app_name, &meta);
                                        }
                                    }

                                    if let Some(url) = find_manifest_url(&manifest_info) {
                                        match client.download_manifest(&url, Some(app_name.as_str())) {
                                            Ok(data) => {
                                                // Save manifest
                                                if let Some(mut p) = crate::auth::get_config_dir() {
                                                    p.push("manifests");
                                                    let _ = std::fs::create_dir_all(&p);
                                                    let manifest_path = p.join(format!("{}.manifest", app_name));
                                                    if std::fs::write(&manifest_path, &data).is_ok() {
                                                        manifest_path_saved = Some(manifest_path.to_string_lossy().to_string());
                                                    }
                                                }
                                                manifest_data_opt = Some(data);
                                                base_url_opt = Some(url.split('?').next().unwrap_or(&url).rsplit_once('/').map(|(b, _)| b.to_string()).unwrap_or_else(|| url.to_string()));
                                            }
                                            Err(e) => log::error!("Failed to download manifest for {}: {}", app_name, e),
                                        }
                                    } else {
                                        log::error!("Manifest URL not found in asset manifest for {}", app_name);
                                    }
                                }
                                Err(e) => log::error!("Failed to fetch asset manifest for {}: {}", app_name, e),
                            }
                        } else if app_name != crate::eos::EOS_OVERLAY_APP_ID {
                            log::error!("Asset not found for {} on platform {}", app_name, platform);
                        }

                        let mut success = false;
                        let mut executable = String::new();
                        if let (Some(manifest_data), Some(base_url)) = (manifest_data_opt, base_url_opt) {
                            if let Ok(manifest) = crate::manifest::parse_manifest(&manifest_data) {
                                install_size = manifest.total_uncompressed_size;
                                download_size = manifest.total_download_size;
                                executable = manifest.meta.launch_exe.clone();
                                let user_agent = crate::api::get_ua_for_app(&app_name).to_string();
                                let downloader = crate::download::Downloader::new(base_url, user_agent, tx.clone(), cancel.clone(), pause.clone());
                                match downloader.download_game(&manifest, &install_path, selected_tags) {
                                    Ok(_) => success = true,
                                    Err(e) => {
                                        let _ = tx.send(WorkerResponse::Error(format!("Download failed: {}", e)));
                                    }
                                }
                            } else {
                                let _ = tx.send(WorkerResponse::Error("Failed to parse manifest".to_string()));
                            }
                        } else {
                            let _ = tx.send(WorkerResponse::Error("Failed to fetch manifest".to_string()));
                        }

                        if success {
                            // Update installed.json
                            let mut installed = crate::auth::load_installed_games();
                            let title = crate::auth::load_local_metadata(&app_name)
                                .map(|m| m.app_title)
                                .unwrap_or_else(|| app_name.clone());

                            installed.push(crate::models::InstalledGame {
                                app_name: app_name.clone(),
                                install_path: install_path.to_string_lossy().to_string(),
                                title: title.clone(),
                                version,
                                executable,
                                install_size,
                                download_size,
                                platform: platform.clone(),
                                manifest_path: manifest_path_saved,
                            });
                            let _ = crate::auth::save_installed_games(&installed);

                            let _ = tx.send(WorkerResponse::TaskFinished(format!("Installation of {} complete", app_name)));
                            let _ = tx.send(WorkerResponse::GamesScanned(installed));
                        }

                        ctx_clone.request_repaint();
                    }
                    WorkerMsg::UninstallGame(app_name) => {
                        let _ = tx.send(WorkerResponse::TaskProgress { task_name: format!("Uninstalling {}", app_name), progress: 0.0, is_paused: false, speed: None, eta: None });

                        let mut installed = crate::auth::load_installed_games();
                        if let Some(game) = installed.iter().find(|g| g.app_name == app_name).cloned() {
                            let path = std::path::Path::new(&game.install_path);
                            if path.exists() {
                                let _ = tx.send(WorkerResponse::TaskProgress { task_name: format!("Deleting files for {}", app_name), progress: 0.5, is_paused: false, speed: None, eta: None });
                                if let Err(e) = std::fs::remove_dir_all(path) {
                                    let _ = tx.send(WorkerResponse::Error(format!("Failed to delete game files: {}", e)));
                                }
                            }
                        }

                        installed.retain(|g| g.app_name != app_name);
                        let _ = crate::auth::save_installed_games(&installed);

                        let _ = tx.send(WorkerResponse::GamesScanned(installed));
                        let _ = tx.send(WorkerResponse::TaskFinished(format!("Uninstalled {}", app_name)));
                        ctx_clone.request_repaint();
                    }
                    WorkerMsg::ScanGames { library, search_paths } => {
                        let _ = tx.send(WorkerResponse::TaskProgress { task_name: "Scanning for games...".to_string(), progress: 0.0, is_paused: false, speed: None, eta: None });
                        let installed = crate::auth::scan_and_import_games(&library, &search_paths);
                        let _ = tx.send(WorkerResponse::GamesScanned(installed));
                        let _ = tx.send(WorkerResponse::TaskFinished("Scan complete".to_string()));
                        ctx_clone.request_repaint();
                    }
                    WorkerMsg::EglSync => {
                        let _ = tx.send(WorkerResponse::TaskProgress { task_name: "Syncing with EGL...".to_string(), progress: 0.0, is_paused: false, speed: None, eta: None });
                        let installed = crate::auth::scan_egl_manifests();
                        let _ = tx.send(WorkerResponse::GamesScanned(installed));
                        let _ = tx.send(WorkerResponse::TaskFinished("EGL Sync complete".to_string()));
                        ctx_clone.request_repaint();
                    }
                    WorkerMsg::ListFiles { app_name, catalog_item_id } => {
                        let _ = tx.send(WorkerResponse::TaskProgress { task_name: format!("Listing files for {}", app_name), progress: 0.0, is_paused: false, speed: None, eta: None });

                        let manifest_path_opt = crate::auth::get_manifest_path(&app_name, &catalog_item_id);
                        let mut manifest_opt = None;

                        if let Some(manifest_path) = manifest_path_opt {
                            if let Ok(data) = std::fs::read(&manifest_path) {
                                manifest_opt = crate::manifest::parse_manifest(&data).ok();
                            }
                        }

                        if manifest_opt.is_none() {
                             if app_name == crate::eos::EOS_OVERLAY_APP_ID {
                                 if let Ok(manifest_info) = client.get_asset_manifest("Windows", crate::eos::EOS_OVERLAY_NAMESPACE, crate::eos::EOS_OVERLAY_CATALOG_ID, &app_name, "Live") {
                                     if let Some(url) = find_manifest_url(&manifest_info) {
                                         if let Ok(manifest_data) = client.download_manifest(&url, Some(app_name.as_str())) {
                                             manifest_opt = crate::manifest::parse_manifest(&manifest_data).ok();
                                         }
                                     }
                                 }
                             } else {
                                 // Try common platforms if not found
                                 for platform in &["Windows", "Mac", "Linux"] {
                                    if let Ok(assets) = client.get_game_assets(platform) {
                                        if let Some(asset) = assets.iter().find(|a| a.app_name == app_name) {
                                            if let Ok(manifest_info) = client.get_asset_manifest(platform, &asset.namespace, &asset.catalog_item_id, &asset.app_name, &asset.label_name) {
                                                if let Some(url) = find_manifest_url(&manifest_info) {
                                                    if let Ok(manifest_data) = client.download_manifest(&url, Some(app_name.as_str())) {
                                                        manifest_opt = crate::manifest::parse_manifest(&manifest_data).ok();
                                                        if manifest_opt.is_some() { break; }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                 }
                             }
                        }

                        if let Some(manifest) = manifest_opt {
                            let files = manifest.list_files();
                            let _ = tx.send(WorkerResponse::FilesListed(files));
                            let _ = tx.send(WorkerResponse::TaskFinished(format!("Listed files for {}", app_name)));
                        } else {
                            let _ = tx.send(WorkerResponse::Error(format!("Could not find manifest for {}", app_name)));
                        }
                        ctx_clone.request_repaint();
                    }
                    WorkerMsg::LaunchGame { app_name, offline } => {
                        println!("--- Launching Game: {} ---", app_name);
                        println!("[1/7] Loading installed game metadata...");
                        let installed_games = crate::auth::load_installed_games();
                        let installed = installed_games.iter().find(|g| g.app_name == app_name).cloned();

                        if let Some(installed) = installed {
                            println!("[2/7] Checking configuration and authentication...");
                            let local_meta = crate::auth::load_local_metadata(&app_name);
                            let path = std::path::PathBuf::from(&installed.install_path);
                            let config = AppConfig::load();
                            let mut found = false;

                            let game_settings = config.games.get(&app_name);

                            // Check if game can run offline
                            let can_run_offline = local_meta.as_ref()
                                .and_then(|m| m.metadata.custom_attributes.as_ref())
                                .and_then(|attrs| attrs.get("CanRunOffline"))
                                .map(|a| a.value.to_lowercase() == "true")
                                .unwrap_or(true);

                            let mut token = "0".to_string();
                            let mut user_id = client.get_account_id().unwrap_or_default();
                            let mut display_name = client.get_display_name();

                            if !offline {
                                println!("      Fetching game token...");
                                match client.get_game_token() {
                                    Ok(t) => {
                                        token = t;
                                        user_id = client.get_account_id().unwrap_or_default();
                                        display_name = client.get_display_name();
                                    }
                                    Err(e) => {
                                        println!("ERROR: Failed to fetch game token: {}", e);
                                        let _ = tx.send(WorkerResponse::Error(format!("Failed to fetch game token: {}", e)));
                                        continue;
                                    }
                                }
                            } else {
                                println!("      Launching in offline mode.");
                            }

                            if token == "0" && !can_run_offline {
                                println!("ERROR: This game cannot run offline and no token was provided.");
                                let _ = tx.send(WorkerResponse::Error(
                                    "This game cannot run offline and no token was provided".to_string()
                                ));
                                continue;
                            }

                            // [3/7] Cloud Save Sync
                            let sync_enabled = game_settings.map(|s| s.cloud_sync_enabled).unwrap_or(true);
                            if !offline && sync_enabled {
                                println!("[3/7] Checking cloud saves...");
                                if let (Some(token), Some(lib_item)) = (crate::auth::load_token().ok(), cached_library_items.iter().find(|i| i.app_name == app_name)) {
                                    let save_path = game_settings.and_then(|s| s.save_path.clone());
                                    if let Some(sp) = save_path {
                                        let local_time = get_latest_local_save_time(&sp);
                                        match client.get_cloud_save_metadata(&lib_item.namespace, &token.account_id, &app_name) {
                                            Ok(files) => {
                                                let mut remote_time = None;
                                                for file in &files {
                                                    if let Ok(dt) = DateTime::parse_from_rfc3339(&file.last_modified) {
                                                        let dt_utc = dt.with_timezone(&Utc);
                                                        if remote_time.is_none() || dt_utc > remote_time.unwrap() {
                                                            remote_time = Some(dt_utc);
                                                        }
                                                    }
                                                }

                                                match (local_time, remote_time) {
                                                    (Some(l), Some(r)) => {
                                                        if l > r {
                                                            println!("      Local save is newer. You might want to upload it.");
                                                        } else if r > l {
                                                            println!("      Cloud save is newer. You might want to download it.");
                                                        } else {
                                                            println!("      Cloud and local saves are in sync.");
                                                        }
                                                    }
                                                    (None, Some(_)) => println!("      Cloud save found, but no local save."),
                                                    (Some(_), None) => println!("      Local save found, but no cloud save."),
                                                    (None, None) => println!("      No cloud or local saves found."),
                                                }
                                            }
                                            Err(e) => println!("      Warning: Failed to fetch cloud save metadata: {}", e),
                                        }
                                    } else {
                                        println!("      Save path not configured, skipping sync check.");
                                    }
                                }
                            } else {
                                println!("[3/7] Cloud sync skipped (offline or disabled).");
                            }

                            // Pre-launch command
                            let pre_launch = if let Some(gs) = game_settings {
                                if !gs.pre_launch_command.is_empty() { Some(&gs.pre_launch_command) }
                                else if !config.global.pre_launch_command.is_empty() { Some(&config.global.pre_launch_command) }
                                else { None }
                            } else if !config.global.pre_launch_command.is_empty() {
                                Some(&config.global.pre_launch_command)
                            } else {
                                None
                            };

                            println!("[4/7] Running pre-launch command...");
                            if let Some(cmd_str) = pre_launch {
                                println!("      Command: {}", cmd_str);
                                log::info!("Running pre-launch command: {}", cmd_str);
                                let mut parts = cmd_str.split_whitespace();
                                if let Some(program) = parts.next() {
                                    let mut child = std::process::Command::new(program);
                                    for arg in parts {
                                        child.arg(arg);
                                    }
                                    match child.spawn().and_then(|mut c| c.wait()) {
                                        Ok(status) => {
                                            if !status.success() {
                                                println!("ERROR: Pre-launch command failed with status: {}", status);
                                                log::error!("Pre-launch command failed with status: {}", status);
                                            } else {
                                                println!("      Pre-launch command finished successfully.");
                                            }
                                        }
                                        Err(e) => {
                                            println!("ERROR: Failed to run pre-launch command: {}", e);
                                            log::error!("Failed to run pre-launch command: {}", e);
                                        }
                                    }
                                }
                            } else {
                                println!("      No pre-launch command configured.");
                            }

                            println!("[5/7] Searching for game executables...");
                            let custom_exe = game_settings.and_then(|s| s.custom_exe_path.clone());

                            let mut possible_exes = Vec::new();

                            // If custom exe is specified, use only that
                            if let Some(ce) = custom_exe {
                                println!("      Using custom executable: {:?}", ce);
                                possible_exes.push(ce);
                            } else {
                                // Build list of possible executable names
                                let mut possible_names = vec![app_name.clone()];

                                // Add names from metadata
                                if let Some(meta) = &local_meta {
                                    if let Some(attrs) = &meta.metadata.custom_attributes {
                                        if let Some(folder) = attrs.get("FolderName") {
                                            possible_names.push(folder.value.clone());
                                        }
                                    }
                                }

                                // If we have an executable from the installed game info, prioritize it
                                if !installed.executable.is_empty() {
                                    let exe_path = path.join(installed.executable.replace('\\', "/").trim_start_matches('/'));
                                    if exe_path.exists() {
                                        possible_exes.push(exe_path);
                                    }
                                }

                                // Search for executables based on possible names
                                for name in possible_names {
                                    for ext in &["exe", "sh", ""] {
                                        let filename = if ext.is_empty() {
                                            name.clone()
                                        } else {
                                            format!("{}.{}", name, ext)
                                        };

                                        let exe_path = path.join(&filename);
                                        if exe_path.exists() && !possible_exes.contains(&exe_path) {
                                            possible_exes.push(exe_path);
                                        }
                                    }
                                }

                                // If still nothing found, scan the install directory for any .exe files
                                if possible_exes.is_empty() {
                                    println!("      No executables found using standard names, scanning directory...");
                                    log::warn!("No executables found using standard names, scanning directory...");
                                    if let Ok(entries) = std::fs::read_dir(&path) {
                                        for entry in entries.flatten() {
                                            let entry_path = entry.path();
                                            if entry_path.is_file() {
                                                if let Some(ext) = entry_path.extension() {
                                                    if ext == "exe" {
                                                        let filename = entry_path.file_name()
                                                            .unwrap_or_default()
                                                            .to_string_lossy()
                                                            .to_lowercase();
                                                        // Skip known non-game executables
                                                        if !filename.contains("crash") &&
                                                           !filename.contains("unins") &&
                                                           !filename.contains("redist") &&
                                                           !filename.contains("prereq") {
                                                            possible_exes.push(entry_path);
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }

                            println!("      Possible executables: {:?}", possible_exes);
                            log::info!("Possible executables: {:?}", possible_exes);

                            let lib_item = cached_library_items.iter().find(|i| i.app_name == app_name);
                            let namespace = lib_item.map(|i| i.namespace.clone())
                                .or_else(|| local_meta.as_ref().map(|m| m.metadata.namespace.clone()));

                            println!("[6/7] Handling ownership token and launch parameters...");
                            // Check if game requires ownership token
                            let mut requires_ot = local_meta.as_ref()
                                .and_then(|m| m.metadata.custom_attributes.as_ref())
                                .and_then(|attrs| attrs.get("OwnershipToken"))
                                .map(|a| a.value.to_lowercase() == "true")
                                .unwrap_or(false);

                            if !requires_ot {
                                if let Some(item) = lib_item {
                                    if let Some(meta) = &item.metadata {
                                        requires_ot = meta["customAttributes"]["OwnershipToken"]["value"].as_str()
                                            .map(|v| v.to_lowercase() == "true")
                                            .unwrap_or(false);
                                    }
                                }
                            }

                            let deployment_id = local_meta.as_ref().and_then(|m| m.metadata.deployment_id.clone());

                            let mut ovt_path_opt = None;

                            // Get ownership token if needed and not offline
                            if !offline {
                                if let Some(item) = lib_item {
                                    if requires_ot {
                                        println!("      Fetching ownership token...");
                                        log::info!("Fetching ownership token for {}...", app_name);
                                        match client.get_ownership_token(&item.namespace, &item.catalog_item_id) {
                                            Ok(ovt_bytes) => {
                                                let ovt_path = std::env::temp_dir()
                                                    .join(format!("{}{}.ovt", item.namespace, item.catalog_item_id));
                                                match std::fs::write(&ovt_path, &ovt_bytes) {
                                                    Ok(_) => {
                                                        println!("      Ownership token saved to {:?}", ovt_path);
                                                        log::info!("Saved ownership token to {:?}", ovt_path);
                                                        ovt_path_opt = Some(ovt_path);
                                                    }
                                                    Err(e) => {
                                                        println!("ERROR: Failed to save ownership token: {}", e);
                                                        log::error!("Failed to save ownership token: {}", e);
                                                    }
                                                }
                                            }
                                            Err(e) => {
                                                println!("WARNING: Failed to get ownership token: {}. Continuing without it...", e);
                                                log::warn!("Failed to get ownership token: {}. Continuing without it...", e);
                                            }
                                        }
                                    }
                                }
                            }

                            println!("[7/7] Launching game...");
                            if possible_exes.is_empty() {
                                println!("ERROR: No executables found for {}.", app_name);
                            }
                            // Try each possible executable
                            'search: for exe_path in possible_exes {
                                if !exe_path.exists() {
                                    continue;
                                }

                                println!("      Executable: {:?}", exe_path);
                                log::info!("Trying to launch: {:?}", exe_path);

                                let use_umu = game_settings.map(|s| s.use_umu).unwrap_or(config.global.use_umu);

                                let mut cmd = if use_umu && std::env::consts::OS == "linux" {
                                    let mut c = std::process::Command::new("/usr/bin/umu-run");
                                    let store = game_settings.and_then(|s| s.umu_store.clone())
                                        .unwrap_or_else(|| config.global.umu_store.clone());
                                    c.env("STORE", store);
                                    c.env("GAMEID", "umu-default");

                                    let pfx_path = if let Some(gs) = game_settings {
                                        if gs.use_custom_pfx { gs.custom_pfx_path.clone() }
                                        else if config.global.use_custom_pfx { config.global.custom_pfx_path.clone() }
                                        else { get_default_compat_data_path() }
                                    } else if config.global.use_custom_pfx {
                                        config.global.custom_pfx_path.clone()
                                    } else {
                                        get_default_compat_data_path()
                                    };

                                    if let Some(path) = pfx_path {
                                        c.env("WINEPREFIX", path);
                                    }

                                    if let Some(CompatibilityTool::SteamProton) | Some(CompatibilityTool::CustomProtonWine) =
                                        game_settings.and_then(|s| s.compatibility_tool.as_ref()) {
                                        if let Some(path) = game_settings.and_then(|s| s.custom_compatibility_path.as_ref()) {
                                            c.env("PROTONPATH", path);
                                        }
                                    }
                                    c.arg(&exe_path);
                                    c
                                } else if std::env::consts::OS == "linux" {
                                    let mut c = match game_settings.and_then(|s| s.compatibility_tool.as_ref()) {
                                        Some(CompatibilityTool::SteamProton) | Some(CompatibilityTool::CustomProtonWine) => {
                                            if let Some(path) = game_settings.and_then(|s| s.custom_compatibility_path.as_ref()) {
                                                let mut p = path.clone();
                                                p.push("proton");
                                                let is_proton = p.exists();
                                                if !is_proton {
                                                    p.pop();
                                                    p.push("bin/wine");
                                                }
                                                let mut command = std::process::Command::new(p);
                                                if is_proton {
                                                    let pfx_path = if let Some(gs) = game_settings {
                                                        if gs.use_custom_pfx { gs.custom_pfx_path.clone() }
                                                        else if config.global.use_custom_pfx { config.global.custom_pfx_path.clone() }
                                                        else { get_default_compat_data_path() }
                                                    } else if config.global.use_custom_pfx {
                                                        config.global.custom_pfx_path.clone()
                                                    } else {
                                                        get_default_compat_data_path()
                                                    };

                                                    if let Some(path) = pfx_path {
                                                        command.env("STEAM_COMPAT_DATA_PATH", path);
                                                    }
                                                    if let Some(home) = home::home_dir() {
                                                        command.env("STEAM_COMPAT_CLIENT_INSTALL_PATH",
                                                                   home.join(".local/share/Steam"));
                                                    }
                                                    command.arg("run");
                                                } else {
                                                    let pfx_path = if let Some(gs) = game_settings {
                                                        if gs.use_custom_pfx { gs.custom_pfx_path.clone() }
                                                        else if config.global.use_custom_pfx { config.global.custom_pfx_path.clone() }
                                                        else { None }
                                                    } else if config.global.use_custom_pfx {
                                                        config.global.custom_pfx_path.clone()
                                                    } else {
                                                        None
                                                    };
                                                    if let Some(path) = pfx_path {
                                                        command.env("WINEPREFIX", path);
                                                    }
                                                }
                                                command
                                            } else {
                                                let mut command = std::process::Command::new("wine");
                                                let pfx_path = if let Some(gs) = game_settings {
                                                    if gs.use_custom_pfx { gs.custom_pfx_path.clone() }
                                                    else if config.global.use_custom_pfx { config.global.custom_pfx_path.clone() }
                                                    else { None }
                                                } else if config.global.use_custom_pfx {
                                                    config.global.custom_pfx_path.clone()
                                                } else {
                                                    None
                                                };
                                                if let Some(path) = pfx_path {
                                                    command.env("WINEPREFIX", path);
                                                }
                                                command
                                            }
                                        }
                                        Some(CompatibilityTool::SystemWine) | None => {
                                            let mut command = std::process::Command::new("wine");
                                            let pfx_path = if let Some(gs) = game_settings {
                                                if gs.use_custom_pfx { gs.custom_pfx_path.clone() }
                                                else if config.global.use_custom_pfx { config.global.custom_pfx_path.clone() }
                                                else { None }
                                            } else if config.global.use_custom_pfx {
                                                config.global.custom_pfx_path.clone()
                                            } else {
                                                None
                                            };
                                            if let Some(path) = pfx_path {
                                                command.env("WINEPREFIX", path);
                                            }
                                            command
                                        }
                                    };
                                    c.arg(&exe_path);
                                    c
                                } else {
                                    std::process::Command::new(&exe_path)
                                };

                                // Set working directory to where the exe is
                                if let Some(parent) = exe_path.parent() {
                                    cmd.current_dir(parent);
                                }

                                // Epic arguments
                                cmd.arg("-AUTH_LOGIN=unused");
                                cmd.arg(format!("-AUTH_PASSWORD={}", token));
                                cmd.arg("-AUTH_TYPE=exchangecode");
                                cmd.arg(format!("-epicapp={}", app_name));
                                cmd.arg("-epicenv=Prod");
                                cmd.arg("-EpicPortal");
                                cmd.arg(format!("-epicuserid={}", user_id));
                                if let Some(dn) = &display_name {
                                    cmd.arg(format!("-epicusername={}", dn));
                                }
                                if let Some(ns) = &namespace {
                                    cmd.arg(format!("-epicsandboxid={}", ns));
                                }
                                if let Some(did) = &deployment_id {
                                    cmd.arg(format!("-epicdeploymentid={}", did));
                                }
                                cmd.arg(format!("-uid={}", user_id));
                                cmd.arg("-epiclocale=en");

                                if let Some(ovt_path) = &ovt_path_opt {
                                    cmd.arg(format!("-epicovt={}", ovt_path.display()));
                                }

                                let eos_installed = installed_games.iter().any(|g| g.app_name == crate::eos::EOS_OVERLAY_APP_ID);
                                let overlay_enabled = if let Some(gs) = game_settings {
                                    gs.eos_overlay_enabled
                                } else {
                                    config.global.eos_overlay_enabled
                                };

                                if !overlay_enabled || !eos_installed {
                                    cmd.env("EOS_OVERLAY_KILLED", "1");
                                }

                                if !offline {
                                    cmd.env("EPIC_AUTH_PASSWORD", &token);
                                    cmd.env("EPIC_AUTH_LOGIN", "unused");
                                    cmd.env("EPIC_AUTH_TYPE", "exchangecode");
                                    cmd.env("EPIC_USER_ID", &user_id);
                                    cmd.env("EPIC_ACCOUNT_ID", &user_id);
                                }
                                cmd.env("EpicApp", &app_name);
                                cmd.env("EpicEnv", "Prod");

                                // Additional environment variables and parameters...
                                if let Some(val) = game_settings.and_then(|s| s.steam_compat_install_path.as_ref())
                                    .or_else(|| config.global.steam_compat_install_path.as_ref()) {
                                    cmd.env("STEAM_COMPAT_INSTALL_PATH", val);
                                }
                                if let Some(val) = game_settings.and_then(|s| s.steam_compat_client_install_path.as_ref())
                                    .or_else(|| config.global.steam_compat_client_install_path.as_ref()) {
                                    cmd.env("STEAM_COMPAT_CLIENT_INSTALL_PATH", val);
                                }
                                if let Some(val) = game_settings.and_then(|s| s.steam_compat_data_path.as_ref())
                                    .or_else(|| config.global.steam_compat_data_path.as_ref()) {
                                    cmd.env("STEAM_COMPAT_DATA_PATH", val);
                                }
                                if let Some(val) = game_settings.and_then(|s| s.steam_compat_app_id.as_ref())
                                    .or_else(|| config.global.steam_compat_app_id.as_ref()) {
                                    cmd.env("STEAM_COMPAT_APP_ID", val);
                                }
                                cmd.env("APP_NAME", &app_name);

                                // Log launch info
                                println!("--- Launch Info ---");
                                println!("Executable: {:?}", exe_path);
                                let vars = ["GAMEID", "STORE", "STEAM_COMPAT_INSTALL_PATH", "LD_PRELOAD",
                                           "STEAM_COMPAT_CLIENT_INSTALL_PATH", "WINEPREFIX", "STEAM_COMPAT_DATA_PATH",
                                           "PROTONPATH", "STEAM_COMPAT_APP_ID", "APP_NAME"];
                                for var in vars {
                                    let val = cmd.get_envs().find(|(k, _)| k.to_str() == Some(var))
                                        .and_then(|(_, v)| v)
                                        .map(|v| v.to_string_lossy().into_owned())
                                        .or_else(|| std::env::var(var).ok())
                                        .unwrap_or_default();
                                    println!("{}: {}", var, val);
                                }
                                println!("-------------------");

                                if let Some(settings) = game_settings {
                                    if settings.play_offline {
                                        cmd.arg("-offline");
                                    }
                                    for param in settings.start_params.split_whitespace() {
                                        cmd.arg(param);
                                    }
                                }

                                match cmd.spawn() {
                                    Ok(child) => {
                                        println!("SUCCESS: Game launched successfully!");
                                        let _ = tx.send(WorkerResponse::GameLaunched {
                                            app_name: app_name.clone(),
                                            child
                                        });
                                        found = true;
                                        break 'search;
                                    }
                                    Err(e) => {
                                        println!("ERROR: Failed to launch game: {}", e);
                                        log::error!("Failed to spawn process for {:?}: {}", exe_path, e);
                                    }
                                }
                            }

                            if !found {
                                println!("ERROR: Failed to launch game {}: no executable found or all failed to start", app_name);
                                let _ = tx.send(WorkerResponse::Error(
                                    format!("Could not find or launch executable in {}", installed.install_path)
                                ));
                            }
                        }
                    }
                    WorkerMsg::QueryEosStatus { prefix } => {
                        let installed_games = crate::auth::load_installed_games();
                        let eos_installed = installed_games.iter().find(|g| g.app_name == crate::eos::EOS_OVERLAY_APP_ID);

                        let mut status = crate::eos::EosOverlayStatus {
                            installed: eos_installed.is_some(),
                            install_path: eos_installed.map(|g| g.install_path.clone()),
                            registry_path: None,
                            available_paths: Vec::new(),
                        };

                        let pref = prefix.or_else(get_default_compat_data_path);

                        if let Some(p) = pref {
                            status.registry_path = crate::eos::query_registry(&p);
                            status.available_paths = crate::eos::search_overlay_installs(Some(&p));
                        }

                        let _ = tx.send(WorkerResponse::EosStatusFetched(status));
                        ctx_clone.request_repaint();
                    }
                    WorkerMsg::UpdateEosRegistry { overlay_path, prefix, enable } => {
                        let res = if enable {
                            crate::eos::add_registry_entries(&overlay_path, &prefix)
                        } else {
                            crate::eos::remove_registry_entries(&prefix)
                        };

                        if let Err(e) = res {
                            let _ = tx.send(WorkerResponse::Error(format!("Failed to update registry: {}", e)));
                        } else {
                            let msg = if enable { "EOS Registry updated" } else { "EOS Registry entries removed" };
                            let _ = tx.send(WorkerResponse::TaskFinished(msg.to_string()));

                            // Re-query status
                            let installed_games = crate::auth::load_installed_games();
                            let eos_installed = installed_games.iter().find(|g| g.app_name == crate::eos::EOS_OVERLAY_APP_ID);
                            let status = crate::eos::EosOverlayStatus {
                                installed: eos_installed.is_some(),
                                install_path: eos_installed.map(|g| g.install_path.clone()),
                                registry_path: crate::eos::query_registry(&prefix),
                                available_paths: crate::eos::search_overlay_installs(Some(&prefix)),
                            };
                            let _ = tx.send(WorkerResponse::EosStatusFetched(status));
                        }
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
            search_query: String::new(),
            status_message: "Welcome to Legendary Rust".to_string(),
            current_view: View::Auth,
            tx: worker_tx,
            rx,
            worker_cancel,
            worker_pause,
            running_processes: HashMap::new(),
            save_sync_status: None,
            install_info: None,
            selected_tags: HashSet::new(),
            current_task: None,
            manifest_files: Vec::new(),
            manifest_search_query: String::new(),
            eos_status: crate::eos::EosOverlayStatus::default(),
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
                    self.current_task = None;
                }
                WorkerResponse::TaskProgress { task_name, progress, is_paused, speed, eta } => {
                    self.current_task = Some(TaskStatus {
                        name: task_name.clone(),
                        progress,
                        is_paused,
                        speed: speed.clone().unwrap_or_default(),
                        eta: eta.clone().unwrap_or_default(),
                    });
                    let mut msg = format!("{}: {:.0}%", task_name, progress * 100.0);
                    if let Some(s) = speed { msg.push_str(&format!(" | {}", s)); }
                    if let Some(e) = eta { msg.push_str(&format!(" | ETA: {}", e)); }
                    if is_paused { msg.push_str(" (Paused)"); }
                    self.status_message = msg;
                }
                WorkerResponse::TaskFinished(msg) => {
                    self.status_message = msg;
                    self.current_task = None;
                }
                WorkerResponse::SaveSyncStatusFetched { app_name, files, local_time, remote_time, error } => {
                    self.save_sync_status = Some(SaveSyncStatus {
                        app_name,
                        files,
                        local_time,
                        remote_time,
                        loading: false,
                        error,
                    });
                }
                WorkerResponse::InstallInfoFetched(info) => {
                    self.selected_tags = info.available_tags.iter().cloned().collect();
                    self.install_info = Some(info);
                    self.current_view = View::InstallDialog;
                }
                WorkerResponse::GamesScanned(games) => {
                    // Check for changes in installation status to refresh images
                    for item in &self.library {
                        let was_installed = self.installed_games.iter().any(|g| g.app_name == item.app_name);
                        let is_now_installed = games.iter().any(|g| g.app_name == item.app_name);

                        if was_installed != is_now_installed {
                            let local_meta = crate::auth::load_local_metadata(&item.app_name);
                            if let Some(meta) = local_meta {
                                for img_info in &meta.metadata.key_images {
                                    self.images.remove(&(item.app_name.clone(), img_info.image_type.clone()));
                                    self.fetching_images.remove(&(item.app_name.clone(), img_info.image_type.clone()));
                                }
                            }
                        }
                    }
                    self.installed_games = games;
                }
                WorkerResponse::FilesListed(files) => {
                    self.manifest_files = files;
                }
                WorkerResponse::GameLaunched { app_name, child } => {
                    self.running_processes.insert(app_name, child);
                }
                WorkerResponse::EosStatusFetched(status) => {
                    self.eos_status = status;
                }
            }
        }

        let is_task_running = self.current_task.is_some();

        egui::SidePanel::left("side_panel")
            .resizable(true)
            .default_width(150.0)
            .show(ctx, |ui| {
            ui.set_enabled(!is_task_running);
            ui.heading("Legendary Rust");
            ui.add_space(10.0);
            if ui.selectable_label(self.current_view == View::Library, "Library").clicked() {
                self.current_view = View::Library;
            }
            if ui.selectable_label(self.current_view == View::Settings, "Settings").clicked() {
                self.current_view = View::Settings;
            }
            if ui.selectable_label(self.current_view == View::Tasks, "Download Queue").clicked() {
                self.current_view = View::Tasks;
            }
            if ui.selectable_label(self.current_view == View::EosOverlay, "EOS Overlay").clicked() {
                self.current_view = View::EosOverlay;
                let _ = self.tx.send(WorkerMsg::QueryEosStatus { prefix: None });
            }
            if self.token.is_some() {
                if ui.selectable_label(self.current_view == View::Account, "Account").clicked() {
                    self.current_view = View::Account;
                }
            }
            ui.add_space(10.0);
            if self.token.is_none() {
                if ui.button("Login").clicked() {
                    self.current_view = View::Auth;
                }
            }
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            let is_task_running = self.current_task.is_some();
            ui.set_enabled(!is_task_running || self.current_view == View::GameDetail);

            match self.current_view {
                View::Auth => self.show_auth_view(ui),
                View::Library => self.show_library_view(ui),
                View::GameDetail => self.show_game_detail_view(ui),
                View::Settings => self.show_settings_view(ui),
                View::SaveSync => self.show_save_sync_view(ui),
                View::InstallDialog => self.show_install_dialog_view(ui),
                View::Tasks => self.show_tasks_view(ui),
                View::Account => self.show_account_view(ui),
                View::EosOverlay => self.show_eos_overlay_view(ui),
            }

            ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                ui.horizontal(|ui| {
                    ui.label(&self.status_message);
                    if let Some(task) = &self.current_task {
                        if self.current_view != View::GameDetail {
                            ui.add(egui::ProgressBar::new(task.progress).show_percentage());
                        }
                    }
                });
                ui.separator();
            });
        });
    }
}

impl LegendaryApp {
    fn launch_game(&mut self, app_name: String, offline: bool) {
        let _ = self.tx.send(WorkerMsg::LaunchGame { app_name, offline });
    }

    fn show_tasks_view(&mut self, ui: &mut egui::Ui) {
        ui.heading("Download Queue");
        ui.separator();

        if let Some(task) = self.current_task.clone() {
            ui.group(|ui| {
                ui.label(format!("Current Task: {}", task.name));
                ui.add(egui::ProgressBar::new(task.progress).show_percentage());
                ui.horizontal(|ui| {
                    if !task.speed.is_empty() {
                        ui.label(format!("Speed: {}", task.speed));
                    }
                    if !task.eta.is_empty() {
                        ui.label(format!("ETA: {}", task.eta));
                    }
                });
                ui.horizontal(|ui| {
                    if task.is_paused {
                        if ui.button("Resume").clicked() {
                            if let Some(t) = &mut self.current_task { t.is_paused = false; }
                            self.worker_pause.store(false, Ordering::SeqCst);
                            let _ = self.tx.send(WorkerMsg::ResumeTask);
                        }
                    } else {
                        if ui.button("Pause").clicked() {
                            if let Some(t) = &mut self.current_task { t.is_paused = true; }
                            self.worker_pause.store(true, Ordering::SeqCst);
                            let _ = self.tx.send(WorkerMsg::PauseTask);
                        }
                    }
                    if ui.button("Cancel").clicked() {
                        self.worker_cancel.store(true, Ordering::SeqCst);
                        let _ = self.tx.send(WorkerMsg::CancelTask);
                    }
                });
            });
        } else {
            ui.label("No active tasks.");
        }
    }

    fn show_auth_view(&mut self, ui: &mut egui::Ui) {
        ui.heading("Authentication");
        ui.label(&self.status_message);

        ui.separator();

        ui.collapsing("Login with Authorization Code", |ui| {
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
        });

        ui.add_space(10.0);

        ui.collapsing("Login with SID", |ui| {
            ui.label("1. Click the button below to open the Epic Games SID page.");
            ui.label("2. You must be already logged in to Epic Games in your browser.");
            ui.label("3. You will see a JSON response containing 'sid'.");
            ui.label("4. Copy that SID and paste it into the field below.");

            if ui.button("Open SID URL").clicked() {
                let _ = open::that("https://www.epicgames.com/id/api/sid");
                self.status_message = "Waiting for SID...".to_string();
            }

            ui.horizontal(|ui| {
                ui.label("SID:");
                ui.text_edit_singleline(&mut self.auth_code);
            });

            if ui.button("Log In with SID").clicked() {
                let _ = self.tx.send(WorkerMsg::LoginSid(self.auth_code.clone()));
                self.status_message = "Logging in with SID...".to_string();
            }
        });
    }

    fn show_library_view(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Library");
            ui.add_space(20.0);
            ui.label("Search:");
            ui.text_edit_singleline(&mut self.search_query);
            if ui.button("Clear").clicked() {
                self.search_query.clear();
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
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
        });

        if self.token.is_none() {
            ui.label("You must be logged in to see your library.");
            return;
        }

        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.vertical(|ui| {
                for item in &self.library {
                    let local_meta = crate::auth::load_local_metadata(&item.app_name);
                    let title = local_meta.as_ref()
                        .map(|m| m.app_title.clone())
                        .or_else(|| item.metadata.as_ref()
                            .and_then(|m| m.get("title"))
                            .and_then(|t| t.as_str())
                            .map(|s| s.to_string()))
                        .unwrap_or_else(|| item.app_name.clone());

                    if !self.search_query.is_empty() && !title.to_lowercase().contains(&self.search_query.to_lowercase()) && !item.app_name.to_lowercase().contains(&self.search_query.to_lowercase()) {
                        continue;
                    }

                    let is_installed = self.installed_games.iter().any(|g| g.app_name == item.app_name);

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
                                        if let Some(installed) = self.installed_games.iter().find(|g| g.app_name == item.app_name) {
                                            if let Some(asset) = self.assets.iter().find(|a| a.app_name == item.app_name) {
                                                if asset.build_version != installed.version {
                                                    ui.colored_label(egui::Color32::YELLOW, "⏫ Update Available");
                                                }
                                            }
                                        }
                                    }
                                    if ui.button(egui::RichText::new(title).strong().size(18.0)).clicked() {
                                        self.selected_app_name = Some(item.app_name.clone());
                                        let _ = self.tx.send(WorkerMsg::FetchAssets);
                                        let _ = self.tx.send(WorkerMsg::FetchGameInfo {
                                            app_name: item.app_name.clone(),
                                            namespace: item.namespace.clone(),
                                            catalog_item_id: item.catalog_item_id.clone(),
                                        });
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
        let selected_game = self.selected_game.clone();
        let selected_app_name = self.selected_app_name.clone();

        if let Some(game) = selected_game {
            let app_name = selected_app_name.unwrap_or_else(|| game.id.clone());
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
                                    let save_path = self.config.games.get(&app_name).and_then(|s| s.save_path.clone());

                                    self.save_sync_status = Some(SaveSyncStatus {
                                        app_name: app_name.clone(),
                                        files: Vec::new(),
                                        local_time: None,
                                        remote_time: None,
                                        loading: true,
                                        error: None,
                                    });
                                    self.current_view = View::SaveSync;

                                    let _ = self.tx.send(WorkerMsg::SyncCloudSaves {
                                        app_name: app_name.clone(),
                                        namespace: item.namespace.clone(),
                                        save_path,
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
                                    ui.horizontal(|ui| {
                                        ui.label(&dlc.title);
                                        let dlc_installed = self.installed_games.iter().any(|g| g.app_name == dlc.id);
                                        if dlc_installed {
                                            ui.label("✅ Installed");
                                            if ui.button("Uninstall").clicked() {
                                                let _ = self.tx.send(WorkerMsg::UninstallGame(dlc.id.clone()));
                                            }
                                        } else {
                                            if ui.button("Install").clicked() {
                                                // We need to fetch install info for DLC first
                                                let _ = self.tx.send(WorkerMsg::FetchInstallInfo {
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

                    ui.add_space(10.0);
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

                    ui.add_space(10.0);
                    ui.label("Pre-launch Command:");
                    if ui.text_edit_singleline(&mut game_settings.pre_launch_command).changed() {
                        changed = true;
                    }

                    if ui.checkbox(&mut game_settings.eos_overlay_enabled, "Enable EOS Overlay").changed() {
                        changed = true;
                    }

                    ui.add_space(10.0);
                    if ui.checkbox(&mut game_settings.use_umu, "Use UMU Launcher").changed() {
                        changed = true;
                    }

                    if game_settings.use_umu {
                        ui.horizontal(|ui| {
                            ui.label("UMU Store:");
                            let mut store_str = game_settings.umu_store.clone().unwrap_or_default();
                            if ui.text_edit_singleline(&mut store_str).changed() {
                                game_settings.umu_store = if store_str.is_empty() { None } else { Some(store_str) };
                                changed = true;
                            }
                        });
                    }

                    ui.add_space(10.0);
                    ui.collapsing("Steam Compatibility Settings", |ui| {
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
                    });

                    if changed {
                        let _ = self.config.save();
                    }
                });

                ui.add_space(10.0);
                ui.collapsing("Manifest Files", |ui| {
                    ui.horizontal(|ui| {
                        if ui.button("Fetch/Refresh").clicked() {
                            let catalog_item_id = self.library.iter().find(|i| i.app_name == app_name)
                                .map(|i| i.catalog_item_id.clone())
                                .unwrap_or_default();
                            let _ = self.tx.send(WorkerMsg::ListFiles {
                                app_name: app_name.clone(),
                                catalog_item_id,
                            });
                        }
                        ui.label("Search:");
                        ui.text_edit_singleline(&mut self.manifest_search_query);
                        if ui.button("×").clicked() {
                            self.manifest_search_query.clear();
                        }
                    });

                    if !self.manifest_files.is_empty() {
                        let query = self.manifest_search_query.to_lowercase();
                        egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                            for file in &self.manifest_files {
                                if query.is_empty() || file.to_lowercase().contains(&query) {
                                    ui.label(file);
                                }
                            }
                        });
                    }
                });

                ui.add_space(20.0);

                if let Some(task) = self.current_task.clone() {
                    ui.group(|ui| {
                        ui.label(format!("Task: {}", task.name));
                        ui.add(egui::ProgressBar::new(task.progress).show_percentage());
                        ui.horizontal(|ui| {
                            if task.is_paused {
                                if ui.button("Resume").clicked() {
                                    if let Some(t) = &mut self.current_task { t.is_paused = false; }
                                    self.worker_pause.store(false, Ordering::SeqCst);
                                    let _ = self.tx.send(WorkerMsg::ResumeTask);
                                }
                            } else {
                                if ui.button("Pause").clicked() {
                                    if let Some(t) = &mut self.current_task { t.is_paused = true; }
                                    self.worker_pause.store(true, Ordering::SeqCst);
                                    let _ = self.tx.send(WorkerMsg::PauseTask);
                                }
                            }
                            if ui.button("Cancel").clicked() {
                                self.worker_cancel.store(true, Ordering::SeqCst);
                                let _ = self.tx.send(WorkerMsg::CancelTask);
                            }
                        });
                    });
                    ui.add_space(10.0);
                }

                ui.horizontal(|ui| {
                    let is_task_running = self.current_task.is_some();
                    let is_running = self.running_processes.contains_key(&app_name);
                    let button_text = if is_running { "Stop Game" } else { "Start Game" };

                    ui.add_enabled_ui(!is_task_running, |ui| {
                        if ui.button(egui::RichText::new(button_text).size(24.0).strong()).clicked() {
                            if is_running {
                                if let Some(mut child) = self.running_processes.remove(&app_name) {
                                    let _ = child.kill();
                                    self.status_message = format!("Stopped game: {}", app_name);
                                }
                            } else {
                                let offline = self.config.games.get(&app_name).map(|s| s.play_offline).unwrap_or(false);
                                if offline {
                                    let can_run_offline = local_meta.as_ref().map(|m| {
                                        m.metadata.custom_attributes.as_ref().and_then(|attrs| {
                                            attrs.get("CanRunOffline").map(|a| a.value == "true")
                                        }).unwrap_or(true)
                                    }).unwrap_or(true);

                                    if !can_run_offline {
                                        self.status_message = "Warning: Game may not support offline mode.".to_string();
                                    }
                                }
                                self.launch_game(app_name.clone(), offline);
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
                                self.current_view = View::Tasks;
                            }
                            if ui.button("Repair").clicked() {
                                let _ = self.tx.send(WorkerMsg::RepairGame(app_name.clone(), false));
                            }
                            if ui.button("Repair and Update").clicked() {
                                let _ = self.tx.send(WorkerMsg::RepairGame(app_name.clone(), true));
                            }
                            if ui.button("Uninstall").clicked() {
                                let _ = self.tx.send(WorkerMsg::UninstallGame(app_name.clone()));
                            }

                            if let Some(installed) = self.installed_games.iter().find(|g| g.app_name == app_name) {
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

            if !info.available_tags.is_empty() {
                ui.add_space(20.0);
                ui.heading("Selective Download:");
                egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                    for tag in &info.available_tags {
                        let mut selected = self.selected_tags.contains(tag);
                        if ui.checkbox(&mut selected, tag).changed() {
                            if selected {
                                self.selected_tags.insert(tag.clone());
                            } else {
                                self.selected_tags.remove(tag);
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
                    let _ = self.tx.send(WorkerMsg::InstallGame {
                        app_name: info.app_name.clone(),
                        install_path: info.install_path.clone(),
                        selected_tags: Some(self.selected_tags.clone()),
                        platform: "Windows".to_string(), // Default to Windows for now
                    });
                    self.current_view = View::Tasks;
                }
                if ui.button("Cancel").clicked() {
                    self.current_view = View::GameDetail;
                }
            });
        }
    }

    fn show_save_sync_view(&mut self, ui: &mut egui::Ui) {
        let status_data = self.save_sync_status.clone();
        if let Some(status) = status_data {
            ui.horizontal(|ui| {
                if ui.button("⬅ Back").clicked() {
                    self.save_sync_status = None;
                    self.current_view = View::GameDetail;
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
                    ui.colored_label(egui::Color32::LIGHT_RED, egui::RichText::new("Failed to fetch cloud save metadata").strong());
                    ui.label(err);
                    if ui.button("Retry").clicked() {
                        if let Some(item) = self.library.iter().find(|i| i.app_name == status.app_name) {
                            let save_path = self.config.games.get(&status.app_name).and_then(|s| s.save_path.clone());
                            self.save_sync_status = Some(SaveSyncStatus {
                                app_name: status.app_name.clone(),
                                files: Vec::new(),
                                local_time: None,
                                remote_time: None,
                                loading: true,
                                error: None,
                            });
                            let _ = self.tx.send(WorkerMsg::SyncCloudSaves {
                                app_name: status.app_name.clone(),
                                namespace: item.namespace.clone(),
                                save_path,
                            });
                        }
                    }
                });
                return;
            }

            let can_upload = self.config.games.get(&status.app_name).and_then(|s| s.save_path.as_ref()).is_some();
            let can_download = !status.files.is_empty();

            ui.horizontal_top(|ui| {
                let box_width = (ui.available_width() - ui.spacing().item_spacing.x) / 2.0;
                let box_height = 250.0;

                // Local Box
                let local_frame = egui::Frame::group(ui.style())
                    .rounding(5.0)
                    .stroke(egui::Stroke::new(1.0, egui::Color32::from_gray(60)));

                local_frame.show(ui, |ui| {
                    ui.set_min_size(egui::vec2(box_width, box_height));
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.colored_label(egui::Color32::from_rgb(100, 100, 200), egui::RichText::new(" Local ").strong().background_color(egui::Color32::from_rgb(40, 40, 80)));
                        });
                        ui.add_space(10.0);
                        ui.vertical_centered(|ui| {
                            if let Some(t) = status.local_time {
                                ui.label(t.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M:%S").to_string());
                            } else {
                                ui.label("No local save found");
                            }
                            ui.add_space(20.0);
                            ui.label(egui::RichText::new("🖴").size(80.0));
                            ui.add_space(20.0);

                            ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                                if ui.add_enabled(can_upload, egui::Button::new(egui::RichText::new("Upload").strong()).min_size(egui::vec2(box_width - 20.0, 30.0))).clicked() {
                                    if let (Some(item), Some(save_path)) = (self.library.iter().find(|i| i.app_name == status.app_name), self.config.games.get(&status.app_name).and_then(|s| s.save_path.clone())) {
                                        let _ = self.tx.send(WorkerMsg::UploadCloudSave {
                                            app_name: status.app_name.clone(),
                                            namespace: item.namespace.clone(),
                                            save_path,
                                        });
                                    }
                                }
                            });
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
                            ui.colored_label(egui::Color32::from_rgb(100, 100, 200), egui::RichText::new(" Cloud ").strong().background_color(egui::Color32::from_rgb(40, 40, 80)));
                        });
                        ui.add_space(10.0);
                        ui.vertical_centered(|ui| {
                            if let Some(t) = status.remote_time {
                                ui.label(t.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M:%S").to_string());
                            } else {
                                ui.label("No cloud save found");
                            }
                            ui.add_space(20.0);
                            ui.label(egui::RichText::new("☁").size(80.0));
                            ui.add_space(20.0);

                            ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                                if ui.add_enabled(can_download, egui::Button::new(egui::RichText::new("Download").strong()).min_size(egui::vec2(box_width - 20.0, 30.0))).clicked() {
                                    if let (Some(item), Some(save_path)) = (self.library.iter().find(|i| i.app_name == status.app_name), self.config.games.get(&status.app_name).and_then(|s| s.save_path.clone())) {
                                        let _ = self.tx.send(WorkerMsg::DownloadCloudSave {
                                            app_name: status.app_name.clone(),
                                            namespace: item.namespace.clone(),
                                            save_path,
                                        });
                                    }
                                }
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
                    ui.colored_label(egui::Color32::YELLOW, format!("⚠ Local save is newer (by {}).", crate::utils::format_duration(l - r)));
                } else {
                    ui.colored_label(egui::Color32::YELLOW, format!("⚠ Cloud save is newer (by {}).", crate::utils::format_duration(r - l)));
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
                        ui.colored_label(egui::Color32::from_rgb(100, 100, 200), egui::RichText::new(" Settings ").strong().background_color(egui::Color32::from_rgb(40, 40, 80)));
                    });
                    ui.add_space(10.0);

                    let game_settings = self.config.games.entry(status.app_name.clone()).or_default();
                    let mut changed = false;

                    ui.horizontal(|ui| {
                        ui.label("Enable sync");
                        if ui.checkbox(&mut game_settings.cloud_sync_enabled, "Automatically synchronize saves with the cloud").changed() {
                            changed = true;
                        }
                    });

                    ui.horizontal(|ui| {
                        ui.label("Saves path");
                        let mut path_str = game_settings.save_path.as_ref().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
                        if ui.text_edit_singleline(&mut path_str).changed() {
                            game_settings.save_path = if path_str.is_empty() { None } else { Some(std::path::PathBuf::from(path_str)) };
                            changed = true;
                        }
                        if ui.button("Browse...").clicked() {
                            if let Some(path) = rfd::FileDialog::new().pick_folder() {
                                game_settings.save_path = Some(path);
                                changed = true;
                            }
                        }
                    });

                    if ui.button("Resolve path").clicked() {
                        let mut resolved_path = None;
                        let mut folder_hint = None;

                        // Try to get folder hint from metadata
                        let local_meta = crate::auth::load_local_metadata(&status.app_name);
                        if let Some(meta) = local_meta {
                            if let Some(attrs) = meta.metadata.custom_attributes {
                                if let Some(attr) = attrs.get("CloudSaveFolder") {
                                    folder_hint = Some(attr.value.clone());
                                }
                            }
                        }

                        if folder_hint.is_none() {
                            folder_hint = Some(status.app_name.clone());
                        }

                        if let Some(hint) = folder_hint {
                            if std::env::consts::OS == "linux" {
                                if let Some(mut p) = get_default_compat_data_path() {
                                    p.push("pfx/drive_c/users/steamuser/AppData/Local");
                                    p.push(&hint);
                                    if p.exists() {
                                        resolved_path = Some(p);
                                    } else {
                                        // Try common variants
                                        p.pop();
                                        p.push(&hint.replace(" ", ""));
                                        if p.exists() { resolved_path = Some(p); }
                                    }
                                }
                            } else if std::env::consts::OS == "windows" {
                                if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
                                    let mut p = std::path::PathBuf::from(local_app_data);
                                    p.push(&hint);
                                    if p.exists() { resolved_path = Some(p); }
                                }
                            }
                        }

                        if let Some(p) = resolved_path {
                            game_settings.save_path = Some(p);
                            changed = true;
                        }
                    }

                    if changed {
                        let _ = self.config.save();
                    }
                });
            });
        }
    }

    fn show_account_view(&mut self, ui: &mut egui::Ui) {
        ui.heading("User Account Overview");
        ui.separator();

        if let Some(token) = &self.token {
            egui::Grid::new("account_grid")
                .num_columns(2)
                .spacing([40.0, 10.0])
                .show(ui, |ui| {
                    ui.label("Display Name:");
                    ui.label(token.display_name.as_deref().unwrap_or("Unknown"));
                    ui.end_row();

                    ui.label("Account ID:");
                    ui.label(&token.account_id);
                    ui.end_row();

                    ui.label("Client ID:");
                    ui.label(&token.client_id);
                    ui.end_row();

                    ui.label("Token Type:");
                    ui.label(&token.token_type);
                    ui.end_row();

                    ui.label("Expires At:");
                    ui.label(&token.expires_at);
                    ui.end_row();
                });

            ui.add_space(20.0);
            if ui.button("Logout").clicked() {
                let _ = self.tx.send(WorkerMsg::Logout);
                self.token = None;
                self.library.clear();
                self.images.clear();
                self.status_message = "Logged out".to_string();
                self.current_view = View::Auth;
            }
        } else {
            ui.label("Not logged in.");
            if ui.button("Go to Login").clicked() {
                self.current_view = View::Auth;
            }
        }
    }

    fn show_eos_overlay_view(&mut self, ui: &mut egui::Ui) {
        ui.heading("EOS Overlay Manager");
        ui.separator();

        ui.group(|ui| {
            ui.heading("Status");
            ui.horizontal(|ui| {
                ui.label("Installed:");
                if self.eos_status.installed {
                    ui.colored_label(egui::Color32::GREEN, "YES");
                } else {
                    ui.colored_label(egui::Color32::RED, "NO");
                }
            });

            if let Some(path) = &self.eos_status.install_path {
                ui.label(format!("Install Path: {}", path));
            } else {
                if ui.button("Install EOS Overlay").clicked() {
                    if let Some(home) = home::home_dir() {
                        let mut p = home;
                        p.push("Games");
                        p.push("eos-overlay");
                        let _ = self.tx.send(WorkerMsg::InstallGame {
                            app_name: crate::eos::EOS_OVERLAY_APP_ID.to_string(),
                            install_path: p,
                            selected_tags: None,
                            platform: "Windows".to_string()
                        });
                        self.current_view = View::Tasks;
                    }
                }
            }

            ui.add_space(5.0);
            ui.horizontal(|ui| {
                ui.label("Registry Status (Default Prefix):");
                if let Some(reg_path) = &self.eos_status.registry_path {
                    ui.colored_label(egui::Color32::GREEN, format!("Configured ({})", reg_path));
                } else {
                    ui.colored_label(egui::Color32::YELLOW, "Not Configured");
                }
            });

            if !self.eos_status.available_paths.is_empty() {
                ui.add_space(5.0);
                ui.label("Other available EOS installs:");
                for path in &self.eos_status.available_paths {
                    ui.horizontal(|ui| {
                        ui.label(path);
                        if ui.button("Use this").clicked() {
                            if let Some(prefix) = get_default_compat_data_path() {
                                let _ = self.tx.send(WorkerMsg::UpdateEosRegistry {
                                    overlay_path: path.clone(),
                                    prefix,
                                    enable: true
                                });
                            }
                        }
                    });
                }
            }

            if self.eos_status.installed {
                ui.horizontal(|ui| {
                    if ui.button("Enable in Default Prefix").clicked() {
                        if let (Some(path), Some(prefix)) = (&self.eos_status.install_path, get_default_compat_data_path()) {
                            let _ = self.tx.send(WorkerMsg::UpdateEosRegistry {
                                overlay_path: path.clone(),
                                prefix,
                                enable: true
                            });
                        }
                    }
                    if ui.button("Disable in Default Prefix").clicked() {
                        if let Some(prefix) = get_default_compat_data_path() {
                            let _ = self.tx.send(WorkerMsg::UpdateEosRegistry {
                                overlay_path: String::new(),
                                prefix,
                                enable: false
                            });
                        }
                    }
                });
            }
        });

        ui.add_space(20.0);

        ui.group(|ui| {
            ui.label("Global EOS Overlay Setting:");
            if ui.checkbox(&mut self.config.global.eos_overlay_enabled, "Enable EOS Overlay by default").changed() {
                let _ = self.config.save();
            }
        });

        ui.add_space(10.0);
        ui.heading("Per-game EOS Overlay Settings");
        egui::ScrollArea::vertical().show(ui, |ui| {
            for game in &self.installed_games {
                if game.app_name == crate::eos::EOS_OVERLAY_APP_ID { continue; }
                ui.horizontal(|ui| {
                    ui.label(&game.title);
                    let settings = self.config.games.entry(game.app_name.clone()).or_default();
                    if ui.checkbox(&mut settings.eos_overlay_enabled, "Enabled").changed() {
                        let _ = self.config.save();
                    }
                });
            }
        });
    }

    fn show_settings_view(&mut self, ui: &mut egui::Ui) {
        ui.heading("Global Settings");
        ui.separator();

        if ui.button("Sync with Epic Games Launcher").clicked() {
            let _ = self.tx.send(WorkerMsg::EglSync);
        }
        ui.add_space(10.0);

        ui.group(|ui| {
            ui.label("Default Compatibility Settings:");
            let mut changed = false;
            if ui.checkbox(&mut self.config.global.use_custom_pfx, "Use Custom WINE/Proton Prefix (PFX)").changed() {
                changed = true;
            }
            if self.config.global.use_custom_pfx {
                ui.horizontal(|ui| {
                    let mut path_str = self.config.global.custom_pfx_path.as_ref().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
                    if ui.text_edit_singleline(&mut path_str).changed() {
                        self.config.global.custom_pfx_path = if path_str.is_empty() { None } else { Some(std::path::PathBuf::from(path_str)) };
                        changed = true;
                    }
                    if ui.button("Browse...").clicked() {
                        if let Some(path) = rfd::FileDialog::new().pick_folder() {
                            self.config.global.custom_pfx_path = Some(path);
                            changed = true;
                        }
                    }
                });
            }
            ui.add_space(10.0);
            ui.label("Global Pre-launch Command:");
            if ui.text_edit_singleline(&mut self.config.global.pre_launch_command).changed() {
                changed = true;
            }
            if ui.checkbox(&mut self.config.global.eos_overlay_enabled, "Enable EOS Overlay by default").changed() {
                changed = true;
            }

            ui.add_space(10.0);
            if ui.checkbox(&mut self.config.global.use_umu, "Use UMU Launcher by default").changed() {
                changed = true;
            }

            if self.config.global.use_umu {
                ui.horizontal(|ui| {
                    ui.label("Default UMU Store:");
                    if ui.text_edit_singleline(&mut self.config.global.umu_store).changed() {
                        changed = true;
                    }
                });
            }

            ui.add_space(10.0);
            ui.collapsing("Default Steam Compatibility Settings", |ui| {
                ui.horizontal(|ui| {
                    ui.label("STEAM_COMPAT_INSTALL_PATH:");
                    let mut path_str = self.config.global.steam_compat_install_path.as_ref().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
                    if ui.text_edit_singleline(&mut path_str).changed() {
                        self.config.global.steam_compat_install_path = if path_str.is_empty() { None } else { Some(std::path::PathBuf::from(path_str)) };
                        changed = true;
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("STEAM_COMPAT_CLIENT_INSTALL_PATH:");
                    let mut path_str = self.config.global.steam_compat_client_install_path.as_ref().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
                    if ui.text_edit_singleline(&mut path_str).changed() {
                        self.config.global.steam_compat_client_install_path = if path_str.is_empty() { None } else { Some(std::path::PathBuf::from(path_str)) };
                        changed = true;
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("STEAM_COMPAT_DATA_PATH:");
                    let mut path_str = self.config.global.steam_compat_data_path.as_ref().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
                    if ui.text_edit_singleline(&mut path_str).changed() {
                        self.config.global.steam_compat_data_path = if path_str.is_empty() { None } else { Some(std::path::PathBuf::from(path_str)) };
                        changed = true;
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("STEAM_COMPAT_APP_ID:");
                    let mut id_str = self.config.global.steam_compat_app_id.clone().unwrap_or_default();
                    if ui.text_edit_singleline(&mut id_str).changed() {
                        self.config.global.steam_compat_app_id = if id_str.is_empty() { None } else { Some(id_str) };
                        changed = true;
                    }
                });
            });

            if changed {
                let _ = self.config.save();
            }
        });

        ui.add_space(10.0);

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
