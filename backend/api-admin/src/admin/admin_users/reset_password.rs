use crate::admin::admin_users::types::{ErrorResponse, ResetPasswordResponse};
use axum::{
    Extension,
    extract::{Path, State},
    http::HeaderMap,
};
use herald_api_base::application::http::auth::util::{ClientIp, user_agent_from_headers};
use herald_api_base::application::http::common::auth_utils::AdminIdentity;
use herald_api_base::application::http::server::api_entities::{ApiError, ApiResult};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::audit::AuditContext;
use herald_core::domain::authentication::Identity;
use herald_core::domain::user::AdminUserService;
use herald_core::domain::user::admin_errors::UserAdminError;
use uuid::Uuid;

/// Reset user password (generate 16-character random password)
#[utoipa::path(
    post,
    path = "/api/users/{userId}/reset-password",
    tag = "users",
    params(
        ("userId" = Uuid, Path, description = "User ID")
    ),
    responses(
        (status = 200, description = "Password reset successfully", body = ResetPasswordResponse),
        (status = 403, description = "Forbidden - Insufficient permissions", body = ErrorResponse),
        (status = 404, description = "User not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn reset_user_password(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(target_user_id): Path<Uuid>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
) -> Result<ApiResult<ResetPasswordResponse>, ApiError> {
    let admin = AdminIdentity::require(identity, "user management")?;
    let realm_id = admin.realm_id().to_string();
    admin.require_permission(&state, "users", "manage").await?;

    tracing::info!(
        realm_id = %realm_id,
        user_id = %target_user_id,
        "Resetting user password"
    );

    // Get admin_user_service
    let admin_user_service = &state.admin_user_service;

    // Call service layer
    let new_password = admin_user_service
        .reset_user_password(
            admin.identity().clone(),
            AuditContext::admin(admin.identity(), ip, user_agent_from_headers(&headers)),
            &realm_id,
            target_user_id,
        )
        .await
        .map_err(|e| match e {
            UserAdminError::PermissionDenied(msg) => {
                tracing::warn!(
                    realm_id = %realm_id,
                    user_id = %target_user_id,
                    error = %msg,
                    "Permission denied for password reset"
                );
                ApiError::forbidden(msg)
            }
            UserAdminError::UserNotFound(id) => {
                tracing::debug!(
                    realm_id = %realm_id,
                    user_id = %target_user_id,
                    "Password reset failed: user not found"
                );
                ApiError::not_found(format!("User not found: {}", id))
            }
            UserAdminError::DatabaseError(msg) => {
                tracing::error!(
                    realm_id = %realm_id,
                    user_id = %target_user_id,
                    error = %msg,
                    "Failed to reset password"
                );
                ApiError::internal(format!("Database error: {}", msg))
            }
            UserAdminError::InternalError(msg) => {
                tracing::error!(
                    realm_id = %realm_id,
                    user_id = %target_user_id,
                    error = %msg,
                    "Failed to reset password"
                );
                ApiError::internal(msg)
            }
            _ => {
                tracing::error!(
                    realm_id = %realm_id,
                    user_id = %target_user_id,
                    "Unexpected error during password reset"
                );
                ApiError::internal("Unexpected error")
            }
        })?;

    tracing::info!(
        user_id = %target_user_id,
        realm_id = %realm_id,
        "User password reset successfully"
    );

    Ok(ApiResult::ok(ResetPasswordResponse { new_password }))
}
