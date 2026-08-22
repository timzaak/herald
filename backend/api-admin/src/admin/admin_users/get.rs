use crate::admin::admin_users::types::{ErrorResponse, UserDetailResponse};
use axum::{
    Extension,
    extract::{Path, State},
    http::HeaderMap,
};
use herald_api_base::application::http::common::auth_utils::AdminIdentity;
use herald_api_base::application::http::server::api_entities::{ApiError, ApiResult};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::authentication::Identity;
use uuid::Uuid;

/// Get user by ID
///
/// **MIGRATED**: Now uses Extension<Identity> for authentication
/// Realm boundary check is enforced in Service layer
#[utoipa::path(
    get,
    path = "/api/users/{realmId}/{userId}",
    tag = "users",
    summary = "Get user by ID",
    description = "Get detailed information about a specific user. Requires `users.view` permission.",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("userId" = Uuid, Path, description = "User ID")
    ),
    responses(
        (status = 200, description = "User found", body = UserDetailResponse),
        (status = 403, description = "Forbidden - Insufficient permissions (requires users.view) or realm boundary violation", body = ErrorResponse),
        (status = 404, description = "User not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_user(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path((realm_id, target_user_id)): Path<(String, Uuid)>,
    _headers: HeaderMap,
) -> Result<ApiResult<UserDetailResponse>, ApiError> {
    use herald_core::domain::common::entities::app_errors::CoreError;

    let admin = AdminIdentity::require(identity, &realm_id, "user management")?;
    admin.require_permission(&state, "users", "view").await?;

    // Call UserService - Realm boundary check is enforced in Service layer
    use herald_core::domain::user::UserService;

    let user = UserService::get_user(
        &*state.service.user_service(),
        admin.identity().clone(),
        target_user_id,
    )
    .await
    .map_err(|e| match e {
        CoreError::NotFound => ApiError::not_found("User not found"),
        CoreError::Forbidden(msg) => {
            tracing::warn!(
                "Realm boundary check failed for get_user: user_id={}, error={}",
                target_user_id,
                msg
            );
            ApiError::forbidden(msg)
        }
        _ => {
            tracing::error!("Failed to get user: {e}");
            ApiError::internal("Failed to get user")
        }
    })?;

    // Fetch nickname from profile table (realm predicate mirrors the user
    // lookup above so the row can never come from another realm)
    let nickname: Option<String> =
        sqlx::query_scalar("SELECT nickname FROM profile WHERE id = $1 AND realm_id = $2")
            .bind(target_user_id)
            .bind(admin.realm_id())
            .fetch_optional(&state.pool)
            .await
            .map_err(|e| {
                tracing::error!("Failed to fetch user nickname: {e}");
                ApiError::internal("Failed to fetch user nickname")
            })?
            .flatten();

    // Map User entity to UserDetailResponse
    Ok(ApiResult::ok(UserDetailResponse {
        id: user.id,
        realm_id: user.realm_id,
        email: user.email,
        nickname,
        status: user.status as i16,
        provider_ids: user.provider_ids,
        created_at: user.created_at.to_rfc3339(),
        updated_at: user.updated_at.to_rfc3339(),
    }))
}
