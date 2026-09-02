// API Key Authentication Middleware Tests
//
// This module contains tests for the API key authentication middleware.
// Tests require Redis and PostgreSQL instances.

use chrono::{Duration, Utc};
use herald_core::domain::client_api_keys::entities::ClientApiKey;
use herald_core::domain::client_api_keys::services::ClientApiKeyService;

/// Helper function to create a test API key
fn create_test_api_key_entity(id: &str, realm_id: &str, enabled: bool) -> ClientApiKey {
    ClientApiKey {
        id: id.to_string(),
        name: format!("Test Key {}", id),
        api_key_hash: format!("hash-{}", id),
        realm_id: realm_id.to_string(),
        client_app_id: None,
        enabled,
        expires_at: None,
        created_at: Utc::now(),
        last_used_at: None,
    }
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn test_api_key_hashing() {
        // Test that API key hashing produces deterministic hashes (SHA-256 with fixed salt)
        let api_key = "test-api-key-12345";

        let hash1 = ClientApiKeyService::hash_api_key(api_key);
        let hash2 = ClientApiKeyService::hash_api_key(api_key);

        // With SHA-256 and deterministic salt, hashes should be identical
        assert_eq!(
            hash1, hash2,
            "Hashes should be identical with deterministic salt"
        );

        // Verify hash format (sha256:...)
        assert!(
            hash1.starts_with("sha256:"),
            "Hash should use sha256: prefix"
        );

        // Both should verify successfully
        assert!(ClientApiKeyService::verify_api_key(api_key, &hash1));
        assert!(ClientApiKeyService::verify_api_key(api_key, &hash2));
    }

    #[test]
    fn test_api_key_verification() {
        let api_key = "test-api-key-12345";
        let hash = ClientApiKeyService::hash_api_key(api_key);

        assert!(ClientApiKeyService::verify_api_key(api_key, &hash));
        assert!(!ClientApiKeyService::verify_api_key("wrong-key", &hash));
    }

    #[test]
    fn test_api_key_validation() {
        let enabled_key = create_test_api_key_entity("key-1", "realm-1", true);
        assert!(enabled_key.is_valid());

        let disabled_key = create_test_api_key_entity("key-2", "realm-1", false);
        assert!(!disabled_key.is_valid());

        let expired_key = ClientApiKey {
            id: "key-3".to_string(),
            name: "Expired Key".to_string(),
            api_key_hash: "hash-3".to_string(),
            realm_id: "realm-1".to_string(),
            client_app_id: None,
            enabled: true,
            expires_at: Some(Utc::now() - Duration::days(1)),
            created_at: Utc::now() - Duration::days(2),
            last_used_at: None,
        };
        assert!(!expired_key.is_valid());
    }
}
