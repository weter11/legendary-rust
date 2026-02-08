use std::path::Path;
use std::io::{BufRead, BufReader, Write};
use std::fs::File;

pub const EOS_OVERLAY_APP_ID: &str = "98bc04bc842e4906993fd6d6644ffb8d";

pub const EOS_OVERLAY_KEY: &str = r"SOFTWARE\Epic Games\EOS";
pub const EOS_OVERLAY_VALUE: &str = "OverlayPath";

#[derive(Debug, Clone, Default)]
pub struct EosOverlayStatus {
    pub installed: bool,
    pub install_path: Option<String>,
    pub registry_path: Option<String>,
    pub available_paths: Vec<String>,
}

pub fn query_registry(prefix: &Path) -> Option<String> {
    let user_reg = prefix.join("user.reg");
    if !user_reg.exists() {
        return None;
    }

    if let Ok(file) = File::open(user_reg) {
        let reader = BufReader::new(file);
        let mut in_section = false;
        let wine_key_base = EOS_OVERLAY_KEY.replace('\\', "\\\\");

        for line in reader.lines().flatten() {
            let line_trimmed = line.trim();
            if line_trimmed.starts_with('[') && line_trimmed.contains(&wine_key_base) {
                in_section = true;
            } else if line_trimmed.starts_with('[') {
                in_section = false;
            } else if in_section && line_trimmed.starts_with(&format!("\"{}\"", EOS_OVERLAY_VALUE)) {
                if let Some(path) = line_trimmed.split('=').nth(1) {
                    return Some(path.trim().trim_matches('"').to_string());
                }
            }
        }
    }
    None
}

pub fn add_registry_entries(overlay_path: &str, prefix: &Path) -> anyhow::Result<()> {
    let user_reg = prefix.join("user.reg");
    if !user_reg.exists() {
        return Err(anyhow::anyhow!("user.reg not found in prefix {:?}", prefix));
    }

    let mut lines: Vec<String> = Vec::new();
    if let Ok(file) = File::open(&user_reg) {
        let reader = BufReader::new(file);
        lines = reader.lines().flatten().collect();
    }

    let wine_key_base = EOS_OVERLAY_KEY.replace('\\', "\\\\");
    let mut overlay_path_wine = overlay_path.replace('\\', "/");
    if !overlay_path_wine.starts_with("Z:") && !overlay_path_wine.starts_with("C:") {
         overlay_path_wine = format!("Z:{}", overlay_path_wine);
    }

    let overlay_line = format!("\"{}\"=\"{}\"", EOS_OVERLAY_VALUE, overlay_path_wine);

    let mut new_lines = Vec::new();
    let mut section_found = false;
    let mut value_replaced = false;
    let mut in_target_section = false;

    for line in lines {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.contains(&wine_key_base) {
            section_found = true;
            in_target_section = true;
            new_lines.push(line);
        } else if trimmed.starts_with('[') && in_target_section {
            if !value_replaced {
                new_lines.push(overlay_line.clone());
                value_replaced = true;
            }
            in_target_section = false;
            new_lines.push(line);
        } else if in_target_section && trimmed.starts_with(&format!("\"{}\"", EOS_OVERLAY_VALUE)) {
            new_lines.push(overlay_line.clone());
            value_replaced = true;
        } else {
            new_lines.push(line);
        }
    }

    if !value_replaced {
        if section_found {
            // Find the section again and insert after it
            if let Some(pos) = new_lines.iter().position(|l| l.trim().starts_with('[') && l.contains(&wine_key_base)) {
                new_lines.insert(pos + 1, overlay_line);
            }
        } else {
            new_lines.push(String::new());
            new_lines.push(format!("[{}]", wine_key_base));
            new_lines.push(overlay_line);
        }
    }

    // Atomic-ish write
    let temp_path = user_reg.with_extension("reg.tmp");
    {
        let mut file = File::create(&temp_path)?;
        for line in new_lines {
            writeln!(file, "{}", line)?;
        }
    }
    std::fs::rename(temp_path, user_reg)?;

    Ok(())
}

pub fn remove_registry_entries(prefix: &Path) -> anyhow::Result<()> {
    let user_reg = prefix.join("user.reg");
    if !user_reg.exists() {
        return Err(anyhow::anyhow!("user.reg not found in prefix {:?}", prefix));
    }

    let mut lines: Vec<String> = Vec::new();
    if let Ok(file) = File::open(&user_reg) {
        let reader = BufReader::new(file);
        lines = reader.lines().flatten().collect();
    }

    let mut new_lines = Vec::new();
    let wine_key_base = EOS_OVERLAY_KEY.replace('\\', "\\\\");
    let mut in_target_section = false;

    for line in lines {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.contains(&wine_key_base) {
            in_target_section = true;
            new_lines.push(line);
        } else if trimmed.starts_with('[') {
            in_target_section = false;
            new_lines.push(line);
        } else if in_target_section && trimmed.starts_with(&format!("\"{}\"", EOS_OVERLAY_VALUE)) {
            // Skip this line
        } else {
            new_lines.push(line);
        }
    }

    let temp_path = user_reg.with_extension("reg.tmp");
    {
        let mut file = File::create(&temp_path)?;
        for line in new_lines {
            writeln!(file, "{}", line)?;
        }
    }
    std::fs::rename(temp_path, user_reg)?;

    Ok(())
}

pub fn search_overlay_installs(prefix: Option<&Path>) -> Vec<String> {
    let mut locations = Vec::new();

    if let Some(p) = prefix {
        // Launcher path
        let launcher_path = p.join("drive_c/Program Files (x86)/Epic Games/Launcher/Portal/Extras/Overlay");
        if launcher_path.exists() {
            locations.push(launcher_path.to_string_lossy().to_string());
        }

        // EOSH path
        let eosh_path = p.join(format!("drive_c/Program Files (x86)/Epic Games/Epic Online Services/managedArtifacts/{}", EOS_OVERLAY_APP_ID));
        if eosh_path.exists() {
            locations.push(eosh_path.to_string_lossy().to_string());
        }

        // Registry path
        if let Some(reg_path) = query_registry(p) {
            let mut normalized = reg_path.replace('\\', "/");
            if normalized.starts_with("Z:") {
                normalized = normalized[2..].to_string();
                let host_path = p.join(normalized.trim_start_matches('/'));
                if host_path.exists() {
                    locations.push(host_path.to_string_lossy().to_string());
                }
            } else if normalized.starts_with("C:") {
                normalized = normalized[2..].to_string();
                let host_path = p.join("drive_c").join(normalized.trim_start_matches('/'));
                if host_path.exists() {
                    locations.push(host_path.to_string_lossy().to_string());
                }
            }
        }
    }

    locations.dedup();
    locations
}
