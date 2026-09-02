use crate::admin::admin_users::types::{ErrorResponse, UserCreateRequest, UserResponse};
use axum::{
    Extension, Json,
    extract::{Path, State},
    http::HeaderMap,
};
use axum_valid::Valid;
use herald_api_base::application::http::auth::util::{
    ClientIp, normalize_email, user_agent_from_headers,
};
use herald_api_base::application::http::common::auth_utils::AdminIdentity;
use herald_api_base::application::http::server::api_entities::{ApiError, ApiResult};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::audit::AuditContext;
use herald_core::domain::authentication::Identity;
use herald_core::domain::user::AdminUserService;
use herald_core::domain::user::admin_dtos::CreateUserWithRolesRequest;
use herald_core::domain::user::admin_errors::UserAdminError;

/// Create a new user account
///
/// Creates a new user with email, password, and optional roles. Only users with the
/// "users.manage" permission can create users. Realm-admins can only assign the "user" role.
#[utoipa::path(
    post,
    path = "/api/users/{realmId}",
    tag = "users",
    summary = "Create a new user",
    description = "Create a new user with email, password, and optional roles. Requires `users.manage` permission. Realm-admins can only assign the 'user' role unless they also have `roles.manage` permission.",
    params(
        ("realmId" = String, Path, description = "Realm ID")
    ),
    request_body = UserCreateRequest,
    responses(
        (status = 201, description = "User created", body = UserResponse),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 403, description = "Forbidden - Insufficient permissions (requires users.manage) or realm boundary violation", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_user(
    Path(realm_id): Path<String>,
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Valid(Json(payload)): Valid<Json<UserCreateRequest>>,
) -> Result<ApiResult<UserResponse>, ApiError> {
    let admin = AdminIdentity::require(identity, &realm_id, "user management")?;
    admin.require_permission(&state, "users", "manage").await?;

    tracing::info!(
        realm_id = %realm_id,
        current_user_id = %admin.user_id_string(),
        "Creating user"
    );

    // 1. Normalize email to lowercase to match login behavior
    let normalized_email = normalize_email(&payload.email);

    // 2. Build CreateUserWithRolesRequest (convert i16 to i32 for status)
    let request = CreateUserWithRolesRequest {
        email: normalized_email.clone(),
        password: payload.password.clone(),
        nickname: payload.nickname.clone(),
        status: payload.status.map(|s| s as i32),
        role_ids: payload.role_ids.clone(),
    };

    // 3. Get admin_user_service
    let admin_user_service = &state.admin_user_service;

    // 4. Call service layer
    let admin_user = admin_user_service
        .create_user_with_roles(
            admin.identity().clone(),
            AuditContext::admin(admin.identity(), ip, user_agent_from_headers(&headers)),
            &realm_id,
            request,
        )
        .await
        .map_err(|e| match e {
            UserAdminError::DuplicateEmail(email) => {
                tracing::debug!(
                    realm_id = %realm_id,
                    "User creation failed: email already exists"
                );
                ApiError::bad_request(format!("Email already exists: {}", email))
            }
            UserAdminError::PermissionDenied(msg) => {
                tracing::warn!(
                    realm_id = %realm_id,
                    error = %msg,
                    "Permission denied for user creation"
                );
                ApiError::forbidden(msg)
            }
            UserAdminError::UserNotFound(id) => {
                tracing::error!(
                    realm_id = %realm_id,
                    user_id = %id,
                    "User not found after creation"
                );
                ApiError::not_found(format!("User not found: {}", id))
            }
            UserAdminError::RoleNotFound(id) => {
                tracing::error!(
                    realm_id = %realm_id,
                    role_id = %id,
                    "Role not found"
                );
                ApiError::bad_request(format!("Role not found: {}", id))
            }
            UserAdminError::InvalidRoleAssignment(msg) => {
                tracing::warn!(
                    realm_id = %realm_id,
                    error = %msg,
                    "Invalid role assignment"
                );
                ApiError::bad_request(format!("Invalid role assignment: {}", msg))
            }
            UserAdminError::DatabaseError(msg) => {
                tracing::error!(
                    realm_id = %realm_id,
                    error = %msg,
                    "Database error during user creation"
                );
                ApiError::internal(format!("Database error: {}", msg))
            }
            UserAdminError::InternalError(msg) => {
                tracing::error!(
                    realm_id = %realm_id,
                    error = %msg,
                    "Internal error during user creation"
                );
                ApiError::internal(msg)
            }
        })?;

    // 5. Convert to UserResponse (convert i32 to i16 for status)
    let response = UserResponse {
        id: admin_user.id,
        realm_id: admin_user.realm_id,
        email: admin_user.email,
        nickname: admin_user.nickname,
        status: admin_user.status as i16,
        created_at: admin_user.created_at,
    };

    tracing::info!(
        user_id = %admin_user.id,
        realm_id = %realm_id,
        "User created successfully"
    );

    // 6. Return result
    Ok(ApiResult::created(response))
}
