use crate::api::EgsClient;
use crate::models::{LibraryItem, OAuthToken};
use eframe::egui;

use crate::models::GameInfo;

use crate::models::Asset;

use crate::models::InstalledGame;

use chrono::{DateTime, Utc};
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{channel, Receiver, Sender};

use crate::config::{AppConfig, CompatibilityTool};

use rand::Rng;
use sha1::{Digest, Sha1};
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
    running_apps: HashSet<String>,
    save_sync_status: Option<SaveSyncStatus>,
    unaccepted_eulas: Vec<serde_json::Value>,
    install_info: Option<crate::models::InstallInfo>,
    selected_tags: HashSet<String>,
    current_task: Option<TaskStatus>,
    manifest_files: Vec<String>,
    manifest_search_query: String,
    eos_status: crate::eos::EosOverlayStatus,
    eos_prefix_path: Option<std::path::PathBuf>,
    advanced_info: Option<AdvancedInfo>,
    new_env_key: String,
    new_env_val: String,
}

#[derive(Clone, Default)]
pub struct AdvancedInfo {
    pub app_name: String,
    pub save_path: Option<std::path::PathBuf>,
    pub backup_path: Option<std::path::PathBuf>,
    pub prefix_path: Option<std::path::PathBuf>,
    pub dlss_path: Option<std::path::PathBuf>,
    pub dlssd_path: Option<std::path::PathBuf>,
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
    pub backup_time: Option<DateTime<Utc>>,
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
    FetchAdvancedInfo {
        app_name: String,
        install_path: String,
    },
    RepairGame(String, bool), // app_name, update
    SyncCloudSaves {
        app_name: String,
        namespace: String,
        save_path: Option<std::path::PathBuf>,
        backup_path: Option<std::path::PathBuf>,
    },
    CheckEula(Vec<String>),
    AcceptEula {
        eula_id: String,
        version: i32,
    },
    LaunchOrigin(String),
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
    StopGame(String),
    QueryEosStatus {
        prefix: Option<std::path::PathBuf>,
    },
    UpdateEosRegistry {
        overlay_path: String,
        prefix: std::path::PathBuf,
        enable: bool,
    },
    CreateLocalBackup {
        app_name: String,
        source: std::path::PathBuf,
        destination: std::path::PathBuf,
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
        backup_time: Option<DateTime<Utc>>,
        error: Option<String>,
    },
    EulaStatusFetched(Vec<serde_json::Value>),
    OriginUriFetched(String),
    InstallInfoFetched(crate::models::InstallInfo),
    GamesScanned(Vec<InstalledGame>),
    FilesListed(Vec<String>),
    GameLaunched(String),
    GameStopped(String),
    EosStatusFetched(crate::eos::EosOverlayStatus),
    AdvancedInfoFetched(AdvancedInfo),
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
        let gray =
            (pixel.r() as f32 * 0.299 + pixel.g() as f32 * 0.587 + pixel.b() as f32 * 0.114) as u8;
        *pixel = egui::Color32::from_rgba_unmultiplied(gray, gray, gray, pixel.a());
    }
}

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

fn find_file_in_dir(dir: &std::path::Path, filename: &str) -> Option<std::path::PathBuf> {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Some(found) = find_file_in_dir(&path, filename) {
                    return Some(found);
                }
            } else if path
                .file_name()
                .and_then(|s| s.to_str())
                .map(|s| s.to_lowercase())
                == Some(filename.to_lowercase())
            {
                return Some(path);
            }
        }
    }
    None
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

fn write_cloud_debug_blob(
    direction: &str,
    app_name: &str,
    remote_path: &str,
    data: &[u8],
) -> Option<std::path::PathBuf> {
    let safe_name: String = remote_path
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let file_name = format!("{}-{}", Utc::now().format("%Y%m%d-%H%M%S-%3f"), safe_name);
    let debug_dir = std::env::temp_dir()
        .join("legendary-rust-cloud-debug")
        .join(app_name)
        .join(direction);
    if std::fs::create_dir_all(&debug_dir).is_err() {
        return None;
    }
    let path = debug_dir.join(file_name);
    if std::fs::write(&path, data).is_ok() {
        Some(path)
    } else {
        None
    }
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
            if i > 0 {
                url.push('&');
            }
            let name = param["name"].as_str().unwrap_or("");
            let value = param["value"].as_str().unwrap_or("");
            url.push_str(&format!(
                "{}={}",
                urlencoding::encode(name),
                urlencoding::encode(value)
            ));
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
    manifest_info["elements"]
        .as_array()?
        .get(0)?
        .get("sidecar")?
        .get("config")?
        .as_str()
        .and_then(|config_str| serde_json::from_str::<serde_json::Value>(config_str).ok())
        .and_then(|config_json| config_json["deploymentId"].as_str().map(|s| s.to_string()))
}

fn get_cache_path(url: &str) -> Option<std::path::PathBuf> {
    use sha2::{Digest, Sha256};
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
            let mut stop_senders: HashMap<String, Sender<()>> = HashMap::new();

            let check_status = || {
                while pause.load(Ordering::SeqCst) {
                    if cancel.load(Ordering::SeqCst) {
                        return true;
                    }
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
                    let _ = tx.send(WorkerResponse::Error(format!(
                        "Failed to initialize client: {}",
                        e
                    )));
                    return;
                }
            };

            // Try initial load
            if let Ok(saved_token) = crate::auth::load_token() {
                client.set_token(&saved_token);
                let _ = tx.send(WorkerResponse::LoggedIn(saved_token));
                match client.get_library_items() {
                    Ok(items) => {
                        cached_library_items = items.clone();
                        let _ = crate::auth::save_library_cache(&items);
                        let _ = tx.send(WorkerResponse::LibraryFetched(items));
                        ctx_clone.request_repaint();
                    }
                    Err(e) => {
                        log::error!("Initial library fetch failed: {}, trying cache", e);
                        let items = crate::auth::load_library_cache();
                        if !items.is_empty() {
                            cached_library_items = items.clone();
                            let _ = tx.send(WorkerResponse::LibraryFetched(items));
                            ctx_clone.request_repaint();
                        }
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
                    WorkerMsg::StopGame(app_name) => {
                        if let Some(stop_tx) = stop_senders.remove(&app_name) {
                            let _ = stop_tx.send(());
                        }
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
                                    let _ = crate::auth::save_library_cache(&items);
                                    let _ = tx.send(WorkerResponse::LibraryFetched(items));
                                    ctx_clone.request_repaint();
                                }
                            }
                            Err(e) => {
                                let _ = tx.send(WorkerResponse::Error(e.to_string()));
                            }
                        }
                    }
                    WorkerMsg::LoginSid(sid) => {
                        let _ =
                            tx.send(WorkerResponse::Error("Logging in with SID...".to_string()));
                        match client.start_session_with_sid(&sid) {
                            Ok(token) => {
                                let _ = crate::auth::save_token(&token);
                                let _ = tx.send(WorkerResponse::LoggedIn(token));
                                // Fetch library immediately after login
                                if let Ok(items) = client.get_library_items() {
                                    cached_library_items = items.clone();
                                    let _ = crate::auth::save_library_cache(&items);
                                    let _ = tx.send(WorkerResponse::LibraryFetched(items));
                                    ctx_clone.request_repaint();
                                }
                            }
                            Err(e) => {
                                let _ = tx.send(WorkerResponse::Error(e.to_string()));
                            }
                        }
                    }
                    WorkerMsg::FetchImage {
                        app_name,
                        url,
                        is_installed,
                        image_type,
                    } => {
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
                                        let resized = img.resize(
                                            img.width() / 2,
                                            img.height() / 2,
                                            image::imageops::FilterType::Triangle,
                                        );
                                        let mut buf = std::io::Cursor::new(Vec::new());
                                        if resized
                                            .write_to(&mut buf, image::ImageFormat::Png)
                                            .is_ok()
                                        {
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
                                    .map(|p| {
                                        egui::Color32::from_rgba_unmultiplied(
                                            p[0], p[1], p[2], p[3],
                                        )
                                    })
                                    .collect();

                                let mut color_image = egui::ColorImage { size, pixels };

                                if !is_installed {
                                    color_to_grayscale(&mut color_image.pixels);
                                }

                                let _ = tx.send(WorkerResponse::ImageFetched {
                                    app_name,
                                    image_type,
                                    image: color_image,
                                });
                                ctx_clone.request_repaint();
                            }
                        }
                    }
                    WorkerMsg::RefreshLibrary => match client.get_library_items() {
                        Ok(items) => {
                            cached_library_items = items.clone();
                            let _ = crate::auth::save_library_cache(&items);
                            let _ = tx.send(WorkerResponse::LibraryFetched(items));
                            ctx_clone.request_repaint();
                        }
                        Err(e) => {
                            let _ = tx.send(WorkerResponse::Error(e.to_string()));
                        }
                    },
                    WorkerMsg::FetchGameInfo {
                        app_name,
                        namespace,
                        catalog_item_id,
                    } => {
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
                                        developer: None,     // Not available in GameInfo
                                        key_images: info.key_images.clone(),
                                        dlc_item_list: None,
                                        custom_attributes: info.custom_attributes.clone(),
                                        release_info: None,
                                    },
                                };
                                let _ = crate::auth::save_local_metadata(&app_name, &meta);

                                let _ = tx.send(WorkerResponse::GameInfoFetched(info));
                                ctx_clone.request_repaint();
                            }
                            Err(e) => {
                                let _ = tx.send(WorkerResponse::Error(e.to_string()));
                            }
                        }
                    }
                    WorkerMsg::FetchAssets => match client.get_game_assets("Windows") {
                        Ok(assets) => {
                            let _ = tx.send(WorkerResponse::AssetsFetched(assets));
                            ctx_clone.request_repaint();
                        }
                        Err(e) => {
                            let _ = tx.send(WorkerResponse::Error(e.to_string()));
                        }
                    },
                    WorkerMsg::Logout => {
                        let _ = client.invalidate_session();
                        let _ = crate::auth::get_config_dir().map(|d| {
                            let _ = std::fs::remove_file(d.join("user.json"));
                            let _ = std::fs::remove_file(d.join("token.json"));
                        });
                    }
                    WorkerMsg::VerifyGame {
                        app_name,
                        catalog_item_id,
                    } => {
                        let _ = tx.send(WorkerResponse::TaskProgress {
                            task_name: format!("Verifying {}", app_name),
                            progress: 0.0,
                            is_paused: false,
                            speed: None,
                            eta: None,
                        });
                        let mut installed = crate::auth::load_installed_games();
                        let game_idx = installed.iter().position(|g| g.app_name == app_name);

                        if let Some(idx) = game_idx {
                            let game = installed[idx].clone();
                            let mut manifest_opt = None;
                            let mut manifest_path_opt =
                                game.manifest_path.as_ref().map(std::path::PathBuf::from);

                            if manifest_path_opt
                                .as_ref()
                                .map(|p| !p.exists())
                                .unwrap_or(true)
                            {
                                manifest_path_opt =
                                    crate::auth::get_manifest_path(&app_name, &catalog_item_id);
                            }

                            if let Some(manifest_path) = manifest_path_opt {
                                let _ = tx.send(WorkerResponse::TaskProgress {
                                    task_name: format!("Reading manifest at {:?}", manifest_path),
                                    progress: 0.0,
                                    is_paused: false,
                                    speed: None,
                                    eta: None,
                                });
                                match std::fs::read(&manifest_path) {
                                    Ok(data) => match crate::manifest::parse_manifest(&data) {
                                        Ok(m) => manifest_opt = Some(m),
                                        Err(e) => log::error!(
                                            "Failed to parse manifest at {:?}: {}",
                                            manifest_path,
                                            e
                                        ),
                                    },
                                    Err(e) => log::error!(
                                        "Failed to read manifest at {:?}: {}",
                                        manifest_path,
                                        e
                                    ),
                                }
                            }

                            if manifest_opt.is_none() {
                                // Try to download manifest
                                let _ = tx.send(WorkerResponse::TaskProgress {
                                    task_name: format!(
                                        "Manifest not found, fetching for {}",
                                        app_name
                                    ),
                                    progress: 0.0,
                                    is_paused: false,
                                    speed: None,
                                    eta: None,
                                });
                                match client.get_game_assets(&game.platform) {
                                    Ok(assets) => {
                                        if let Some(asset) = assets.iter().find(|a| {
                                            a.app_name == app_name
                                                || a.catalog_item_id == catalog_item_id
                                        }) {
                                            match client.get_asset_manifest(
                                                &game.platform,
                                                &asset.namespace,
                                                &asset.catalog_item_id,
                                                &asset.app_name,
                                                &asset.label_name,
                                            ) {
                                                Ok(manifest_info) => {
                                                    // Update deployment_id in metadata if possible
                                                    if let Some(did) =
                                                        extract_deployment_id(&manifest_info)
                                                    {
                                                        if let Some(mut meta) =
                                                            crate::auth::load_local_metadata(
                                                                &app_name,
                                                            )
                                                        {
                                                            meta.metadata.deployment_id = Some(did);
                                                            let _ =
                                                                crate::auth::save_local_metadata(
                                                                    &app_name, &meta,
                                                                );
                                                        }
                                                    }

                                                    if let Some(url) =
                                                        find_manifest_url(&manifest_info)
                                                    {
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
                                                Err(e) => log::error!(
                                                    "Failed to fetch asset manifest for {}: {}",
                                                    app_name,
                                                    e
                                                ),
                                            }
                                        } else {
                                            log::error!(
                                                "Asset not found for {} on platform {}",
                                                app_name,
                                                game.platform
                                            );
                                        }
                                    }
                                    Err(e) => log::error!(
                                        "Failed to fetch assets for platform {}: {}",
                                        game.platform,
                                        e
                                    ),
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
                                        let _ = tx.send(WorkerResponse::TaskFinished(
                                            "Verification cancelled".to_string(),
                                        ));
                                        break;
                                    }

                                    let file_path = game_dir.join(filename);
                                    if file_path.exists() {
                                        if let Ok(actual_hash) = crate::utils::hash_file(&file_path)
                                        {
                                            let expected_hash = hex::encode(&info.hash);
                                            if actual_hash == expected_hash {
                                                verified += 1;
                                            } else {
                                                mismatches += 1;
                                                log::warn!(
                                                    "Hash mismatch for {}: expected {}, got {}",
                                                    filename,
                                                    expected_hash,
                                                    actual_hash
                                                );
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
                                let _ = tx.send(WorkerResponse::Error(format!(
                                    "Manifest for {} not found and could not be downloaded",
                                    app_name
                                )));
                            }
                        } else {
                            let _ = tx.send(WorkerResponse::Error(format!(
                                "Game {} not found in installed games",
                                app_name
                            )));
                        }
                        ctx_clone.request_repaint();
                    }
                    WorkerMsg::RepairGame(app_name, update) => {
                        let task_name = if update {
                            format!("Repairing and Updating {}", app_name)
                        } else {
                            format!("Repairing {}", app_name)
                        };
                        let _ = tx.send(WorkerResponse::TaskProgress {
                            task_name: task_name.clone(),
                            progress: 0.0,
                            is_paused: false,
                            speed: None,
                            eta: None,
                        });
                        // Placeholder for repair logic
                        for i in 1..=10 {
                            if check_status() {
                                let _ = tx.send(WorkerResponse::TaskFinished(format!(
                                    "Task '{}' cancelled",
                                    task_name
                                )));
                                break;
                            }
                            std::thread::sleep(std::time::Duration::from_millis(300));
                            let _ = tx.send(WorkerResponse::TaskProgress {
                                task_name: task_name.clone(),
                                progress: i as f32 / 10.0,
                                is_paused: pause.load(Ordering::SeqCst),
                                speed: None,
                                eta: None,
                            });
                        }
                        let _ = tx.send(WorkerResponse::TaskFinished(format!(
                            "Task '{}' complete",
                            task_name
                        )));
                        ctx_clone.request_repaint();
                    }
                    WorkerMsg::CheckEula(eula_ids) => {
                        let mut unaccepted = Vec::new();
                        for id in eula_ids {
                            match client.eula_get_status(&id) {
                                Ok(Some(eula)) => unaccepted.push(eula),
                                Ok(None) => {}
                                Err(e) => {
                                    log::warn!("Failed to check EULA status for {}: {}", id, e)
                                }
                            }
                        }
                        let _ = tx.send(WorkerResponse::EulaStatusFetched(unaccepted));
                        ctx_clone.request_repaint();
                    }
                    WorkerMsg::AcceptEula { eula_id, version } => {
                        match client.eula_accept(&eula_id, version, None) {
                            Ok(_) => {
                                // Re-check if other EULAs exist
                                // Simplified: assume it was the only one or user will refresh
                            }
                            Err(e) => {
                                let _ = tx.send(WorkerResponse::Error(format!(
                                    "Failed to accept EULA: {}",
                                    e
                                )));
                            }
                        }
                    }
                    WorkerMsg::LaunchOrigin(app_name) => {
                        match client.get_game_token() {
                            Ok(token) => {
                                let user_name = client.get_display_name().unwrap_or_default();
                                let account_id = client.get_account_id().unwrap_or_default();
                                let mut url = format!("link2ea://launchgame/{}?AUTH_PASSWORD={}&AUTH_TYPE=exchangecode&epicusername={}&epicuserid={}&epiclocale=en",
                                    app_name, token, urlencoding::encode(&user_name), account_id);

                                // Find metadata for extra args if any
                                if let Some(item) =
                                    cached_library_items.iter().find(|i| i.app_name == app_name)
                                {
                                    if let Some(meta) = &item.metadata {
                                        if let Some(extra) = meta["customAttributes"]
                                            ["AdditionalCommandline"]["value"]
                                            .as_str()
                                        {
                                            for part in extra.split('&') {
                                                url.push('&');
                                                url.push_str(part);
                                            }
                                        }
                                    }
                                }

                                let _ = tx.send(WorkerResponse::OriginUriFetched(url));
                            }
                            Err(e) => {
                                let _ = tx.send(WorkerResponse::Error(format!(
                                    "Failed to get game token for Origin: {}",
                                    e
                                )));
                            }
                        }
                    }
                    WorkerMsg::SyncCloudSaves {
                        app_name,
                        namespace,
                        save_path,
                        backup_path,
                    } => {
                        let _ = tx.send(WorkerResponse::TaskProgress {
                            task_name: format!("Checking cloud saves for {}", app_name),
                            progress: 0.0,
                            is_paused: false,
                            speed: None,
                            eta: None,
                        });
                        println!(
                            "[CloudSaves] Checking saves for {}, namespace: {}",
                            app_name, namespace
                        );
                        if let Some(ref p) = save_path {
                            println!("[CloudSaves] Local save path: {:?}", p);
                        } else {
                            println!("[CloudSaves] No local save path configured/discovered yet.");
                        }

                        let local_time = save_path
                            .as_ref()
                            .and_then(|p| get_latest_local_save_time(p));
                        let backup_time = backup_path
                            .as_ref()
                            .and_then(|p| get_latest_local_save_time(p));

                        let result = if let Some(token) = crate::auth::load_token().ok() {
                            client.get_cloud_save_metadata(&namespace, &token.account_id, &app_name)
                        } else {
                            Err(anyhow::anyhow!("No authentication token found"))
                        };

                        match result {
                            Ok(files) => {
                                let mut remote_time = None;
                                for file in &files {
                                    if let Ok(dt) =
                                        DateTime::parse_from_rfc3339(&file.last_modified)
                                    {
                                        let dt_utc = dt.with_timezone(&Utc);
                                        if remote_time.is_none() || dt_utc > remote_time.unwrap() {
                                            remote_time = Some(dt_utc);
                                        }
                                    }
                                }

                                let _ = tx.send(WorkerResponse::SaveSyncStatusFetched {
                                    app_name: app_name.clone(),
                                    files,
                                    local_time,
                                    remote_time,
                                    backup_time,
                                    error: None,
                                });
                                let _ = tx.send(WorkerResponse::TaskFinished(format!(
                                    "Checked cloud saves for {}",
                                    app_name
                                )));
                            }
                            Err(e) => {
                                let _ = tx.send(WorkerResponse::SaveSyncStatusFetched {
                                    app_name: app_name.clone(),
                                    files: Vec::new(),
                                    local_time,
                                    remote_time: None,
                                    backup_time,
                                    error: Some(e.to_string()),
                                });
                                let _ = tx.send(WorkerResponse::Error(format!(
                                    "Failed to check cloud saves for {}: {}",
                                    app_name, e
                                )));
                            }
                        }
                        ctx_clone.request_repaint();
                    }
                    WorkerMsg::CreateLocalBackup {
                        app_name,
                        source,
                        destination,
                    } => {
                        let _ = tx.send(WorkerResponse::TaskProgress {
                            task_name: format!("Creating backup for {}", app_name),
                            progress: 0.0,
                            is_paused: false,
                            speed: None,
                            eta: None,
                        });

                        // Use a more robust recursive copy
                        fn copy_dir_all(
                            src: impl AsRef<std::path::Path>,
                            dst: impl AsRef<std::path::Path>,
                        ) -> std::io::Result<()> {
                            std::fs::create_dir_all(&dst)?;
                            for entry in std::fs::read_dir(src)? {
                                let entry = entry?;
                                let ty = entry.file_type()?;
                                if ty.is_dir() {
                                    copy_dir_all(
                                        entry.path(),
                                        dst.as_ref().join(entry.file_name()),
                                    )?;
                                } else {
                                    std::fs::copy(
                                        entry.path(),
                                        dst.as_ref().join(entry.file_name()),
                                    )?;
                                }
                            }
                            Ok(())
                        }

                        match copy_dir_all(&source, &destination) {
                            Ok(_) => {
                                let _ = tx.send(WorkerResponse::TaskFinished(format!(
                                    "Backup for {} complete",
                                    app_name
                                )));
                            }
                            Err(e) => {
                                let _ =
                                    tx.send(WorkerResponse::Error(format!("Backup failed: {}", e)));
                            }
                        }
                        ctx_clone.request_repaint();
                    }
                    WorkerMsg::UploadCloudSave {
                        app_name,
                        namespace: _,
                        save_path,
                    } => {
                        let _ = tx.send(WorkerResponse::TaskProgress {
                            task_name: format!("Packaging and uploading saves for {}", app_name),
                            progress: 0.0,
                            is_paused: false,
                            speed: None,
                            eta: None,
                        });
                        if let Ok(token) = crate::auth::load_token() {
                            let mut files_to_package = get_all_files(&save_path);
                            if files_to_package.is_empty() {
                                let _ = tx.send(WorkerResponse::TaskFinished(format!(
                                    "No save files found for {} to upload.",
                                    app_name
                                )));
                                return;
                            }
                            files_to_package.sort_by(|a, b| {
                                a.to_string_lossy()
                                    .to_lowercase()
                                    .cmp(&b.to_string_lossy().to_lowercase())
                            });

                            let mut manifest = crate::manifest::Manifest {
                                manifest_version: 18,
                                meta: crate::manifest::ManifestMeta {
                                    app_name: format!("{}{}", app_name, token.account_id),
                                    build_version: Utc::now()
                                        .format("%Y.%m.%d-%H.%M.%S")
                                        .to_string(),
                                    ..Default::default()
                                },
                                chunks: HashMap::new(),
                                files: HashMap::new(),
                                custom_fields: HashMap::new(),
                                total_uncompressed_size: 0,
                                total_download_size: 0,
                            };

                            // Get CloudSaveFolder from metadata
                            let local_meta = crate::auth::load_local_metadata(&app_name);
                            if let Some(meta) = local_meta {
                                if let Some(attrs) = meta.metadata.custom_attributes {
                                    if let Some(attr) = attrs.get("CloudSaveFolder") {
                                        manifest.custom_fields.insert(
                                            "CloudSaveFolder".to_string(),
                                            attr.value.clone(),
                                        );
                                    }
                                }
                            }

                            let mut chunks_data = HashMap::new();
                            let mut current_chunk_data = Vec::with_capacity(1024 * 1024);
                            let mut current_chunk_guid = [
                                rand::thread_rng().gen::<u32>(),
                                rand::thread_rng().gen::<u32>(),
                                rand::thread_rng().gen::<u32>(),
                                rand::thread_rng().gen::<u32>(),
                            ];

                            for file_path in &files_to_package {
                                if let Ok(data) = std::fs::read(file_path) {
                                    let rel_path = file_path
                                        .strip_prefix(&save_path)
                                        .unwrap_or(file_path)
                                        .to_string_lossy()
                                        .replace('\\', "/");
                                    let mut file_manifest = crate::manifest::FileManifest {
                                        filename: rel_path.clone(),
                                        hash: [0u8; 20],
                                        chunk_parts: Vec::new(),
                                        file_size: data.len() as u64,
                                        install_tags: Vec::new(),
                                    };

                                    let mut hasher = Sha1::new();
                                    hasher.update(&data);
                                    file_manifest.hash = hasher.finalize().into();

                                    let mut file_offset = 0;
                                    while file_offset < data.len() {
                                        let remaining_in_chunk =
                                            1024 * 1024 - current_chunk_data.len();
                                        let to_copy = std::cmp::min(
                                            remaining_in_chunk,
                                            data.len() - file_offset,
                                        );

                                        file_manifest.chunk_parts.push(
                                            crate::manifest::ChunkPart {
                                                guid: current_chunk_guid,
                                                offset: current_chunk_data.len() as u32,
                                                size: to_copy as u32,
                                                file_offset: file_offset as u64,
                                            },
                                        );

                                        current_chunk_data.extend_from_slice(
                                            &data[file_offset..file_offset + to_copy],
                                        );
                                        file_offset += to_copy;

                                        if current_chunk_data.len() >= 1024 * 1024 {
                                            let mut chunk_sha1 = Sha1::new();
                                            chunk_sha1.update(&current_chunk_data);
                                            let sha_hash: [u8; 20] = chunk_sha1.finalize().into();
                                            let rolling = crate::manifest::get_rolling_hash(
                                                &current_chunk_data,
                                            );

                                            let chunk_info = crate::manifest::ChunkInfo {
                                                guid: current_chunk_guid,
                                                hash: rolling,
                                                sha_hash,
                                                group_num: (rolling % 100) as u8,
                                                window_size: 1024 * 1024,
                                                file_size: 0, // Will be set after compression
                                            };

                                            let serialized = crate::manifest::serialize_chunk(
                                                &current_chunk_data,
                                                current_chunk_guid,
                                                rolling,
                                                sha_hash,
                                            )
                                            .unwrap();
                                            let mut chunk_info_final = chunk_info.clone();
                                            chunk_info_final.file_size = serialized.len() as i64;

                                            manifest
                                                .chunks
                                                .insert(current_chunk_guid, chunk_info_final);
                                            chunks_data.insert(
                                                chunk_info.path(manifest.manifest_version),
                                                serialized,
                                            );

                                            current_chunk_data.clear();
                                            current_chunk_guid = [
                                                rand::thread_rng().gen::<u32>(),
                                                rand::thread_rng().gen::<u32>(),
                                                rand::thread_rng().gen::<u32>(),
                                                rand::thread_rng().gen::<u32>(),
                                            ];
                                        }
                                    }
                                    manifest.files.insert(rel_path, file_manifest);
                                }
                            }

                            if !current_chunk_data.is_empty() {
                                let unpadded_len = current_chunk_data.len();
                                let mut padded_data = current_chunk_data.clone();
                                padded_data.resize(1024 * 1024, 0);

                                let mut chunk_sha1 = Sha1::new();
                                chunk_sha1.update(&padded_data);
                                let sha_hash: [u8; 20] = chunk_sha1.finalize().into();
                                let rolling = crate::manifest::get_rolling_hash(&padded_data);

                                let chunk_info = crate::manifest::ChunkInfo {
                                    guid: current_chunk_guid,
                                    hash: rolling,
                                    sha_hash,
                                    group_num: (rolling % 100) as u8,
                                    window_size: unpadded_len as u32,
                                    file_size: 0,
                                };

                                let serialized = crate::manifest::serialize_chunk(
                                    &current_chunk_data,
                                    current_chunk_guid,
                                    rolling,
                                    sha_hash,
                                )
                                .unwrap();
                                let mut chunk_info_final = chunk_info.clone();
                                chunk_info_final.file_size = serialized.len() as i64;

                                manifest.chunks.insert(current_chunk_guid, chunk_info_final);
                                chunks_data
                                    .insert(chunk_info.path(manifest.manifest_version), serialized);
                            }

                            let manifest_serialized = manifest.serialize().unwrap();
                            let manifest_path =
                                format!("manifests/{}.manifest", manifest.meta.build_version);

                            let mut all_uploads = chunks_data;
                            all_uploads.insert(manifest_path.clone(), manifest_serialized);

                            let filenames: Vec<String> = all_uploads.keys().cloned().collect();

                            println!(
                                "Generated cloud save manifest and {} chunks for {}",
                                all_uploads.len() - 1,
                                app_name
                            );

                            match client.get_cloud_save_links(
                                &token.account_id,
                                &app_name,
                                &filenames,
                            ) {
                                Ok(links) => {
                                    let total = filenames.len();
                                    let mut success = true;
                                    for (i, remote_path) in filenames.iter().enumerate() {
                                        let link_info = match links.get(remote_path) {
                                            Some(info) => info,
                                            None => {
                                                let err = format!("Cloud API did not return upload metadata for {}", remote_path);
                                                eprintln!("{}", err);
                                                let _ = tx.send(WorkerResponse::Error(err));
                                                success = false;
                                                break;
                                            }
                                        };

                                        let write_link = match &link_info.write_link {
                                            Some(link) => link,
                                            None => {
                                                let err = format!(
                                                    "Cloud API did not return writeLink for {}",
                                                    remote_path
                                                );
                                                eprintln!("{}", err);
                                                let _ = tx.send(WorkerResponse::Error(err));
                                                success = false;
                                                break;
                                            }
                                        };

                                        let data = all_uploads.get(remote_path).unwrap();
                                        println!(
                                            "[CloudSave][Upload] Sending {} bytes for {}",
                                            data.len(),
                                            remote_path
                                        );
                                        if let Some(tmp) = write_cloud_debug_blob(
                                            "upload",
                                            &app_name,
                                            remote_path,
                                            data,
                                        ) {
                                            println!(
                                                "[CloudSave][Upload] Temp payload path: {}",
                                                tmp.display()
                                            );
                                        }
                                        match client
                                            .client
                                            .put(write_link)
                                            .body(data.clone())
                                            .send()
                                        {
                                            Ok(resp) if resp.status().is_success() => {}
                                            Ok(resp) => {
                                                let err = format!(
                                                    "Failed to upload chunk {}: {}",
                                                    remote_path,
                                                    resp.status()
                                                );
                                                eprintln!("{}", err);
                                                let _ = tx.send(WorkerResponse::Error(err));
                                                success = false;
                                                break;
                                            }
                                            Err(e) => {
                                                let err = format!(
                                                    "Failed to upload chunk {}: {}",
                                                    remote_path, e
                                                );
                                                eprintln!("{}", err);
                                                let _ = tx.send(WorkerResponse::Error(err));
                                                success = false;
                                                break;
                                            }
                                        }
                                        let _ = tx.send(WorkerResponse::TaskProgress {
                                            task_name: format!("Uploading saves for {}", app_name),
                                            progress: (i + 1) as f32 / total as f32,
                                            is_paused: false,
                                            speed: None,
                                            eta: None,
                                        });
                                    }
                                    if success {
                                        let _ = tx.send(WorkerResponse::TaskFinished(format!(
                                            "Upload for {} complete. {} files processed.",
                                            app_name, total
                                        )));
                                    }
                                }
                                Err(e) => {
                                    let err = format!("Failed to create cloud save entries: {}", e);
                                    eprintln!("{}", err);
                                    let _ = tx.send(WorkerResponse::Error(err));
                                }
                            }
                        } else {
                            let _ = tx.send(WorkerResponse::Error("Not logged in".to_string()));
                        }
                        ctx_clone.request_repaint();
                    }
                    WorkerMsg::DownloadCloudSave {
                        app_name,
                        namespace,
                        save_path,
                    } => {
                        let _ = tx.send(WorkerResponse::TaskProgress {
                            task_name: format!("Downloading saves for {}", app_name),
                            progress: 0.0,
                            is_paused: false,
                            speed: None,
                            eta: None,
                        });
                        if let Ok(token) = crate::auth::load_token() {
                            match client.get_cloud_save_metadata(
                                &namespace,
                                &token.account_id,
                                &app_name,
                            ) {
                                Ok(mut manifest_files) => {
                                    manifest_files.sort_by(|a, b| a.file_name.cmp(&b.file_name));
                                    let mut success = true;
                                    let mut manifests_downloaded = 0;
                                    for file in manifest_files {
                                        let manifest_data = match client.download_cloud_file(
                                            &namespace,
                                            &token.account_id,
                                            &app_name,
                                            &file.file_name,
                                        ) {
                                            Ok(d) => d,
                                            Err(e) => {
                                                let err = format!(
                                                    "Failed to download manifest {}: {}",
                                                    file.file_name, e
                                                );
                                                eprintln!("{}", err);
                                                let _ = tx.send(WorkerResponse::Error(err));
                                                success = false;
                                                break;
                                            }
                                        };

                                        println!(
                                            "[CloudSave][Download] Received manifest {} ({} bytes)",
                                            file.file_name,
                                            manifest_data.len()
                                        );
                                        if let Some(tmp) = write_cloud_debug_blob(
                                            "download-manifest",
                                            &app_name,
                                            &file.file_name,
                                            &manifest_data,
                                        ) {
                                            println!(
                                                "[CloudSave][Download] Temp manifest path: {}",
                                                tmp.display()
                                            );
                                        }
                                        let manifest =
                                            match crate::manifest::parse_manifest(&manifest_data) {
                                                Ok(m) => m,
                                                Err(e) => {
                                                    let err = format!(
                                                        "Failed to parse manifest {}: {}",
                                                        file.file_name, e
                                                    );
                                                    eprintln!("{}", err);
                                                    let _ = tx.send(WorkerResponse::Error(err));
                                                    success = false;
                                                    break;
                                                }
                                            };

                                        let chunk_paths: Vec<String> = manifest
                                            .chunks
                                            .values()
                                            .map(|c| c.path(manifest.manifest_version))
                                            .collect();
                                        let chunk_links = match client.get_cloud_save_links(
                                            &token.account_id,
                                            &app_name,
                                            &chunk_paths,
                                        ) {
                                            Ok(links) => links,
                                            Err(e) => {
                                                let err =
                                                    format!("Failed to get chunk links: {}", e);
                                                eprintln!("{}", err);
                                                let _ = tx.send(WorkerResponse::Error(err));
                                                success = false;
                                                break;
                                            }
                                        };

                                        let mut downloaded_chunks =
                                            std::collections::HashMap::new();
                                        let total_chunks = manifest.chunks.len();
                                        for (i, (guid, chunk_info)) in
                                            manifest.chunks.iter().enumerate()
                                        {
                                            let path = chunk_info.path(manifest.manifest_version);
                                            let link_info = match chunk_links.get(&path) {
                                                Some(l) => l,
                                                None => {
                                                    let _ =
                                                        tx.send(WorkerResponse::Error(format!(
                                                            "Chunk {} not found in cloud",
                                                            path
                                                        )));
                                                    success = false;
                                                    break;
                                                }
                                            };
                                            let read_link = match &link_info.read_link {
                                                Some(rl) => rl,
                                                None => {
                                                    let _ = tx.send(WorkerResponse::Error(
                                                        format!("No read link for chunk {}", path),
                                                    ));
                                                    success = false;
                                                    break;
                                                }
                                            };

                                            let chunk_data =
                                                match client.download_manifest(read_link, None) {
                                                    Ok(d) => d,
                                                    Err(e) => {
                                                        let err = format!(
                                                            "Failed to download chunk {}: {}",
                                                            path, e
                                                        );
                                                        eprintln!("{}", err);
                                                        let _ = tx.send(WorkerResponse::Error(err));
                                                        success = false;
                                                        break;
                                                    }
                                                };

                                            println!("[CloudSave][Download] Received chunk {} ({} bytes compressed)", path, chunk_data.len());
                                            if let Some(tmp) = write_cloud_debug_blob(
                                                "download-chunk",
                                                &app_name,
                                                &path,
                                                &chunk_data,
                                            ) {
                                                println!(
                                                    "[CloudSave][Download] Temp chunk path: {}",
                                                    tmp.display()
                                                );
                                            }
                                            let raw_data =
                                                match crate::manifest::parse_chunk(&chunk_data) {
                                                    Ok(d) => d,
                                                    Err(e) => {
                                                        let err = format!(
                                                            "Failed to parse chunk {}: {}",
                                                            path, e
                                                        );
                                                        eprintln!("{}", err);
                                                        let _ = tx.send(WorkerResponse::Error(err));
                                                        success = false;
                                                        break;
                                                    }
                                                };
                                            println!("[CloudSave][Download] Parsed chunk {} into {} bytes", path, raw_data.len());
                                            downloaded_chunks.insert(*guid, raw_data);
                                            let _ = tx.send(WorkerResponse::TaskProgress {
                                                task_name: format!(
                                                    "Downloading chunks for {}",
                                                    app_name
                                                ),
                                                progress: (i + 1) as f32 / total_chunks as f32,
                                                is_paused: false,
                                                speed: None,
                                                eta: None,
                                            });
                                        }

                                        if !success {
                                            break;
                                        }

                                        for (_, file_manifest) in manifest.files {
                                            let target_file_path =
                                                save_path.join(&file_manifest.filename);
                                            if let Some(parent) = target_file_path.parent() {
                                                let _ = std::fs::create_dir_all(parent);
                                            }

                                            let mut file_data = Vec::with_capacity(
                                                file_manifest.file_size as usize,
                                            );
                                            for part in file_manifest.chunk_parts {
                                                if let Some(chunk_raw) =
                                                    downloaded_chunks.get(&part.guid)
                                                {
                                                    let start = part.offset as usize;
                                                    let end = start + part.size as usize;
                                                    if end <= chunk_raw.len() {
                                                        file_data.extend_from_slice(
                                                            &chunk_raw[start..end],
                                                        );
                                                    } else {
                                                        let _ = tx.send(WorkerResponse::Error(format!("Chunk part out of bounds for file {}", file_manifest.filename)));
                                                        success = false;
                                                        break;
                                                    }
                                                } else {
                                                    let _ =
                                                        tx.send(WorkerResponse::Error(format!(
                                                            "Missing chunk for file {}",
                                                            file_manifest.filename
                                                        )));
                                                    success = false;
                                                    break;
                                                }
                                            }
                                            if !success {
                                                break;
                                            }

                                            // Verify file SHA1
                                            {
                                                let mut hasher = Sha1::new();
                                                hasher.update(&file_data);
                                                let actual_hash = hasher.finalize();
                                                if actual_hash.as_slice() != &file_manifest.hash {
                                                    let _ =
                                                        tx.send(WorkerResponse::Error(format!(
                                                            "File SHA1 mismatch: {}",
                                                            file_manifest.filename
                                                        )));
                                                    success = false;
                                                    break;
                                                }
                                            }

                                            if let Err(e) =
                                                std::fs::write(&target_file_path, &file_data)
                                            {
                                                let err = format!(
                                                    "Failed to write file {}: {}",
                                                    file_manifest.filename, e
                                                );
                                                eprintln!("{}", err);
                                                let _ = tx.send(WorkerResponse::Error(err));
                                                success = false;
                                                break;
                                            }

                                            // Set modification time from manifest timestamp
                                            if let Some(timestamp_str) =
                                                file.manifest_name.strip_suffix(".manifest")
                                            {
                                                if let Ok(naive_dt) =
                                                    chrono::NaiveDateTime::parse_from_str(
                                                        timestamp_str,
                                                        "%Y.%m.%d-%H.%M.%S",
                                                    )
                                                {
                                                    let dt = naive_dt.and_utc();
                                                    let system_time: std::time::SystemTime =
                                                        dt.into();
                                                    if let Ok(f) = std::fs::OpenOptions::new()
                                                        .write(true)
                                                        .open(&target_file_path)
                                                    {
                                                        let _ = f.set_times(
                                                            std::fs::FileTimes::new()
                                                                .set_modified(system_time)
                                                                .set_accessed(system_time),
                                                        );
                                                    }
                                                }
                                            }
                                        }
                                        if !success {
                                            break;
                                        }
                                        manifests_downloaded += 1;
                                    }
                                    if success {
                                        let msg = format!("Cloud save download for {} complete. {} manifests processed. Save path: {}", app_name, manifests_downloaded, save_path.display());
                                        println!("{}", msg);
                                        let _ = tx.send(WorkerResponse::TaskFinished(msg));
                                    }
                                }
                                Err(e) => {
                                    let err = format!("Failed to get cloud metadata: {}", e);
                                    eprintln!("{}", err);
                                    let _ = tx.send(WorkerResponse::Error(err));
                                }
                            }
                        } else {
                            let _ = tx.send(WorkerResponse::Error("Not logged in".to_string()));
                        }
                        ctx_clone.request_repaint();
                    }
                    WorkerMsg::FetchInstallInfo { app_name, title } => {
                        let _ = tx.send(WorkerResponse::TaskProgress {
                            task_name: format!("Fetching install info for {}", app_name),
                            progress: 0.0,
                            is_paused: false,
                            speed: None,
                            eta: None,
                        });

                        let mut available_tags = Vec::new();

                        let asset_info = if app_name == crate::eos::EOS_OVERLAY_APP_ID {
                            Some((
                                "Windows".to_string(),
                                crate::eos::EOS_OVERLAY_NAMESPACE.to_string(),
                                crate::eos::EOS_OVERLAY_CATALOG_ID.to_string(),
                                app_name.clone(),
                                "Live".to_string(),
                            ))
                        } else {
                            if let Ok(assets) = client.get_game_assets("Windows") {
                                assets.iter().find(|a| a.app_name == app_name).map(|asset| {
                                    (
                                        "Windows".to_string(),
                                        asset.namespace.clone(),
                                        asset.catalog_item_id.clone(),
                                        asset.app_name.clone(),
                                        asset.label_name.clone(),
                                    )
                                })
                            } else {
                                None
                            }
                        };

                        if let Some((plat, namespace, catalog_id, app, label)) = asset_info {
                            if let Ok(manifest_info) = client.get_asset_manifest(
                                &plat,
                                &namespace,
                                &catalog_id,
                                &app,
                                &label,
                            ) {
                                // Update deployment_id in metadata if possible
                                if let Some(did) = extract_deployment_id(&manifest_info) {
                                    if let Some(mut meta) =
                                        crate::auth::load_local_metadata(&app_name)
                                    {
                                        meta.metadata.deployment_id = Some(did);
                                        let _ = crate::auth::save_local_metadata(&app_name, &meta);
                                    }
                                }

                                if let Some(url) = find_manifest_url(&manifest_info) {
                                    if let Ok(manifest_data) =
                                        client.download_manifest(&url, Some(app_name.as_str()))
                                    {
                                        if let Ok(manifest) =
                                            crate::manifest::parse_manifest(&manifest_data)
                                        {
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
                                    install_size =
                                        size.value.parse::<u64>().unwrap_or(0) * 1024 * 1024;
                                }
                            }
                        }

                        // Fallback/Estimate
                        if install_size == 0 {
                            install_size = 10 * 1024 * 1024 * 1024;
                        } // 10GB default
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
                    WorkerMsg::InstallGame {
                        app_name,
                        install_path,
                        selected_tags,
                        platform,
                    } => {
                        let _ = tx.send(WorkerResponse::TaskProgress {
                            task_name: format!("Preparing installation for {}", app_name),
                            progress: 0.0,
                            is_paused: false,
                            speed: None,
                            eta: None,
                        });

                        // Actual implementation: Create directory
                        if let Err(e) = std::fs::create_dir_all(&install_path) {
                            let _ = tx.send(WorkerResponse::Error(format!(
                                "Failed to create directory: {}",
                                e
                            )));
                            continue;
                        }

                        // Try to get manifest URL
                        let _ = tx.send(WorkerResponse::TaskProgress {
                            task_name: format!("Fetching manifest for {}", app_name),
                            progress: 0.05,
                            is_paused: false,
                            speed: None,
                            eta: None,
                        });

                        let mut manifest_data_opt = None;
                        let mut base_url_opt = None;
                        let mut manifest_path_saved = None;
                        let mut version = "1.0.0".to_string();
                        let mut install_size = 0u64;
                        let mut download_size = 0u64;

                        let asset_info = if app_name == crate::eos::EOS_OVERLAY_APP_ID {
                            Some((
                                "Windows".to_string(),
                                crate::eos::EOS_OVERLAY_NAMESPACE.to_string(),
                                crate::eos::EOS_OVERLAY_CATALOG_ID.to_string(),
                                app_name.clone(),
                                "Live".to_string(),
                            ))
                        } else {
                            match client.get_game_assets(&platform) {
                                Ok(assets) => {
                                    assets.iter().find(|a| a.app_name == app_name).map(|asset| {
                                        version = asset.build_version.clone();
                                        (
                                            platform.clone(),
                                            asset.namespace.clone(),
                                            asset.catalog_item_id.clone(),
                                            asset.app_name.clone(),
                                            asset.label_name.clone(),
                                        )
                                    })
                                }
                                Err(e) => {
                                    log::error!(
                                        "Failed to fetch assets for platform {}: {}",
                                        platform,
                                        e
                                    );
                                    None
                                }
                            }
                        };

                        if let Some((plat, namespace, catalog_id, app, label)) = asset_info {
                            match client.get_asset_manifest(
                                &plat,
                                &namespace,
                                &catalog_id,
                                &app,
                                &label,
                            ) {
                                Ok(manifest_info) => {
                                    // Update deployment_id in metadata if possible
                                    if let Some(did) = extract_deployment_id(&manifest_info) {
                                        if let Some(mut meta) =
                                            crate::auth::load_local_metadata(&app_name)
                                        {
                                            meta.metadata.deployment_id = Some(did);
                                            let _ =
                                                crate::auth::save_local_metadata(&app_name, &meta);
                                        }
                                    }

                                    if let Some(url) = find_manifest_url(&manifest_info) {
                                        match client
                                            .download_manifest(&url, Some(app_name.as_str()))
                                        {
                                            Ok(data) => {
                                                // Save manifest
                                                if let Some(mut p) = crate::auth::get_config_dir() {
                                                    p.push("manifests");
                                                    let _ = std::fs::create_dir_all(&p);
                                                    let manifest_path =
                                                        p.join(format!("{}.manifest", app_name));
                                                    if std::fs::write(&manifest_path, &data).is_ok()
                                                    {
                                                        manifest_path_saved = Some(
                                                            manifest_path
                                                                .to_string_lossy()
                                                                .to_string(),
                                                        );
                                                    }
                                                }
                                                manifest_data_opt = Some(data);
                                                base_url_opt = Some(
                                                    url.split('?')
                                                        .next()
                                                        .unwrap_or(&url)
                                                        .rsplit_once('/')
                                                        .map(|(b, _)| b.to_string())
                                                        .unwrap_or_else(|| url.to_string()),
                                                );
                                            }
                                            Err(e) => log::error!(
                                                "Failed to download manifest for {}: {}",
                                                app_name,
                                                e
                                            ),
                                        }
                                    } else {
                                        log::error!(
                                            "Manifest URL not found in asset manifest for {}",
                                            app_name
                                        );
                                    }
                                }
                                Err(e) => log::error!(
                                    "Failed to fetch asset manifest for {}: {}",
                                    app_name,
                                    e
                                ),
                            }
                        } else if app_name != crate::eos::EOS_OVERLAY_APP_ID {
                            log::error!(
                                "Asset not found for {} on platform {}",
                                app_name,
                                platform
                            );
                        }

                        let mut success = false;
                        let mut executable = String::new();
                        if let (Some(manifest_data), Some(base_url)) =
                            (manifest_data_opt, base_url_opt)
                        {
                            if let Ok(manifest) = crate::manifest::parse_manifest(&manifest_data) {
                                install_size = manifest.total_uncompressed_size;
                                download_size = manifest.total_download_size;
                                executable = manifest.meta.launch_exe.clone();
                                let user_agent = crate::api::get_ua_for_app(&app_name).to_string();
                                let downloader = crate::download::Downloader::new(
                                    base_url,
                                    user_agent,
                                    tx.clone(),
                                    cancel.clone(),
                                    pause.clone(),
                                );
                                match downloader.download_game(
                                    &manifest,
                                    &install_path,
                                    selected_tags,
                                ) {
                                    Ok(_) => success = true,
                                    Err(e) => {
                                        let _ = tx.send(WorkerResponse::Error(format!(
                                            "Download failed: {}",
                                            e
                                        )));
                                    }
                                }
                            } else {
                                let _ = tx.send(WorkerResponse::Error(
                                    "Failed to parse manifest".to_string(),
                                ));
                            }
                        } else {
                            let _ = tx.send(WorkerResponse::Error(
                                "Failed to fetch manifest".to_string(),
                            ));
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

                            let _ = tx.send(WorkerResponse::TaskFinished(format!(
                                "Installation of {} complete",
                                app_name
                            )));
                            let _ = tx.send(WorkerResponse::GamesScanned(installed));
                        }

                        ctx_clone.request_repaint();
                    }
                    WorkerMsg::UninstallGame(app_name) => {
                        let _ = tx.send(WorkerResponse::TaskProgress {
                            task_name: format!("Uninstalling {}", app_name),
                            progress: 0.0,
                            is_paused: false,
                            speed: None,
                            eta: None,
                        });

                        let mut installed = crate::auth::load_installed_games();
                        if let Some(game) =
                            installed.iter().find(|g| g.app_name == app_name).cloned()
                        {
                            let path = std::path::Path::new(&game.install_path);
                            if path.exists() {
                                let _ = tx.send(WorkerResponse::TaskProgress {
                                    task_name: format!("Deleting files for {}", app_name),
                                    progress: 0.5,
                                    is_paused: false,
                                    speed: None,
                                    eta: None,
                                });
                                if let Err(e) = std::fs::remove_dir_all(path) {
                                    let _ = tx.send(WorkerResponse::Error(format!(
                                        "Failed to delete game files: {}",
                                        e
                                    )));
                                }
                            }
                        }

                        installed.retain(|g| g.app_name != app_name);
                        let _ = crate::auth::save_installed_games(&installed);

                        let _ = tx.send(WorkerResponse::GamesScanned(installed));
                        let _ = tx.send(WorkerResponse::TaskFinished(format!(
                            "Uninstalled {}",
                            app_name
                        )));
                        ctx_clone.request_repaint();
                    }
                    WorkerMsg::ScanGames {
                        library,
                        search_paths,
                    } => {
                        let _ = tx.send(WorkerResponse::TaskProgress {
                            task_name: "Scanning for games...".to_string(),
                            progress: 0.0,
                            is_paused: false,
                            speed: None,
                            eta: None,
                        });
                        let installed = crate::auth::scan_and_import_games(&library, &search_paths);
                        let _ = tx.send(WorkerResponse::GamesScanned(installed));
                        let _ = tx.send(WorkerResponse::TaskFinished("Scan complete".to_string()));
                        ctx_clone.request_repaint();
                    }
                    WorkerMsg::EglSync => {
                        let _ = tx.send(WorkerResponse::TaskProgress {
                            task_name: "Syncing with EGL...".to_string(),
                            progress: 0.0,
                            is_paused: false,
                            speed: None,
                            eta: None,
                        });
                        let installed = crate::auth::scan_egl_manifests();
                        let _ = tx.send(WorkerResponse::GamesScanned(installed));
                        let _ = tx.send(WorkerResponse::TaskFinished(
                            "EGL Sync complete".to_string(),
                        ));
                        ctx_clone.request_repaint();
                    }
                    WorkerMsg::FetchAdvancedInfo {
                        app_name,
                        install_path,
                    } => {
                        let mut info = AdvancedInfo {
                            app_name: app_name.clone(),
                            ..Default::default()
                        };
                        let config = AppConfig::load();
                        let game_settings = config.games.get(&app_name);

                        // Prefix path
                        info.prefix_path = if std::env::consts::OS == "linux" {
                            if let Some(gs) = game_settings {
                                if gs.use_custom_pfx {
                                    gs.custom_pfx_path.clone()
                                } else if config.global.use_custom_pfx {
                                    config.global.custom_pfx_path.clone()
                                } else {
                                    get_default_compat_data_path()
                                }
                            } else if config.global.use_custom_pfx {
                                config.global.custom_pfx_path.clone()
                            } else {
                                get_default_compat_data_path()
                            }
                        } else {
                            None
                        };

                        // Save path resolution
                        let mut resolved_path = None;
                        let mut folder_hint = None;
                        let local_meta = crate::auth::load_local_metadata(&app_name);
                        if let Some(meta) = local_meta {
                            if let Some(attrs) = meta.metadata.custom_attributes {
                                if let Some(attr) = attrs.get("CloudSaveFolder") {
                                    folder_hint = Some(attr.value.clone());
                                }
                            }
                        }
                        if folder_hint.is_none() {
                            folder_hint = Some(app_name.clone());
                        }

                        let account_id = client.get_account_id();

                        if let Some(hint) = folder_hint {
                            if std::env::consts::OS == "linux" {
                                if let Some(mut p_users) = info.prefix_path.clone() {
                                    p_users.push("pfx/drive_c/users");
                                    if let Ok(entries) = std::fs::read_dir(&p_users) {
                                        for entry in entries.flatten() {
                                            let user_path = entry.path();
                                            if !user_path.is_dir() {
                                                continue;
                                            }

                                            let base_search_paths = vec![
                                                user_path.join("AppData/Local"),
                                                user_path.join("Documents"),
                                                user_path.join("Saved Games"),
                                                user_path.join("My Documents"),
                                            ];

                                            for base_path in base_search_paths {
                                                let possible_hints =
                                                    vec![hint.clone(), hint.replace(" ", "")];
                                                for h in possible_hints {
                                                    let p_hint = base_path.join(&h);
                                                    if p_hint.exists() {
                                                        if let Some(ref aid) = account_id {
                                                            // Check for folder with account ID
                                                            let p_aid = p_hint.join(aid);
                                                            if p_aid.exists() {
                                                                resolved_path = Some(p_aid);
                                                                break;
                                                            }

                                                            // Check for Saved/SaveGames/AccountID (UE style)
                                                            let p_saved = p_hint
                                                                .join("Saved/SaveGames")
                                                                .join(aid);
                                                            if p_saved.exists() {
                                                                resolved_path = Some(p_saved);
                                                                break;
                                                            }
                                                        }

                                                        // Fallback to hint folder itself if no account-specific folder found
                                                        if resolved_path.is_none() {
                                                            resolved_path = Some(p_hint);
                                                        }
                                                        break;
                                                    }
                                                }
                                                if resolved_path.is_some() {
                                                    break;
                                                }
                                            }
                                            if resolved_path.is_some() {
                                                break;
                                            }
                                        }
                                    }
                                }
                            } else if std::env::consts::OS == "windows" {
                                if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
                                    let mut p = std::path::PathBuf::from(local_app_data);
                                    p.push(&hint);
                                    if p.exists() {
                                        if let Some(ref aid) = account_id {
                                            let mut p_aid = p.clone();
                                            p_aid.push(aid);
                                            if p_aid.exists() {
                                                resolved_path = Some(p_aid);
                                            } else {
                                                let mut p_saved = p.clone();
                                                p_saved.push("Saved/SaveGames");
                                                p_saved.push(aid);
                                                if p_saved.exists() {
                                                    resolved_path = Some(p_saved);
                                                } else {
                                                    resolved_path = Some(p);
                                                }
                                            }
                                        } else {
                                            resolved_path = Some(p);
                                        }
                                    }
                                }
                            }
                        }
                        info.save_path = resolved_path;

                        let mut backup_path =
                            game_settings.and_then(|s| s.local_backup_path.clone());
                        if backup_path.is_none() {
                            if let Some(config_dir) = crate::auth::get_config_dir() {
                                backup_path = Some(config_dir.join(&app_name));
                            }
                        }
                        info.backup_path = backup_path;

                        // DLSS / DLSSD
                        let game_dir = std::path::Path::new(&install_path);
                        info.dlss_path = find_file_in_dir(game_dir, "nvngx_dlss.dll");
                        info.dlssd_path = find_file_in_dir(game_dir, "nvngx_dlssd.dll");

                        let _ = tx.send(WorkerResponse::AdvancedInfoFetched(info));
                        ctx_clone.request_repaint();
                    }
                    WorkerMsg::ListFiles {
                        app_name,
                        catalog_item_id,
                    } => {
                        let _ = tx.send(WorkerResponse::TaskProgress {
                            task_name: format!("Listing files for {}", app_name),
                            progress: 0.0,
                            is_paused: false,
                            speed: None,
                            eta: None,
                        });

                        let manifest_path_opt =
                            crate::auth::get_manifest_path(&app_name, &catalog_item_id);
                        let mut manifest_opt = None;

                        if let Some(manifest_path) = manifest_path_opt {
                            if let Ok(data) = std::fs::read(&manifest_path) {
                                manifest_opt = crate::manifest::parse_manifest(&data).ok();
                            }
                        }

                        if manifest_opt.is_none() {
                            if app_name == crate::eos::EOS_OVERLAY_APP_ID {
                                if let Ok(manifest_info) = client.get_asset_manifest(
                                    "Windows",
                                    crate::eos::EOS_OVERLAY_NAMESPACE,
                                    crate::eos::EOS_OVERLAY_CATALOG_ID,
                                    &app_name,
                                    "Live",
                                ) {
                                    if let Some(url) = find_manifest_url(&manifest_info) {
                                        if let Ok(manifest_data) =
                                            client.download_manifest(&url, Some(app_name.as_str()))
                                        {
                                            manifest_opt =
                                                crate::manifest::parse_manifest(&manifest_data)
                                                    .ok();
                                        }
                                    }
                                }
                            } else {
                                // Try common platforms if not found
                                for platform in &["Windows", "Mac", "Linux"] {
                                    if let Ok(assets) = client.get_game_assets(platform) {
                                        if let Some(asset) =
                                            assets.iter().find(|a| a.app_name == app_name)
                                        {
                                            if let Ok(manifest_info) = client.get_asset_manifest(
                                                platform,
                                                &asset.namespace,
                                                &asset.catalog_item_id,
                                                &asset.app_name,
                                                &asset.label_name,
                                            ) {
                                                if let Some(url) = find_manifest_url(&manifest_info)
                                                {
                                                    if let Ok(manifest_data) = client
                                                        .download_manifest(
                                                            &url,
                                                            Some(app_name.as_str()),
                                                        )
                                                    {
                                                        manifest_opt =
                                                            crate::manifest::parse_manifest(
                                                                &manifest_data,
                                                            )
                                                            .ok();
                                                        if manifest_opt.is_some() {
                                                            break;
                                                        }
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
                            let _ = tx.send(WorkerResponse::TaskFinished(format!(
                                "Listed files for {}",
                                app_name
                            )));
                        } else {
                            let _ = tx.send(WorkerResponse::Error(format!(
                                "Could not find manifest for {}",
                                app_name
                            )));
                        }
                        ctx_clone.request_repaint();
                    }
                    WorkerMsg::LaunchGame { app_name, offline } => {
                        println!("--- Launching Game: {} ---", app_name);
                        println!("[1/7] Loading installed game metadata...");
                        let installed_games = crate::auth::load_installed_games();
                        let installed = installed_games
                            .iter()
                            .find(|g| g.app_name == app_name)
                            .cloned();

                        if let Some(installed) = installed {
                            println!("[2/7] Checking configuration and authentication...");
                            let local_meta = crate::auth::load_local_metadata(&app_name);
                            let lib_item =
                                cached_library_items.iter().find(|i| i.app_name == app_name);
                            let path = std::path::PathBuf::from(&installed.install_path);
                            let config = AppConfig::load();
                            let mut found = false;

                            let game_settings = config.games.get(&app_name);

                            // Check if game can run offline
                            let can_run_offline = local_meta
                                .as_ref()
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
                                        let _ = tx.send(WorkerResponse::Error(format!(
                                            "Failed to fetch game token: {}",
                                            e
                                        )));
                                        continue;
                                    }
                                }
                            } else {
                                println!("      Launching in offline mode.");
                            }

                            if token == "0" && !can_run_offline {
                                println!("ERROR: This game cannot run offline and no token was provided.");
                                let _ = tx.send(WorkerResponse::Error(
                                    "This game cannot run offline and no token was provided"
                                        .to_string(),
                                ));
                                continue;
                            }

                            // [3/7] Cloud Save Sync
                            let sync_enabled =
                                game_settings.map(|s| s.cloud_sync_enabled).unwrap_or(true);
                            if !offline && sync_enabled {
                                println!("[3/7] Checking cloud saves...");
                                if let (Some(auth_token), Some(item)) =
                                    (crate::auth::load_token().ok(), lib_item)
                                {
                                    let save_path = game_settings.and_then(|s| s.save_path.clone());
                                    if let Some(sp) = save_path {
                                        let local_time = get_latest_local_save_time(&sp);
                                        match client.get_cloud_save_metadata(&item.namespace, &auth_token.account_id, &app_name) {
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
                                        println!(
                                            "      Save path not configured, skipping sync check."
                                        );
                                    }
                                }
                            } else {
                                println!("[3/7] Cloud sync skipped (offline or disabled).");
                            }

                            // Pre-launch command
                            let pre_launch = if let Some(gs) = game_settings {
                                if !gs.pre_launch_command.is_empty() {
                                    Some(&gs.pre_launch_command)
                                } else if !config.global.pre_launch_command.is_empty() {
                                    Some(&config.global.pre_launch_command)
                                } else {
                                    None
                                }
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
                                                log::error!(
                                                    "Pre-launch command failed with status: {}",
                                                    status
                                                );
                                            } else {
                                                println!("      Pre-launch command finished successfully.");
                                            }
                                        }
                                        Err(e) => {
                                            println!(
                                                "ERROR: Failed to run pre-launch command: {}",
                                                e
                                            );
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
                                    let exe_path = path.join(
                                        installed
                                            .executable
                                            .replace('\\', "/")
                                            .trim_start_matches('/'),
                                    );
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
                                                        let filename = entry_path
                                                            .file_name()
                                                            .unwrap_or_default()
                                                            .to_string_lossy()
                                                            .to_lowercase();
                                                        // Skip known non-game executables
                                                        if !filename.contains("crash")
                                                            && !filename.contains("unins")
                                                            && !filename.contains("redist")
                                                            && !filename.contains("prereq")
                                                        {
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

                            let namespace = lib_item.map(|i| i.namespace.clone()).or_else(|| {
                                local_meta.as_ref().map(|m| m.metadata.namespace.clone())
                            });

                            println!("[6/7] Handling ownership token and launch parameters...");
                            // Check if game requires ownership token
                            let mut requires_ot = local_meta
                                .as_ref()
                                .and_then(|m| m.metadata.custom_attributes.as_ref())
                                .and_then(|attrs| attrs.get("OwnershipToken"))
                                .map(|a| a.value.to_lowercase() == "true")
                                .unwrap_or(false);

                            if !requires_ot {
                                if let Some(item) = lib_item {
                                    if let Some(meta) = &item.metadata {
                                        requires_ot = meta["customAttributes"]["OwnershipToken"]
                                            ["value"]
                                            .as_str()
                                            .map(|v| v.to_lowercase() == "true")
                                            .unwrap_or(false);
                                    }
                                }
                            }

                            let deployment_id = local_meta
                                .as_ref()
                                .and_then(|m| m.metadata.deployment_id.clone());

                            let mut ovt_path_opt = None;

                            // Get ownership token if needed and not offline
                            if !offline {
                                if let Some(item) = lib_item {
                                    if requires_ot {
                                        println!("      Fetching ownership token...");
                                        log::info!("Fetching ownership token for {}...", app_name);
                                        match client.get_ownership_token(
                                            &item.namespace,
                                            &item.catalog_item_id,
                                        ) {
                                            Ok(ovt_bytes) => {
                                                let ovt_path = std::env::temp_dir().join(format!(
                                                    "{}{}.ovt",
                                                    item.namespace, item.catalog_item_id
                                                ));
                                                match std::fs::write(&ovt_path, &ovt_bytes) {
                                                    Ok(_) => {
                                                        println!(
                                                            "      Ownership token saved to {:?}",
                                                            ovt_path
                                                        );
                                                        log::info!(
                                                            "Saved ownership token to {:?}",
                                                            ovt_path
                                                        );
                                                        ovt_path_opt = Some(ovt_path);
                                                    }
                                                    Err(e) => {
                                                        println!("ERROR: Failed to save ownership token: {}", e);
                                                        log::error!(
                                                            "Failed to save ownership token: {}",
                                                            e
                                                        );
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

                                let tool = game_settings
                                    .and_then(|s| s.compatibility_tool.clone())
                                    .or_else(|| config.global.compatibility_tool.clone());
                                let custom_path = game_settings
                                    .and_then(|s| s.custom_compatibility_path.clone())
                                    .or_else(|| config.global.custom_compatibility_path.clone());

                                let mut cmd = if std::env::consts::OS == "linux" {
                                    let mut c = match tool {
                                        Some(CompatibilityTool::UmuLauncher) => {
                                            let mut command =
                                                std::process::Command::new("/usr/bin/umu-run");
                                            let store = game_settings
                                                .and_then(|s| s.umu_store.clone())
                                                .or_else(|| Some(config.global.umu_store.clone()))
                                                .unwrap_or_else(|| "egs".to_string());
                                            command.env("STORE", store);
                                            command.env("GAMEID", "umu-default");

                                            let pfx_path = if let Some(gs) = game_settings {
                                                if gs.use_custom_pfx {
                                                    gs.custom_pfx_path.clone()
                                                } else if config.global.use_custom_pfx {
                                                    config.global.custom_pfx_path.clone()
                                                } else {
                                                    get_default_compat_data_path()
                                                }
                                            } else if config.global.use_custom_pfx {
                                                config.global.custom_pfx_path.clone()
                                            } else {
                                                get_default_compat_data_path()
                                            };

                                            if let Some(path) = pfx_path {
                                                command.env("WINEPREFIX", path);
                                            }
                                            if let Some(path) = custom_path {
                                                command.env("PROTONPATH", path);
                                            }
                                            command
                                        }
                                        Some(CompatibilityTool::SteamProton)
                                        | Some(CompatibilityTool::CustomProtonWine) => {
                                            if let Some(path) = custom_path {
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
                                                        if gs.use_custom_pfx {
                                                            gs.custom_pfx_path.clone()
                                                        } else if config.global.use_custom_pfx {
                                                            config.global.custom_pfx_path.clone()
                                                        } else {
                                                            get_default_compat_data_path()
                                                        }
                                                    } else if config.global.use_custom_pfx {
                                                        config.global.custom_pfx_path.clone()
                                                    } else {
                                                        get_default_compat_data_path()
                                                    };

                                                    if let Some(path) = pfx_path {
                                                        command.env("STEAM_COMPAT_DATA_PATH", path);
                                                    }
                                                    if let Some(home) = home::home_dir() {
                                                        command.env(
                                                            "STEAM_COMPAT_CLIENT_INSTALL_PATH",
                                                            home.join(".local/share/Steam"),
                                                        );
                                                    }
                                                    command.arg("run");
                                                } else {
                                                    let pfx_path = if let Some(gs) = game_settings {
                                                        if gs.use_custom_pfx {
                                                            gs.custom_pfx_path.clone()
                                                        } else if config.global.use_custom_pfx {
                                                            config.global.custom_pfx_path.clone()
                                                        } else {
                                                            None
                                                        }
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
                                                let mut command =
                                                    std::process::Command::new("wine");
                                                let pfx_path = if let Some(gs) = game_settings {
                                                    if gs.use_custom_pfx {
                                                        gs.custom_pfx_path.clone()
                                                    } else if config.global.use_custom_pfx {
                                                        config.global.custom_pfx_path.clone()
                                                    } else {
                                                        None
                                                    }
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
                                        _ => {
                                            let mut command = std::process::Command::new("wine");
                                            let pfx_path = if let Some(gs) = game_settings {
                                                if gs.use_custom_pfx {
                                                    gs.custom_pfx_path.clone()
                                                } else if config.global.use_custom_pfx {
                                                    config.global.custom_pfx_path.clone()
                                                } else {
                                                    None
                                                }
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

                                let eos_installed = installed_games
                                    .iter()
                                    .any(|g| g.app_name == crate::eos::EOS_OVERLAY_APP_ID);
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

                                // Proton Prefer SDL
                                let prefer_sdl = game_settings
                                    .map(|s| s.proton_prefer_sdl)
                                    .unwrap_or(config.global.proton_prefer_sdl);
                                if prefer_sdl {
                                    cmd.env("PROTON_PREFER_SDL", "1");
                                }

                                // Global environment variables
                                for (k, v) in &config.global.env_vars {
                                    cmd.env(k, v);
                                }

                                // Per-game environment variables (override global)
                                if let Some(gs) = game_settings {
                                    for (k, v) in &gs.env_vars {
                                        cmd.env(k, v);
                                    }
                                }

                                // Additional environment variables and parameters...
                                if let Some(val) = game_settings
                                    .and_then(|s| s.steam_compat_install_path.as_ref())
                                    .or_else(|| config.global.steam_compat_install_path.as_ref())
                                {
                                    cmd.env("STEAM_COMPAT_INSTALL_PATH", val);
                                }
                                if let Some(val) = game_settings
                                    .and_then(|s| s.steam_compat_client_install_path.as_ref())
                                    .or_else(|| {
                                        config.global.steam_compat_client_install_path.as_ref()
                                    })
                                {
                                    cmd.env("STEAM_COMPAT_CLIENT_INSTALL_PATH", val);
                                }
                                if let Some(val) = game_settings
                                    .and_then(|s| s.steam_compat_data_path.as_ref())
                                    .or_else(|| config.global.steam_compat_data_path.as_ref())
                                {
                                    cmd.env("STEAM_COMPAT_DATA_PATH", val);
                                }
                                if let Some(val) = game_settings
                                    .and_then(|s| s.steam_compat_app_id.as_ref())
                                    .or_else(|| config.global.steam_compat_app_id.as_ref())
                                {
                                    cmd.env("STEAM_COMPAT_APP_ID", val);
                                }
                                cmd.env("APP_NAME", &app_name);

                                // Log launch info
                                println!("--- Launch Info ---");
                                println!("Executable: {:?}", exe_path);
                                let vars = [
                                    "GAMEID",
                                    "STORE",
                                    "STEAM_COMPAT_INSTALL_PATH",
                                    "LD_PRELOAD",
                                    "STEAM_COMPAT_CLIENT_INSTALL_PATH",
                                    "WINEPREFIX",
                                    "STEAM_COMPAT_DATA_PATH",
                                    "PROTONPATH",
                                    "STEAM_COMPAT_APP_ID",
                                    "APP_NAME",
                                    "PROTON_PREFER_SDL",
                                ];
                                for var in vars {
                                    let val = cmd
                                        .get_envs()
                                        .find(
                                            |(k, _): &(
                                                &std::ffi::OsStr,
                                                Option<&std::ffi::OsStr>,
                                            )| {
                                                k.to_str() == Some(var)
                                            },
                                        )
                                        .and_then(|(_, v)| {
                                            v.map(|v| v.to_string_lossy().into_owned())
                                        })
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
                                    Ok(mut child) => {
                                        println!("SUCCESS: Game launched successfully!");
                                        let _ =
                                            tx.send(WorkerResponse::GameLaunched(app_name.clone()));

                                        let (stop_tx, stop_rx) = channel::<()>();
                                        stop_senders.insert(app_name.clone(), stop_tx);

                                        let tx_clone = tx.clone();
                                        let app_name_clone = app_name.clone();

                                        std::thread::spawn(move || {
                                            let mut stopped_gracefully = false;
                                            loop {
                                                match stop_rx.try_recv() {
                                                    Ok(_) => {
                                                        // Stop requested!
                                                        #[cfg(unix)]
                                                        {
                                                            unsafe {
                                                                libc::kill(
                                                                    child.id() as i32,
                                                                    libc::SIGTERM,
                                                                );
                                                            }
                                                        }

                                                        // Wait up to 10 seconds
                                                        for _ in 0..100 {
                                                            match child.try_wait() {
                                                                Ok(Some(_)) => {
                                                                    stopped_gracefully = true;
                                                                    break;
                                                                }
                                                                _ => std::thread::sleep(std::time::Duration::from_millis(100)),
                                                            }
                                                        }

                                                        if !stopped_gracefully {
                                                            let _ = child.kill();
                                                        }
                                                        break;
                                                    }
                                                    Err(
                                                        std::sync::mpsc::TryRecvError::Disconnected,
                                                    ) => break,
                                                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                                                        match child.try_wait() {
                                                            Ok(Some(_)) => break,
                                                            _ => std::thread::sleep(
                                                                std::time::Duration::from_millis(
                                                                    100,
                                                                ),
                                                            ),
                                                        }
                                                    }
                                                }
                                            }
                                            let _ = tx_clone
                                                .send(WorkerResponse::GameStopped(app_name_clone));
                                        });

                                        found = true;
                                        break 'search;
                                    }
                                    Err(e) => {
                                        println!("ERROR: Failed to launch game: {}", e);
                                        log::error!(
                                            "Failed to spawn process for {:?}: {}",
                                            exe_path,
                                            e
                                        );
                                    }
                                }
                            }

                            if !found {
                                println!("ERROR: Failed to launch game {}: no executable found or all failed to start", app_name);
                                let _ = tx.send(WorkerResponse::Error(format!(
                                    "Could not find or launch executable in {}",
                                    installed.install_path
                                )));
                            }
                        }
                    }
                    WorkerMsg::QueryEosStatus { prefix } => {
                        let installed_games = crate::auth::load_installed_games();
                        let eos_installed = installed_games
                            .iter()
                            .find(|g| g.app_name == crate::eos::EOS_OVERLAY_APP_ID);

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
                    WorkerMsg::UpdateEosRegistry {
                        overlay_path,
                        prefix,
                        enable,
                    } => {
                        let res = if enable {
                            crate::eos::add_registry_entries(&overlay_path, &prefix)
                        } else {
                            crate::eos::remove_registry_entries(&prefix)
                        };

                        if let Err(e) = res {
                            let _ = tx.send(WorkerResponse::Error(format!(
                                "Failed to update registry: {}",
                                e
                            )));
                        } else {
                            let msg = if enable {
                                "EOS Registry updated"
                            } else {
                                "EOS Registry entries removed"
                            };
                            let _ = tx.send(WorkerResponse::TaskFinished(msg.to_string()));

                            // Re-query status
                            let installed_games = crate::auth::load_installed_games();
                            let eos_installed = installed_games
                                .iter()
                                .find(|g| g.app_name == crate::eos::EOS_OVERLAY_APP_ID);
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
            running_apps: HashSet::new(),
            save_sync_status: None,
            unaccepted_eulas: Vec::new(),
            install_info: None,
            selected_tags: HashSet::new(),
            current_task: None,
            manifest_files: Vec::new(),
            manifest_search_query: String::new(),
            eos_status: crate::eos::EosOverlayStatus::default(),
            eos_prefix_path: None,
            advanced_info: None,
            new_env_key: String::new(),
            new_env_val: String::new(),
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
                    if let Some(ref eulas) = info.eula_ids {
                        if !eulas.is_empty() {
                            let _ = self.tx.send(WorkerMsg::CheckEula(eulas.clone()));
                        }
                    }
                    self.selected_game = Some(info);
                    self.current_view = View::GameDetail;
                }
                WorkerResponse::AssetsFetched(assets) => {
                    self.assets = assets;
                }
                WorkerResponse::ImageFetched {
                    app_name,
                    image_type,
                    image,
                } => {
                    let name = format!("{}_{}", app_name, image_type);
                    self.images.insert(
                        (app_name, image_type),
                        egui_extras::RetainedImage::from_color_image(name, image),
                    );
                }
                WorkerResponse::Error(e) => {
                    self.status_message = format!("Error: {}", e);
                    self.current_task = None;
                }
                WorkerResponse::TaskProgress {
                    task_name,
                    progress,
                    is_paused,
                    speed,
                    eta,
                } => {
                    self.current_task = Some(TaskStatus {
                        name: task_name.clone(),
                        progress,
                        is_paused,
                        speed: speed.clone().unwrap_or_default(),
                        eta: eta.clone().unwrap_or_default(),
                    });
                    let mut msg = format!("{}: {:.0}%", task_name, progress * 100.0);
                    if let Some(s) = speed {
                        msg.push_str(&format!(" | {}", s));
                    }
                    if let Some(e) = eta {
                        msg.push_str(&format!(" | ETA: {}", e));
                    }
                    if is_paused {
                        msg.push_str(" (Paused)");
                    }
                    self.status_message = msg;
                }
                WorkerResponse::TaskFinished(msg) => {
                    self.status_message = msg;
                    self.current_task = None;
                }
                WorkerResponse::EulaStatusFetched(eulas) => {
                    self.unaccepted_eulas = eulas;
                }
                WorkerResponse::OriginUriFetched(uri) => {
                    let _ = open::that(uri);
                }
                WorkerResponse::SaveSyncStatusFetched {
                    app_name,
                    files,
                    local_time,
                    remote_time,
                    backup_time,
                    error,
                } => {
                    self.save_sync_status = Some(SaveSyncStatus {
                        app_name,
                        files,
                        local_time,
                        remote_time,
                        backup_time,
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
                        let was_installed = self
                            .installed_games
                            .iter()
                            .any(|g| g.app_name == item.app_name);
                        let is_now_installed = games.iter().any(|g| g.app_name == item.app_name);

                        if was_installed != is_now_installed {
                            let local_meta = crate::auth::load_local_metadata(&item.app_name);
                            if let Some(meta) = local_meta {
                                for img_info in &meta.metadata.key_images {
                                    self.images.remove(&(
                                        item.app_name.clone(),
                                        img_info.image_type.clone(),
                                    ));
                                    self.fetching_images.remove(&(
                                        item.app_name.clone(),
                                        img_info.image_type.clone(),
                                    ));
                                }
                            }
                        }
                    }
                    self.installed_games = games;
                }
                WorkerResponse::FilesListed(files) => {
                    self.manifest_files = files;
                }
                WorkerResponse::GameLaunched(app_name) => {
                    self.running_apps.insert(app_name);
                }
                WorkerResponse::GameStopped(app_name) => {
                    self.running_apps.remove(&app_name);
                }
                WorkerResponse::EosStatusFetched(status) => {
                    self.eos_status = status;
                }
                WorkerResponse::AdvancedInfoFetched(info) => {
                    self.advanced_info = Some(info);
                }
            }
        }

        egui::SidePanel::left("side_panel")
            .resizable(true)
            .default_width(150.0)
            .show(ctx, |ui| {
                ui.heading("Legendary Rust");
                ui.add_space(10.0);
                if ui
                    .selectable_label(self.current_view == View::Library, "Library")
                    .clicked()
                {
                    self.current_view = View::Library;
                }
                if ui
                    .selectable_label(self.current_view == View::Settings, "Settings")
                    .clicked()
                {
                    self.current_view = View::Settings;
                }
                if ui
                    .selectable_label(self.current_view == View::Tasks, "Manager")
                    .clicked()
                {
                    self.current_view = View::Tasks;
                }
                if ui
                    .selectable_label(self.current_view == View::EosOverlay, "EOS Overlay")
                    .clicked()
                {
                    self.current_view = View::EosOverlay;
                    let _ = self.tx.send(WorkerMsg::QueryEosStatus { prefix: None });
                }
                if self.token.is_some() {
                    if ui
                        .selectable_label(self.current_view == View::Account, "Account")
                        .clicked()
                    {
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
        ui.heading("Manager");
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
                            if let Some(t) = &mut self.current_task {
                                t.is_paused = false;
                            }
                            self.worker_pause.store(false, Ordering::SeqCst);
                            let _ = self.tx.send(WorkerMsg::ResumeTask);
                        }
                    } else {
                        if ui.button("Pause").clicked() {
                            if let Some(t) = &mut self.current_task {
                                t.is_paused = true;
                            }
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
            ui.label(
                "1. Click the button below to open the Epic Games login page in your browser.",
            );
            ui.label(
                "2. After logging in, you will see a JSON response containing 'authorizationCode'.",
            );
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

                    if !self.search_query.is_empty()
                        && !title
                            .to_lowercase()
                            .contains(&self.search_query.to_lowercase())
                        && !item
                            .app_name
                            .to_lowercase()
                            .contains(&self.search_query.to_lowercase())
                    {
                        continue;
                    }

                    let is_installed = self
                        .installed_games
                        .iter()
                        .any(|g| g.app_name == item.app_name);

                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            let first_img_info = local_meta
                                .as_ref()
                                .and_then(|m| m.metadata.key_images.get(0));
                            if let Some(info) = first_img_info {
                                if let Some(img) = self
                                    .images
                                    .get(&(item.app_name.clone(), info.image_type.clone()))
                                {
                                    let size = img.size_vec2();
                                    let ratio = size.x / size.y;
                                    img.show_max_size(ui, egui::vec2(100.0 * ratio, 100.0));
                                } else {
                                    if !self
                                        .fetching_images
                                        .contains(&(item.app_name.clone(), info.image_type.clone()))
                                    {
                                        self.fetching_images.insert((
                                            item.app_name.clone(),
                                            info.image_type.clone(),
                                        ));
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
                                        if let Some(installed) = self
                                            .installed_games
                                            .iter()
                                            .find(|g| g.app_name == item.app_name)
                                        {
                                            if let Some(asset) = self
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
                                        self.selected_app_name = Some(item.app_name.clone());
                                        self.unaccepted_eulas.clear();
                                        self.advanced_info = None;
                                        let _ = self.tx.send(WorkerMsg::FetchAssets);
                                        let _ = self.tx.send(WorkerMsg::FetchGameInfo {
                                            app_name: item.app_name.clone(),
                                            namespace: item.namespace.clone(),
                                            catalog_item_id: item.catalog_item_id.clone(),
                                        });

                                        if let Some(installed) = self
                                            .installed_games
                                            .iter()
                                            .find(|g| g.app_name == item.app_name)
                                        {
                                            let _ = self.tx.send(WorkerMsg::FetchAdvancedInfo {
                                                app_name: item.app_name.clone(),
                                                install_path: installed.install_path.clone(),
                                            });
                                        }

                                        self.status_message = "Fetching game info...".to_string();
                                    }
                                });
                                ui.horizontal(|ui| {
                                    ui.label(format!("ID: {}", item.app_name));
                                    if let Some(installed) = self
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
                                    let _ = self.tx.send(WorkerMsg::LaunchOrigin(app_name.clone()));
                                }
                            });
                        }

                        if let Some(installed) = self.installed_games.iter().find(|g| g.app_name == app_name) {
                            ui.label(format!("Installed at: {}", installed.install_path));
                            if ui.button("☁ Compare local and cloud save files").clicked() {
                                if let Some(item) = self.library.iter().find(|i| i.app_name == app_name) {
                                    let save_path = self.advanced_info.as_ref().and_then(|i| i.save_path.clone())
                                        .or_else(|| self.config.games.get(&app_name).and_then(|s| s.save_path.clone()));
                                    let backup_path = self.advanced_info.as_ref().and_then(|i| i.backup_path.clone());

                                    self.save_sync_status = Some(SaveSyncStatus {
                                        app_name: app_name.clone(),
                                        files: Vec::new(),
                                        local_time: None,
                                        remote_time: None,
                                        backup_time: None,
                                        loading: true,
                                        error: None,
                                    });
                                    self.current_view = View::SaveSync;

                                    let _ = self.tx.send(WorkerMsg::SyncCloudSaves {
                                        app_name: app_name.clone(),
                                        namespace: item.namespace.clone(),
                                        save_path,
                                        backup_path,
                                    });
                                }
                            }
                        }

                        let settings = self.config.games.get(&app_name);

                        if let Some(s) = settings {
                            let hours = s.play_time_seconds / 3600;
                            let mins = (s.play_time_seconds % 3600) / 60;
                            ui.label(format!("Time in game: {}h {}m", hours, mins));
                        }

                        if let Some(info) = &self.advanced_info {
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
                            let _ = self.config.save();
                            if let Some(installed) = self.installed_games.iter().find(|g| g.app_name == app_name) {
                                let _ = self.tx.send(WorkerMsg::FetchAdvancedInfo {
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
                if !self.unaccepted_eulas.is_empty() {
                    ui.add_space(10.0);
                    ui.group(|ui| {
                        ui.colored_label(egui::Color32::YELLOW, egui::RichText::new("⚠ Unaccepted EULAs").strong());
                        for eula in self.unaccepted_eulas.clone() {
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
                                    let _ = self.tx.send(WorkerMsg::AcceptEula { eula_id: id, version });
                                    // Remove from list
                                    self.unaccepted_eulas.retain(|e| e["key"] != eula["key"]);
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
                    let game_settings = self.config.games.entry(app_name.clone()).or_default();

                    if let Some(info) = &self.advanced_info {
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
                        if let Some(info) = &self.advanced_info {
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
                        if let Some(info) = &self.advanced_info {
                            if let (Some(src), Some(dst)) = (&info.save_path, &info.backup_path) {
                                if ui.button("Backup Now (Local -> Backup)").clicked() {
                                    let _ = self.tx.send(WorkerMsg::CreateLocalBackup {
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

                        let new_key = &mut self.new_env_key;
                        let new_val = &mut self.new_env_val;
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
                        let _ = self.config.save();
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
                    let is_running = self.running_apps.contains(&app_name);
                    let button_text = if is_running { "Stop Game" } else { "Start Game" };

                    ui.vertical(|ui| {
                        if ui.button(egui::RichText::new(button_text).size(24.0).strong()).clicked() {
                            if is_running {
                                let _ = self.tx.send(WorkerMsg::StopGame(app_name.clone()));
                                self.status_message = format!("Stopping game: {}...", app_name);
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
                            ui.horizontal(|ui| {
                                if ui.button(egui::RichText::new("Install").size(24.0).strong()).clicked() {
                                    let _ = self.tx.send(WorkerMsg::FetchInstallInfo {
                                        app_name: app_name.clone(),
                                        title: game.title.clone(),
                                    });
                                }
                                if ui.button(egui::RichText::new("Verify").size(24.0).strong()).clicked() {
                                    let catalog_item_id = self.library.iter().find(|i| i.app_name == app_name)
                                        .map(|i| i.catalog_item_id.clone())
                                        .unwrap_or_default();
                                    let _ = self.tx.send(WorkerMsg::VerifyGame {
                                        app_name: app_name.clone(),
                                        catalog_item_id
                                    });
                                    self.current_view = View::Tasks;
                                }
                            });
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
                    ui.colored_label(
                        egui::Color32::LIGHT_RED,
                        egui::RichText::new("Failed to fetch cloud save metadata").strong(),
                    );
                    ui.label(err);
                    if ui.button("Retry").clicked() {
                        if let Some(item) =
                            self.library.iter().find(|i| i.app_name == status.app_name)
                        {
                            let save_path = self
                                .config
                                .games
                                .get(&status.app_name)
                                .and_then(|s| s.save_path.clone());
                            let backup_path = self
                                .advanced_info
                                .as_ref()
                                .and_then(|i| i.backup_path.clone());
                            self.save_sync_status = Some(SaveSyncStatus {
                                app_name: status.app_name.clone(),
                                files: Vec::new(),
                                local_time: None,
                                remote_time: None,
                                backup_time: None,
                                loading: true,
                                error: None,
                            });
                            let _ = self.tx.send(WorkerMsg::SyncCloudSaves {
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

            let can_upload = self
                .config
                .games
                .get(&status.app_name)
                .and_then(|s| s.save_path.as_ref())
                .is_some()
                || (self
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

                                let current_save_path = self
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
                                } else if let Some(info) = &self.advanced_info {
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
                                        let game_settings = self
                                            .config
                                            .games
                                            .entry(status.app_name.clone())
                                            .or_default();
                                        game_settings.save_path = Some(path.clone());
                                        let _ = self.config.save();

                                        // Refresh status
                                        if let Some(item) = self
                                            .library
                                            .iter()
                                            .find(|i| i.app_name == status.app_name)
                                        {
                                            let backup_path = self
                                                .advanced_info
                                                .as_ref()
                                                .and_then(|i| i.backup_path.clone());
                                            self.save_sync_status = Some(SaveSyncStatus {
                                                app_name: status.app_name.clone(),
                                                files: Vec::new(),
                                                local_time: None,
                                                remote_time: None,
                                                backup_time: None,
                                                loading: true,
                                                error: None,
                                            });
                                            let _ = self.tx.send(WorkerMsg::SyncCloudSaves {
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
                                            let save_path = self
                                                .advanced_info
                                                .as_ref()
                                                .and_then(|i| i.save_path.clone())
                                                .or_else(|| {
                                                    self.config
                                                        .games
                                                        .get(&status.app_name)
                                                        .and_then(|s| s.save_path.clone())
                                                });

                                            if let (Some(item), Some(sp)) = (
                                                self.library
                                                    .iter()
                                                    .find(|i| i.app_name == status.app_name),
                                                save_path,
                                            ) {
                                                let _ = self.tx.send(WorkerMsg::UploadCloudSave {
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
                                        let backup_path = self
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
                                                self.library
                                                    .iter()
                                                    .find(|i| i.app_name == status.app_name),
                                                backup_path,
                                            ) {
                                                let _ = self.tx.send(WorkerMsg::UploadCloudSave {
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
                                                let save_path = self
                                                    .advanced_info
                                                    .as_ref()
                                                    .and_then(|i| i.save_path.clone())
                                                    .or_else(|| {
                                                        self.config
                                                            .games
                                                            .get(&status.app_name)
                                                            .and_then(|s| s.save_path.clone())
                                                    });

                                                if let (Some(item), Some(sp)) = (
                                                    self.library
                                                        .iter()
                                                        .find(|i| i.app_name == status.app_name),
                                                    save_path,
                                                ) {
                                                    let _ = self.tx.send(
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
                                                let backup_path = self
                                                    .advanced_info
                                                    .as_ref()
                                                    .and_then(|i| i.backup_path.clone());
                                                if let (Some(item), Some(bp)) = (
                                                    self.library
                                                        .iter()
                                                        .find(|i| i.app_name == status.app_name),
                                                    backup_path,
                                                ) {
                                                    let _ = self.tx.send(
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

                    let game_settings = self
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
                        if let Some(info) = &self.advanced_info {
                            if let Some(p) = &info.save_path {
                                let _ = open::that(p);
                            }
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

        ui.horizontal(|ui| {
            ui.label("Manage Prefix:");
            let _path_str = self
                .eos_prefix_path
                .as_ref()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| "Default Prefix".to_string());
            if ui
                .selectable_label(self.eos_prefix_path.is_none(), "Default")
                .clicked()
            {
                self.eos_prefix_path = None;
                let _ = self.tx.send(WorkerMsg::QueryEosStatus { prefix: None });
            }
            if ui.button("Browse...").clicked() {
                if let Some(path) = rfd::FileDialog::new().pick_folder() {
                    self.eos_prefix_path = Some(path.clone());
                    let _ = self
                        .tx
                        .send(WorkerMsg::QueryEosStatus { prefix: Some(path) });
                }
            }
            if let Some(p) = &self.eos_prefix_path {
                ui.label(p.to_string_lossy());
            }
        });

        ui.add_space(10.0);

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
                            platform: "Windows".to_string(),
                        });
                        self.current_view = View::Tasks;
                    }
                }
            }

            ui.add_space(5.0);
            ui.horizontal(|ui| {
                ui.label("Registry Status:");
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
                            let prefix = self
                                .eos_prefix_path
                                .clone()
                                .or_else(get_default_compat_data_path);
                            if let Some(p) = prefix {
                                let _ = self.tx.send(WorkerMsg::UpdateEosRegistry {
                                    overlay_path: path.clone(),
                                    prefix: p,
                                    enable: true,
                                });
                            }
                        }
                    });
                }
            }

            if self.eos_status.installed {
                ui.horizontal(|ui| {
                    if ui.button("Enable in Prefix").clicked() {
                        let prefix = self
                            .eos_prefix_path
                            .clone()
                            .or_else(get_default_compat_data_path);
                        if let (Some(path), Some(p)) = (&self.eos_status.install_path, prefix) {
                            let _ = self.tx.send(WorkerMsg::UpdateEosRegistry {
                                overlay_path: path.clone(),
                                prefix: p,
                                enable: true,
                            });
                        }
                    }
                    if ui.button("Disable in Prefix").clicked() {
                        let prefix = self
                            .eos_prefix_path
                            .clone()
                            .or_else(get_default_compat_data_path);
                        if let Some(p) = prefix {
                            let _ = self.tx.send(WorkerMsg::UpdateEosRegistry {
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
                    &mut self.config.global.eos_overlay_enabled,
                    "Enable EOS Overlay by default",
                )
                .changed()
            {
                let _ = self.config.save();
            }
        });

        ui.add_space(10.0);
        ui.heading("Per-game EOS Overlay Settings");
        egui::ScrollArea::vertical().show(ui, |ui| {
            for game in &self.installed_games {
                if game.app_name == crate::eos::EOS_OVERLAY_APP_ID {
                    continue;
                }
                ui.horizontal(|ui| {
                    ui.label(&game.title);
                    let settings = self.config.games.entry(game.app_name.clone()).or_default();
                    if ui
                        .checkbox(&mut settings.eos_overlay_enabled, "Enabled")
                        .changed()
                    {
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
            if ui
                .checkbox(
                    &mut self.config.global.use_custom_pfx,
                    "Use Custom WINE/Proton Prefix (PFX)",
                )
                .changed()
            {
                changed = true;
            }
            if self.config.global.use_custom_pfx {
                ui.horizontal(|ui| {
                    let mut path_str = self
                        .config
                        .global
                        .custom_pfx_path
                        .as_ref()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_default();
                    if ui.text_edit_singleline(&mut path_str).changed() {
                        self.config.global.custom_pfx_path = if path_str.is_empty() {
                            None
                        } else {
                            Some(std::path::PathBuf::from(path_str))
                        };
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

            ui.separator();
            ui.horizontal(|ui| {
                if ui
                    .radio_value(
                        &mut self.config.global.compatibility_tool,
                        Some(CompatibilityTool::SteamProton),
                        "Steam Proton",
                    )
                    .changed()
                {
                    changed = true;
                }
                if ui
                    .radio_value(
                        &mut self.config.global.compatibility_tool,
                        Some(CompatibilityTool::CustomProtonWine),
                        "Custom Proton/Wine",
                    )
                    .changed()
                {
                    changed = true;
                }
                if ui
                    .radio_value(
                        &mut self.config.global.compatibility_tool,
                        Some(CompatibilityTool::SystemWine),
                        "System Wine",
                    )
                    .changed()
                {
                    changed = true;
                }
                if ui
                    .radio_value(
                        &mut self.config.global.compatibility_tool,
                        Some(CompatibilityTool::UmuLauncher),
                        "UMU Launcher",
                    )
                    .changed()
                {
                    changed = true;
                }
            });

            match self.config.global.compatibility_tool {
                Some(CompatibilityTool::SteamProton) => {
                    let protons = crate::config::find_steam_protons();
                    egui::ComboBox::from_label("Default Proton Version")
                        .selected_text(
                            self.config
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
                                        &mut self.config.global.custom_compatibility_path,
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
                            self.config
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
                                        &mut self.config.global.custom_compatibility_path,
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
                                    self.config.global.custom_compatibility_path = Some(path);
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
                    &mut self.config.global.eos_overlay_enabled,
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
                        &mut self.config.global.proton_prefer_sdl,
                        "Proton Prefer SDL (PROTON_PREFER_SDL=1)",
                    )
                    .changed()
                {
                    changed = true;
                }

                ui.separator();
                ui.label("Steam Compatibility Overrides:");
                ui.horizontal(|ui| {
                    ui.label("STEAM_COMPAT_INSTALL_PATH:");
                    let mut path_str = self
                        .config
                        .global
                        .steam_compat_install_path
                        .as_ref()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_default();
                    if ui.text_edit_singleline(&mut path_str).changed() {
                        self.config.global.steam_compat_install_path = if path_str.is_empty() {
                            None
                        } else {
                            Some(std::path::PathBuf::from(path_str))
                        };
                        changed = true;
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("STEAM_COMPAT_CLIENT_INSTALL_PATH:");
                    let mut path_str = self
                        .config
                        .global
                        .steam_compat_client_install_path
                        .as_ref()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_default();
                    if ui.text_edit_singleline(&mut path_str).changed() {
                        self.config.global.steam_compat_client_install_path = if path_str.is_empty()
                        {
                            None
                        } else {
                            Some(std::path::PathBuf::from(path_str))
                        };
                        changed = true;
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("STEAM_COMPAT_DATA_PATH:");
                    let mut path_str = self
                        .config
                        .global
                        .steam_compat_data_path
                        .as_ref()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_default();
                    if ui.text_edit_singleline(&mut path_str).changed() {
                        self.config.global.steam_compat_data_path = if path_str.is_empty() {
                            None
                        } else {
                            Some(std::path::PathBuf::from(path_str))
                        };
                        changed = true;
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("STEAM_COMPAT_APP_ID:");
                    let mut id_str = self
                        .config
                        .global
                        .steam_compat_app_id
                        .clone()
                        .unwrap_or_default();
                    if ui.text_edit_singleline(&mut id_str).changed() {
                        self.config.global.steam_compat_app_id = if id_str.is_empty() {
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
                for (k, v) in &mut self.config.global.env_vars {
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
                    self.config.global.env_vars.remove(&k);
                    changed = true;
                }

                let new_key = &mut self.new_env_key;
                let new_val = &mut self.new_env_val;
                let global_env_vars = &mut self.config.global.env_vars;
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
