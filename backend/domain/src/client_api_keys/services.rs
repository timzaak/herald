// Client API Key domain services
//
// This module provides domain services for API key generation and validation.
// Following six-sided architecture, this layer contains ZERO external dependencies.

use super::constants::{SHA256_HASH_PREFIX, get_active_api_key_salt};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// Client API Key domain service
///
/// Provides methods for generating, hashing, and verifying API keys.
/// Uses UUID v7 for key generation and SHA-256 for hashing.
///
/// # Security
/// - API keys are generated as UUID v7 (time-ordered, RFC 9562)
/// - API keys use SHA-256 with deterministic salt for O(1) lookup
/// - Hashed keys are stored in the database (never plaintext)
///
/// # Performance
/// SHA-256 enables O(1) hash-based database lookups instead of O(N) iteration.
///
/// # Example
/// ```rust,no_run
/// use herald_core::domain::client_api_keys::services::ClientApiKeyService;
///
/// // Generate a new API key (UUID v7, returns plaintext)
/// let api_key_plaintext = ClientApiKeyService::generate_api_key();
///
/// // Hash the API key for storage (SHA-256)
/// let api_key_hash = ClientApiKeyService::hash_api_key(&api_key_plaintext);
///
/// // Verify an API key against its hash
/// let is_valid = ClientApiKeyService::verify_api_key(&api_key_plaintext, &api_key_hash);
/// assert!(is_valid);
/// ```
pub struct ClientApiKeyService;

impl ClientApiKeyService {
    /// Generate a new API Key (UUID v7, plaintext)
    ///
    /// This generates a new UUID v7 which is time-ordered and provides better
    /// database index locality compared to random UUID v4.
    ///
    /// # Returns
    /// A UUID v7 string (e.g., "017f22e2-79b0-7cc3-98c4-dc0c0c07398f")
    ///
    /// # Example
    /// ```rust,no_run
    /// use herald_core::domain::client_api_keys::services::ClientApiKeyService;
    ///
    /// let api_key = ClientApiKeyService::generate_api_key();
    /// println!("Generated API Key: {}", api_key);
    /// ```
    ///
    /// # Security Note
    /// **IMPORTANT**: This returns the plaintext API key. You should:
    /// 1. Return this to the user ONCE (during creation)
    /// 2. Hash it immediately using `hash_api_key()`
    /// 3. Store only the hash in the database
    /// 4. Never log or store the plaintext key
    pub fn generate_api_key() -> String {
        Uuid::now_v7().to_string()
    }

    /// Hash an API Key using SHA-256
    ///
    /// Uses SHA-256 with a deterministic salt for O(1) database lookups.
    /// The deterministic salt allows hash-based indexing for performance.
    ///
    /// # Security Note
    /// SHA-256 is fast and deterministic, which is appropriate for API keys
    /// because they are high-entropy random strings (UUID v7). Security comes
    /// from the randomness of the key itself, not from hash algorithm slowness.
    ///
    /// # Arguments
    /// * `api_key` - The plaintext API key to hash
    ///
    /// # Returns
    /// The SHA-256 hash string with prefix (e.g., "sha256:a1b2c3...")
    ///
    /// # Example
    /// ```rust,no_run
    /// use herald_core::domain::client_api_keys::services::ClientApiKeyService;
    ///
    /// let api_key = "017f22e2-79b0-7cc3-98c4-dc0c0c07398f";
    /// let hash = ClientApiKeyService::hash_api_key(api_key);
    /// assert!(hash.starts_with("sha256:"));
    /// ```
    pub fn hash_api_key(api_key: &str) -> String {
        let salt = get_active_api_key_salt();
        let data = format!("{}:{}", api_key, salt);

        let mut hasher = Sha256::new();
        hasher.update(data.as_bytes());
        let hash = hasher.finalize();

        format!("{}{}", SHA256_HASH_PREFIX, hex::encode(hash))
    }

    /// Verify an API Key against its hash
    ///
    /// Uses SHA-256 for verification.
    ///
    /// # Arguments
    /// * `api_key` - The plaintext API key to verify
    /// * `hash` - The stored hash (SHA-256 format)
    ///
    /// # Returns
    /// * `true` if the API key matches the hash
    /// * `false` if the API key is invalid or the hash is malformed
    ///
    /// # Example
    /// ```rust,no_run
    /// use herald_core::domain::client_api_keys::services::ClientApiKeyService;
    ///
    /// let api_key = "017f22e2-79b0-7cc3-98c4-dc0c0c07398f";
    /// let hash = ClientApiKeyService::hash_api_key(api_key);
    ///
    /// assert!(ClientApiKeyService::verify_api_key(api_key, &hash));
    /// assert!(!ClientApiKeyService::verify_api_key("wrong-key", &hash));
    /// ```
    pub fn verify_api_key(api_key: &str, hash: &str) -> bool {
        if hash.starts_with(SHA256_HASH_PREFIX) {
            return hash == Self::hash_api_key(api_key);
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_api_key_returns_sha256_format() {
        let api_key = "test-api-key-12345";
        let hash = ClientApiKeyService::hash_api_key(api_key);

        // Should have sha256: prefix
        assert!(
            hash.starts_with("sha256:"),
            "Hash should have sha256: prefix"
        );

        // SHA-256 hash is 64 hex characters
        assert_eq!(
            hash.len(),
            7 + 64,
            "Hash should be 71 characters (prefix + 64 hex)"
        );
    }

    #[test]
    fn test_hash_api_key_is_deterministic() {
        let api_key = "test-api-key-12345";

        let hash1 = ClientApiKeyService::hash_api_key(api_key);
        let hash2 = ClientApiKeyService::hash_api_key(api_key);

        // Hashes should be identical (deterministic salt)
        assert_eq!(
            hash1, hash2,
            "Hashes should be identical with deterministic salt"
        );
    }

    #[test]
    fn test_verify_api_key_with_correct_key() {
        let api_key = "test-api-key-12345";
        let hash = ClientApiKeyService::hash_api_key(api_key);

        assert!(ClientApiKeyService::verify_api_key(api_key, &hash));
    }

    #[test]
    fn test_verify_api_key_with_incorrect_key() {
        let api_key = "test-api-key-12345";
        let hash = ClientApiKeyService::hash_api_key(api_key);

        assert!(!ClientApiKeyService::verify_api_key("wrong-key", &hash));
    }

    #[test]
    fn test_verify_api_key_with_invalid_hash() {
        let api_key = "test-api-key-12345";
        let invalid_hash = "invalid-hash-format";

        assert!(!ClientApiKeyService::verify_api_key(api_key, invalid_hash));
    }
}
