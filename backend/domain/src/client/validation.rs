//! Client App validation utilities
//!
//! This module provides validation functions for Client App settings,
//! including redirect URI and browser-origin validation.

use crate::common::entities::app_errors::CoreError;
use url::Url;

pub fn validate_origin(origin: &str) -> Result<String, CoreError> {
    if origin.contains('*') {
        return Err(CoreError::BadRequest(
            "Origin wildcards are not allowed".to_string(),
        ));
    }
    let url = Url::parse(origin)
        .map_err(|_| CoreError::BadRequest(format!("Invalid origin: {origin}")))?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || url.username() != ""
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(CoreError::BadRequest(
            "Origin must contain only scheme, host, and optional port".to_string(),
        ));
    }
    let is_loopback = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    if url.scheme() != "https" && !is_loopback {
        return Err(CoreError::BadRequest(
            "Origins must use HTTPS except for loopback development hosts".to_string(),
        ));
    }
    Ok(url.origin().ascii_serialization())
}

pub fn normalize_origins(origins: &[String]) -> Result<Vec<String>, CoreError> {
    let mut normalized = origins
        .iter()
        .map(|origin| validate_origin(origin))
        .collect::<Result<Vec<_>, _>>()?;
    normalized.sort();
    normalized.dedup();
    Ok(normalized)
}

/// Validates a Client App icon URL.
///
/// The icon is rendered in first-party and custom-UI pages, so an arbitrary
/// scheme like `javascript:` is a stored XSS vector, not a cosmetic value.
/// `None`/empty keeps the icon unset; any value must be an absolute http(s)
/// URL.
pub fn validate_icon_url(url: Option<&String>) -> Result<(), CoreError> {
    let Some(url) = url else {
        return Ok(());
    };
    if url.is_empty() {
        return Ok(());
    }
    let parsed =
        Url::parse(url).map_err(|_| CoreError::BadRequest(format!("Invalid icon URL: {url}")))?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err(CoreError::BadRequest(
            "Icon URL must be an absolute http(s) URL".to_string(),
        ));
    }
    Ok(())
}

/// Validates a redirect URI according to security requirements
///
/// # Rules
/// - Must be a valid URL
/// - Must NOT use `javascript:` protocol (security risk)
/// - Must NOT be a protocol-relative URL (starts with `//`)
/// - Production environments require HTTPS
///
/// # Arguments
/// * `uri` - The redirect URI to validate
/// * `is_development` - Whether this is a development environment (allows HTTP)
///
/// # Returns
/// * `Ok(())` if the URI is valid
/// * `Err(CoreError)` if validation fails
pub fn validate_redirect_uri(uri: &str, is_development: bool) -> Result<(), CoreError> {
    // Check for protocol-relative URLs (security risk)
    if uri.starts_with("//") {
        return Err(CoreError::BadRequest(
            "Protocol-relative URLs (starting with //) are not allowed for redirect URIs"
                .to_string(),
        ));
    }

    // Check for javascript: protocol (security risk)
    if uri.to_lowercase().starts_with("javascript:") {
        return Err(CoreError::BadRequest(
            "javascript: protocol is not allowed for redirect URIs".to_string(),
        ));
    }

    // Parse and validate the URL
    let url = Url::parse(uri)
        .map_err(|_| CoreError::BadRequest(format!("Invalid redirect URI: {}", uri)))?;

    // Ensure the URL has a scheme
    if url.scheme().is_empty() {
        return Err(CoreError::BadRequest(
            "Redirect URI must have a scheme (http:// or https://)".to_string(),
        ));
    }

    // Enforce HTTPS in production
    if !is_development && url.scheme() != "https" {
        return Err(CoreError::BadRequest(
            "Redirect URIs must use HTTPS in production environments".to_string(),
        ));
    }

    // Only allow http and https schemes
    match url.scheme() {
        "http" | "https" => Ok(()),
        _ => Err(CoreError::BadRequest(format!(
            "Unsupported URL scheme: {}. Only http and https are allowed",
            url.scheme()
        ))),
    }
}

/// Validates a list of redirect URIs
///
/// # Rules
/// - Must contain at least one URI
/// - Each URI must pass `validate_redirect_uri`
///
/// # Arguments
/// * `uris` - The list of redirect URIs to validate
/// * `is_development` - Whether this is a development environment
///
/// # Returns
/// * `Ok(())` if all URIs are valid
/// * `Err(CoreError)` if validation fails
pub fn validate_redirect_uris(uris: &[String], is_development: bool) -> Result<(), CoreError> {
    // Must have at least one redirect URI
    if uris.is_empty() {
        return Err(CoreError::BadRequest(
            "At least one redirect URI is required".to_string(),
        ));
    }

    // Validate each URI
    for uri in uris {
        validate_redirect_uri(uri, is_development)
            .map_err(|e| CoreError::BadRequest(format!("Invalid redirect URI '{}': {}", uri, e)))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_normalization_deduplicates_equivalent_origins() {
        let origins = vec![
            "https://EXAMPLE.com".to_string(),
            "https://example.com:443".to_string(),
        ];
        assert_eq!(
            normalize_origins(&origins).unwrap(),
            vec!["https://example.com"]
        );
    }

    #[test]
    fn origin_rejects_paths_wildcards_and_credentials() {
        for origin in [
            "https://example.com/path",
            "https://*.example.com",
            "https://user@example.com",
            "http://example.com",
        ] {
            assert!(
                validate_origin(origin).is_err(),
                "unsafe origin must be rejected: {origin}"
            );
        }
    }

    #[test]
    fn test_validate_redirect_uri_valid_https() {
        assert!(validate_redirect_uri("https://example.com/callback", false).is_ok());
    }

    #[test]
    fn test_validate_redirect_uri_valid_http_development() {
        assert!(validate_redirect_uri("http://localhost:3000/callback", true).is_ok());
    }

    #[test]
    fn test_validate_redirect_uri_http_rejected_in_production() {
        assert!(validate_redirect_uri("http://example.com/callback", false).is_err());
    }

    #[test]
    fn test_validate_redirect_uri_javascript_protocol_rejected() {
        assert!(validate_redirect_uri("javascript:alert('xss')", true).is_err());
    }

    #[test]
    fn test_validate_redirect_uri_protocol_relative_rejected() {
        assert!(validate_redirect_uri("//evil.com/callback", true).is_err());
    }

    #[test]
    fn test_validate_redirect_uri_invalid_url() {
        assert!(validate_redirect_uri("not-a-url", true).is_err());
    }

    #[test]
    fn test_validate_redirect_uris_requires_at_least_one() {
        assert!(validate_redirect_uris(&[], true).is_err());
    }

    #[test]
    fn test_validate_redirect_uris_valid_list() {
        let uris = vec![
            "https://example.com/callback".to_string(),
            "https://app.example.com/auth".to_string(),
        ];
        assert!(validate_redirect_uris(&uris, false).is_ok());
    }
}
