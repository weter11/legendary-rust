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
        if let Ok(installed) = serde_json::from_str::<std::collections::HashMap<String, crate::models::InstalledGame>>(&content) {
            return installed.values().cloned().collect();
        }
    }
    Vec::new()
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

pub fn get_manifest_path(app_name: &str) -> Option<PathBuf> {
    let mut p = get_config_dir()?;
    p.push("manifests");
    p.push(format!("{}.manifest", app_name));
    if p.exists() {
        Some(p)
    } else {
        None
    }
}
