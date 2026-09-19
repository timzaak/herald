use axum::extract::{Path, Query, State};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use validator::Validate;

use axum_valid::Valid;
use herald_api_base::application::http::auth::util::{ClientIp, rate_limit_hit};
pub use herald_api_base::application::http::server::api_entities::ErrorResponse;
use herald_api_base::application::http::server::api_entities::{ApiError, ApiResult};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::client::ports::ClientService;
use herald_core::domain::security_constants::TURNSTILE_STATUS_IP_RATE_LIMIT;

#[derive(Serialize, Deserialize, ToSchema, Validate)]
#[serde(rename_all = "camelCase")]
pub struct TurnstileStatusRequest {
    /// Client App public identifier (1-36 chars).
    #[validate(length(min = 1, max = 36))]
    pub client_id: String,
}

#[derive(Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TurnstileStatusResponse {
    pub enabled: bool,
    pub site_key: Option<String>,
}

/// Get Turnstile configuration status for a Client App
///
/// Returns whether Turnstile verification is enabled for the specified Client
/// App and the site key to render the widget. Turnstile is fully delegated to
/// the Client App (D-PROTECT-01); the legacy realm_config source is no longer
/// read. A missing or disabled Client App is rejected with 401, matching the
/// other anonymous Client-App-bound endpoints. Pass `clientId` as a query
/// parameter.
#[utoipa::path(
    get,
    path = "/api/auth/{realmId}/turnstile/status",
    tag = "auth",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("clientId" = String, Query, description = "Client App public identifier (1-36 chars)")
    ),
    responses(
        (status = 200, description = "Turnstile status retrieved successfully", body = TurnstileStatusResponse),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Client app not found or disabled", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn get_turnstile_status(
    Path(realm_id): Path<String>,
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    Valid(Query(payload)): Valid<Query<TurnstileStatusRequest>>,
) -> Result<ApiResult<TurnstileStatusResponse>, ApiError> {
    // Per-IP cap, mirroring the sibling anonymous public GETs (OIDC
    // discovery / JWKS): this endpoint answers before any authentication and
    // must not be probed at scale.
    rate_limit_hit(
        &state,
        format!("rl:turnstile-status:ip:{ip}"),
        TURNSTILE_STATUS_IP_RATE_LIMIT.0,
        TURNSTILE_STATUS_IP_RATE_LIMIT.1,
    )
    .await?;

    let client_app = state
        .service
        .client_service()
        .get_client_app_by_client_id(&realm_id, &payload.client_id)
        .await
        .map_err(|_| ApiError::unauthorized("Invalid clientId"))?;
    if !client_app.enabled {
        // Indistinguishable from the not-found branch: distinct bodies would
        // give an anonymous caller a client-app existence / disable-state
        // enumeration oracle (audit run-1: turnstile_status oracle).
        return Err(ApiError::unauthorized("Invalid clientId"));
    }

    Ok(ApiResult::ok(TurnstileStatusResponse {
        enabled: client_app.turnstile_enabled,
        site_key: client_app.turnstile_site_key,
    }))
}
