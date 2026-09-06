// User Management API for Third-Party Integration
//
// Allows third-party apps to manage users within a realm using API Key authentication.

use axum::{
    Json,
    extract::{Extension, Path, Query, State},
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
use herald_core::domain::user::AdminUserService;
use herald_core::domain::user::admin_dtos::CreateUserWithRolesRequest;
use herald_core::domain::user::admin_errors::UserAdminError;
use herald_core::domain::user::ports::UserService;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::authz::{require_principal_permission, require_realm_membership};

// ============================================================================
// Request DTOs
// ============================================================================

/// Request body for creating a user via the ext API
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateUserExtRequest {
    pub email: String,
    pub password: String,
    pub nickname: Option<String>,
}

pub(crate) fn is_valid_email(email: &str) -> bool {
    let trimmed = email.trim();
    let Some((local, domain)) = trimmed.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !domain.contains("..")
}

/// Query parameters for listing users via the ext API
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListUsersExtQuery {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
}

// ============================================================================
// Response DTOs
// ============================================================================

/// User info returned in responses
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserInfoResponse {
    pub id: String,
    pub email: String,
    pub nickname: Option<String>,
    pub status: i32,
    pub created_at: String,
}

/// Paginated list of users in a realm
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserListResponse {
    pub items: Vec<UserInfoResponse>,
    pub page: u64,
    pub page_size: u64,
    pub total: i64,
}

// ============================================================================
// Handlers
// ============================================================================

/// Create a new user in a realm
///
/// Creates a user in the specified realm. Only principals with `users:create` permission
/// in the target realm may invoke this endpoint.
///
/// # Authentication
/// Requires valid API Key via X-API-Key header
///
/// # Authorization
/// The caller must have `users:create` permission in the target realm.
#[utoipa::path(
    post,
    path = "/api/ext/realms/{realmId}/users",
    tag = "ext",
    params(
        ("realmId" = String, Path, description = "Realm ID")
    ),
    request_body = CreateUserExtRequest,
    responses(
        (status = 201, description = "User created successfully", body = UserInfoResponse),
        (status = 400, description = "Validation error", body = ErrorResponse),
        (status = 401, description = "Unauthorized - Invalid or missing API Key", body = ErrorResponse),
        (status = 403, description = "Forbidden - Cross-realm access or permission denied", body = ErrorResponse),
        (status = 409, description = "Email already exists", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("api_key" = []))
)]
pub async fn create_user(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(realm_id): Path<String>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Json(req): Json<CreateUserExtRequest>,
) -> Response {
    // 1. Authorization: requires users:create in the target realm
    if let Err(resp) =
        require_principal_permission(&state, &identity, &realm_id, "users", "create").await
    {
        return resp.into_response();
    }

    // 2. Cross-realm ownership check
    if let Err(resp) = require_realm_membership(&identity, &realm_id, "user creation") {
        return resp.into_response();
    }

    // 3. Validate input
    if !is_valid_email(&req.email) {
        return json_error(StatusCode::BAD_REQUEST, ErrorCode::ValidationError);
    }
    if req.password.len() < 8 || req.password.len() > 100 {
        return json_error(StatusCode::BAD_REQUEST, ErrorCode::ValidationError);
    }

    tracing::info!(
        realm_id = %realm_id,
        email = %req.email,
        "User creation requested via ext API"
    );

    // 4. Build domain request
    let create_req = CreateUserWithRolesRequest {
        email: req.email,
        password: req.password,
        nickname: req.nickname,
        role_ids: vec![],
        status: Some(1),
    };

    // 5. Call domain service
    let ctx = AuditContext::admin(&identity, ip, user_agent_from_headers(&headers));
    match state
        .admin_user_service
        .create_user_with_roles(identity, ctx, &realm_id, create_req)
        .await
    {
        Ok(admin_user) => {
            tracing::info!(user_id = %admin_user.id, "User created successfully via ext API");
            (
                StatusCode::CREATED,
                Json(admin_user_to_response(admin_user)),
            )
                .into_response()
        }
        Err(UserAdminError::DuplicateEmail(_)) => {
            json_error(StatusCode::CONFLICT, ErrorCode::EmailAlreadyExists)
        }
        Err(e) => {
            tracing::error!("Failed to create user: {}", e);
            ApiError::from(herald_core::domain::common::entities::app_errors::CoreError::from(e))
                .into_response()
        }
    }
}

/// List users in a realm
///
/// Returns a paginated list of users in the specified realm.
/// Default page_size is 20, maximum is 100.
///
/// # Authentication
/// Requires valid API Key via X-API-Key header
///
/// # Authorization
/// The caller must have `users:view` permission in the target realm.
#[utoipa::path(
    get,
    path = "/api/ext/realms/{realmId}/users",
    tag = "ext",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("page" = Option<u64>, Query, description = "Page number (1-based, default 1)"),
        ("pageSize" = Option<u64>, Query, description = "Page size (default 20, max 100)")
    ),
    responses(
        (status = 200, description = "Users listed successfully", body = UserListResponse),
        (status = 401, description = "Unauthorized - Invalid or missing API Key", body = ErrorResponse),
        (status = 403, description = "Forbidden - Cross-realm access or permission denied", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("api_key" = []))
)]
pub async fn list_users(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(realm_id): Path<String>,
    Query(query): Query<ListUsersExtQuery>,
) -> Response {
    // 1. Authorization: requires users:view in the target realm
    if let Err(resp) =
        require_principal_permission(&state, &identity, &realm_id, "users", "view").await
    {
        return resp.into_response();
    }

    // 2. Cross-realm ownership check
    if let Err(resp) = require_realm_membership(&identity, &realm_id, "user list") {
        return resp.into_response();
    }

    // 3. Normalize pagination: default page=1, page_size=20, max 100
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(20).clamp(1, 100);

    tracing::info!(
        realm_id = %realm_id,
        page,
        page_size,
        "User list requested via ext API"
    );

    // 4. Call domain service with pagination
    match state
        .service
        .user_service()
        .list_users(identity, realm_id.clone(), page, page_size, None, None)
        .await
    {
        Ok((users, total)) => {
            let user_responses: Vec<UserInfoResponse> =
                users.into_iter().map(user_to_response).collect();
            tracing::info!(
                realm_id = %realm_id,
                user_count = user_responses.len(),
                total,
                "Users listed successfully via ext API"
            );
            Json(UserListResponse {
                items: user_responses,
                page,
                page_size,
                total,
            })
            .into_response()
        }
        Err(e) => {
            tracing::error!("Failed to list users: {}", e);
            ApiError::from(e).into_response()
        }
    }
}

/// Get a single user by ID within a realm
///
/// Returns detailed information for the specified user.
///
/// # Authentication
/// Requires valid API Key via X-API-Key header
///
/// # Authorization
/// The caller must have `users:view` permission in the target realm.
#[utoipa::path(
    get,
    path = "/api/ext/realms/{realmId}/users/{userId}",
    tag = "ext",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("userId" = String, Path, description = "User ID")
    ),
    responses(
        (status = 200, description = "User retrieved successfully", body = UserInfoResponse),
        (status = 400, description = "Invalid user ID format", body = ErrorResponse),
        (status = 401, description = "Unauthorized - Invalid or missing API Key", body = ErrorResponse),
        (status = 403, description = "Forbidden - Cross-realm access or permission denied", body = ErrorResponse),
        (status = 404, description = "User not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("api_key" = []))
)]
pub async fn get_user(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path((realm_id, user_id)): Path<(String, String)>,
) -> Response {
    // 1. Authorization: requires users:view in the target realm
    if let Err(resp) =
        require_principal_permission(&state, &identity, &realm_id, "users", "view").await
    {
        return resp.into_response();
    }

    // 2. Cross-realm ownership check
    if let Err(resp) = require_realm_membership(&identity, &realm_id, "user access") {
        return resp.into_response();
    }

    // 3. Parse user ID
    let user_uuid = match Uuid::parse_str(&user_id) {
        Ok(uuid) => uuid,
        Err(_) => {
            return json_error(StatusCode::BAD_REQUEST, ErrorCode::InvalidUserIdFormat);
        }
    };

    tracing::info!(
        realm_id = %realm_id,
        user_id = %user_uuid,
        "User detail requested via ext API"
    );

    // 4. Call domain service
    match state
        .admin_user_service
        .get_user_admin(identity, &realm_id, user_uuid)
        .await
    {
        Ok(admin_user) => {
            tracing::info!(user_id = %admin_user.id, "User retrieved successfully via ext API");
            Json(admin_user_to_response(admin_user)).into_response()
        }
        Err(UserAdminError::UserNotFound(_)) => {
            json_error(StatusCode::NOT_FOUND, ErrorCode::UserNotFound)
        }
        Err(e) => {
            tracing::error!("Failed to get user: {}", e);
            ApiError::from(herald_core::domain::common::entities::app_errors::CoreError::from(e))
                .into_response()
        }
    }
}

// ============================================================================
// Mappers
// ============================================================================

/// Map AdminUser (from admin service) to UserInfoResponse
fn admin_user_to_response(
    user: herald_core::domain::user::admin_dtos::AdminUser,
) -> UserInfoResponse {
    UserInfoResponse {
        id: user.id.to_string(),
        email: user.email,
        nickname: user.nickname,
        status: user.status,
        created_at: user.created_at,
    }
}

/// Map User entity (from user service) to UserInfoResponse
fn user_to_response(user: herald_core::domain::user::entities::User) -> UserInfoResponse {
    UserInfoResponse {
        id: user.id.to_string(),
        email: user.email,
        nickname: user.nickname,
        status: i16::from(user.status) as i32,
        created_at: user.created_at.to_rfc3339(),
    }
}
