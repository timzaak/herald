//! Shopify webhook security utilities
//!
//! Provides HMAC-SHA256 signature verification for Shopify webhooks.
//! This is a pure SDK-level helper: given the raw webhook body, the
//! `X-Shopify-Hmac-SHA256` header value, and the Shopify App Client
//! Secret, it verifies the request was actually sent by Shopify.

use base64::{Engine, engine::general_purpose::STANDARD as BASE64_STANDARD};
use herald_domain::common::entities::app_errors::CoreError;
use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Verify Shopify webhook HMAC signature.
///
/// Returns `Ok(())` when the computed HMAC-SHA256 of `body` (base64-encoded)
/// matches `header_hmac` in constant time; otherwise returns
/// `CoreError::Unauthorized`.
pub fn verify_webhook_hmac(
    body: &[u8],
    header_hmac: &str,
    client_secret: &str,
) -> Result<(), CoreError> {
    // Calculate HMAC-SHA256
    let mut mac = HmacSha256::new_from_slice(client_secret.as_bytes())
        .map_err(|e| CoreError::InternalServerError(format!("HMAC key error: {}", e)))?;
    mac.update(body);
    let calculated_hmac = mac.finalize().into_bytes();

    // Base64 encode
    let encoded_hmac = BASE64_STANDARD.encode(calculated_hmac);

    // Constant-time comparison
    if constant_time_compare(&encoded_hmac, header_hmac) {
        Ok(())
    } else {
        Err(CoreError::Unauthorized)
    }
}

/// Constant-time string comparison.
///
/// Compares two ASCII strings byte-by-byte without short-circuiting, so
/// timing does not leak the position of the first mismatched byte. Returns
/// early (non-constant time) only when the lengths differ, which does not
/// reveal secret material.
pub fn constant_time_compare(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }

    let mut result = 0u8;
    for (byte_a, byte_b) in a.bytes().zip(b.bytes()) {
        result |= byte_a ^ byte_b;
    }

    result == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_hmac_verification() {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;

        let client_secret = "test_secret_key";
        let body = b"test_body";

        type HmacSha256 = Hmac<Sha256>;
        let mut mac = HmacSha256::new_from_slice(client_secret.as_bytes()).unwrap();
        mac.update(body);
        let calculated_hmac = mac.finalize().into_bytes();
        let valid_hmac = BASE64_STANDARD.encode(calculated_hmac);

        assert!(verify_webhook_hmac(body, &valid_hmac, client_secret).is_ok());
    }

    #[test]
    fn test_invalid_hmac_rejected() {
        let client_secret = "test_secret_key";
        let body = b"test_body";
        let invalid_hmac = "invalid_hmac_signature";

        assert!(matches!(
            verify_webhook_hmac(body, invalid_hmac, client_secret),
            Err(CoreError::Unauthorized)
        ));
    }

    #[test]
    fn test_tampered_body_rejected() {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;

        let client_secret = "test_secret_key";
        let original_body = b"test_body";

        type HmacSha256 = Hmac<Sha256>;
        let mut mac = HmacSha256::new_from_slice(client_secret.as_bytes()).unwrap();
        mac.update(original_body);
        let calculated_hmac = mac.finalize().into_bytes();
        let valid_hmac = BASE64_STANDARD.encode(calculated_hmac);

        let tampered_body = b"test_body_tampered";

        assert!(matches!(
            verify_webhook_hmac(tampered_body, &valid_hmac, client_secret),
            Err(CoreError::Unauthorized)
        ));
    }
}
