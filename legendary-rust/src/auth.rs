use crate::models::OAuthToken;
use std::fs;
use std::path::PathBuf;

pub fn get_config_dir() -> Option<PathBuf> {
    let mut p = home::home_dir()?;
    if cfg!(target_os = "windows") {
        p.push("AppData/Local/legendary");
    } else {
        p.push(".config/legendary");
    }
    Some(p)
}

pub fn save_token(token: &OAuthToken) -> anyhow::Result<()> {
    let config_dir = get_config_dir().ok_or_else(|| anyhow::anyhow!("Could not find config directory"))?;
    fs::create_dir_all(&config_dir)?;
    // Use user.json for compatibility with original Legendary
    let token_path = config_dir.join("user.json");
    let token_json = serde_json::to_string(token)?;
    fs::write(token_path, token_json)?;
    Ok(())
}

pub fn load_token() -> anyhow::Result<OAuthToken> {
    let config_dir = get_config_dir().ok_or_else(|| anyhow::anyhow!("Could not find config directory"))?;

    // Check user.json (Legendary) first, then token.json (legendary-rust old)
    let token_path = config_dir.join("user.json");
    if let Ok(token_json) = fs::read_to_string(&token_path) {
        if let Ok(token) = serde_json::from_str(&token_json) {
            return Ok(token);
        }
    }

    let old_token_path = config_dir.join("token.json");
    let token_json = fs::read_to_string(old_token_path)?;
    let token: OAuthToken = serde_json::from_str(&token_json)?;
    Ok(token)
}

pub fn load_installed_games() -> Vec<crate::models::InstalledGame> {
    let mut path = if let Some(config_dir) = get_config_dir() {
        config_dir
    } else {
        return Vec::new();
    };
    path.push("installed.json");

    if let Ok(content) = std::fs::read_to_string(path) {
        if let Ok(mut installed_map) = serde_json::from_str::<std::collections::HashMap<String, crate::models::InstalledGame>>(&content) {
            for (_app_name, game) in installed_map.iter_mut() {
                if game.install_size == 0 {
                    // Try to calculate from manifest or directory
                    let mut size = 0;
                    let path = std::path::Path::new(&game.install_path);
                    if path.exists() {
                        size = get_dir_size(path);
                    }
                    game.install_size = size;
                }
            }
            return installed_map.values().cloned().collect();
        }
    }
    Vec::new()
}

fn get_dir_size(path: &std::path::Path) -> u64 {
    let mut size = 0;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                size += get_dir_size(&p);
            } else if let Ok(meta) = std::fs::metadata(p) {
                size += meta.len();
            }
        }
    }
    size
}

pub fn load_local_metadata(app_name: &str) -> Option<crate::models::LocalGameMetadata> {
    let mut path = get_config_dir()?;
    path.push("metadata");
    let metadata_file = path.join(format!("{}.json", app_name));

    if let Ok(content) = std::fs::read_to_string(&metadata_file) {
        match serde_json::from_str(&content) {
            Ok(meta) => return Some(meta),
            Err(e) => log::error!("Failed to parse metadata file {:?}: {}", metadata_file, e),
        }
    } else {
        log::debug!("Metadata file not found: {:?}", metadata_file);
    }
    None
}

pub fn scan_egl_manifests() -> Vec<crate::models::InstalledGame> {
    let mut installed = load_installed_games();
    let mut changed = false;

    let mut manifests_path = std::path::PathBuf::new();
    if cfg!(target_os = "windows") {
        if let Some(app_data) = std::env::var_os("PROGRAMDATA") {
            manifests_path = std::path::PathBuf::from(app_data);
            manifests_path.push("Epic/EpicGamesLauncher/Data/Manifests");
        }
    } else {
        // On Linux, we might want to check common Wine prefixes or let user specify
        // For now, let's just use a placeholder or check a default Wine path
        if let Some(home) = home::home_dir() {
            manifests_path = home.join(".local/share/Steam/steamapps/compatdata/2344520/pfx/drive_c/users/steamuser/AppData/Local/EpicGamesLauncher/Saved/Manifests");
            // Note: 2344520 is some random ID, it varies. Better to skip or allow custom path.
        }
    }

    if manifests_path.exists() {
        if let Ok(entries) = std::fs::read_dir(manifests_path) {
            for entry in entries.flatten() {
                if entry.path().extension().and_then(|s| s.to_str()) == Some("item") {
                    if let Ok(content) = std::fs::read_to_string(entry.path()) {
                        if let Ok(egl_manifest) = serde_json::from_str::<serde_json::Value>(&content) {
                            let app_name = egl_manifest["AppName"].as_str().unwrap_or_default().to_string();
                            if app_name.is_empty() || installed.iter().any(|g| g.app_name == app_name) {
                                continue;
                            }

                            let install_path = egl_manifest["InstallLocation"].as_str().unwrap_or_default().to_string();
                            let title = egl_manifest["DisplayName"].as_str().unwrap_or_default().to_string();
                            let version = egl_manifest["AppVersionString"].as_str().unwrap_or_default().to_string();
                            let executable = egl_manifest["LaunchExecutable"].as_str().unwrap_or_default().to_string();

                            if !install_path.is_empty() && std::path::Path::new(&install_path).exists() {
                                let new_game = crate::models::InstalledGame {
                                    app_name,
                                    install_path,
                                    title,
                                    version,
                                    executable,
                                    install_size: 0, // Will be calculated on load
                                    download_size: 0,
                                    platform: "Windows".to_string(),
                                    manifest_path: None,
                                };
                                installed.push(new_game);
                                changed = true;
                            }
                        }
                    }
                }
            }
        }
    }

    if changed {
        let _ = save_installed_games(&installed);
    }
    installed
}

pub fn scan_and_import_games(library: &[crate::models::LibraryItem], search_paths: &[PathBuf]) -> Vec<crate::models::InstalledGame> {
    let mut installed = load_installed_games();
    let mut changed = false;

    for search_path in search_paths {
        if let Ok(entries) = std::fs::read_dir(search_path) {
            for entry in entries.flatten() {
                if let Ok(file_type) = entry.file_type() {
                    if file_type.is_dir() {
                        let folder_name = entry.file_name().to_string_lossy().to_string();

                        // Check if this folder matches any library item
                        for item in library {
                            if installed.iter().any(|g| g.app_name == item.app_name) {
                                continue;
                            }

                            let mut matches = folder_name == item.app_name;

                            // Check local metadata for FolderName
                            if !matches {
                                if let Some(meta) = load_local_metadata(&item.app_name) {
                                    if let Some(attrs) = meta.metadata.custom_attributes {
                                        if let Some(folder_attr) = attrs.get("FolderName") {
                                            if folder_attr.value == folder_name {
                                                matches = true;
                                            }
                                        }
                                    }
                                }
                            }

                            if matches {
                                let local_meta = load_local_metadata(&item.app_name);
                                let title = local_meta.as_ref()
                                    .map(|m| m.app_title.clone())
                                    .unwrap_or_else(|| item.app_name.clone());

                                let manifest_path_buf = get_manifest_path(&item.app_name, &item.catalog_item_id);
                                let mut executable = String::new();
                                let mut version = "0.0.0".to_string();
                                if let Some(ref mp) = manifest_path_buf {
                                    if let Ok(data) = std::fs::read(mp) {
                                        if let Ok(manifest) = crate::manifest::parse_manifest(&data) {
                                            executable = manifest.meta.launch_exe;
                                            version = manifest.meta.build_version;
                                        }
                                    }
                                }

                                let manifest_path = manifest_path_buf
                                    .map(|p| p.to_string_lossy().to_string());
                                let new_game = crate::models::InstalledGame {
                                    app_name: item.app_name.clone(),
                                    install_path: entry.path().to_string_lossy().to_string(),
                                    title,
                                    version,
                                    executable,
                                    install_size: get_dir_size(&entry.path()),
                                    download_size: 0,
                                    platform: "Windows".to_string(),
                                    manifest_path,
                                };
                                installed.push(new_game);
                                changed = true;
                            }
                        }
                    }
                }
            }
        }
    }

    if changed {
        let _ = save_installed_games(&installed);
    }

    installed
}

pub fn save_installed_games(games: &[crate::models::InstalledGame]) -> anyhow::Result<()> {
    let config_dir = get_config_dir().ok_or_else(|| anyhow::anyhow!("Could not find config directory"))?;
    let path = config_dir.join("installed.json");

    let mut map = std::collections::HashMap::new();
    for game in games {
        map.insert(game.app_name.clone(), game.clone());
    }

    let json = serde_json::to_string_pretty(&map)?;
    fs::write(path, json)?;
    Ok(())
}

pub fn get_manifest_path(app_name: &str, catalog_item_id: &str) -> Option<PathBuf> {
    let mut p = get_config_dir()?;
    p.push("manifests");

    if !p.exists() { return None; }

    // Try exact matches first
    let p1 = p.join(format!("{}.manifest", app_name));
    if p1.exists() { return Some(p1); }
    let p2 = p.join(format!("{}.manifest", catalog_item_id));
    if p2.exists() { return Some(p2); }

    // Scan for prefixed manifests (e.g. app_name_Windows_label.manifest)
    if let Ok(entries) = std::fs::read_dir(&p) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Some(filename) = path.file_name().and_then(|s| s.to_str()) {
                    if filename.ends_with(".manifest") && (filename.starts_with(app_name) || filename.starts_with(catalog_item_id)) {
                        return Some(path);
                    }
                }
            }
        }
    }

    None
}
