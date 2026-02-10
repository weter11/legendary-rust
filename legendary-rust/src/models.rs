use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

fn deserialize_string_or_default<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<String>::deserialize(deserializer)?.unwrap_or_default())
}

fn deserialize_option_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer)
}

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct AdvancedInfo {
    pub app_name: String,
    pub save_path: Option<std::path::PathBuf>,
    pub backup_path: Option<std::path::PathBuf>,
    pub prefix_path: Option<std::path::PathBuf>,
    pub dlss_path: Option<std::path::PathBuf>,
    pub dlssd_path: Option<std::path::PathBuf>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TaskStatus {
    pub name: String,
    pub progress: f32,
    pub is_paused: bool,
    pub speed: String,
    pub eta: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SaveSyncStatus {
    pub app_name: String,
    pub files: Vec<CloudSaveFile>,
    pub local_time: Option<DateTime<Utc>>,
    pub remote_time: Option<DateTime<Utc>>,
    pub backup_time: Option<DateTime<Utc>>,
    pub loading: bool,
    pub error: Option<String>,
}

#[derive(PartialEq, Clone, Copy, Debug, Serialize, Deserialize)]
pub enum View {
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
    #[serde(rename = "customAttributes")]
    pub custom_attributes: Option<std::collections::HashMap<String, CustomAttribute>>,
    #[serde(rename = "partnerLinkType")]
    pub partner_link_type: Option<String>,
    #[serde(rename = "eulaIds")]
    pub eula_ids: Option<Vec<String>>,
    #[serde(rename = "dlcItemList", default)]
    pub dlc_item_list: Vec<DlcItem>,
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
    pub executable: String,
    #[serde(default)]
    pub install_size: u64,
    #[serde(default)]
    pub download_size: u64,
    #[serde(default = "default_platform")]
    pub platform: String,
    #[serde(default)]
    pub manifest_path: Option<String>,
}

fn default_platform() -> String {
    "Windows".to_string()
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
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub namespace: String,
    #[serde(default)]
    pub deployment_id: Option<String>,
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
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub app_name: String,
    #[serde(
        rename = "fileName",
        default,
        deserialize_with = "deserialize_string_or_default"
    )]
    pub file_name: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub manifest_name: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub hash: String,
    #[serde(default)]
    pub length: u64,
    #[serde(
        rename = "lastModified",
        default,
        deserialize_with = "deserialize_option_string"
    )]
    pub last_modified: Option<String>,
    #[serde(rename = "readLink")]
    pub read_link: Option<String>,
    #[serde(rename = "writeLink")]
    pub write_link: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CloudSaveResponse {
    pub files: std::collections::HashMap<String, CloudSaveFile>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Entitlement {
    #[serde(rename = "entitlementId")]
    pub entitlement_id: String,
    #[serde(rename = "entitlementName")]
    pub entitlement_name: String,
    pub namespace: String,
    #[serde(rename = "catalogItemId")]
    pub catalog_item_id: String,
    #[serde(rename = "accountId")]
    pub account_id: String,
    #[serde(rename = "identityId")]
    pub identity_id: String,
    #[serde(rename = "entitlementType")]
    pub entitlement_type: String,
    #[serde(rename = "grantDate")]
    pub grant_date: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DownloadTicket {
    #[serde(rename = "manifestUrl")]
    pub manifest_url: String,
}
