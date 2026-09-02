use axum::{
    Json,
    extract::{Extension, State},
};
use axum_valid::Valid;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;
use validator::Validate;

use crate::application::http::common::auth_utils::require_token_scope;
pub use crate::application::http::server::api_entities::ErrorResponse;
use crate::application::http::server::api_entities::{ApiError, ApiResult};
use crate::application::http::state::AppState;
use herald_api_auth::reauth::consume_reauth;
use herald_core::domain::authentication::{
    BrowserTokenService, CredentialScope, Identity, TargetOperation, TokenCredentialContext,
};
use herald_core::domain::security_constants::DEFAULT_BCRYPT_COST;
use herald_core::domain::user::entities::Profile;
use herald_core::domain::user::ports::UserRepository;
use herald_core::infrastructure::authentication::RedisBrowserTokenService;
use herald_core::infrastructure::user::repositories::PostgresUserRepository;

// ==================== Request/Response Types ====================

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct UserProfile {
    pub id: Uuid,
    pub email: String,
    pub nickname: Option<String>,
    pub status: i16,
}

#[derive(Debug, Serialize, Deserialize, ToSchema, Validate)]
pub struct UpdateProfileRequest {
    #[validate(length(max = 50))]
    pub nickname: Option<String>,
}

// ==================== Handler Functions ====================

/// Get current user profile
#[utoipa::path(
    get,
    path = "/api/user/profile",
    tag = "user",
    responses(
        (status = 200, description = "User profile", body = UserProfile),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn get_profile(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Extension(context): Extension<TokenCredentialContext>,
) -> Result<ApiResult<UserProfile>, ApiError> {
    require_token_scope(&identity, &context, CredentialScope::ProfileRead)?;

    // Use repository to access data
    let user_repo = PostgresUserRepository::new(state.db.clone());

    // Get user
    let user_id = uuid::Uuid::parse_str(&identity.user_id()).map_err(|e| {
        tracing::error!("Invalid user_id format: {}", e);
        ApiError::internal("Invalid user_id format")
    })?;

    let user = user_repo.get_user_by_id(user_id).await.map_err(|e| {
        tracing::error!("Failed to get user profile: {}", e);
        ApiError::internal("Failed to get user profile")
    })?;

    // Try to get profile for additional nickname
    let nickname = match user_repo.get_profile(user_id).await {
        Ok(profile) => profile.nickname,
        Err(_) => user.nickname.clone(), // Fallback to user.nickname if profile doesn't exist
    };

    Ok(ApiResult::ok(UserProfile {
        id: user.id,
        email: user.email,
        nickname,
        status: user.status.into(),
    }))
}

/// Update current user profile
#[utoipa::path(
    put,
    path = "/api/user/profile",
    tag = "user",
    request_body = UpdateProfileRequest,
    responses(
        (status = 200, description = "Profile updated", body = UserProfile),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn update_profile(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Extension(context): Extension<TokenCredentialContext>,
    Valid(Json(payload)): Valid<Json<UpdateProfileRequest>>,
) -> Result<ApiResult<UserProfile>, ApiError> {
    require_token_scope(&identity, &context, CredentialScope::ProfileWriteNickname)?;

    // Use repository to access data
    let user_repo = PostgresUserRepository::new(state.db.clone());

    let user_id = uuid::Uuid::parse_str(&identity.user_id()).map_err(|e| {
        tracing::error!("Invalid user_id format: {}", e);
        ApiError::internal("Invalid user_id format")
    })?;

    // Update profile if any field is provided; create the row when missing.
    // Both write paths return the stored row, so the response is built from it
    // without re-reading the profile table.
    let mut updated_profile: Option<Profile> = None;
    if payload.nickname.is_some() {
        match user_repo
            .update_profile(user_id, payload.nickname.clone().map(Some))
            .await
        {
            Ok(profile) => updated_profile = Some(profile),
            Err(herald_core::domain::common::entities::app_errors::CoreError::NotFound) => {
                // Profile doesn't exist, create it
                let now = chrono::Utc::now();
                let profile = Profile {
                    id: user_id,
                    realm_id: identity.realm_id(),
                    nickname: payload.nickname.clone(),
                    created_at: now,
                    updated_at: now,
                };
                updated_profile = Some(user_repo.create_profile(profile).await.map_err(|e| {
                    tracing::error!("Failed to create profile: {}", e);
                    ApiError::internal("Failed to create profile")
                })?);
            }
            Err(e) => {
                tracing::error!("Failed to update profile: {}", e);
                return Err(ApiError::internal("Failed to update profile".to_string()));
            }
        }
    }

    // Fetch updated user and profile
    let user = user_repo.get_user_by_id(user_id).await.map_err(|e| {
        tracing::error!("Failed to fetch updated profile: {}", e);
        ApiError::internal("Failed to fetch updated profile")
    })?;

    // The no-write path (no fields provided) still reads the stored profile.
    let nickname = match updated_profile {
        Some(profile) => profile.nickname,
        None => match user_repo.get_profile(user_id).await {
            Ok(profile) => profile.nickname,
            Err(_) => user.nickname.clone(),
        },
    };

    Ok(ApiResult::ok(UserProfile {
        id: user.id,
        email: user.email,
        nickname,
        status: user.status.into(),
    }))
}

/// Change password for current user
#[utoipa::path(
    post,
    path = "/api/user/change-password",
    tag = "user",
    operation_id = "change_user_password",
    request_body = ChangePasswordRequest,
    responses(
        (status = 200, description = "Password changed successfully", body = serde_json::Value),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn change_password(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Extension(context): Extension<TokenCredentialContext>,
    Valid(Json(payload)): Valid<Json<ChangePasswordRequest>>,
) -> Result<ApiResult<serde_json::Value>, ApiError> {
    require_token_scope(&identity, &context, CredentialScope::ChangePassword)?;
    consume_reauth(
        &state,
        &identity,
        &context,
        &payload.reauth_token,
        TargetOperation::ChangePassword,
    )
    .await?;

    // Use repository to access data
    let user_repo = PostgresUserRepository::new(state.db.clone());

    let user_id = uuid::Uuid::parse_str(&identity.user_id()).map_err(|e| {
        tracing::error!("Invalid user_id format: {}", e);
        ApiError::internal("Invalid user_id format")
    })?;

    // Hash new password
    let new_password_hash = bcrypt::hash(&payload.new_pass, DEFAULT_BCRYPT_COST).map_err(|e| {
        tracing::error!("Failed to hash password: {}", e);
        ApiError::internal("Failed to hash password")
    })?;

    // Update password using repository
    user_repo
        .change_password(&identity.realm_id(), user_id, new_password_hash)
        .await
        .map_err(|e| {
            tracing::error!("Failed to update password: {}", e);
            ApiError::internal("Failed to update password")
        })?;

    // After a successful password change, revoke all of the user's other
    // sessions (every device/browser except the current one). The current
    // session is preserved so the user stays logged in. Best-effort: the
    // password has already been changed, so a revocation failure is logged but
    // does not block the response.
    let token_service = RedisBrowserTokenService::new(state.redis_manager.clone());
    let current_family_id = context.family_id;
    match token_service.list_user_sessions(&identity.user_id()).await {
        Ok(sessions) => {
            for session in sessions {
                if session.family_id == current_family_id {
                    continue;
                }
                if let Err(e) = token_service.revoke_family(session.family_id).await {
                    tracing::warn!(
                        error = %e,
                        family_id = %session.family_id,
                        "change_password: failed to revoke other session"
                    );
                }
            }
        }
        Err(e) => {
            tracing::warn!(
                error = %e,
                "change_password: failed to list sessions for revocation"
            );
        }
    }

    Ok(ApiResult::ok(
        serde_json::json!({"message": "Password changed successfully"}),
    ))
}

#[derive(Debug, Serialize, Deserialize, ToSchema, Validate)]
#[serde(rename_all = "camelCase")]
pub struct ChangePasswordRequest {
    pub reauth_token: String,
    #[validate(length(min = 8, max = 36))]
    pub new_pass: String,
}
