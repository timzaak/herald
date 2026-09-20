// Client App Management API for Third-Party Integration
//
// Allows third-party apps to manage client apps within a realm using API Key authentication.

use axum::{
    Json,
    extract::{Extension, Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use herald_api_base::application::http::common::error_codes::ErrorCode;
use herald_api_base::application::http::common::error_helpers::json_error;
use herald_api_base::application::http::server::api_entities::{ApiError, ErrorResponse};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::authentication::Identity;
use herald_core::domain::client::ports::ClientService;
use herald_core::domain::client::value_objects::CreateClientAppRequest;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::authz::{require_principal_permission, require_realm_membership};
use crate::client_app_scope::{ensure_client_app_scope, is_admin_api_key};

// ============================================================================
// Request DTOs
// ============================================================================

/// Request body for creating a client app via the ext API
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateClientAppExtRequest {
    pub name: String,
    pub description: Option<String>,
    pub redirect_uris: Vec<String>,
    /// Enable device code grant flow for this client app.
    /// When enabled, redirect_uris may be empty (device flow has no callback).
    pub device_code_grant_enabled: Option<bool>,
}

// ============================================================================
// Response DTOs
// ============================================================================

/// Client app detail response for create/get endpoints
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClientAppInfoResponse {
    pub id: String,
    pub client_id: String,
    pub client_secret: Option<String>,
    pub name: String,
    pub description: Option<String>,
    pub redirect_uris: Vec<String>,
    pub enabled: bool,
    pub created_at: String,
}

/// Single client app item in list responses
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClientAppListItem {
    pub id: String,
    pub client_id: String,
    pub name: String,
    pub enabled: bool,
    pub created_at: String,
}

/// List of client apps in a realm
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClientAppListResponse {
    pub client_apps: Vec<ClientAppListItem>,
}

// ============================================================================
// Handlers
// ============================================================================

/// Create a new client app in a realm
///
/// Creates a client app in the specified realm. Only principals with `clients:manage` permission
/// in the target realm may invoke this endpoint.
///
/// # Authentication
/// Requires valid API Key via X-API-Key header
///
/// # Authorization
/// The caller must have `clients:manage` permission in the target realm.
#[utoipa::path(
    post,
    path = "/api/ext/realms/{realmId}/client-apps",
    tag = "ext",
    params(
        ("realmId" = String, Path, description = "Realm ID")
    ),
    request_body = CreateClientAppExtRequest,
    responses(
        (status = 201, description = "Client app created successfully", body = ClientAppInfoResponse),
        (status = 400, description = "Validation error", body = ErrorResponse),
        (status = 401, description = "Unauthorized - Invalid or missing API Key", body = ErrorResponse),
        (status = 403, description = "Forbidden - Cross-realm access or permission denied", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("api_key" = []))
)]
pub async fn create_client_app(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(realm_id): Path<String>,
    Json(req): Json<CreateClientAppExtRequest>,
) -> Response {
    // 1. Authorization: requires clients:manage in the target realm
    if let Err(resp) =
        require_principal_permission(&state, &identity, &realm_id, "clients", "manage").await
    {
        return resp.into_response();
    }

    // 2. Cross-realm ownership check
    if let Err(resp) = require_realm_membership(&identity, &realm_id, "client app creation") {
        return resp.into_response();
    }

    // 3. Validate input
    if req.name.is_empty() {
        return json_error(StatusCode::BAD_REQUEST, ErrorCode::ValidationError);
    }

    tracing::info!(
        realm_id = %realm_id,
        name = %req.name,
        "Client app creation requested via ext API"
    );

    // 4. Generate client_id using UUID v7
    let client_id = Uuid::now_v7().to_string();

    // 5. Build domain request
    let create_req = CreateClientAppRequest {
        realm_id: realm_id.clone(),
        client_id,
        name: req.name,
        description: req.description,
        redirect_uris: Some(req.redirect_uris),
        allowed_origins: None,
        email_verify_return_url: None,
        password_reset_return_url: None,
        browser_refresh_absolute_ttl_seconds: None,
        enabled: None,
        icon_url: None,
        device_code_grant_enabled: req.device_code_grant_enabled,
        turnstile_enabled: None,
        turnstile_site_key: None,
        turnstile_secret_key: None,
    };

    // 6. Call domain service
    match state
        .service
        .client_service()
        .create_client_app(identity, create_req)
        .await
    {
        Ok(client_app) => {
            tracing::info!(
                client_app_id = %client_app.id,
                "Client app created successfully via ext API"
            );
            (
                StatusCode::CREATED,
                Json(client_app_to_response(client_app, true)),
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!("Failed to create client app: {}", e);
            ApiError::from(e).into_response()
        }
    }
}

/// List client apps in a realm
///
/// Returns all client apps in the specified realm.
///
/// # Authentication
/// Requires valid API Key via X-API-Key header
///
/// # Authorization
/// The caller must have `clients:view` permission in the target realm.
#[utoipa::path(
    get,
    path = "/api/ext/realms/{realmId}/client-apps",
    tag = "ext",
    params(
        ("realmId" = String, Path, description = "Realm ID")
    ),
    responses(
        (status = 200, description = "Client apps listed successfully", body = ClientAppListResponse),
        (status = 401, description = "Unauthorized - Invalid or missing API Key", body = ErrorResponse),
        (status = 403, description = "Forbidden - Cross-realm access or permission denied", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("api_key" = []))
)]
pub async fn list_client_apps(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(realm_id): Path<String>,
) -> Response {
    // 1. Authorization: requires clients:view in the target realm
    if let Err(resp) =
        require_principal_permission(&state, &identity, &realm_id, "clients", "view").await
    {
        return resp.into_response();
    }

    // 2. Cross-realm ownership check
    if let Err(resp) = require_realm_membership(&identity, &realm_id, "client app list") {
        return resp.into_response();
    }

    tracing::info!(
        realm_id = %realm_id,
        "Client app list requested via ext API"
    );

    // 3. Call domain service
    match state
        .service
        .client_service()
        .list_client_apps(identity.clone(), realm_id.clone())
        .await
    {
        Ok(mut client_apps) => {
            let admin_api_key = match is_admin_api_key(&state, &identity).await {
                Ok(value) => value,
                Err(resp) => return resp,
            };
            if !admin_api_key
                && let Some(bound_client_app_id) = identity
                    .as_third_party()
                    .and_then(|api_key| api_key.client_app_id)
            {
                client_apps.retain(|client_app| client_app.id == bound_client_app_id);
            }

            let items: Vec<ClientAppListItem> = client_apps
                .into_iter()
                .map(client_app_to_list_item)
                .collect();
            tracing::info!(
                realm_id = %realm_id,
                client_app_count = items.len(),
                "Client apps listed successfully via ext API"
            );
            Json(ClientAppListResponse { client_apps: items }).into_response()
        }
        Err(e) => {
            tracing::error!("Failed to list client apps: {}", e);
            ApiError::from(e).into_response()
        }
    }
}

/// Get a single client app by ID within a realm
///
/// Returns detailed information for the specified client app.
///
/// # Authentication
/// Requires valid API Key via X-API-Key header
///
/// # Authorization
/// The caller must have `clients:view` permission in the target realm.
#[utoipa::path(
    get,
    path = "/api/ext/realms/{realmId}/client-apps/{clientAppId}",
    tag = "ext",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("clientAppId" = String, Path, description = "Client App ID")
    ),
    responses(
        (status = 200, description = "Client app retrieved successfully", body = ClientAppInfoResponse),
        (status = 400, description = "Invalid client app ID format", body = ErrorResponse),
        (status = 401, description = "Unauthorized - Invalid or missing API Key", body = ErrorResponse),
        (status = 403, description = "Forbidden - Cross-realm access or permission denied", body = ErrorResponse),
        (status = 404, description = "Client app not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("api_key" = []))
)]
pub async fn get_client_app(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path((realm_id, client_app_id)): Path<(String, String)>,
) -> Response {
    // 1. Authorization: requires clients:view in the target realm
    if let Err(resp) =
        require_principal_permission(&state, &identity, &realm_id, "clients", "view").await
    {
        return resp.into_response();
    }

    // 2. Cross-realm ownership check
    if let Err(resp) = require_realm_membership(&identity, &realm_id, "client app access") {
        return resp.into_response();
    }

    // 3. Parse client app ID
    let client_app_uuid = match Uuid::parse_str(&client_app_id) {
        Ok(uuid) => uuid,
        Err(_) => {
            return json_error(StatusCode::BAD_REQUEST, ErrorCode::InvalidClientAppIdFormat);
        }
    };

    if let Err(resp) = ensure_client_app_scope(&state, &identity, client_app_uuid).await {
        return resp;
    }

    tracing::info!(
        realm_id = %realm_id,
        client_app_id = %client_app_uuid,
        "Client app detail requested via ext API"
    );

    // 4. Call domain service
    match state
        .service
        .client_service()
        .get_client_app(identity, client_app_uuid)
        .await
    {
        Ok(client_app) => {
            tracing::info!(
                client_app_id = %client_app.id,
                "Client app retrieved successfully via ext API"
            );
            Json(client_app_to_response(client_app, false)).into_response()
        }
        // Cross-realm rows must be indistinguishable from unknown ids on the
        // ext surface: the domain's post-fetch realm check answers Forbidden
        // ("client belongs to a different realm") while an unknown id answers
        // 404, and the 403/404 split is a cross-tenant existence oracle
        // (audit run-2: cross-realm-existence-oracle-fetch-then-realm-check).
        // Collapsed here — the admin surface keeps its distinct 403.
        Err(herald_core::domain::common::entities::app_errors::CoreError::NotFound)
        | Err(herald_core::domain::common::entities::app_errors::CoreError::Forbidden(_)) => {
            json_error(StatusCode::NOT_FOUND, ErrorCode::ClientAppNotFound)
        }
        Err(e) => {
            tracing::error!("Failed to get client app: {}", e);
            ApiError::from(e).into_response()
        }
    }
}

// ============================================================================
// Mappers
// ============================================================================

/// Map ClientApp to ClientAppInfoResponse
fn client_app_to_response(
    app: herald_core::domain::client::entities::ClientApp,
    include_secret: bool,
) -> ClientAppInfoResponse {
    ClientAppInfoResponse {
        id: app.id.to_string(),
        client_id: app.client_id,
        client_secret: if include_secret {
            app.client_secret
        } else {
            None
        },
        name: app.name,
        description: app.description,
        redirect_uris: app.redirect_uris,
        enabled: app.enabled,
        created_at: app.created_at.to_rfc3339(),
    }
}

/// Map ClientApp to ClientAppListItem for list endpoint (no client_secret)
fn client_app_to_list_item(
    app: herald_core::domain::client::entities::ClientApp,
) -> ClientAppListItem {
    ClientAppListItem {
        id: app.id.to_string(),
        client_id: app.client_id,
        name: app.name,
        enabled: app.enabled,
        created_at: app.created_at.to_rfc3339(),
    }
}
