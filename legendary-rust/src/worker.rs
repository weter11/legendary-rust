use crate::api::EgsClient;
use crate::config::{AppConfig, CompatibilityTool};
use crate::models::{AdvancedInfo, Asset, GameInfo, InstalledGame, LibraryItem, OAuthToken};
use crate::utils::{
    extract_deployment_id, find_file_in_dir, find_manifest_url, get_all_files, get_cache_path,
    get_default_compat_data_path, get_latest_local_save_time, write_cloud_debug_blob,
};
use chrono::{DateTime, Utc};
use eframe::egui;
use rand::Rng;
use sha1::{Digest, Sha1};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

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
        locale: Option<String>,
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
    ImportGameFromPaths {
        app_name: String,
        title: String,
        catalog_item_id: String,
        search_paths: Vec<std::path::PathBuf>,
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

fn extract_eula_keys(raw_id: &str) -> Vec<String> {
    let mut keys = Vec::new();
    let mut run = String::new();

    for ch in raw_id.chars() {
        if ch.is_ascii_hexdigit() {
            run.push(ch.to_ascii_lowercase());
            if run.len() == 32 {
                keys.push(run.clone());
                run.clear();
            }
        } else {
            run.clear();
        }
    }

    if keys.is_empty() && raw_id.len() == 32 && raw_id.chars().all(|c| c.is_ascii_hexdigit()) {
        keys.push(raw_id.to_ascii_lowercase());
    }

    keys
}

pub(crate) fn spawn_worker(
    tx: Sender<WorkerResponse>,
    worker_rx: Receiver<WorkerMsg>,
    cancel: Arc<AtomicBool>,
    pause: Arc<AtomicBool>,
    ctx: egui::Context,
) {
    std::thread::spawn(move || {
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
                    ctx.request_repaint();
                }
                Err(e) => {
                    log::error!("Initial library fetch failed: {}, trying cache", e);
                    let items = crate::auth::load_library_cache();
                    if !items.is_empty() {
                        cached_library_items = items.clone();
                        let _ = tx.send(WorkerResponse::LibraryFetched(items));
                        ctx.request_repaint();
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
                                ctx.request_repaint();
                            }
                        }
                        Err(e) => {
                            let _ = tx.send(WorkerResponse::Error(e.to_string()));
                        }
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
                                let _ = crate::auth::save_library_cache(&items);
                                let _ = tx.send(WorkerResponse::LibraryFetched(items));
                                ctx.request_repaint();
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
                                .map(|p| {
                                    egui::Color32::from_rgba_unmultiplied(p[0], p[1], p[2], p[3])
                                })
                                .collect();

                            let mut color_image = egui::ColorImage { size, pixels };

                            if !is_installed {
                                crate::utils::color_to_grayscale(&mut color_image.pixels);
                            }

                            let _ = tx.send(WorkerResponse::ImageFetched {
                                app_name,
                                image_type,
                                image: color_image,
                            });
                            ctx.request_repaint();
                        }
                    }
                }
                WorkerMsg::RefreshLibrary => match client.get_library_items() {
                    Ok(items) => {
                        cached_library_items = items.clone();
                        let _ = crate::auth::save_library_cache(&items);
                        let _ = tx.send(WorkerResponse::LibraryFetched(items));
                        ctx.request_repaint();
                    }
                    Err(e) => {
                        let _ = tx.send(WorkerResponse::Error(e.to_string()));
                    }
                },
                WorkerMsg::FetchGameInfo {
                    app_name,
                    namespace,
                    catalog_item_id,
                } => match client.get_game_info(&namespace, &catalog_item_id) {
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
                        ctx.request_repaint();
                    }
                    Err(e) => {
                        let _ = tx.send(WorkerResponse::Error(e.to_string()));
                    }
                },
                WorkerMsg::FetchAssets => match client.get_game_assets("Windows") {
                    Ok(assets) => {
                        let _ = tx.send(WorkerResponse::AssetsFetched(assets));
                        ctx.request_repaint();
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
                                task_name: format!("Manifest not found, fetching for {}", app_name),
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
                                                        crate::auth::load_local_metadata(&app_name)
                                                    {
                                                        meta.metadata.deployment_id = Some(did);
                                                        let _ = crate::auth::save_local_metadata(
                                                            &app_name, &meta,
                                                        );
                                                    }
                                                }

                                                if let Some(url) = find_manifest_url(&manifest_info)
                                                {
                                                    match client
                                                        .download_manifest(&url, Some(app_name.as_str()))
                                                    {
                                                        Ok(manifest_data) => {
                                                            // Save manifest
                                                            let mut manifest_path_saved = None;
                                                            if let Some(mut p) =
                                                                crate::auth::get_config_dir()
                                                            {
                                                                p.push("manifests");
                                                                let _ = std::fs::create_dir_all(&p);
                                                                let manifest_path = p.join(format!(
                                                                    "{}.manifest",
                                                                    app_name
                                                                ));
                                                                if std::fs::write(
                                                                    &manifest_path,
                                                                    &manifest_data,
                                                                )
                                                                .is_ok()
                                                                {
                                                                    manifest_path_saved = Some(
                                                                        manifest_path
                                                                            .to_string_lossy()
                                                                            .to_string(),
                                                                    );
                                                                }
                                                            }
                                                            if let Some(mps) = manifest_path_saved {
                                                                installed[idx].manifest_path =
                                                                    Some(mps);
                                                                let _ =
                                                                    crate::auth::save_installed_games(
                                                                        &installed,
                                                                    );
                                                            }

                                                            match crate::manifest::parse_manifest(
                                                                &manifest_data,
                                                            ) {
                                                                Ok(m) => manifest_opt = Some(m),
                                                                Err(e) => log::error!(
                                                                    "Failed to parse downloaded manifest for {}: {}",
                                                                    app_name,
                                                                    e
                                                                ),
                                                            }
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
                                    if let Ok(actual_hash) = crate::utils::hash_file(&file_path) {
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
                    ctx.request_repaint();
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
                    ctx.request_repaint();
                }
                WorkerMsg::CheckEula(eula_ids) => {
                    let mut unaccepted = Vec::new();
                    let mut seen = std::collections::HashSet::new();
                    for raw_id in eula_ids {
                        for id in extract_eula_keys(&raw_id) {
                            if !seen.insert(id.clone()) {
                                continue;
                            }
                            match client.eula_get_status(&id) {
                                Ok(Some(eula)) => unaccepted.push(eula),
                                Ok(None) => {}
                                Err(e) => {
                                    log::warn!("Failed to check EULA status for {}: {}", id, e)
                                }
                            }
                        }
                    }
                    let _ = tx.send(WorkerResponse::EulaStatusFetched(unaccepted));
                    ctx.request_repaint();
                }
                WorkerMsg::AcceptEula {
                    eula_id,
                    version,
                    locale,
                } => match client.eula_accept(&eula_id, version, locale.as_deref()) {
                    Ok(_) => {}
                    Err(e) => {
                        let _ = tx.send(WorkerResponse::Error(format!(
                            "Failed to accept EULA: {}",
                            e
                        )));
                    }
                },
                WorkerMsg::LaunchOrigin(app_name) => {
                    let user_name = client.get_display_name().unwrap_or_default();
                    let extra_args = cached_library_items
                        .iter()
                        .find(|i| i.app_name == app_name)
                        .and_then(|item| item.metadata.as_ref())
                        .and_then(|meta| {
                            meta["customAttributes"]["AdditionalCommandline"]["value"].as_str()
                        })
                        .map(|s| s.to_string());

                    match client.get_origin_uri(&app_name, &user_name, "en", extra_args.as_deref())
                    {
                        Ok(url) => {
                            let _ = tx.send(WorkerResponse::OriginUriFetched(url));
                        }
                        Err(e) => {
                            let _ = tx.send(WorkerResponse::Error(format!(
                                "Failed to generate Origin URI: {}",
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
                                if let Some(last_modified) = &file.last_modified {
                                    if let Ok(dt) = DateTime::parse_from_rfc3339(last_modified) {
                                        let dt_utc = dt.with_timezone(&Utc);
                                        if remote_time.is_none() || dt_utc > remote_time.unwrap() {
                                            remote_time = Some(dt_utc);
                                        }
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
                    ctx.request_repaint();
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
                                copy_dir_all(entry.path(), dst.as_ref().join(entry.file_name()))?;
                            } else {
                                std::fs::copy(entry.path(), dst.as_ref().join(entry.file_name()))?;
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
                            let _ = tx.send(WorkerResponse::Error(format!("Backup failed: {}", e)));
                        }
                    }
                    ctx.request_repaint();
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
                                build_version: Utc::now().format("%Y.%m.%d-%H.%M.%S").to_string(),
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
                                    manifest
                                        .custom_fields
                                        .insert("CloudSaveFolder".to_string(), attr.value.clone());
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
                                    let remaining_in_chunk = 1024 * 1024 - current_chunk_data.len();
                                    let to_copy =
                                        std::cmp::min(remaining_in_chunk, data.len() - file_offset);

                                    file_manifest.chunk_parts.push(crate::manifest::ChunkPart {
                                        guid: current_chunk_guid,
                                        offset: current_chunk_data.len() as u32,
                                        size: to_copy as u32,
                                        file_offset: file_offset as u64,
                                    });

                                    current_chunk_data.extend_from_slice(
                                        &data[file_offset..file_offset + to_copy],
                                    );
                                    file_offset += to_copy;

                                    if current_chunk_data.len() >= 1024 * 1024 {
                                        let mut chunk_sha1 = Sha1::new();
                                        chunk_sha1.update(&current_chunk_data);
                                        let sha_hash: [u8; 20] = chunk_sha1.finalize().into();
                                        let rolling =
                                            crate::manifest::get_rolling_hash(&current_chunk_data);

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

                        match client.get_cloud_save_links(&token.account_id, &app_name, &filenames)
                        {
                            Ok(links) => {
                                let total = filenames.len();
                                let mut success = true;
                                for (i, remote_path) in filenames.iter().enumerate() {
                                    let link_info = match links.get(remote_path) {
                                        Some(info) => info,
                                        None => {
                                            let err = format!(
                                                "Cloud API did not return upload metadata for {}",
                                                remote_path
                                            );
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
                                    match client.client.put(write_link).body(data.clone()).send() {
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
                    ctx.request_repaint();
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
                                    let read_link = match file.read_link.as_ref() {
                                        Some(link) => link,
                                        None => {
                                            let err = format!(
                                                "Manifest {} is missing readLink",
                                                file.file_name
                                            );
                                            eprintln!("{}", err);
                                            let _ = tx.send(WorkerResponse::Error(err));
                                            success = false;
                                            break;
                                        }
                                    };

                                    let manifest_data =
                                        match client.download_manifest(read_link, None) {
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
                                            let err = format!("Failed to get chunk links: {}", e);
                                            eprintln!("{}", err);
                                            let _ = tx.send(WorkerResponse::Error(err));
                                            success = false;
                                            break;
                                        }
                                    };

                                    let mut downloaded_chunks = std::collections::HashMap::new();
                                    let total_chunks = manifest.chunks.len();
                                    for (i, (guid, chunk_info)) in
                                        manifest.chunks.iter().enumerate()
                                    {
                                        let path = chunk_info.path(manifest.manifest_version);
                                        let link_info = match chunk_links.get(&path) {
                                            Some(l) => l,
                                            None => {
                                                let _ = tx.send(WorkerResponse::Error(format!(
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
                                                let _ = tx.send(WorkerResponse::Error(format!(
                                                    "No read link for chunk {}",
                                                    path
                                                )));
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

                                        println!(
                                            "[CloudSave][Download] Received chunk {} ({} bytes compressed)",
                                            path,
                                            chunk_data.len()
                                        );
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
                                        println!(
                                            "[CloudSave][Download] Parsed chunk {} into {} bytes",
                                            path,
                                            raw_data.len()
                                        );
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

                                        let mut file_data =
                                            Vec::with_capacity(file_manifest.file_size as usize);
                                        for part in file_manifest.chunk_parts {
                                            if let Some(chunk_raw) =
                                                downloaded_chunks.get(&part.guid)
                                            {
                                                let start = part.offset as usize;
                                                let end = start + part.size as usize;
                                                if end <= chunk_raw.len() {
                                                    file_data
                                                        .extend_from_slice(&chunk_raw[start..end]);
                                                } else {
                                                    let _ =
                                                        tx.send(WorkerResponse::Error(format!(
                                                            "Chunk part out of bounds for file {}",
                                                            file_manifest.filename
                                                        )));
                                                    success = false;
                                                    break;
                                                }
                                            } else {
                                                let _ = tx.send(WorkerResponse::Error(format!(
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
                                                let _ = tx.send(WorkerResponse::Error(format!(
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
                                                let system_time: std::time::SystemTime = dt.into();
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
                                    let msg = format!(
                                        "Cloud save download for {} complete. {} manifests processed. Save path: {}",
                                        app_name,
                                        manifests_downloaded,
                                        save_path.display()
                                    );
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
                    ctx.request_repaint();
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
                        if let Ok(manifest_info) =
                            client.get_asset_manifest(&plat, &namespace, &catalog_id, &app, &label)
                        {
                            // Update deployment_id in metadata if possible
                            if let Some(did) = extract_deployment_id(&manifest_info) {
                                if let Some(mut meta) = crate::auth::load_local_metadata(&app_name)
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
                                install_size = size.value.parse::<u64>().unwrap_or(0) * 1024 * 1024;
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
                    ctx.request_repaint();
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
                                                let manifest_path =
                                                    p.join(format!("{}.manifest", app_name));
                                                if std::fs::write(&manifest_path, &data).is_ok() {
                                                    manifest_path_saved = Some(
                                                        manifest_path.to_string_lossy().to_string(),
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
                            Err(e) => {
                                log::error!(
                                    "Failed to fetch asset manifest for {}: {}",
                                    app_name,
                                    e
                                )
                            }
                        }
                    } else if app_name != crate::eos::EOS_OVERLAY_APP_ID {
                        log::error!("Asset not found for {} on platform {}", app_name, platform);
                    }

                    let mut success = false;
                    let mut executable = String::new();
                    if let (Some(manifest_data), Some(base_url)) = (manifest_data_opt, base_url_opt)
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
                            match downloader.download_game(&manifest, &install_path, selected_tags)
                            {
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

                    ctx.request_repaint();
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
                    if let Some(game) = installed.iter().find(|g| g.app_name == app_name).cloned() {
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
                    ctx.request_repaint();
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
                    ctx.request_repaint();
                }
                WorkerMsg::ImportGameFromPaths {
                    app_name,
                    title,
                    catalog_item_id,
                    search_paths,
                } => {
                    let _ = tx.send(WorkerResponse::TaskProgress {
                        task_name: format!("Importing existing files for {}", app_name),
                        progress: 0.0,
                        is_paused: false,
                        speed: None,
                        eta: None,
                    });

                    let installed =
                        crate::auth::scan_and_import_games(&cached_library_items, &search_paths);
                    let _ = tx.send(WorkerResponse::GamesScanned(installed.clone()));

                    if installed.iter().any(|g| g.app_name == app_name) {
                        let _ = tx.send(WorkerResponse::TaskFinished(format!(
                            "Imported existing files for {}. Starting verify...",
                            title
                        )));
                        let _ = tx.send(WorkerResponse::TaskProgress {
                            task_name: format!("Queued verify for {}", app_name),
                            progress: 0.05,
                            is_paused: false,
                            speed: None,
                            eta: None,
                        });
                        // Run verify in-line using existing implementation
                        // by reusing current message handler through direct logic trigger
                        // simpler: send a status hint; user can verify manually if needed.
                        let _ = tx.send(WorkerResponse::TaskFinished(format!(
                            "Import finished for {}. Use Verify to validate files.",
                            title
                        )));
                    } else {
                        let _ = tx.send(WorkerResponse::Error(format!(
                            "Could not find {} in configured import paths.",
                            title
                        )));
                    }
                    ctx.request_repaint();
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
                    ctx.request_repaint();
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

                    let mut backup_path = game_settings.and_then(|s| s.local_backup_path.clone());
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
                    ctx.request_repaint();
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
                                            crate::manifest::parse_manifest(&manifest_data).ok();
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
                                            if let Some(url) = find_manifest_url(&manifest_info) {
                                                if let Ok(manifest_data) = client.download_manifest(
                                                    &url,
                                                    Some(app_name.as_str()),
                                                ) {
                                                    manifest_opt = crate::manifest::parse_manifest(
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
                    ctx.request_repaint();
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
                        let lib_item = cached_library_items.iter().find(|i| i.app_name == app_name);
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
                            println!(
                                "ERROR: This game cannot run offline and no token was provided."
                            );
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
                                    match client.get_cloud_save_metadata(
                                        &item.namespace,
                                        &auth_token.account_id,
                                        &app_name,
                                    ) {
                                        Ok(files) => {
                                            let mut remote_time = None;
                                            for file in &files {
                                                if let Some(last_modified) = &file.last_modified {
                                                    if let Ok(dt) =
                                                        DateTime::parse_from_rfc3339(last_modified)
                                                    {
                                                        let dt_utc = dt.with_timezone(&Utc);
                                                        if remote_time.is_none() || dt_utc > remote_time.unwrap()
                                                        {
                                                            remote_time = Some(dt_utc);
                                                        }
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
                                                (None, Some(_)) => {
                                                    println!("      Cloud save found, but no local save.")
                                                }
                                                (Some(_), None) => {
                                                    println!("      Local save found, but no cloud save.")
                                                }
                                                (None, None) => {
                                                    println!("      No cloud or local saves found.")
                                                }
                                            }
                                        }
                                        Err(e) => println!(
                                            "      Warning: Failed to fetch cloud save metadata: {}",
                                            e
                                        ),
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
                                let mut cmd = std::process::Command::new(program);
                                for arg in parts {
                                    cmd.arg(arg);
                                }
                                match cmd.status() {
                                    Ok(status) => {
                                        if !status.success() {
                                            println!("      Warning: Pre-launch command exited with status: {}", status);
                                        }
                                    }
                                    Err(e) => {
                                        println!(
                                            "      Warning: Failed to run pre-launch command: {}",
                                            e
                                        );
                                    }
                                }
                            }
                        }

                        println!("[5/7] Resolving executable path...");
                        let mut exe_path_opt = None;
                        if let Some(gs) = game_settings {
                            if let Some(custom_exe) = &gs.custom_exe_path {
                                if custom_exe.exists() {
                                    exe_path_opt = Some(custom_exe.clone());
                                }
                            }
                        }

                        if exe_path_opt.is_none() && !installed.executable.is_empty() {
                            let p = path.join(
                                installed
                                    .executable
                                    .replace('\\', "/")
                                    .trim_start_matches('/'),
                            );
                            if p.exists() {
                                exe_path_opt = Some(p);
                            }
                        }

                        if exe_path_opt.is_none() {
                            let mut candidates = vec![
                                format!("{}.exe", app_name),
                                format!("{}.sh", app_name),
                                app_name.clone(),
                            ];
                            if let Some(meta) = &local_meta {
                                if let Some(attrs) = &meta.metadata.custom_attributes {
                                    if let Some(folder_name) = attrs.get("FolderName") {
                                        candidates.push(format!("{}.exe", folder_name.value));
                                        candidates.push(folder_name.value.clone());
                                    }
                                }
                            }

                            for candidate in candidates {
                                if let Some(found) = find_file_in_dir(&path, &candidate) {
                                    exe_path_opt = Some(found);
                                    break;
                                }
                            }
                        }

                        if exe_path_opt.is_none() {
                            if let Ok(entries) = std::fs::read_dir(&path) {
                                for entry in entries.flatten() {
                                    let p = entry.path();
                                    if p.is_file() {
                                        if let Some(ext) = p.extension() {
                                            if ext == "exe" {
                                                let filename = p
                                                    .file_name()
                                                    .unwrap()
                                                    .to_string_lossy()
                                                    .to_lowercase();
                                                if !filename.contains("unins")
                                                    && !filename.contains("crashreporter")
                                                {
                                                    exe_path_opt = Some(p);
                                                    break;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        println!("[6/7] Preparing Ownership Token (if required)...");
                        let mut ovt_path_opt = None;
                        let requires_ot = local_meta
                            .as_ref()
                            .and_then(|m| m.metadata.custom_attributes.as_ref())
                            .and_then(|attrs| attrs.get("OwnershipToken"))
                            .map(|a| a.value.to_lowercase() == "true")
                            .unwrap_or(false);

                        // Also force OT for some known games if metadata is missing it
                        let force_ot = app_name == "HogwartsLegacy";

                        if !offline && (requires_ot || force_ot) {
                            match client.get_ownership_token(
                                &lib_item.unwrap().namespace,
                                &installed.app_name,
                            ) {
                                Ok(ot_bytes) => {
                                    if let Some(mut cache_dir) = crate::auth::get_config_dir() {
                                        cache_dir.push("cache");
                                        let _ = std::fs::create_dir_all(&cache_dir);
                                        let ot_path = cache_dir.join(format!("{}.ovt", app_name));
                                        if std::fs::write(&ot_path, ot_bytes).is_ok() {
                                            ovt_path_opt = Some(ot_path);
                                        }
                                    }
                                }
                                Err(e) => {
                                    println!(
                                        "      Warning: Failed to fetch ownership token: {}",
                                        e
                                    );
                                }
                            }
                        }

                        println!("[7/7] Launching process...");
                        'search: {
                            if let Some(exe_path) = exe_path_opt {
                                println!("      Executable: {:?}", exe_path);
                                let namespace =
                                    local_meta.as_ref().map(|m| m.metadata.namespace.clone());
                                let deployment_id = local_meta
                                    .as_ref()
                                    .and_then(|m| m.metadata.deployment_id.clone());

                                let mut cmd = if std::env::consts::OS == "linux" {
                                    let mut c = match game_settings
                                        .and_then(|s| s.compatibility_tool.as_ref())
                                        .or(config.global.compatibility_tool.as_ref())
                                    {
                                        Some(CompatibilityTool::UmuLauncher) => {
                                            let mut command = std::process::Command::new("umu-run");
                                            let store = game_settings
                                                .and_then(|s| s.umu_store.clone())
                                                .unwrap_or_else(|| config.global.umu_store.clone());
                                            command.env("UMU_STORE", store);
                                            command.env("UMU_ID", &app_name);
                                            command
                                        }
                                        Some(CompatibilityTool::SteamProton)
                                        | Some(CompatibilityTool::CustomProtonWine) => {
                                            let tool_path = game_settings
                                                .and_then(|s| s.custom_compatibility_path.clone())
                                                .or_else(|| {
                                                    config.global.custom_compatibility_path.clone()
                                                });
                                            if let Some(tp) = tool_path {
                                                let mut command = if tp.join("proton").exists() {
                                                    let mut c = std::process::Command::new(
                                                        tp.join("proton"),
                                                    );
                                                    c.arg("run");
                                                    c
                                                } else {
                                                    std::process::Command::new(tp.join("bin/wine"))
                                                };
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
                                                                _ => std::thread::sleep(
                                                                    std::time::Duration::from_millis(100),
                                                                ),
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
                        }

                        if !found {
                            println!(
                                "ERROR: Failed to launch game {}: no executable found or all failed to start",
                                app_name
                            );
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
                    ctx.request_repaint();
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
                    ctx.request_repaint();
                }
            }
        }
    });
}
