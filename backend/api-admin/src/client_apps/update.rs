use axum::{
    Json,
    extract::{Extension, Path, State},
    http::HeaderMap,
};
use axum_valid::Valid;
use herald_core::domain::authentication::Identity;
use uuid::Uuid;

use crate::client_apps::types::{ClientAppItem, ClientAppUpdateRequest};
use herald_api_base::application::http::common::auth_utils::AdminIdentity;
use herald_api_base::application::http::server::api_entities::{ApiError, ApiResult};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::authentication::ports::BrowserTokenService;
use herald_core::domain::client::ports::ClientService;
use herald_core::domain::client::value_objects::UpdateClientAppRequest;
use herald_core::infrastructure::authentication::RedisBrowserTokenService;

/// Update a client app configuration
///
/// Updates the configuration of an existing OAuth client application.
/// Optionally regenerate the client secret.
#[utoipa::path(
    put,
    path = "/api/client/{realmId}/{clientAppId}",
    tag = "client",
    request_body = ClientAppUpdateRequest,
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("clientAppId" = Uuid, Path, description = "client app UUID"),
    ),
    responses(
        (status = 200, description = "ClientApp updated", body = ClientAppItem),
        (status = 404, description = "ClientApp not found", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 500, description = "Internal server error", body = herald_api_base::application::http::server::api_entities::ErrorResponse)
    )
)]
pub async fn update_client_app(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path((realm_id, id)): Path<(String, Uuid)>,
    _headers: HeaderMap,
    Valid(Json(payload)): Valid<Json<ClientAppUpdateRequest>>,
) -> Result<ApiResult<ClientAppItem>, ApiError> {
    let admin = AdminIdentity::require(identity, &realm_id, "client applications")?;
    admin
        .require_permission(&state, "clients", "manage")
        .await?;

    tracing::debug!(
        realm_id = %realm_id,
        user_id = %admin.user_id_string(),
        "Updating client app"
    );

    // Create service request
    let service_request = UpdateClientAppRequest {
        name: payload.name.clone(),
        description: payload.description.clone(),
        redirect_uris: payload.redirect_uris.clone(),
        allowed_origins: payload.allowed_origins.clone(),
        email_verify_return_url: payload.email_verify_return_url.clone(),
        password_reset_return_url: payload.password_reset_return_url.clone(),
        browser_refresh_absolute_ttl_seconds: payload.browser_refresh_absolute_ttl_seconds,
        enabled: payload.enabled,
        icon_url: payload.icon_url.clone(),
        regenerate_secret: payload.regenerate_secret,
        device_code_grant_enabled: payload.device_code_grant_enabled,
        turnstile_enabled: payload.turnstile_enabled,
        turnstile_site_key: payload.turnstile_site_key.clone(),
        turnstile_secret_key: payload.turnstile_secret_key.clone(),
    };

    // Persist through the service layer first: the realm boundary check and
    // request validation (redirect URIs, TTL bounds) run inside, so a rejected
    // update returns before any session side effect.
    let client_service = state.service.client_service();
    let client_app = client_service
        .update_client_app(admin.identity().clone(), id, service_request)
        .await
        .map_err(|e| match e {
            herald_core::domain::common::entities::app_errors::CoreError::NotFound => {
                ApiError::not_found("client_app not found")
            }
            herald_core::domain::common::entities::app_errors::CoreError::BadRequest(msg) => {
                tracing::error!("Bad request: {}", msg);
                ApiError::bad_request(msg)
            }
            e => {
                tracing::error!("Failed to update client app: {}", e);
                ApiError::internal(format!("Failed to update client app: {e}"))
            }
        })?;

    // Revoke browser token families only after a disable persisted, so a
    // disabled app does not keep active tokens in the wild. Revocation follows
    // persistence (rather than preceding it) so a rejected update never logs
    // the app's users out; a revocation failure after a persisted disable
    // surfaces as 500 — the disable holds, and outstanding tokens die at their
    // natural expiry.
    if payload.enabled == Some(false) {
        RedisBrowserTokenService::new(state.redis_manager.clone())
            .revoke_client_families(id)
            .await
            .map_err(|e| {
                ApiError::internal(format!(
                    "Browser token revocation failed after disabling client app: {e}"
                ))
            })?;
    }

    // Convert domain model to API response model. Echo the new secret only when
    // the caller asked to regenerate it.
    let client_secret = payload
        .regenerate_secret
        .filter(|regenerate| *regenerate)
        .and(client_app.client_secret.clone());
    let mut response: ClientAppItem = client_app.into();
    response.client_secret = client_secret;

    Ok(ApiResult::ok(response))
}
