// Public configuration endpoint for realm settings

use axum::{
    extract::{ConnectInfo, Path, State},
    http::HeaderMap,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::application::http::realm::white_label_config::{WhiteLabelBackground, WhiteLabelConfig};
use crate::application::http::realm_config::public_helper::{
    normalize_custom_domain_host, parse_bool, query_config_value,
};
pub use crate::application::http::server::api_entities::ErrorResponse;
use crate::application::http::server::api_entities::{ApiError, ApiResult};
use crate::application::http::state::AppState;
use herald_core::domain::custom_domain::CustomDomainMappingRepository;
use herald_core::domain::realm::{RealmService, RealmSummary};

#[derive(Debug, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RegistrationConfig {
    pub enabled: bool,
    pub require_email_verification: bool,
}

#[derive(Debug, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OAuthProviderInfo {
    pub name: String,
    pub display_name: String,
    pub enabled: bool,
    /// Provider `client_id` (e.g. Google One Tap / GIS initialization). Not a
    /// secret — the redirect-style `/login` flow already surfaces it in the
    /// auth URL. `client_secret` is never exposed here. Omitted from the
    /// serialized payload when absent to keep the response stable for existing
    /// consumers that only read `name`/`displayName`/`enabled`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
}

#[derive(Debug, Default, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PublicWhiteLabelConfig {
    pub brand_name: Option<String>,
    pub logo_url: Option<String>,
    pub favicon_url: Option<String>,
    pub accent_color: Option<String>,
    pub background: Option<WhiteLabelBackground>,
    pub footer_text: Option<String>,
    pub login_title: Option<String>,
    pub login_subtitle: Option<String>,
    pub register_title: Option<String>,
    pub register_subtitle: Option<String>,
}

impl From<WhiteLabelConfig> for PublicWhiteLabelConfig {
    fn from(config: WhiteLabelConfig) -> Self {
        Self {
            brand_name: config.brand_name,
            logo_url: config.logo_url,
            favicon_url: config.favicon_url,
            accent_color: config.accent_color,
            background: config.background,
            footer_text: config.footer_text,
            login_title: config.login_title,
            login_subtitle: config.login_subtitle,
            register_title: config.register_title,
            register_subtitle: config.register_subtitle,
        }
    }
}

#[derive(Debug, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PublicConfigResponse {
    pub realm_name: String,
    pub realm_description: Option<String>,
    pub registration: RegistrationConfig,
    pub oauth_providers: Vec<OAuthProviderInfo>,
    pub white_label: PublicWhiteLabelConfig,
}

#[derive(Debug, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResolveCustomDomainResponse {
    pub realm_id: String,
    pub public_config: PublicConfigResponse,
}

/// Queries a single boolean config value from the realm_config table.
///
/// # Arguments
///
/// * `pool` - Database connection pool
/// * `realm_id` - The realm ID
/// * `config_key` - The config key to query (e.g., "enabled", "require_email_verification")
/// * `default_value` - Default value if config is not found or cannot be parsed
///
/// # Returns
///
/// The parsed boolean value or the default value.
async fn query_bool_config(
    pool: &sqlx::PgPool,
    realm_id: &str,
    config_key: &str,
    default_value: bool,
) -> Result<bool, ApiError> {
    let value = query_config_value(pool, realm_id, "registration", config_key).await?;

    Ok(value.and_then(|v| parse_bool(&v)).unwrap_or(default_value))
}

async fn query_public_white_label_config(
    pool: &sqlx::PgPool,
    realm_id: &str,
) -> Result<PublicWhiteLabelConfig, ApiError> {
    let Some(value) = query_config_value(pool, realm_id, "white_label", "settings").await? else {
        return Ok(PublicWhiteLabelConfig::default());
    };

    match serde_json::from_str::<WhiteLabelConfig>(&value) {
        Ok(config) => Ok(config.into()),
        Err(e) => {
            tracing::warn!(
                realm_id = %realm_id,
                error = %e,
                "Failed to parse public white-label config JSON"
            );
            Ok(PublicWhiteLabelConfig::default())
        }
    }
}

/// Get public configuration for a realm
///
/// Returns publicly accessible configuration for the specified realm, including:
/// - Registration settings (whether registration is allowed and if email verification is required)
/// - List of enabled OAuth providers
/// - Published white-label settings
///
/// This endpoint does not require authentication.
#[utoipa::path(
    get,
    path = "/api/public-config/{realmId}",
    tag = "system",
    params(
        ("realmId" = String, Path, description = "Realm ID")
    ),
    responses(
        (status = 200, description = "Public configuration retrieved successfully", body = PublicConfigResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn get_public_config(
    Path(realm_id): Path<String>,
    State(state): State<AppState>,
    _headers: HeaderMap,
) -> Result<ApiResult<PublicConfigResponse>, ApiError> {
    Ok(ApiResult::ok(load_public_config(&state, &realm_id).await?))
}

/// Resolve the current request host to a published custom-domain realm.
///
/// Used by the SPA on custom domains before it knows the realm id. This endpoint
/// is intentionally separate from the internal Caddy ask endpoint: the ask
/// endpoint stays shared-secret protected and never leaks realm identity, while
/// this public endpoint is only for hosts already serving the app.
#[utoipa::path(
    get,
    path = "/api/public-config/custom-domain/resolve",
    tag = "system",
    responses(
        (status = 200, description = "Custom domain resolved", body = ResolveCustomDomainResponse),
        (status = 404, description = "Custom domain not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn resolve_custom_domain(
    State(state): State<AppState>,
    // The trusted-proxy config and socket peer are read from the request
    // extensions (the same way the `ClientIp` extractor does) so the handler
    // also mounts on scenario-test routers that lack the connect-info layer;
    // absent extensions mean "no trusted proxy signal", which fails closed
    // below (no status write).
    request: axum::extract::Request,
) -> Result<axum::response::Response, ApiError> {
    let real_ip = request
        .extensions()
        .get::<herald_api_base::application::http::real_ip::RealIpConfig>();
    let peer = request
        .extensions()
        .get::<ConnectInfo<std::net::SocketAddr>>()
        .map(|connect_info| connect_info.0);
    let headers = request.headers();
    // Bind the lookup to the host the request actually arrived on. There is
    // deliberately no `?host=` override: arbitrary host-to-realm probing of
    // the custom-domain registry is available only through the shared-secret
    // ask endpoint.
    let raw_host = headers
        .get(axum::http::header::HOST)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| ApiError::not_found("Custom domain not found"))?;

    let host_without_port = raw_host
        .split_once(':')
        .map(|(host, _)| host)
        .unwrap_or(raw_host);
    let host = normalize_custom_domain_host(host_without_port)
        .ok_or_else(|| ApiError::not_found("Custom domain not found"))?;

    let mapping = state
        .custom_domain_mapping_repo
        .find_by_hostname(&host)
        .await
        .map_err(|e| {
            tracing::error!(host = %host, error = %e, "Failed to resolve custom-domain host");
            ApiError::internal("Failed to resolve custom domain")
        })?
        .ok_or_else(|| ApiError::not_found("Custom domain not found"))?;

    // TLS-status observation. The old gate was tautological (request_host ==
    // host always holds — host is derived from the same Host header) and the
    // X-Forwarded-Proto value is client-forgeable on direct reachability, so
    // ANY anonymous request could flip cname_verified/tls_ready (audit run-2:
    // resolve_custom_domain unauthenticated-host-header-realm-oracle). The
    // forwarded proto is honored only when the socket peer is a configured
    // trusted proxy — an operator-declared ingress that terminates TLS and is
    // the only party allowed to speak forwarded headers.
    let https_via_trusted_proxy = matches!((real_ip, peer), (Some(real_ip), Some(peer)) if real_ip.trusts(peer.ip()))
        && headers
            .get("x-forwarded-proto")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.eq_ignore_ascii_case("https"));
    if https_via_trusted_proxy
        && let Err(error) = state
            .custom_domain_mapping_repo
            .update_status(&host, true, true)
            .await
    {
        tracing::error!(%host, %error, "Failed to record custom-domain TLS status");
    }

    let public_config = load_public_config(&state, &mapping.realm_id).await?;

    // The entire response body is keyed by the Host header (the sole varying
    // request input since the ?host= override was removed). A shared cache
    // that keys on the URL would store one tenant's config and replay it for
    // every other host hitting the same path — forbid storage and pin the
    // variance (audit run-2: resolve_custom_domain
    // host-keyed-response-missing-cache-directives).
    let mut response =
        axum::response::IntoResponse::into_response(ApiResult::ok(ResolveCustomDomainResponse {
            realm_id: mapping.realm_id,
            public_config,
        }));
    response.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    response.headers_mut().insert(
        axum::http::header::VARY,
        axum::http::HeaderValue::from_static("Host"),
    );
    Ok(response)
}

async fn load_public_config(
    state: &AppState,
    realm_id: &str,
) -> Result<PublicConfigResponse, ApiError> {
    let registration_enabled = query_bool_config(&state.pool, realm_id, "enabled", false).await?;
    let require_email_verification =
        query_bool_config(&state.pool, realm_id, "require_email_verification", false).await?;
    let white_label = query_public_white_label_config(&state.pool, realm_id).await?;

    let configs = state
        .service
        .oauth_config_service()
        .list_enabled_providers(realm_id)
        .await
        .map_err(|e| {
            tracing::error!(realm_id = %realm_id, error = %e, error_details = format!("{:?}", e), "Failed to list oauth providers");
            ApiError::internal(format!("Failed to list oauth providers: {}", e))
        })?;

    let oauth_providers: Vec<OAuthProviderInfo> = configs
        .into_iter()
        .map(|config| OAuthProviderInfo {
            name: config.provider_type.as_str().to_string(),
            display_name: config.provider_type.display_name().to_string(),
            enabled: config.enabled,
            client_id: Some(config.client_id.clone()),
        })
        .collect();

    let realm_info = match state
        .service
        .realm_service()
        .get_public_realm_info(realm_id.to_string())
        .await
    {
        Ok(info) => info,
        Err(_) => RealmSummary {
            id: realm_id.to_string(),
            name: realm_id.to_string(),
            description: None,
        },
    };

    Ok(PublicConfigResponse {
        realm_name: realm_info.name,
        realm_description: realm_info.description,
        registration: RegistrationConfig {
            enabled: registration_enabled,
            require_email_verification,
        },
        oauth_providers,
        white_label,
    })
}
