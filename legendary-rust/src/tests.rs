#[cfg(test)]
mod tests {
    use crate::models::*;

    #[test]
    fn test_library_item_deserialization() {
        let json = r#"{
            "appName": "Anemone",
            "catalogItemId": "item_id",
            "namespace": "ns",
            "metadata": {
                "title": "World of Goo"
            }
        }"#;
        let item: LibraryItem = serde_json::from_str(json).unwrap();
        assert_eq!(item.app_name, "Anemone");
        assert_eq!(item.metadata.unwrap().get("title").unwrap().as_str().unwrap(), "World of Goo");
    }

    #[test]
    fn test_oauth_token_deserialization() {
        let json = r#"{
            "access_token": "abc",
            "expires_in": 3600,
            "expires_at": "2023-01-01T00:00:00Z",
            "token_type": "bearer",
            "account_id": "acc_id",
            "client_id": "cli_id",
            "internal_client": true,
            "client_service": "service",
            "displayName": "User",
            "app": "app",
            "in_app_id": "in_app",
            "device_id": "dev_id"
        }"#;
        let token: OAuthToken = serde_json::from_str(json).unwrap();
        assert_eq!(token.access_token, "abc");
        assert_eq!(token.display_name.unwrap(), "User");
    }

    #[test]
    fn test_entitlement_deserialization() {
        let json = r#"{
            "entitlementId": "ent_id",
            "entitlementName": "ent_name",
            "namespace": "ns",
            "catalogItemId": "item_id",
            "accountId": "acc_id",
            "identityId": "id_id",
            "entitlementType": "EXECUTABLE",
            "grantDate": "2023-01-01T00:00:00Z"
        }"#;
        let ent: Entitlement = serde_json::from_str(json).unwrap();
        assert_eq!(ent.entitlement_id, "ent_id");
        assert_eq!(ent.entitlement_type, "EXECUTABLE");
    }

    #[test]
    fn test_download_ticket_deserialization() {
        let json = r#"{
            "manifestUrl": "https://example.com/manifest"
        }"#;
        let ticket: DownloadTicket = serde_json::from_str(json).unwrap();
        assert_eq!(ticket.manifest_url, "https://example.com/manifest");
    }

    #[test]
    fn test_local_metadata_deserialization() {
        let json = r#"{
            "app_name": "Anemone",
            "app_title": "World of Goo",
            "metadata": {
                "id": "item_id",
                "namespace": "ns",
                "developer": "2D BOY",
                "customAttributes": {
                    "CanRunOffline": { "value": "true" },
                    "OwnershipToken": { "value": "true" }
                }
            }
        }"#;
        let meta: LocalGameMetadata = serde_json::from_str(json).unwrap();
        assert_eq!(meta.metadata.id, "item_id");
        assert_eq!(meta.metadata.namespace, "ns");
        assert_eq!(meta.metadata.developer.unwrap(), "2D BOY");
        let attrs = meta.metadata.custom_attributes.unwrap();
        assert_eq!(attrs.get("CanRunOffline").unwrap().value, "true");
        assert_eq!(attrs.get("OwnershipToken").unwrap().value, "true");
    }
}
