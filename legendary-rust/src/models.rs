use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LibraryItem {
    #[serde(rename = "appName")]
    pub app_name: String,
    #[serde(rename = "catalogItemId")]
    pub catalog_item_id: String,
    pub namespace: String,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LibraryResponse {
    pub records: Vec<LibraryItem>,
    #[serde(rename = "responseMetadata")]
    pub response_metadata: ResponseMetadata,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ResponseMetadata {
    #[serde(rename = "nextCursor")]
    pub next_cursor: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct OAuthToken {
    pub access_token: String,
    pub expires_in: i32,
    pub expires_at: String,
    pub token_type: String,
    pub refresh_token: Option<String>,
    pub refresh_expires: Option<i32>,
    pub refresh_expires_at: Option<String>,
    pub account_id: String,
    pub client_id: String,
    pub internal_client: bool,
    pub client_service: String,
    #[serde(rename = "displayName")]
    pub display_name: Option<String>,
    pub app: String,
    pub in_app_id: String,
    pub device_id: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Asset {
    #[serde(rename = "appName")]
    pub app_name: String,
    #[serde(rename = "catalogItemId")]
    pub catalog_item_id: String,
    pub namespace: String,
    #[serde(rename = "assetId")]
    pub asset_id: String,
    #[serde(rename = "buildVersion")]
    pub build_version: String,
    #[serde(rename = "labelName")]
    pub label_name: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GameInfo {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    #[serde(rename = "keyImages")]
    pub key_images: Vec<KeyImage>,
    pub namespace: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct KeyImage {
    pub url: String,
    #[serde(rename = "type")]
    pub image_type: String,
    pub md5: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct InstalledGame {
    pub app_name: String,
    pub install_path: String,
    pub title: String,
    pub version: String,
    #[serde(default)]
    pub install_size: u64,
    #[serde(default)]
    pub download_size: u64,
}

#[derive(Debug, Clone)]
pub struct InstallInfo {
    pub app_name: String,
    pub title: String,
    pub install_path: std::path::PathBuf,
    pub download_size: u64,
    pub install_size: u64,
    pub free_space: u64,
    pub available_tags: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LocalGameMetadata {
    pub app_name: String,
    pub app_title: String,
    pub metadata: LocalMetadataDetails,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct LocalMetadataDetails {
    pub developer: Option<String>,
    #[serde(rename = "keyImages", default)]
    pub key_images: Vec<KeyImage>,
    #[serde(rename = "dlcItemList")]
    pub dlc_item_list: Option<Vec<DlcItem>>,
    #[serde(rename = "customAttributes")]
    pub custom_attributes: Option<std::collections::HashMap<String, CustomAttribute>>,
    #[serde(rename = "releaseInfo")]
    pub release_info: Option<Vec<ReleaseInfo>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DlcItem {
    pub title: String,
    pub id: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CustomAttribute {
    pub value: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ReleaseInfo {
    #[serde(rename = "appId")]
    pub app_id: String,
    pub platform: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CloudSaveFile {
    #[serde(rename = "fileName")]
    pub file_name: String,
    pub hash: String,
    pub length: u64,
    #[serde(rename = "lastModified")]
    pub last_modified: String,
}
