use crate::models::*;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, USER_AGENT};
use anyhow::Result;

pub struct EgsClient {
    client: reqwest::blocking::Client,
    user_basic: String,
    pw_basic: String,
    access_token: Option<String>,
}

impl EgsClient {
    pub fn new() -> Self {
        let user_agent = "UELauncher/11.0.1-14907503+++Portal+Release-Live Windows/10.0.19041.1.256.64bit".to_string();
        let user_basic = "34a02cf8f4414e29b15921876da36f9a".to_string();
        let pw_basic = "daafbccc737745039dffe53d94fc76cf".to_string();

        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_str(&user_agent).unwrap());

        let client = reqwest::blocking::Client::builder()
            .default_headers(headers)
            .build()
            .unwrap();

        Self {
            client,
            user_basic,
            pw_basic,
            access_token: None,
        }
    }

    pub fn get_auth_url() -> String {
        let user_basic = "34a02cf8f4414e29b15921876da36f9a";
        let login_url = "https://www.epicgames.com/id/login?redirectUrl=";
        let redirect_url = format!("https://www.epicgames.com/id/api/redirect?clientId={}&responseType=code", user_basic);
        format!("{}{}", login_url, urlencoding::encode(&redirect_url))
    }

    pub fn start_session(&mut self, code: &str) -> Result<OAuthToken> {
        self.do_auth(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("token_type", "eg1"),
        ])
    }

    pub fn refresh_session(&mut self, refresh_token: &str) -> Result<OAuthToken> {
        self.do_auth(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("token_type", "eg1"),
        ])
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
        self.access_token = Some(token.access_token.clone());
        Ok(token)
    }

    pub fn set_token(&mut self, token: &str) {
        self.access_token = Some(token.to_string());
    }

    pub fn get_library_items(&self) -> Result<Vec<LibraryItem>> {
        let mut records = Vec::new();
        let mut cursor = None;

        loop {
            let mut url = "https://library-service.live.use1a.on.epicgames.com/library/api/public/items?includeMetadata=true".to_string();
            if let Some(c) = &cursor {
                url.push_str(&format!("&cursor={}", c));
            }

            let response = self.client.get(&url)
                .header(AUTHORIZATION, format!("bearer {}", self.access_token.as_ref().unwrap()))
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

    pub fn get_game_assets(&self, platform: &str) -> Result<Vec<Asset>> {
        let url = format!("https://launcher-public-service-prod06.ol.epicgames.com/launcher/api/public/assets/{}", platform);
        let response = self.client.get(&url)
            .header(AUTHORIZATION, format!("bearer {}", self.access_token.as_ref().unwrap()))
            .send()?;

        let assets: Vec<Asset> = response.json()?;
        Ok(assets)
    }

    pub fn get_game_info(&self, namespace: &str, catalog_item_id: &str) -> Result<GameInfo> {
        let url = format!("https://catalog-public-service-prod06.ol.epicgames.com/catalog/api/shared/namespace/{}/bulk/items", namespace);
        let response = self.client.get(&url)
            .query(&[("id", catalog_item_id)])
            .header(AUTHORIZATION, format!("bearer {}", self.access_token.as_ref().unwrap()))
            .send()?;

        let j: serde_json::Value = response.json()?;
        let info_val = j.get(catalog_item_id).ok_or_else(|| anyhow::anyhow!("Item not found"))?;
        let info: GameInfo = serde_json::from_value(info_val.clone())?;
        Ok(info)
    }
}
