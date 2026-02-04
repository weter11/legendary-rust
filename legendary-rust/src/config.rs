use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use crate::auth::get_config_dir;

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct AppConfig {
    pub global: GlobalSettings,
    pub games: HashMap<String, GameSettings>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GlobalSettings {
    pub game_paths: Vec<PathBuf>,
}

impl Default for GlobalSettings {
    fn default() -> Self {
        Self {
            game_paths: Vec::new(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct GameSettings {
    pub compatibility_tool: Option<CompatibilityTool>,
    pub custom_compatibility_path: Option<PathBuf>,
    pub play_time_seconds: u64,
    pub save_path: Option<PathBuf>,
    #[serde(default)]
    pub start_params: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub enum CompatibilityTool {
    SteamProton,
    CustomProtonWine,
    SystemWine,
}

impl AppConfig {
    pub fn load() -> Self {
        if let Some(mut p) = get_config_dir() {
            p.push("config.toml");
            if let Ok(content) = std::fs::read_to_string(&p) {
                match toml::from_str(&content) {
                    Ok(config) => return config,
                    Err(e) => log::error!("Failed to parse config.toml at {:?}: {}", p, e),
                }
            } else {
                log::info!("No config.toml found at {:?}, using defaults", p);
            }
        }
        Self::default()
    }

    pub fn save(&self) -> anyhow::Result<()> {
        if let Some(mut p) = get_config_dir() {
            let _ = std::fs::create_dir_all(&p);
            p.push("config.toml");
            let content = toml::to_string_pretty(self)?;
            std::fs::write(p, content)?;
        }
        Ok(())
    }
}

#[cfg(target_os = "linux")]
pub fn find_steam_protons() -> Vec<PathBuf> {
    let mut protons = Vec::new();
    let home = home::home_dir().unwrap_or_default();
    let steam_paths = vec![
        home.join(".local/share/Steam/compatibilitytools.d"),
        home.join(".steam/root/compatibilitytools.d"),
        home.join(".steam/steam/compatibilitytools.d"),
        PathBuf::from("/usr/share/steam/compatibilitytools.d"),
    ];

    for path in steam_paths {
        if let Ok(entries) = std::fs::read_dir(path) {
            for entry in entries.flatten() {
                if entry.path().is_dir() {
                    protons.push(entry.path());
                }
            }
        }
    }
    protons
}

#[cfg(not(target_os = "linux"))]
pub fn find_steam_protons() -> Vec<PathBuf> { Vec::new() }

#[cfg(target_os = "linux")]
pub fn find_custom_wines() -> Vec<PathBuf> {
    let mut wines = Vec::new();
    if let Some(p) = get_config_dir() {
        // Look in .config/legendary/ (as requested)
        if let Ok(entries) = std::fs::read_dir(&p) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let name = path.file_name().unwrap_or_default().to_string_lossy();
                    if name.contains("wine") || name.contains("proton") {
                        wines.push(path);
                    }
                }
            }
        }
    }
    wines
}

#[cfg(not(target_os = "linux"))]
pub fn find_custom_wines() -> Vec<PathBuf> { Vec::new() }
