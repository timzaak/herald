// Public realm configuration helpers
//
// This module provides read-only helper functions for public/unauthenticated endpoints
// that need to query realm configuration without authentication or policy checks.

use crate::application::http::server::api_entities::ApiError;
use crate::application::http::state::AppState;
use sqlx::PgPool;

fn public_config_query_error(message: &'static str) -> ApiError {
    ApiError::internal(message)
}

/// Normalize a request/published host for custom-domain lookup.
///
/// The publish path stores lowercase hostnames without a trailing dot. Read
/// paths apply the same canonicalization so `Login.Example.COM.` resolves to
/// the stored `login.example.com` row.
pub fn normalize_custom_domain_host(host: &str) -> Option<String> {
    let host = host.trim().to_lowercase();
    let host = host.strip_suffix('.').unwrap_or(&host);
    if host.is_empty() {
        None
    } else {
        Some(host.to_string())
    }
}

/// Whether the realm row exists. The single existence probe shared by the
/// unauthenticated discovery/protocol faces — a realm-semantics change
/// (soft delete, cache) lands here once instead of per-face copies that
/// would drift. Distinguishes "no such realm" from infrastructure failure;
/// callers decide which to render as 404 vs 5xx.
pub async fn realm_exists(pool: &PgPool, realm_id: &str) -> Result<bool, sqlx::Error> {
    let exists: Option<(String,)> = sqlx::query_as("SELECT id FROM realm WHERE id = $1")
        .bind(realm_id)
        .fetch_optional(pool)
        .await?;
    Ok(exists.is_some())
}

/// Build the public web base for a realm and whether realm path prefix is
/// required in user-facing URLs.
///
/// Realms with a published custom domain use `https://{hostname}` and no
/// `/{realmId}` prefix. Realms without one continue to use
/// `{public_base_url}/{realmId}` for backward compatibility.
pub async fn realm_public_url_parts(
    state: &AppState,
    realm_id: &str,
) -> Result<(String, bool), ApiError> {
    let row = sqlx::query_as::<_, (String,)>(
        "SELECT hostname FROM custom_domain_mapping
         WHERE realm_id = $1 AND enabled = true
         ORDER BY updated_at DESC
         LIMIT 1",
    )
    .bind(realm_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!(
            realm_id = %realm_id,
            error = %e,
            "Failed to query custom-domain mapping for public URL"
        );
        public_config_query_error("Failed to query custom-domain mapping")
    })?;

    if let Some((hostname,)) = row {
        Ok((format!("https://{hostname}"), false))
    } else {
        Ok((
            state.public_base_url.trim_end_matches('/').to_string(),
            true,
        ))
    }
}

/// Build a user-facing absolute URL for a realm-relative frontend path.
pub async fn realm_public_url(
    state: &AppState,
    realm_id: &str,
    path: &str,
) -> Result<String, ApiError> {
    let (base, include_realm_prefix) = realm_public_url_parts(state, realm_id).await?;
    let path = path.trim_start_matches('/');
    if include_realm_prefix {
        Ok(format!("{base}/{realm_id}/{path}"))
    } else if path.is_empty() {
        Ok(base)
    } else {
        Ok(format!("{base}/{path}"))
    }
}

/// Query a single config value from realm_config table
///
/// This is a lightweight helper for public endpoints that don't require authentication.
/// For authenticated endpoints, use RealmConfigService instead.
///
/// # Arguments
///
/// * `pool` - Database connection pool
/// * `realm_id` - The realm ID
/// * `config_type` - The config type (e.g., "registration", "turnstile")
/// * `config_key` - The config key (e.g., "enabled", "secret_key")
///
/// # Returns
///
/// The config value if found and enabled, None otherwise
pub async fn query_config_value(
    pool: &PgPool,
    realm_id: &str,
    config_type: &str,
    config_key: &str,
) -> Result<Option<String>, ApiError> {
    let value = sqlx::query_as::<_, (String,)>(
        "SELECT config_value FROM realm_config
         WHERE realm_id = $1 AND config_type = $2 AND config_key = $3 AND enabled = true",
    )
    .bind(realm_id)
    .bind(config_type)
    .bind(config_key)
    .fetch_optional(pool)
    .await
    .map_err(|e| {
        tracing::error!(
            realm_id = %realm_id,
            config_type = %config_type,
            config_key = %config_key,
            error = %e,
            "Failed to query realm config"
        );
        public_config_query_error("Failed to query realm config")
    })?;

    Ok(value.map(|(v,)| v))
}

/// Query a single config value with metadata from realm_config table
///
/// This is a lightweight helper for public endpoints that need both value and metadata.
///
/// # Arguments
///
/// * `pool` - Database connection pool
/// * `realm_id` - The realm ID
/// * `config_type` - The config type (e.g., "turnstile")
/// * `config_key` - The config key (e.g., "secret_key")
///
/// # Returns
///
/// The config value and metadata if found and enabled, None otherwise
pub async fn query_config_with_metadata(
    pool: &PgPool,
    realm_id: &str,
    config_type: &str,
    config_key: &str,
) -> Result<Option<(String, Option<serde_json::Value>)>, ApiError> {
    let row = sqlx::query_as::<_, (String, Option<serde_json::Value>)>(
        "SELECT config_value, metadata FROM realm_config
         WHERE realm_id = $1 AND config_type = $2 AND config_key = $3 AND enabled = true",
    )
    .bind(realm_id)
    .bind(config_type)
    .bind(config_key)
    .fetch_optional(pool)
    .await
    .map_err(|e| {
        tracing::error!(
            realm_id = %realm_id,
            config_type = %config_type,
            config_key = %config_key,
            error = %e,
            "Failed to query realm config with metadata"
        );
        public_config_query_error("Failed to query realm config")
    })?;

    Ok(row)
}

/// Parse a string value into a boolean
///
/// Supports: true/false, 1/0, yes/no (case-insensitive).
/// Returns None for unrecognized values.
pub fn parse_bool(value: &str) -> Option<bool> {
    match value.to_lowercase().as_str() {
        "true" | "1" | "yes" => Some(true),
        "false" | "0" | "no" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;
    use axum::response::IntoResponse;

    #[test]
    fn test_public_config_query_error_is_generic() {
        let err = public_config_query_error("Failed to query realm config");
        let response = err.into_response();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
