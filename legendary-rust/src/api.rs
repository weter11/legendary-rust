use crate::models::*;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, USER_AGENT};
use anyhow::Result;
use serde_json;
use std::collections::HashMap;

const OAUTH_HOST: &str = "account-public-service-prod03.ol.epicgames.com";
const LAUNCHER_HOST: &str = "launcher-public-service-prod06.ol.epicgames.com";
const LAUNCHER_HOST_AK: &str = "launcher-public-service-prod.ak.epicgames.com";
const ENTITLEMENT_HOST: &str = "entitlement-public-service-prod08.ol.epicgames.com";
const EULATRACKING_HOST: &str = "eulatracking-public-service-prod06.ol.epicgames.com";
const CATALOG_HOST: &str = "catalog-public-service-prod06.ol.epicgames.com";
const ECOMMERCE_HOST: &str = "ecommerceintegration-public-service-ecomprod02.ol.epicgames.com";
const DATASTORAGE_HOST: &str = "datastorage-public-service-liveegs.live.use1a.on.epicgames.com";
const LIBRARY_HOST: &str = "library-service.live.use1a.on.epicgames.com";

pub const UA_DEFAULT: &str = "UELauncher/11.0.1-14907503+++Portal+Release-Live Windows/10.0.19041.1.256.64bit";
pub const UA_EGS: &str = "EpicGamesLauncher/14.0.8-22004686+++Portal+Release-Live";

pub fn get_ua_for_app(app_name: &str) -> &'static str {
    if app_name == crate::eos::EOS_OVERLAY_APP_ID {
        UA_EGS
    } else {
        UA_DEFAULT
    }
}

pub struct EgsClient {
    client: reqwest::blocking::Client,
    user_basic: String,
    pw_basic: String,
    token_info: Option<OAuthToken>,
    save_token_path: Option<std::path::PathBuf>,
}

impl EgsClient {
    pub fn new() -> Result<Self> {
        let user_agent = UA_DEFAULT.to_string();
        let user_basic = "34a02cf8f4414e29b15921876da36f9a".to_string();
        let pw_basic = "daafbccc737745039dffe53d94fc76cf".to_string();

        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_str(&user_agent)?);

        let client = reqwest::blocking::Client::builder()
            .default_headers(headers)
            .timeout(std::time::Duration::from_secs(30))
            .build()?;

        Ok(Self {
            client,
            user_basic,
            pw_basic,
            token_info: None,
            save_token_path: None,
        })
    }

    pub fn get_auth_url() -> String {
        let user_basic = "34a02cf8f4414e29b15921876da36f9a";
        let login_url = "https://www.epicgames.com/id/login?redirectUrl=";
        let redirect_url = format!("https://www.epicgames.com/id/api/redirect?clientId={}&responseType=code", user_basic);
        format!("{}{}", login_url, urlencoding::encode(&redirect_url))
    }

    pub fn start_session(&mut self, code: &str) -> Result<OAuthToken> {
        let token = self.do_auth(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("token_type", "eg1"),
        ])?;
        Ok(token)
    }

    pub fn refresh_session(&mut self, refresh_token: &str) -> Result<OAuthToken> {
        let token = self.do_auth(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("token_type", "eg1"),
        ])?;
        Ok(token)
    }

    fn do_auth(&mut self, params: &[(&str, &str)]) -> Result<OAuthToken> {
        let url = format!("https://{}/account/api/oauth/token", OAUTH_HOST);

        let response = self.client.post(url)
            .basic_auth(&self.user_basic, Some(&self.pw_basic))
            .form(&params)
            .send()?;

        if !response.status().is_success() {
            let err_text = response.text()?;
            return Err(anyhow::anyhow!("Auth failed: {}", err_text));
        }

        let token: OAuthToken = response.json()?;
        self.token_info = Some(token.clone());

        if let Some(path) = &self.save_token_path {
            if let Ok(json) = serde_json::to_string(&token) {
                let _ = std::fs::write(path, json);
            }
        }

        Ok(token)
    }

    pub fn refresh_if_needed(&mut self) -> Result<()> {
        let refresh_token = if let Some(token) = &self.token_info {
            // Check if expired (simplified: check if expires_at is in the past)
            if let Ok(expires_at) = chrono::DateTime::parse_from_rfc3339(&token.expires_at) {
                if expires_at.with_timezone(&chrono::Utc) > chrono::Utc::now() + chrono::Duration::minutes(5) {
                    return Ok(());
                }
            }
            token.refresh_token.clone()
        } else {
            return Err(anyhow::anyhow!("Not logged in"));
        };

        if let Some(rt) = refresh_token {
            self.refresh_session(&rt)?;
            Ok(())
        } else {
            Err(anyhow::anyhow!("No refresh token available"))
        }
    }

    pub fn get_game_token(&mut self) -> Result<String> {
        self.refresh_if_needed()?;
        let url = format!("https://{}/account/api/oauth/exchange", OAUTH_HOST);
        let token = self.token_info.as_ref().map(|t| &t.access_token).ok_or_else(|| anyhow::anyhow!("Not logged in"))?;

        let response = self.client.get(url)
            .header(AUTHORIZATION, format!("bearer {}", token))
            .send()?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!("Failed to get game token: {}", response.status()));
        }

        let json: serde_json::Value = response.json()?;
        let code = json.get("code").and_then(|c| c.as_str()).ok_or_else(|| anyhow::anyhow!("Code not found in response"))?;
        Ok(code.to_string())
    }

    pub fn set_token(&mut self, token: &OAuthToken) {
        self.token_info = Some(token.clone());
    }

    pub fn set_save_token_path(&mut self, path: std::path::PathBuf) {
        self.save_token_path = Some(path);
    }

    pub fn get_account_id(&self) -> Option<String> {
        self.token_info.as_ref().map(|t| t.account_id.clone())
    }

    pub fn get_display_name(&self) -> Option<String> {
        self.token_info.as_ref().and_then(|t| t.display_name.clone())
    }

    pub fn get_library_items(&mut self) -> Result<Vec<LibraryItem>> {
        self.refresh_if_needed()?;
        let mut records = Vec::new();
        let mut cursor = None;

        loop {
            let mut url = format!("https://{}/library/api/public/items?includeMetadata=true", LIBRARY_HOST);
            if let Some(c) = &cursor {
                url.push_str(&format!("&cursor={}", c));
            }

            let token = self.token_info.as_ref().map(|t| &t.access_token).ok_or_else(|| anyhow::anyhow!("Not logged in"))?;
            let response = self.client.get(&url)
                .header(AUTHORIZATION, format!("bearer {}", token))
                .send()?;

            let library_response: LibraryResponse = response.json()?;
            records.extend(library_response.records);

            if let Some(next_cursor) = library_response.response_metadata.next_cursor {
                if next_cursor.is_empty() {
                    break;
                }
                cursor = Some(next_cursor);
            } else {
                break;
            }
        }

        Ok(records)
    }

    pub fn get_game_assets(&mut self, platform: &str) -> Result<Vec<Asset>> {
        self.refresh_if_needed()?;
        let token = self.token_info.as_ref().map(|t| &t.access_token).ok_or_else(|| anyhow::anyhow!("Not logged in"))?;
        let url = format!("https://{}/launcher/api/public/assets/{}", LAUNCHER_HOST, platform);
        let response = self.client.get(&url)
            .header(AUTHORIZATION, format!("bearer {}", token))
            .send()?;

        let assets: Vec<Asset> = response.json()?;
        Ok(assets)
    }

    pub fn download_manifest(&self, url: &str, app_name: Option<&str>) -> Result<Vec<u8>> {
        let mut req = self.client.get(url);
        if let Some(app) = app_name {
            req = req.header(USER_AGENT, get_ua_for_app(app));
        }
        let response = req.send()?;
        if !response.status().is_success() {
            return Err(anyhow::anyhow!("Failed to download manifest: {}", response.status()));
        }
        Ok(response.bytes()?.to_vec())
    }

    pub fn get_asset_manifest(&mut self, platform: &str, namespace: &str, catalog_item_id: &str, app_name: &str, label_name: &str) -> Result<serde_json::Value> {
        self.refresh_if_needed()?;
        let token = self.token_info.as_ref().map(|t| &t.access_token).ok_or_else(|| anyhow::anyhow!("Not logged in"))?;
        let url = format!("https://{}/launcher/api/public/assets/v2/platform/{}/namespace/{}/catalogItem/{}/app/{}/label/{}",
            LAUNCHER_HOST, platform, namespace, catalog_item_id, app_name, label_name);

        let response = self.client.get(&url)
            .header(AUTHORIZATION, format!("bearer {}", token))
            .header(USER_AGENT, get_ua_for_app(app_name))
            .send()?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!("Failed to fetch asset manifest info: {}", response.status()));
        }

        let json: serde_json::Value = response.json()?;
        Ok(json)
    }

    pub fn get_game_info(&mut self, namespace: &str, catalog_item_id: &str) -> Result<GameInfo> {
        self.refresh_if_needed()?;
        let token = self.token_info.as_ref().map(|t| &t.access_token).ok_or_else(|| anyhow::anyhow!("Not logged in"))?;
        let url = format!("https://{}/catalog/api/shared/namespace/{}/bulk/items", CATALOG_HOST, namespace);
        let response = self.client.get(&url)
            .query(&[("id", catalog_item_id)])
            .header(AUTHORIZATION, format!("bearer {}", token))
            .send()?;

        let j: serde_json::Value = response.json()?;
        let info_val = j.get(catalog_item_id).ok_or_else(|| anyhow::anyhow!("Item not found"))?;
        let info: GameInfo = serde_json::from_value(info_val.clone())?;
        Ok(info)
    }

    pub fn get_cloud_save_metadata(&mut self, _namespace: &str, account_id: &str, app_id: &str) -> Result<Vec<CloudSaveFile>> {
        self.refresh_if_needed()?;
        let token = self.token_info.as_ref()
            .map(|t| &t.access_token)
            .ok_or_else(|| anyhow::anyhow!("Not logged in"))?;

        let url = format!("https://{}/api/v1/access/egstore/savesync/{}/{}/",
            DATASTORAGE_HOST, account_id, app_id);

        let response = self.client.get(&url)
            .header(AUTHORIZATION, format!("bearer {}", token))
            .send()?;

        let status = response.status();

        if status == reqwest::StatusCode::NOT_FOUND {
            return Ok(Vec::new());
        }

        if !status.is_success() {
            let err_text = response.text()?;
            return Err(anyhow::anyhow!("Failed to fetch cloud saves: {} - {}", status, err_text));
        }

        let body = response.text()?;

        let json: serde_json::Value = serde_json::from_str(&body)?;
        let save_response: CloudSaveResponse = serde_json::from_value(json)?;

        let mut files = Vec::new();
        for (filename, mut file_info) in save_response.files {
            // Filter for manifest files only as requested
            if !filename.contains(".manifest") {
                continue;
            }

            let parts: Vec<&str> = filename.split('/').collect();
            if parts.len() >= 5 {
                file_info.app_name = parts[2].to_string();
                file_info.file_name = filename.clone();
                file_info.manifest_name = parts[4].to_string();
                files.push(file_info);
            } else {
                if file_info.file_name.is_empty() {
                    file_info.file_name = filename;
                }
                files.push(file_info);
            }
        }

        Ok(files)
    }

    pub fn get_cloud_save_links(&mut self, account_id: &str, app_id: &str, filenames: &[String]) -> Result<HashMap<String, CloudSaveFile>> {
        self.refresh_if_needed()?;
        let token = self.token_info.as_ref().map(|t| &t.access_token).ok_or_else(|| anyhow::anyhow!("Not logged in"))?;

        let url = format!("https://{}/api/v1/access/egstore/savesync/{}/{}/", DATASTORAGE_HOST, account_id, app_id);
        let response = self.client.post(&url)
            .header(AUTHORIZATION, format!("bearer {}", token))
            .json(&serde_json::json!({"files": filenames}))
            .send()?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!("Failed to get cloud save links: {}", response.status()));
        }

        let body = response.text()?;
        let json: serde_json::Value = serde_json::from_str(&body)?;
        let save_response: CloudSaveResponse = serde_json::from_value(json)?;
        Ok(save_response.files)
    }

    pub fn download_cloud_file(&mut self, _namespace: &str, account_id: &str, app_id: &str, filename: &str) -> Result<Vec<u8>> {
        let links = self.get_cloud_save_links(account_id, app_id, &[filename.to_string()])?;

        let file = links.get(filename)
            .ok_or_else(|| anyhow::anyhow!("File {} not found in metadata", filename))?;

        let download_url = file.read_link.as_ref()
            .ok_or_else(|| anyhow::anyhow!("No readLink provided for {}", filename))?;

        let response = self.client.get(download_url).send()?;
        if !response.status().is_success() {
            return Err(anyhow::anyhow!("Failed to download cloud file: {}", response.status()));
        }

        Ok(response.bytes()?.to_vec())
    }

    pub fn upload_cloud_file(&mut self, _namespace: &str, account_id: &str, app_id: &str, filename: &str, data: Vec<u8>) -> Result<()> {
        self.refresh_if_needed()?;
        let token = self.token_info.as_ref().map(|t| &t.access_token).ok_or_else(|| anyhow::anyhow!("Not logged in"))?;

        // First get the upload URL
        let url = format!("https://{}/api/v1/access/egstore/savesync/{}/{}/", DATASTORAGE_HOST, account_id, app_id);
        let response = self.client.post(&url)
            .header(AUTHORIZATION, format!("bearer {}", token))
            .json(&serde_json::json!({"files": [filename]}))
            .send()?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!("Failed to get upload URL: {}", response.status()));
        }

        let body = response.text()?;
        let json: serde_json::Value = serde_json::from_str(&body)?;
        let response: CloudSaveResponse = serde_json::from_value(json)?;

        let file = response.files.get(filename)
            .ok_or_else(|| anyhow::anyhow!("File {} not found in metadata", filename))?;

        let upload_url = file.write_link.as_ref()
            .ok_or_else(|| anyhow::anyhow!("No writeLink provided for {}", filename))?;

        let response = self.client.put(upload_url)
            .body(data)
            .send()?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!("Failed to upload cloud file: {}", response.status()));
        }

        Ok(())
    }

    pub fn invalidate_session(&mut self) -> Result<()> {
        let token = self.token_info.as_ref().map(|t| &t.access_token).ok_or_else(|| anyhow::anyhow!("Not logged in"))?;
        let url = format!("https://{}/account/api/oauth/sessions/kill/{}", OAUTH_HOST, token);

        let response = self.client.delete(&url)
            .header(AUTHORIZATION, format!("bearer {}", token))
            .send()?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!("Failed to invalidate session: {}", response.status()));
        }

        self.token_info = None;
        Ok(())
    }

    pub fn get_ownership_token(&mut self, namespace: &str, catalog_item_id: &str) -> Result<Vec<u8>> {
        self.refresh_if_needed()?;
        let token = self.token_info.as_ref()
            .map(|t| &t.access_token)
            .ok_or_else(|| anyhow::anyhow!("Not logged in"))?;

        let account_id = self.token_info.as_ref()
            .map(|t| t.account_id.as_str())
            .ok_or_else(|| anyhow::anyhow!("No account ID"))?;

        let url = format!(
            "https://{}/ecommerceintegration/api/public/platforms/EPIC/identities/{}/ownershipToken",
            ECOMMERCE_HOST, account_id
        );

        let ns_catalog = format!("{}:{}", namespace, catalog_item_id);

        let mut form_data = std::collections::HashMap::new();
        form_data.insert("nsCatalogItemId", ns_catalog);

        let response = self.client.post(&url)
            .header(AUTHORIZATION, format!("bearer {}", token))
            .form(&form_data)
            .send()?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!("Failed to get ownership token: {} - {}",
                                       response.status(),
                                       response.text().unwrap_or_default()));
        }

        Ok(response.bytes()?.to_vec())
    }

    pub fn get_launcher_manifests(&mut self, platform: &str) -> Result<serde_json::Value> {
        self.refresh_if_needed()?;
        let url = format!("https://{}/launcher/api/public/assets/v2/platform/{}/launcher", LAUNCHER_HOST, platform);
        let token = self.token_info.as_ref().map(|t| &t.access_token).ok_or_else(|| anyhow::anyhow!("Not logged in"))?;

        let response = self.client.get(url)
            .header(AUTHORIZATION, format!("bearer {}", token))
            .send()?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!("Failed to get launcher manifests: {}", response.status()));
        }

        let json: serde_json::Value = response.json()?;
        Ok(json)
    }

    pub fn get_user_entitlements(&mut self) -> Result<Vec<Entitlement>> {
        self.refresh_if_needed()?;
        let token = self.token_info.as_ref().map(|t| &t.access_token).ok_or_else(|| anyhow::anyhow!("Not logged in"))?;
        let url = format!("https://{}/entitlement/api/public/entitlements", ENTITLEMENT_HOST);

        let response = self.client.get(url)
            .header(AUTHORIZATION, format!("bearer {}", token))
            .send()?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!("Failed to get entitlements: {}", response.status()));
        }

        let entitlements: Vec<Entitlement> = response.json()?;
        Ok(entitlements)
    }

    pub fn get_download_ticket(&mut self, platform: &str, namespace: &str, catalog_item_id: &str, app_name: &str) -> Result<DownloadTicket> {
        self.refresh_if_needed()?;
        let url = format!("https://{}/launcher/api/public/assets/v2/platform/{}/namespace/{}/catalogItem/{}/app/{}/downloadTicket", LAUNCHER_HOST, platform, namespace, catalog_item_id, app_name);
        let token = self.token_info.as_ref().map(|t| &t.access_token).ok_or_else(|| anyhow::anyhow!("Not logged in"))?;

        let response = self.client.get(&url)
            .header(AUTHORIZATION, format!("bearer {}", token))
            .header(USER_AGENT, get_ua_for_app(app_name))
            .send()?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!("Failed to get download ticket: {}", response.status()));
        }

        let ticket: DownloadTicket = response.json()?;
        Ok(ticket)
    }

    pub fn get_game_manifest_by_ticket(&self, manifest_url: &str, app_name: Option<&str>) -> Result<Vec<u8>> {
        self.download_manifest(manifest_url, app_name)
    }

    pub fn start_session_with_sid(&mut self, sid: &str) -> Result<OAuthToken> {
        let token = self.do_auth(&[
            ("grant_type", "sid"),
            ("sid", sid),
            ("token_type", "eg1"),
        ])?;
        Ok(token)
    }

    pub fn eula_get_status(&mut self, eula_id: &str) -> Result<Option<serde_json::Value>> {
        self.refresh_if_needed()?;
        let token = self.token_info.as_ref().map(|t| &t.access_token).ok_or_else(|| anyhow::anyhow!("Not logged in"))?;
        let account_id = self.get_account_id().unwrap_or_default();
        let url = format!("https://{}/eulatracking/api/public/agreements/{}/account/{}", EULATRACKING_HOST, eula_id, account_id);

        let response = self.client.get(&url)
            .query(&[("includeAll", "true")])
            .header(AUTHORIZATION, format!("bearer {}", token))
            .send()?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }

        if !response.status().is_success() {
            return Err(anyhow::anyhow!("Failed to fetch EULA status: {}", response.status()));
        }

        let json: serde_json::Value = response.json()?;
        if json.get("accepted").and_then(|v| v.as_bool()).unwrap_or(false) {
            return Ok(None);
        }
        Ok(Some(json))
    }

    pub fn eula_accept(&mut self, eula_id: &str, version: i32, locale: Option<&str>) -> Result<()> {
        self.refresh_if_needed()?;
        let token = self.token_info.as_ref().map(|t| &t.access_token).ok_or_else(|| anyhow::anyhow!("Not logged in"))?;
        let account_id = self.get_account_id().unwrap_or_default();
        let url = format!("https://{}/eulatracking/api/public/agreements/{}/version/{}/account/{}/accept", EULATRACKING_HOST, eula_id, version, account_id);

        let mut req = self.client.post(&url)
            .header(AUTHORIZATION, format!("bearer {}", token));

        if let Some(l) = locale {
            req = req.query(&[("locale", l)]);
        }

        let response = req.send()?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!("Failed to accept EULA: {}", response.status()));
        }

        Ok(())
    }

    pub fn get_external_auths(&mut self) -> Result<serde_json::Value> {
        self.refresh_if_needed()?;
        let token = self.token_info.as_ref().map(|t| &t.access_token).ok_or_else(|| anyhow::anyhow!("Not logged in"))?;
        let account_id = self.get_account_id().unwrap_or_default();
        let url = format!("https://{}/account/api/public/account/{}/externalAuths", OAUTH_HOST, account_id);

        let response = self.client.get(&url)
            .header(AUTHORIZATION, format!("bearer {}", token))
            .send()?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!("Failed to fetch external auths: {}", response.status()));
        }

        let json: serde_json::Value = response.json()?;
        Ok(json)
    }
}
