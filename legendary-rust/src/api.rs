use crate::models::*;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, USER_AGENT};
use anyhow::Result;

pub struct EgsClient {
    client: reqwest::blocking::Client,
    user_basic: String,
    pw_basic: String,
    token_info: Option<OAuthToken>,
}

impl EgsClient {
    pub fn new() -> Result<Self> {
        let user_agent = "UELauncher/11.0.1-14907503+++Portal+Release-Live Windows/10.0.19041.1.256.64bit".to_string();
        let user_basic = "34a02cf8f4414e29b15921876da36f9a".to_string();
        let pw_basic = "daafbccc737745039dffe53d94fc76cf".to_string();

        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_str(&user_agent)?);

        let client = reqwest::blocking::Client::builder()
            .default_headers(headers)
            .build()?;

        Ok(Self {
            client,
            user_basic,
            pw_basic,
            access_token: None,
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
        let url = "https://account-public-service-prod03.ol.epicgames.com/account/api/oauth/token";

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
        let url = "https://account-public-service-prod03.ol.epicgames.com/account/api/oauth/exchange";
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

    pub fn get_library_items(&mut self) -> Result<Vec<LibraryItem>> {
        self.refresh_if_needed()?;
        let mut records = Vec::new();
        let mut cursor = None;

        loop {
            let mut url = "https://library-service.live.use1a.on.epicgames.com/library/api/public/items?includeMetadata=true".to_string();
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
        let url = format!("https://launcher-public-service-prod06.ol.epicgames.com/launcher/api/public/assets/{}", platform);
        let response = self.client.get(&url)
            .header(AUTHORIZATION, format!("bearer {}", token))
            .send()?;

        let assets: Vec<Asset> = response.json()?;
        Ok(assets)
    }

    pub fn download_manifest(&self, url: &str) -> Result<Vec<u8>> {
        let response = self.client.get(url).send()?;
        if !response.status().is_success() {
            return Err(anyhow::anyhow!("Failed to download manifest: {}", response.status()));
        }
        Ok(response.bytes()?.to_vec())
    }

    pub fn get_asset_manifest(&mut self, platform: &str, namespace: &str, catalog_item_id: &str, app_name: &str, label_name: &str) -> Result<serde_json::Value> {
        self.refresh_if_needed()?;
        let token = self.token_info.as_ref().map(|t| &t.access_token).ok_or_else(|| anyhow::anyhow!("Not logged in"))?;
        let url = format!("https://launcher-public-service-prod06.ol.epicgames.com/launcher/api/public/assets/v2/platform/{}/namespace/{}/catalogItem/{}/app/{}/label/{}",
            platform, namespace, catalog_item_id, app_name, label_name);

        let response = self.client.get(&url)
            .header(AUTHORIZATION, format!("bearer {}", token))
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
        let url = format!("https://catalog-public-service-prod06.ol.epicgames.com/catalog/api/shared/namespace/{}/bulk/items", namespace);
        let response = self.client.get(&url)
            .query(&[("id", catalog_item_id)])
            .header(AUTHORIZATION, format!("bearer {}", token))
            .send()?;

        let j: serde_json::Value = response.json()?;
        let info_val = j.get(catalog_item_id).ok_or_else(|| anyhow::anyhow!("Item not found"))?;
        let info: GameInfo = serde_json::from_value(info_val.clone())?;
        Ok(info)
    }

    pub fn get_cloud_save_metadata(&mut self, namespace: &str, account_id: &str, app_id: &str) -> Result<Vec<CloudSaveFile>> {
        self.refresh_if_needed()?;
        let token = self.token_info.as_ref().map(|t| &t.access_token).ok_or_else(|| anyhow::anyhow!("Not logged in"))?;
        let url = format!("https://cloudstorage-public-service-live.ak.epicgames.com/cloudstorage/api/storage/{}/{}/{}", namespace, account_id, app_id);
        let response = self.client.get(&url)
            .header(AUTHORIZATION, format!("bearer {}", token))
            .send()?;

        if response.status() == 404 {
            return Ok(Vec::new());
        }

        if !response.status().is_success() {
            let err_text = response.text()?;
            return Err(anyhow::anyhow!("Failed to fetch cloud saves: {}", err_text));
        }

        let files: Vec<CloudSaveFile> = response.json()?;
        Ok(files)
    }

    pub fn download_cloud_file(&mut self, namespace: &str, account_id: &str, app_id: &str, filename: &str) -> Result<Vec<u8>> {
        self.refresh_if_needed()?;
        let token = self.token_info.as_ref().map(|t| &t.access_token).ok_or_else(|| anyhow::anyhow!("Not logged in"))?;
        let url = format!("https://cloudstorage-public-service-live.ak.epicgames.com/cloudstorage/api/storage/{}/{}/{}/{}", namespace, account_id, app_id, filename);
        let response = self.client.get(&url)
            .header(AUTHORIZATION, format!("bearer {}", token))
            .send()?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!("Failed to download cloud file: {}", response.status()));
        }

        Ok(response.bytes()?.to_vec())
    }

    pub fn upload_cloud_file(&mut self, namespace: &str, account_id: &str, app_id: &str, filename: &str, data: Vec<u8>) -> Result<()> {
        self.refresh_if_needed()?;
        let token = self.token_info.as_ref().map(|t| &t.access_token).ok_or_else(|| anyhow::anyhow!("Not logged in"))?;
        let url = format!("https://cloudstorage-public-service-live.ak.epicgames.com/cloudstorage/api/storage/{}/{}/{}/{}", namespace, account_id, app_id, filename);
        let response = self.client.put(&url)
            .header(AUTHORIZATION, format!("bearer {}", token))
            .body(data)
            .send()?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!("Failed to upload cloud file: {}", response.status()));
        }

        Ok(())
    }

    pub fn invalidate_session(&mut self) -> Result<()> {
        let token = self.token_info.as_ref().map(|t| &t.access_token).ok_or_else(|| anyhow::anyhow!("Not logged in"))?;
        let url = format!("https://account-public-service-prod03.ol.epicgames.com/account/api/oauth/sessions/kill/{}", token);

        let response = self.client.delete(&url)
            .header(AUTHORIZATION, format!("bearer {}", token))
            .send()?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!("Failed to invalidate session: {}", response.status()));
        }

        self.token_info = None;
        Ok(())
    }

    pub fn get_ownership_token(&mut self, namespace: &str, catalog_item_id: &str) -> Result<String> {
        self.refresh_if_needed()?;
        let url = format!("https://ecommerce-public-service-ecomprod02.ol.epicgames.com/ecommerce/api/public/namespaces/{}/items/{}/ownership/token", namespace, catalog_item_id);
        let token = self.token_info.as_ref().map(|t| &t.access_token).ok_or_else(|| anyhow::anyhow!("Not logged in"))?;

        let response = self.client.get(&url)
            .header(AUTHORIZATION, format!("bearer {}", token))
            .send()?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!("Failed to get ownership token: {}", response.status()));
        }

        let json: OwnershipTokenResponse = response.json()?;
        Ok(json.token)
    }

    pub fn get_launcher_manifests(&mut self, platform: &str) -> Result<serde_json::Value> {
        self.refresh_if_needed()?;
        let url = format!("https://launcher-public-service-prod06.ol.epicgames.com/launcher/api/public/assets/v2/platform/{}/launcher", platform);
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
        let url = "https://entitlement-public-service-prod08.ol.epicgames.com/entitlement/api/public/entitlements";

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
        let url = format!("https://launcher-public-service-prod06.ol.epicgames.com/launcher/api/public/assets/v2/platform/{}/namespace/{}/catalogItem/{}/app/{}/downloadTicket", platform, namespace, catalog_item_id, app_name);
        let token = self.token_info.as_ref().map(|t| &t.access_token).ok_or_else(|| anyhow::anyhow!("Not logged in"))?;

        let response = self.client.get(&url)
            .header(AUTHORIZATION, format!("bearer {}", token))
            .send()?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!("Failed to get download ticket: {}", response.status()));
        }

        let ticket: DownloadTicket = response.json()?;
        Ok(ticket)
    }

    pub fn get_game_manifest_by_ticket(&self, manifest_url: &str) -> Result<Vec<u8>> {
        self.download_manifest(manifest_url)
    }

    pub fn start_session_with_sid(&mut self, sid: &str) -> Result<OAuthToken> {
        let token = self.do_auth(&[
            ("grant_type", "sid"),
            ("sid", sid),
            ("token_type", "eg1"),
        ])?;
        Ok(token)
    }
}
