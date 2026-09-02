use crate::admin::admin_users::types::{ErrorResponse, UserResponse, UserUpdateRequest};
use axum::{
    Extension, Json,
    extract::{Path, State},
    http::HeaderMap,
};
use axum_valid::Valid;
use herald_api_base::application::http::auth::util::{ClientIp, user_agent_from_headers};
use herald_api_base::application::http::common::auth_utils::AdminIdentity;
use herald_api_base::application::http::server::api_entities::{ApiError, ApiResult};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::audit::AuditContext;
use herald_core::domain::authentication::Identity;
use herald_core::domain::user::{
    AdminUserService, admin_dtos::UpdateUserAdminRequest, admin_errors::UserAdminError,
};
use uuid::Uuid;

/// Update user account
///
/// Updates user status or nickname. Email is read-only after creation. Requires "users.manage" permission.
#[utoipa::path(
    put,
    path = "/api/users/{realmId}/{userId}",
    tag = "users",
    summary = "Update a user",
    description = "Update user status or nickname. Email is read-only after creation. Requires `users.manage` permission.",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("userId" = Uuid, Path, description = "User ID")
    ),
    request_body = UserUpdateRequest,
    responses(
        (status = 200, description = "User updated", body = UserResponse),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 403, description = "Forbidden - Insufficient permissions (requires users.manage) or realm boundary violation", body = ErrorResponse),
        (status = 404, description = "User not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_user(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path((realm_id, target_user_id)): Path<(String, Uuid)>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Valid(Json(payload)): Valid<Json<UserUpdateRequest>>,
) -> Result<ApiResult<UserResponse>, ApiError> {
    let admin = AdminIdentity::require(identity, &realm_id, "user management")?;
    admin.require_permission(&state, "users", "manage").await?;

    tracing::info!(
        realm_id = %realm_id,
        user_id = %target_user_id,
        "Updating user"
    );

    // Build UpdateUserAdminRequest (convert i16 to i32 for status)
    let request = UpdateUserAdminRequest {
        nickname: payload.nickname,
        status: payload.status.map(|s| s as i32),
    };

    // Get admin_user_service
    let admin_user_service = &state.admin_user_service;

    // Call service layer
    let admin_user = admin_user_service
        .update_user_admin(
            admin.identity().clone(),
            AuditContext::admin(admin.identity(), ip, user_agent_from_headers(&headers)),
            &realm_id,
            target_user_id,
            request,
        )
        .await
        .map_err(|e| match e {
            UserAdminError::DuplicateEmail(email) => {
                tracing::debug!(
                    realm_id = %realm_id,
                    user_id = %target_user_id,
                    "User update failed: email already exists"
                );
                ApiError::bad_request(format!("Email already exists: {}", email))
            }
            UserAdminError::PermissionDenied(msg) => {
                tracing::warn!(
                    "Policy check failed: realm_id={}, user_id={}, error={}",
                    realm_id,
                    target_user_id,
                    msg
                );
                ApiError::forbidden(msg)
            }
            UserAdminError::UserNotFound(id) => {
                tracing::debug!(
                    realm_id = %realm_id,
                    user_id = %target_user_id,
                    "User update failed: user not found"
                );
                ApiError::not_found(format!("User not found: {}", id))
            }
            UserAdminError::DatabaseError(msg) => {
                tracing::error!(
                    realm_id = %realm_id,
                    user_id = %target_user_id,
                    error = %msg,
                    "Failed to update user"
                );
                ApiError::internal(format!("Database error: {}", msg))
            }
            UserAdminError::InternalError(msg) => {
                tracing::error!(
                    realm_id = %realm_id,
                    user_id = %target_user_id,
                    error = %msg,
                    "Failed to update user"
                );
                ApiError::internal(msg)
            }
            _ => {
                tracing::error!(
                    realm_id = %realm_id,
                    user_id = %target_user_id,
                    "Unexpected error during user update"
                );
                ApiError::internal("Unexpected error")
            }
        })?;

    // Convert to UserResponse (convert i32 to i16 for status)
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
        "User updated successfully"
    );

    Ok(ApiResult::ok(response))
}
