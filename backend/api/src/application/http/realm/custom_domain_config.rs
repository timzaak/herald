// Realm custom-domain configuration handlers, DTOs, and helpers.
//
// Custom-domain config is a single `settings` row under
// `ConfigType::CustomDomain`. A PUT to the config endpoint normalizes and
// globally-uniqueness-checks the hostname, then atomically writes the
// `custom_domain_mapping` host→realm table (request-time host resolution +
// Caddy On-Demand TLS authorization) and the `settings` row. There is no
// draft/publish/restore lifecycle: the low frequency of this config and the
// CNAME/TLS verification gate make a multi-step flow unnecessary.

use axum::{
    Json,
    extract::{Extension, Path, Query, State},
    http::HeaderMap,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::application::http::server::api_entities::ApiError;
use crate::application::http::state::AppState;
use herald_api_base::application::http::auth::util::{ClientIp, user_agent_from_headers};
use herald_api_base::application::http::common::auth_utils::AdminIdentity;
use herald_api_base::application::http::common::public_helper::normalize_custom_domain_host;
use herald_core::domain::audit::{
    AuditAction, AuditCategory, AuditEventRepository, AuditResult, AuditTargetType, NewAuditEvent,
};
use herald_core::domain::authentication::Identity;
use herald_core::domain::common::entities::app_errors::CoreError;
use herald_core::domain::custom_domain::CustomDomainMappingRepository;
use herald_core::domain::realm_config::{
    ConfigType, CustomDomainConfig, CustomDomainStatus, RealmConfig, RealmConfigService,
    UpsertRealmConfigRequest, normalize_and_validate_hostname,
};

pub use crate::application::http::server::api_entities::ErrorResponse;

const SETTINGS_KEY: &str = "settings";
const CUSTOM_DOMAIN_CLAIM_LOCK_ID: i64 = 0x4845_5241_4C44;

// Failure budget for the `X-Herald-Ask-Key` shared-secret gate below. Own
// instance so ask-key guessing cannot lock out (or be masked by) the internal
// API-key gate's budget.
static ASK_KEY_FAILURE_THROTTLE:
    herald_api_base::application::http::internal_auth::FailureThrottle =
    herald_api_base::application::http::internal_auth::FailureThrottle::new();

// ---------------------------------------------------------------------------
// Response / request DTOs
// ---------------------------------------------------------------------------

/// Custom-domain management state shown on the realm admin config page.
#[derive(Debug, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CustomDomainConfigStateResponse {
    /// Current configuration (effective for host→realm resolution).
    /// `hostname = null` means the realm has no custom domain configured.
    pub published: CustomDomainConfig,
    /// Herald-owned hostname tenants must CNAME their custom login domain to
    /// (global config, e.g. `custom.herald.com`).
    pub cname_target: String,
    /// Live CNAME/TLS status of the configured hostname. `null` when no
    /// hostname is configured or no mapping row exists yet.
    pub status: Option<CustomDomainStatus>,
}

/// Request body for updating custom-domain configuration.
#[derive(Debug, Clone, Default, Deserialize, Serialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCustomDomainConfigRequest {
    /// Precise custom login hostname (e.g. `login.acme.com`). `null`/empty
    /// clears the configured hostname.
    pub hostname: Option<String>,
}

/// Response shape returned by the custom-domain update operation.
#[derive(Debug, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CustomDomainUpdateResponse {
    pub message: String,
    /// Live CNAME/TLS status of the configured hostname after the operation.
    pub status: Option<CustomDomainStatus>,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// Get custom-domain management state.
#[utoipa::path(
    get,
    path = "/api/realms/{realmId}/config/custom-domain",
    tag = "realms",
    params(
        ("realmId" = String, Path, description = "Realm ID")
    ),
    responses(
        (status = 200, description = "Custom-domain configuration state", body = CustomDomainConfigStateResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden - Insufficient permissions", body = ErrorResponse),
        (status = 404, description = "Realm not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn handle_get_custom_domain_config(
    State(state): State<AppState>,
    Path(realm_id): Path<String>,
    Extension(identity): Extension<Identity>,
) -> Result<Json<CustomDomainConfigStateResponse>, ApiError> {
    let admin = AdminIdentity::require(identity, &realm_id, "realm custom-domain configuration")?;
    admin.require_permission(&state, "settings", "view").await?;

    Ok(Json(
        load_state(&state, admin.identity().clone(), realm_id).await?,
    ))
}

/// Update custom-domain configuration (writes the host→realm mapping).
#[utoipa::path(
    put,
    path = "/api/realms/{realmId}/config/custom-domain",
    tag = "realms",
    params(
        ("realmId" = String, Path, description = "Realm ID")
    ),
    request_body = UpdateCustomDomainConfigRequest,
    responses(
        (status = 200, description = "Custom-domain configuration updated", body = CustomDomainUpdateResponse),
        (status = 400, description = "Invalid custom domain", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden - Insufficient permissions", body = ErrorResponse),
        (status = 409, description = "Custom domain already in use", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn handle_update_custom_domain_config(
    State(state): State<AppState>,
    Path(realm_id): Path<String>,
    Extension(identity): Extension<Identity>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Json(req): Json<UpdateCustomDomainConfigRequest>,
) -> Result<Json<CustomDomainUpdateResponse>, ApiError> {
    let admin = AdminIdentity::require(identity, &realm_id, "realm custom-domain configuration")?;
    admin
        .require_permission(&state, "settings", "manage")
        .await?;

    let identity = admin.identity().clone();

    // Empty / whitespace hostname clears the configuration.
    let normalized_hostname = match req.hostname.as_deref() {
        None => None,
        Some(raw) if raw.trim().is_empty() => None,
        Some(raw) => {
            let hostname = normalize_and_validate_hostname(raw)
                .map_err(|_| ApiError::bad_request("Invalid custom domain"))?;
            Some(hostname)
        }
    };

    let claim_lock = acquire_custom_domain_claim_lock(&state).await?;

    // Global uniqueness: a hostname must not be claimed by another realm. We
    // check the published mapping table and other realms' realm_config
    // custom_domain settings rows. The current realm's own row is excluded.
    if let Some(ref hostname) = normalized_hostname {
        assert_hostname_not_reserved(&state, hostname)?;
        assert_hostname_globally_unique(&state, &realm_id, hostname).await?;
    }

    // Write the host→realm mapping BEFORE committing the config row. The
    // mapping op is atomic (own conflict guard), so a failure here leaves only
    // read-only state touched and the admin can retry. `upsert_for_realm`
    // deletes the realm's prior enabled hostname (if different), resets
    // CNAME/TLS status to pending unless the hostname is unchanged (idempotent),
    // and surfaces a hostname owned by another realm as Conflict.
    //
    // Clearing the hostname removes every mapping row for this realm so that
    // request-time resolution no longer treats it as having a custom domain.
    let status = match normalized_hostname.as_deref() {
        Some(new_hostname) => {
            let mapping = state
                .custom_domain_mapping_repo
                .upsert_for_realm(&realm_id, new_hostname)
                .await
                .map_err(map_mapping_error)?;
            Some(CustomDomainStatus {
                cname_verified: mapping.cname_verified,
                tls_ready: mapping.tls_ready,
                checked_at: mapping.status_checked_at,
            })
        }
        None => {
            state
                .custom_domain_mapping_repo
                .delete_by_realm_or_hostname(Some(realm_id.clone()), None)
                .await
                .map_err(map_mapping_error)?;
            None
        }
    };

    // Persist the settings row to match the mapping. The mapping is the
    // request-time source of truth; `settings` is the admin-view source.
    let config = CustomDomainConfig {
        hostname: normalized_hostname,
    };
    let request = build_custom_domain_upsert_request(SETTINGS_KEY, &config)?;
    state
        .service
        .realm_config_service()
        .upsert_config(identity.clone(), realm_id.clone(), request)
        .await
        .map_err(map_realm_config_error)?;

    claim_lock.commit().await.map_err(|error| {
        tracing::error!(%error, "Failed to release custom-domain claim lock");
        ApiError::internal("Failed to update custom-domain configuration")
    })?;

    if let Err(error) = state
        .audit_event_repository
        .create(NewAuditEvent {
            realm_id: realm_id.clone(),
            category: AuditCategory::RealmManagement,
            action: AuditAction::RealmConfigUpdate,
            actor_id: identity.user_id(),
            actor_type: None,
            actor_name: identity.as_user().map(|user| user.email.clone()),
            target_type: AuditTargetType::Realm,
            target_id: realm_id.clone(),
            target_name: Some("custom_domain/settings".to_string()),
            result: AuditResult::Success,
            details: Some(serde_json::json!({
                "config_type": "custom_domain",
                "operation": if config.hostname.is_some() { "saved" } else { "cleared" },
                "hostname": config.hostname,
            })),
            ip_address: Some(ip),
            user_agent: user_agent_from_headers(&headers),
            trace_id: None,
        })
        .await
    {
        tracing::warn!(%error, %realm_id, "Failed to record custom-domain audit event");
    }

    Ok(Json(CustomDomainUpdateResponse {
        message: "Custom-domain configuration updated".to_string(),
        status,
    }))
}

// ---------------------------------------------------------------------------
// Internal endpoint: Caddy On-Demand TLS ask authorization
//
// Unauthenticated top-level route registered in `server/mod.rs` (NOT under
// `/api/realms`, so no Bearer identity). It shares the
// `custom_domain_mapping` table with the management handlers with a minimal
// response shape:
//   - ask → `{"authorized": true}` only (NO realm info — certificate-abuse
//           gate; leaking realmId would let an attacker map a host to a realm
//           without owning it).
// It filters on the unified effectiveness predicate `enabled = true`;
// `cname_verified`/`tls_ready` are display-only.
//
// The public host→realmId resolve endpoint is deliberately separate: it is
// needed by the SPA before login, while this endpoint must never disclose it.
// ---------------------------------------------------------------------------

/// Query parameters for the internal custom-domain ask endpoint.
#[derive(Debug, Deserialize, ToSchema)]
pub struct CustomDomainHostQuery {
    /// The hostname to look up (e.g. `login.acme.com`). Compared as-is against
    /// the normalized, lowercased `custom_domain_mapping.hostname` column.
    pub host: String,
}

/// Response body for the Caddy On-Demand TLS ask authorization gate.
///
/// Deliberately contains ONLY the `authorized` boolean — no realm id or any
/// other realm information (certificate-abuse gate).
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CustomDomainAuthorizeResponse {
    pub authorized: bool,
}

/// Caddy On-Demand TLS ask authorization endpoint.
///
/// Returns `200 {"authorized": true}` when `host` matches a published+enabled
/// `custom_domain_mapping` row; `404` on a miss (Caddy declines TLS issuance); `401` when the
/// `X-Herald-Ask-Key` header is missing or mismatches the configured shared
/// secret. Never exposes realm information — this is a certificate-abuse gate.
#[utoipa::path(
    get,
    path = "/api/internal/custom-domain/authorize",
    tag = "realms",
    params(
        ("host" = String, Query, description = "Hostname to authorize for TLS issuance")
    ),
    responses(
        (status = 200, description = "Host is authorized for TLS issuance", body = CustomDomainAuthorizeResponse),
        (status = 401, description = "Missing or mismatched X-Herald-Ask-Key", body = ErrorResponse),
        (status = 404, description = "Host is not a published custom domain; Caddy declines issuance", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
)]
pub async fn handle_custom_domain_authorize(
    State(state): State<AppState>,
    Query(query): Query<CustomDomainHostQuery>,
    headers: HeaderMap,
) -> Result<Json<CustomDomainAuthorizeResponse>, ApiError> {
    // Shared ask-key gate (`require_ask_key`): missing header stays a plain
    // 401, non-empty failed comparisons are throttled with this endpoint's own
    // sliding-window budget and compared in constant time. The configured key
    // is validated non-empty at server startup (build_app_state_with_migrations).
    herald_api_base::application::http::internal_auth::require_ask_key(
        &headers,
        &state.custom_domain_ask_key,
        &ASK_KEY_FAILURE_THROTTLE,
    )?;

    // Effectiveness predicate: `enabled = true` only. The repo
    // filters this; `cname_verified`/`tls_ready` are display-only and play no
    // role in authorization (otherwise ask ↔ TLS issuance would be circular).
    // Normalize the read path symmetrically with the write path (publish stores
    // a lowercase, trailing-dot-stripped hostname). Caddy's `host`/SNI may
    // arrive with differing case or a trailing dot; without normalizing here,
    // a legitimately published domain would miss and return 404, declining TLS
    // issuance. Full validation (`normalize_and_validate_hostname`) is avoided
    // on this hot path — a syntactically invalid host simply won't match a row.
    let host = normalize_custom_domain_host(&query.host)
        .ok_or_else(|| ApiError::not_found("Custom domain not found"))?;
    let mapping = state
        .custom_domain_mapping_repo
        .find_by_hostname(&host)
        .await
        .map_err(map_mapping_error)?;

    match mapping {
        // Hit → authorize. NEVER include realm_id or any realm info in the
        // body (certificate-abuse gate).
        Some(mapping) => {
            if let Err(error) = state
                .custom_domain_mapping_repo
                .update_status(&host, true, mapping.tls_ready)
                .await
            {
                tracing::error!(%host, %error, "Failed to record custom-domain CNAME status");
            }
            Ok(Json(CustomDomainAuthorizeResponse { authorized: true }))
        }
        // Miss → 404 so Caddy declines issuance for unregistered hosts.
        None => Err(ApiError::not_found("Custom domain not found")),
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

struct LoadedCustomDomainConfig {
    config: CustomDomainConfig,
    #[allow(dead_code)]
    updated_at: String,
}

async fn load_state(
    state: &AppState,
    identity: Identity,
    realm_id: String,
) -> Result<CustomDomainConfigStateResponse, ApiError> {
    let published = load_config(state, identity, realm_id.clone(), SETTINGS_KEY).await?;

    let published_config = published
        .as_ref()
        .map(|entry| entry.config.clone())
        .unwrap_or_default();

    // The configured hostname drives the mapping lookup for live status. A
    // missing/empty hostname means no mapping row, hence null status.
    let status = match published_config.hostname.as_deref() {
        Some(hostname) => load_status(state, hostname).await?,
        None => None,
    };

    Ok(CustomDomainConfigStateResponse {
        published: published_config,
        cname_target: state.custom_domain_cname_target.clone(),
        status,
    })
}

/// Load the live CNAME/TLS status for a published hostname from the mapping
/// table. Returns `None` when no enabled row exists.
async fn load_status(
    state: &AppState,
    hostname: &str,
) -> Result<Option<CustomDomainStatus>, ApiError> {
    let row = state
        .custom_domain_mapping_repo
        .find_by_hostname(hostname)
        .await
        .map_err(map_mapping_error)?;
    Ok(row.map(|mapping| CustomDomainStatus {
        cname_verified: mapping.cname_verified,
        tls_ready: mapping.tls_ready,
        checked_at: mapping.status_checked_at,
    }))
}

async fn load_config(
    state: &AppState,
    identity: Identity,
    realm_id: String,
    config_key: &str,
) -> Result<Option<LoadedCustomDomainConfig>, ApiError> {
    let entry = state
        .service
        .realm_config_service()
        .get_config(
            identity,
            realm_id.clone(),
            ConfigType::CustomDomain.as_ref().to_string(),
            config_key.to_string(),
        )
        .await
        .map_err(map_realm_config_error)?;

    Ok(entry.map(|entry| parse_config_entry(&realm_id, config_key, entry)))
}

fn parse_config_entry(
    realm_id: &str,
    config_key: &str,
    entry: RealmConfig,
) -> LoadedCustomDomainConfig {
    let config =
        serde_json::from_str::<CustomDomainConfig>(&entry.config_value).unwrap_or_else(|e| {
            tracing::error!(
                realm_id = %realm_id,
                config_type = %ConfigType::CustomDomain.as_ref(),
                config_key = %config_key,
                error = %e,
                "Failed to parse custom-domain config JSON"
            );
            CustomDomainConfig::default()
        });

    LoadedCustomDomainConfig {
        config,
        updated_at: entry.updated_at.to_rfc3339(),
    }
}

fn build_custom_domain_upsert_request(
    config_key: &str,
    config: &CustomDomainConfig,
) -> Result<UpsertRealmConfigRequest, ApiError> {
    let config_value = serde_json::to_string(config).map_err(|e| {
        tracing::error!("Failed to serialize custom-domain config: {}", e);
        ApiError::internal("Failed to serialize custom-domain config")
    })?;

    Ok(UpsertRealmConfigRequest {
        config_type: ConfigType::CustomDomain,
        config_key: config_key.to_string(),
        config_value,
        is_secret: Some(false),
        enabled: Some(true),
        metadata: None,
    })
}

/// Extract the lowercase host of a configured deployment URL for the
/// reserved-host comparison.
///
/// `Url::parse` is authoritative for host extraction — the hand-rolled
/// splitter it replaced produced garbage hosts for upper-case schemes
/// (`HTTPS://host` → `https`), userinfo (`user@host`), and IPv6 literals,
/// and each of those gaps meant the deployment-owned name was NOT reserved
/// and stayed claimable by a tenant.
fn host_of_configured_url(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let parsed = url::Url::parse(raw)
        .ok()
        .filter(|url| url.host_str().is_some());
    let url = match parsed {
        Some(url) => url,
        // `Url::parse` requires an absolute URL: a bare host configured
        // without a scheme (or a `host:port` value that parses as a
        // scheme-only URL with no host) is still deployment-owned, so retry
        // under an assumed https scheme. Inputs that already carry a scheme
        // are not retried — the retry could only manufacture a bogus host.
        None if !raw.contains("://") => url::Url::parse(&format!("https://{raw}")).ok()?,
        None => return None,
    };
    let host = url.host_str()?.trim_matches(['[', ']']);
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

/// Assert a hostname is not claimed by another realm.
///
/// Checks two sources:
/// Rejects deployment-owned hostnames at the claim boundary.
///
/// The SPA host (`[frontend].url`, carried on `AppState.public_base_url`) and
/// the Herald CNAME target (`[custom_domain].cname_target`) are platform
/// infrastructure: a tenant claiming either would re-home the platform host's
/// realm resolution, white-label surface and OIDC issuer onto its own realm on
/// the operator's legitimate domain. `localhost` is rejected outright. No
/// DNS-control proof exists anywhere in the claim flow, so this static guard
/// is the only protection for deployment-owned names.
fn assert_hostname_not_reserved(state: &AppState, hostname: &str) -> Result<(), ApiError> {
    let claimed = hostname.to_ascii_lowercase();
    if claimed == "localhost" {
        return Err(ApiError::bad_request(
            "localhost cannot be claimed as a custom domain",
        ));
    }
    for reserved in [
        host_of_configured_url(&state.public_base_url),
        host_of_configured_url(&state.custom_domain_cname_target),
    ]
    .into_iter()
    .flatten()
    {
        if claimed == reserved {
            return Err(ApiError::bad_request(
                "This hostname is reserved by the deployment and cannot be claimed as a custom domain",
            ));
        }
    }
    Ok(())
}

/// 1. The `custom_domain_mapping` table (configured hostnames) — via the repo
///    port, filtered to enabled rows.
/// 2. Other realms' `realm_config` `custom_domain` rows for the `settings`
///    key — a direct SQL query against `state.pool` because the
///    `RealmConfigRepository` port is per-realm and cannot express a
///    cross-realm scan. The current realm's own row is excluded so updating
///    a realm's own config is not a self-conflict.
///
/// Returns `ApiError::conflict("Custom domain already in use")` on conflict.
async fn assert_hostname_globally_unique(
    state: &AppState,
    realm_id: &str,
    hostname: &str,
) -> Result<(), ApiError> {
    // 1) Configured mapping table.
    if let Some(mapping) = state
        .custom_domain_mapping_repo
        .find_by_hostname(hostname)
        .await
        .map_err(map_mapping_error)?
        && mapping.realm_id != realm_id
    {
        return Err(ApiError::conflict("Custom domain already in use"));
    }

    // 2) Other realms' realm_config custom_domain settings rows.
    //
    // Parse the stored JSON and compare the hostname field itself. A textual
    // LIKE depends on serializer whitespace/key ordering and can miss an
    // otherwise identical claim.
    let conflict: Option<(String,)> = sqlx::query_as(
        "SELECT realm_id FROM realm_config
         WHERE config_type = 'custom_domain'
           AND config_key = 'settings'
           AND realm_id <> $1
           AND config_value::jsonb ->> 'hostname' = $2
         LIMIT 1",
    )
    .bind(realm_id)
    .bind(hostname)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!(
            realm_id = %realm_id,
            hostname = %hostname,
            "Failed to check custom-domain uniqueness: {e}"
        );
        ApiError::internal("Failed to check custom-domain uniqueness")
    })?;

    if conflict.is_some() {
        return Err(ApiError::conflict("Custom domain already in use"));
    }

    Ok(())
}

async fn acquire_custom_domain_claim_lock(
    state: &AppState,
) -> Result<sqlx::Transaction<'_, sqlx::Postgres>, ApiError> {
    let mut transaction = state.pool.begin().await.map_err(|error| {
        tracing::error!(%error, "Failed to begin custom-domain claim transaction");
        ApiError::internal("Failed to lock custom-domain claim")
    })?;
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(CUSTOM_DOMAIN_CLAIM_LOCK_ID)
        .execute(&mut *transaction)
        .await
        .map_err(|error| {
            tracing::error!(%error, "Failed to acquire custom-domain claim lock");
            ApiError::internal("Failed to lock custom-domain claim")
        })?;
    Ok(transaction)
}

fn map_realm_config_error(error: CoreError) -> ApiError {
    match error {
        CoreError::Forbidden(msg) => ApiError::forbidden(msg),
        CoreError::NotFound => ApiError::not_found("Realm not found"),
        CoreError::BadRequest(msg) => ApiError::bad_request(msg),
        CoreError::Conflict(msg) => ApiError::conflict(msg),
        _ => {
            tracing::error!("Custom-domain realm config operation failed: {}", error);
            ApiError::internal("Internal server error")
        }
    }
}

fn map_mapping_error(error: CoreError) -> ApiError {
    match error {
        CoreError::Conflict(msg) => ApiError::conflict(msg),
        CoreError::NotFound => ApiError::not_found("Custom-domain mapping not found"),
        _ => {
            tracing::error!("Custom-domain mapping operation failed: {}", error);
            ApiError::internal("Internal server error")
        }
    }
}

#[cfg(test)]
mod reserved_host_tests {
    use super::host_of_configured_url;

    // WHY: every shape here is a form of `[frontend].url` /
    // `[custom_domain].cname_target` an operator could write. A shape the
    // extractor fails to resolve is a deployment-owned hostname that
    // `assert_hostname_not_reserved` does NOT reserve — and with no DNS-proof
    // in the claim flow, that hostname stays claimable by a tenant, re-homing
    // the platform's realm resolution onto their realm. These cases pin the
    // exact shapes the previous hand-rolled splitter silently mis-parsed
    // (upper-case scheme, userinfo, IPv6) plus the ones it already got right,
    // so the switch to `Url::parse` can never narrow the reserved set.
    #[test]
    fn extracts_the_deployment_host_from_every_configured_url_shape() {
        // Canonical shapes the old splitter handled — unchanged.
        assert_eq!(
            host_of_configured_url("https://App.Example.com/base"),
            Some("app.example.com".to_string())
        );
        assert_eq!(
            host_of_configured_url("http://app.example.com?x=1"),
            Some("app.example.com".to_string())
        );
        assert_eq!(
            host_of_configured_url("https://app.example.com:8443"),
            Some("app.example.com".to_string())
        );
        // Scheme-less bare host / host:port stay reserved — `Url::parse`
        // needs an absolute URL, so these go through the https retry.
        assert_eq!(
            host_of_configured_url("app.example.com"),
            Some("app.example.com".to_string())
        );
        assert_eq!(
            host_of_configured_url("app.example.com:8443"),
            Some("app.example.com".to_string())
        );

        // Shapes the old splitter mis-parsed into garbage hosts ("https",
        // "ops@app.example.com", "[") — each used to leave the real host
        // unreserved.
        assert_eq!(
            host_of_configured_url("HTTPS://App.Example.com"),
            Some("app.example.com".to_string())
        );
        assert_eq!(
            host_of_configured_url("https://ops@app.example.com"),
            Some("app.example.com".to_string())
        );
        assert_eq!(
            host_of_configured_url("https://[2001:db8::1]:8443"),
            Some("2001:db8::1".to_string())
        );

        // Nothing extractable — no bogus reservation either.
        assert_eq!(host_of_configured_url(""), None);
        assert_eq!(host_of_configured_url("   "), None);
        assert_eq!(host_of_configured_url("https://"), None);
    }
}
