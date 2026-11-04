use crate::admin::admin_users::phone::read_pending_rebind_phone;
use crate::admin::admin_users::types::{ErrorResponse, UserDetailResponse};
use axum::{
    Extension,
    extract::{Path, State},
    http::HeaderMap,
};
use chrono::{DateTime, Utc};
use herald_api_base::application::http::auth::util::mask_phone;
use herald_api_base::application::http::common::auth_utils::AdminIdentity;
use herald_api_base::application::http::server::api_entities::{ApiError, ApiResult};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::authentication::Identity;
use herald_core::domain::user::AdminUserService;
use herald_core::domain::user::admin_errors::UserAdminError;
use uuid::Uuid;

/// Get user by ID
///
/// Uses the admin-user service lookup, so a target outside the caller's
/// realm is indistinguishable from a missing one (404, no id oracle) — the
/// same `require_target_user_in_realm` semantics as the module's other
/// handlers.
#[utoipa::path(
    get,
    path = "/api/users/{userId}",
    tag = "users",
    summary = "Get user by ID",
    description = "Get detailed information about a specific user. Requires `users.view` permission. Cross-realm targets return 404 (indistinguishable from missing).",
    params(
        ("userId" = Uuid, Path, description = "User ID")
    ),
    responses(
        (status = 200, description = "User found", body = UserDetailResponse),
        (status = 403, description = "Forbidden - Insufficient permissions (requires users.view)", body = ErrorResponse),
        (status = 404, description = "User not found (including cross-realm targets)", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_user(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(target_user_id): Path<Uuid>,
    _headers: HeaderMap,
) -> Result<ApiResult<UserDetailResponse>, ApiError> {
    let admin = AdminIdentity::require(identity, "user management")?;
    admin.require_permission(&state, "users", "view").await?;

    let realm_id = admin.realm_id().to_string();

    // In-realm lookup via the admin-user service (require_target_user_in_realm):
    // a cross-realm id yields UserNotFound → 404, matching the module's
    // id-oracle convention (update/delete/reset-password do the same).
    let admin_user = state
        .admin_user_service
        .get_user_admin(admin.identity().clone(), &realm_id, target_user_id)
        .await
        .map_err(|e| match e {
            UserAdminError::UserNotFound(id) => {
                tracing::debug!(
                    realm_id = %realm_id,
                    user_id = %target_user_id,
                    "Get user failed: user not found in realm"
                );
                ApiError::not_found(format!("User not found: {}", id))
            }
            UserAdminError::PermissionDenied(msg) => {
                tracing::warn!(
                    realm_id = %realm_id,
                    user_id = %target_user_id,
                    error = %msg,
                    "Get user failed: permission denied"
                );
                ApiError::forbidden(msg)
            }
            UserAdminError::DatabaseError(msg) => {
                tracing::error!(
                    realm_id = %realm_id,
                    user_id = %target_user_id,
                    error = %msg,
                    "Failed to get user"
                );
                ApiError::internal(format!("Database error: {}", msg))
            }
            UserAdminError::InternalError(msg) => {
                tracing::error!(
                    realm_id = %realm_id,
                    user_id = %target_user_id,
                    error = %msg,
                    "Failed to get user"
                );
                ApiError::internal(msg)
            }
            _ => {
                tracing::error!(
                    realm_id = %realm_id,
                    user_id = %target_user_id,
                    "Unexpected error getting user"
                );
                ApiError::internal("Unexpected error")
            }
        })?;

    // Detail columns not carried by the AdminUser DTO. The realm predicate
    // mirrors the service-level lookup above so the row can never come from
    // another realm.
    let (provider_ids, phone, updated_at): (Vec<Uuid>, Option<String>, DateTime<Utc>) =
        sqlx::query_as(
            "SELECT provider_ids, phone, updated_at FROM account WHERE id = $1 AND realm_id = $2",
        )
        .bind(target_user_id)
        .bind(&realm_id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| {
            tracing::error!("Failed to fetch user detail columns: {e}");
            ApiError::internal("Failed to fetch user details")
        })?
        .ok_or_else(|| ApiError::not_found("User not found"))?;

    let pending_phone_masked = read_pending_rebind_phone(&state, &realm_id, target_user_id).await;

    // Map to UserDetailResponse
    Ok(ApiResult::ok(UserDetailResponse {
        id: admin_user.id,
        realm_id: admin_user.realm_id,
        email: admin_user.email,
        nickname: admin_user.nickname,
        status: herald_core::domain::user::UserStatus::from(admin_user.status),
        provider_ids,
        phone_masked: phone.as_deref().and_then(mask_phone),
        pending_phone_masked,
        created_at: admin_user.created_at,
        updated_at: updated_at.to_rfc3339(),
    }))
}
