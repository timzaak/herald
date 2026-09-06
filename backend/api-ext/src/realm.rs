// Realm Management API for Third-Party Integration
//
// Allows third-party apps to manage realms using API Key authentication.

use axum::{
    Json,
    extract::{Extension, Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use herald_api_base::application::http::auth::util::{ClientIp, user_agent_from_headers};
use herald_api_base::application::http::common::error_codes::ErrorCode;
use herald_api_base::application::http::common::error_helpers::json_error;
use herald_api_base::application::http::server::api_entities::{ApiError, ErrorResponse};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::audit::AuditContext;
use herald_core::domain::authentication::Identity;
use herald_core::domain::realm::{CreateRealmRequest, InitialAdminUser, Realm, RealmService};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::authz::{require_principal_permission, require_realm_membership};

// ============================================================================
// Request DTOs
// ============================================================================

/// Request body for creating a realm via the ext API
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateRealmExtRequest {
    #[schema(min_length = 3, max_length = 50)]
    pub name: String,

    pub description: Option<String>,

    pub admin_user: AdminUserInput,
}

/// Admin user credentials for realm creation
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AdminUserInput {
    pub email: String,

    #[schema(min_length = 8)]
    pub password: String,
}

// ============================================================================
// Response DTOs
// ============================================================================

/// Realm detail response for create/get endpoints
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RealmInfoResponse {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub admin_user: Option<AdminUserOutput>,
    pub created_at: String,
    pub updated_at: String,
}

/// Admin user info returned as part of realm detail
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AdminUserOutput {
    pub id: String,
    pub email: String,
    pub role: String,
}

/// Single realm item in list responses
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RealmListItem {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Paginated list of realms
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RealmListResponse {
    pub realms: Vec<RealmListItem>,
}

// ============================================================================
// Mappers
// ============================================================================

fn realm_to_info(realm: Realm) -> RealmInfoResponse {
    RealmInfoResponse {
        id: realm.id,
        name: realm.name,
        description: realm.description,
        admin_user: realm.admin_user.map(|u| AdminUserOutput {
            id: u.id,
            email: u.email,
            role: u.role,
        }),
        created_at: realm.created_at.to_rfc3339(),
        updated_at: realm.updated_at.to_rfc3339(),
    }
}

fn realm_to_list_item(realm: Realm) -> RealmListItem {
    RealmListItem {
        id: realm.id,
        name: realm.name,
        description: realm.description,
        created_at: realm.created_at.to_rfc3339(),
        updated_at: realm.updated_at.to_rfc3339(),
    }
}

// ============================================================================
// Handlers
// ============================================================================

/// Create a new realm
///
/// Creates a new realm in the system. Only principals with `realm:manage` permission
/// in the admin realm may invoke this endpoint.
///
/// # Authentication
/// Requires valid API Key via X-API-Key header
///
/// # Authorization
/// The caller must have `realm:manage` permission in the `admin` realm.
#[utoipa::path(
    post,
    path = "/api/ext/realms",
    tag = "ext",
    request_body = CreateRealmExtRequest,
    responses(
        (status = 201, description = "Realm created successfully", body = RealmInfoResponse),
        (status = 400, description = "Validation error", body = ErrorResponse),
        (status = 401, description = "Unauthorized - Invalid or missing API Key", body = ErrorResponse),
        (status = 403, description = "Forbidden - Permission denied", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("api_key" = []))
)]
pub async fn create_realm(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Json(req): Json<CreateRealmExtRequest>,
) -> Response {
    // 1. Authorization: requires realm:manage in the admin realm
    if let Err(resp) =
        require_principal_permission(&state, &identity, "admin", "realm", "manage").await
    {
        return resp.into_response();
    }

    // 2. Validate input
    if req.name.len() < 3 || req.name.len() > 50 {
        return json_error(StatusCode::BAD_REQUEST, ErrorCode::ValidationError);
    }
    if !crate::user::is_valid_email(&req.admin_user.email) {
        return json_error(StatusCode::BAD_REQUEST, ErrorCode::ValidationError);
    }
    if req.admin_user.password.len() < 8 || req.admin_user.password.len() > 100 {
        return json_error(StatusCode::BAD_REQUEST, ErrorCode::ValidationError);
    }

    tracing::info!(
        name = %req.name,
        identity_realm = %identity.realm_id(),
        "Realm creation requested via ext API"
    );

    // 3. Build domain request
    let create_req = CreateRealmRequest {
        id: None,
        name: req.name,
        description: req.description,
        admin_user: InitialAdminUser {
            email: req.admin_user.email,
            password: req.admin_user.password,
        },
    };

    // 4. Call domain service
    let ctx = AuditContext::admin(&identity, ip, user_agent_from_headers(&headers));
    match state
        .service
        .realm_service()
        .create_realm(identity, ctx, create_req)
        .await
    {
        Ok(realm) => {
            tracing::info!(realm_id = %realm.id, "Realm created successfully via ext API");
            (StatusCode::CREATED, Json(realm_to_info(realm))).into_response()
        }
        Err(e) => {
            tracing::error!("Failed to create realm: {}", e);
            ApiError::from(e).into_response()
        }
    }
}

/// List realms
///
/// Returns the realms visible to the caller. Principals in the `admin` realm
/// see all realms (platform-level enumeration); every other principal sees
/// only its own realm. Realm-level detail endpoints remain strictly
/// own-realm — this list is the only platform-wide view.
///
/// # Authentication
/// Requires valid API Key via X-API-Key header
///
/// # Authorization
/// The caller must have `realm:view` permission in their own realm.
#[utoipa::path(
    get,
    path = "/api/ext/realms",
    tag = "ext",
    responses(
        (status = 200, description = "Realms listed successfully", body = RealmListResponse),
        (status = 401, description = "Unauthorized - Invalid or missing API Key", body = ErrorResponse),
        (status = 403, description = "Forbidden - Permission denied", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("api_key" = []))
)]
pub async fn list_realms(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
) -> Response {
    let identity_realm = identity.realm_id();

    // 1. Authorization: requires realm:view in the caller's realm
    if let Err(resp) =
        require_principal_permission(&state, &identity, &identity_realm, "realm", "view").await
    {
        return resp.into_response();
    }

    tracing::info!(
        identity_realm = %identity_realm,
        "Realm list requested via ext API"
    );

    // 2. Call domain service
    let realms = match state.service.realm_service().list_realms(identity).await {
        Ok(realms) => realms,
        Err(e) => {
            tracing::error!("Failed to list realms: {}", e);
            return ApiError::from(e).into_response();
        }
    };

    // 3. Filter: the admin realm enumerates the whole platform; everyone
    // else only ever sees their own realm.
    let items: Vec<RealmListItem> = if identity_realm == "admin" {
        realms.into_iter().map(realm_to_list_item).collect()
    } else {
        realms
            .into_iter()
            .filter(|r| r.id == identity_realm)
            .map(realm_to_list_item)
            .collect()
    };

    tracing::info!(
        realm_count = items.len(),
        "Realms listed successfully via ext API"
    );

    Json(RealmListResponse { realms: items }).into_response()
}

/// Get a single realm by ID
///
/// Returns detailed information for the specified realm.
///
/// # Authentication
/// Requires valid API Key via X-API-Key header
///
/// # Authorization
/// The caller must have `realm:view` permission in the target realm.
/// Every principal may only view its own realm (no super-admin).
#[utoipa::path(
    get,
    path = "/api/ext/realms/{realmId}",
    tag = "ext",
    params(
        ("realmId" = String, Path, description = "Realm ID")
    ),
    responses(
        (status = 200, description = "Realm retrieved successfully", body = RealmInfoResponse),
        (status = 401, description = "Unauthorized - Invalid or missing API Key", body = ErrorResponse),
        (status = 403, description = "Forbidden - Cross-realm access or permission denied", body = ErrorResponse),
        (status = 404, description = "Realm not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("api_key" = []))
)]
pub async fn get_realm(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(realm_id): Path<String>,
) -> Response {
    let identity_realm = identity.realm_id();

    // 1. Authorization: requires realm:view in the identity's own realm (where bindings exist)
    if let Err(resp) =
        require_principal_permission(&state, &identity, &identity_realm, "realm", "view").await
    {
        return resp.into_response();
    }

    // 2. Cross-realm ownership check: non-admin principals may only view their own realm
    if let Err(resp) = require_realm_membership(&identity, &realm_id, "realm access") {
        return resp.into_response();
    }

    tracing::info!(
        realm_id = %realm_id,
        "Realm detail requested via ext API"
    );

    // 3. Call domain service
    match state
        .service
        .realm_service()
        .get_realm(identity, realm_id)
        .await
    {
        Ok(realm) => {
            tracing::info!(realm_id = %realm.id, "Realm retrieved successfully via ext API");
            Json(realm_to_info(realm)).into_response()
        }
        Err(herald_core::domain::common::entities::app_errors::CoreError::NotFound) => {
            json_error(StatusCode::NOT_FOUND, ErrorCode::RealmNotFound)
        }
        Err(e) => {
            tracing::error!("Failed to get realm: {}", e);
            ApiError::from(e).into_response()
        }
    }
}
