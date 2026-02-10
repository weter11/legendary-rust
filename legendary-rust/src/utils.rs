use chrono::{DateTime, Utc};
use eframe::egui;
use sha1::{Digest, Sha1};
use std::io::{Read, Result};
use std::path::{Path, PathBuf};

pub fn color_to_grayscale(pixels: &mut [egui::Color32]) {
    for pixel in pixels {
        let gray =
            (pixel.r() as f32 * 0.299 + pixel.g() as f32 * 0.587 + pixel.b() as f32 * 0.114) as u8;
        *pixel = egui::Color32::from_rgba_unmultiplied(gray, gray, gray, pixel.a());
    }
}

pub fn get_latest_local_save_time(path: &Path) -> Option<DateTime<Utc>> {
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

pub fn find_file_in_dir(dir: &Path, filename: &str) -> Option<PathBuf> {
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

pub fn get_all_files(dir: &Path) -> Vec<PathBuf> {
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

pub fn write_cloud_debug_blob(
    direction: &str,
    app_name: &str,
    remote_path: &str,
    data: &[u8],
) -> Option<PathBuf> {
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

pub fn construct_manifest_url(manifest_node: &serde_json::Value) -> Option<String> {
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

pub fn find_manifest_url(asset_manifest: &serde_json::Value) -> Option<String> {
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

pub fn extract_deployment_id(manifest_info: &serde_json::Value) -> Option<String> {
    manifest_info["elements"]
        .as_array()?
        .get(0)?
        .get("sidecar")?
        .get("config")?
        .as_str()
        .and_then(|config_str| serde_json::from_str::<serde_json::Value>(config_str).ok())
        .and_then(|config_json| config_json["deploymentId"].as_str().map(|s| s.to_string()))
}

pub fn get_cache_path(url: &str) -> Option<PathBuf> {
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

pub fn get_default_compat_data_path() -> Option<PathBuf> {
    let mut p = crate::auth::get_config_dir()?;
    p.push("compatdata");
    p.push("default");
    let _ = std::fs::create_dir_all(&p);
    Some(p)
}

pub fn hash_file(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha1::new();
    let mut buffer = [0u8; 8192];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 { break; }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

pub fn format_duration(dur: chrono::Duration) -> String {
    let secs = dur.num_seconds().abs();
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else if secs < 86400 {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    } else {
        format!("{}d {}h", secs / 86400, (secs % 86400) / 3600)
    }
}
