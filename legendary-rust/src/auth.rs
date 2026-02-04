use crate::models::OAuthToken;
use std::fs;
use std::path::PathBuf;
use directories::ProjectDirs;

pub fn get_config_dir() -> Option<PathBuf> {
    ProjectDirs::from("com", "legendary", "legendary-rust").map(|proj| proj.config_dir().to_path_buf())
}

pub fn save_token(token: &OAuthToken) -> anyhow::Result<()> {
    let config_dir = get_config_dir().ok_or_else(|| anyhow::anyhow!("Could not find config directory"))?;
    fs::create_dir_all(&config_dir)?;
    let token_path = config_dir.join("token.json");
    let token_json = serde_json::to_string(token)?;
    fs::write(token_path, token_json)?;
    Ok(())
}

pub fn load_token() -> anyhow::Result<OAuthToken> {
    let config_dir = get_config_dir().ok_or_else(|| anyhow::anyhow!("Could not find config directory"))?;
    let token_path = config_dir.join("token.json");
    let token_json = fs::read_to_string(token_path)?;
    let token: OAuthToken = serde_json::from_str(&token_json)?;
    Ok(token)
}

pub fn load_installed_games() -> Vec<crate::models::InstalledGame> {
    let mut path = if let Some(_config_dir) = get_config_dir() {
        // Legendary config is typically one level up from our app's config dir if we use the same base
        // Actually, Legendary's default is ~/.config/legendary
        // Let's just use the default path for Legendary specifically
        let mut p = home::home_dir().unwrap_or_default();
        if cfg!(target_os = "windows") {
            p.push("AppData/Local/legendary");
        } else {
            p.push(".config/legendary");
        }
        p
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
