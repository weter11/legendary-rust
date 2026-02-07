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
    #[serde(default)]
    pub use_custom_pfx: bool,
    #[serde(default)]
    pub custom_pfx_path: Option<PathBuf>,
    #[serde(default)]
    pub pre_launch_command: String,
    #[serde(default)]
    pub eos_overlay_enabled: bool,
    #[serde(default)]
    pub use_umu: bool,
    #[serde(default = "default_store")]
    pub umu_store: String,
}

fn default_store() -> String {
    "egs".to_string()
}

impl Default for GlobalSettings {
    fn default() -> Self {
        Self {
            game_paths: Vec::new(),
            use_custom_pfx: false,
            custom_pfx_path: None,
            pre_launch_command: String::new(),
            eos_overlay_enabled: true,
            use_umu: false,
            umu_store: default_store(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GameSettings {
    pub compatibility_tool: Option<CompatibilityTool>,
    pub custom_compatibility_path: Option<PathBuf>,
    pub play_time_seconds: u64,
    pub save_path: Option<PathBuf>,
    #[serde(default)]
    pub start_params: String,
    #[serde(default)]
    pub play_offline: bool,
    #[serde(default)]
    pub custom_exe_path: Option<PathBuf>,
    #[serde(default = "default_true")]
    pub cloud_sync_enabled: bool,
    #[serde(default)]
    pub use_custom_pfx: bool,
    #[serde(default)]
    pub custom_pfx_path: Option<PathBuf>,
    #[serde(default)]
    pub pre_launch_command: String,
    #[serde(default = "default_true")]
    pub eos_overlay_enabled: bool,
    #[serde(default)]
    pub use_umu: bool,
    #[serde(default)]
    pub umu_store: Option<String>,
}

fn default_true() -> bool {
    true
}

impl Default for GameSettings {
    fn default() -> Self {
        Self {
            compatibility_tool: None,
            custom_compatibility_path: None,
            play_time_seconds: 0,
            save_path: None,
            start_params: String::new(),
            play_offline: false,
            custom_exe_path: None,
            cloud_sync_enabled: true,
            use_custom_pfx: false,
            custom_pfx_path: None,
            pre_launch_command: String::new(),
            eos_overlay_enabled: true,
            use_umu: false,
            umu_store: None,
        }
    }
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
    let steam_common_paths = vec![
        home.join(".local/share/Steam/steamapps/common"),
        home.join(".steam/root/steamapps/common"),
        home.join(".steam/steam/steamapps/common"),
    ];

    for path in steam_common_paths {
        if let Ok(entries) = std::fs::read_dir(path) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    if let Some(name) = p.file_name() {
                        let name_str = name.to_string_lossy();
                        if name_str.starts_with("Proton") {
                            protons.push(p);
                        }
                    }
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
    let home = home::home_dir().unwrap_or_default();

    let mut search_paths = vec![
        home.join(".local/share/Steam/compatibilitytools.d"),
    ];
    if let Some(p) = get_config_dir() {
        search_paths.push(p);
    }

    for p in search_paths {
        if let Ok(entries) = std::fs::read_dir(&p) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let name = path.file_name().unwrap_or_default().to_string_lossy();
                    let name_lower = name.to_lowercase();
                    if name_lower.contains("wine") || name_lower.contains("proton") {
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
