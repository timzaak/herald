use crate::admin::admin_users::types::{
    AssignPermissionRequest, EffectivePermission, EffectivePermissionsResponse, ErrorResponse,
    UserPermissionsResponse,
};
use axum::{
    Extension, Json,
    extract::{Path, State},
    http::HeaderMap,
};
use axum_valid::Valid;
use herald_api_base::application::http::common::auth_utils::AdminIdentity;
use herald_api_base::application::http::server::api_entities::{ApiError, ApiResult};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::authentication::Identity;
use herald_core::domain::authorization::permission_service::PermissionService;
use herald_core::domain::user::UserPermissionService;
use herald_core::domain::user::admin_errors::UserAdminError;
use sqlx::Row;
use uuid::Uuid;

/// Get user's directly assigned permissions
///
/// Returns permissions that were directly assigned to the user (not through roles).
/// These are policies where the subject is the user_id itself.
///
/// **Note**: This endpoint returns only direct permissions. For effective permissions
/// (including those inherited from roles), use `/effective-permissions`.
#[utoipa::path(
    get,
    path = "/api/users/{realmId}/{userId}/permissions",
    tag = "users",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("userId" = Uuid, Path, description = "User ID")
    ),
    responses(
        (status = 200, description = "User direct permissions", body = UserPermissionsResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden - Insufficient permissions", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn get_user_permissions(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path((realm_id, user_id)): Path<(String, Uuid)>,
    _headers: HeaderMap,
) -> Result<ApiResult<UserPermissionsResponse>, ApiError> {
    let admin = AdminIdentity::require(identity, &realm_id, "user permission management")?;
    admin.require_permission(&state, "users", "view").await?;

    // Get user's direct permissions from role_policies table
    // Note: Direct user permissions are stored with user_id as role_id
    let permissions = sqlx::query(
        "SELECT resource, action FROM role_policies WHERE realm_id = $1 AND role_id = $2::uuid",
    )
    .bind(&realm_id)
    .bind(user_id)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!(
            realm_id = %realm_id,
            user_id = %user_id,
            error = %e,
            "Failed to fetch user direct permissions"
        );
        ApiError::internal("Failed to fetch user permissions")
    })?
    .iter()
    .map(|row| crate::admin::admin_users::types::UserPermission {
        resource: row.get("resource"),
        action: row.get("action"),
    })
    .collect();

    Ok(ApiResult::ok(UserPermissionsResponse { permissions }))
}

/// Assign direct permission to user
///
/// Assigns a permission directly to a user (bypassing roles).
///
/// **Security constraints**:
/// - Requires policies.manage permission
/// - Cannot create "All" or wildcard policies (privileged policies)
#[utoipa::path(
    post,
    path = "/api/users/{realmId}/{userId}/permissions",
    tag = "users",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("userId" = Uuid, Path, description = "User ID")
    ),
    request_body = AssignPermissionRequest,
    responses(
        (status = 200, description = "Permission assigned"),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden - cannot create privileged policies", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn assign_user_permission(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path((realm_id, target_user_id)): Path<(String, Uuid)>,
    _headers: HeaderMap,
    Valid(Json(payload)): Valid<Json<AssignPermissionRequest>>,
) -> Result<ApiResult<()>, ApiError> {
    let admin = AdminIdentity::require(identity, &realm_id, "user permission management")?;
    let current_user_id = admin.user_id_string();
    let permission_checker = &state.permission_checker;

    tracing::info!(
        realm_id = %realm_id,
        target_user_id = %target_user_id,
        current_user_id = %current_user_id,
        resource = %payload.resource,
        action = %payload.action,
        "Assigning direct permission to user"
    );

    // Security: Cannot create "All" or wildcard policies
    if payload.resource == "All" || payload.resource.contains("*") {
        tracing::warn!(
            current_user_id = %current_user_id,
            realm_id = %realm_id,
            resource = %payload.resource,
            "Attempted to create privileged policy"
        );
        return Err(ApiError::forbidden("Cannot create privileged policies"));
    }

    // Check policies.manage permission
    admin
        .require_permission(&state, "policies", "manage")
        .await?;

    // Security: a delegated policies.manage holder must not grant a
    // permission they do not hold themselves — otherwise a sub-admin could
    // self-assign e.g. ("users","manage") and take over the realm.
    admin
        .require_permission(&state, &payload.resource, &payload.action)
        .await?;

    // Security: the target user must exist in this realm, so policy rows are
    // never written for arbitrary ids (including users of other realms).
    let target_exists =
        sqlx::query_scalar::<_, Uuid>("SELECT id FROM account WHERE id = $1 AND realm_id = $2")
            .bind(target_user_id)
            .bind(&realm_id)
            .fetch_optional(&state.pool)
            .await
            .map_err(|e| {
                tracing::error!(
                    realm_id = %realm_id,
                    target_user_id = %target_user_id,
                    error = %e,
                    "Failed to check target user for permission assignment"
                );
                ApiError::internal("Failed to check target user")
            })?;
    if target_exists.is_none() {
        return Err(ApiError::not_found("User not found in this realm"));
    }

    // Check if permission already exists
    let existing = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM role_policies
         WHERE realm_id = $1 AND role_id = $2::uuid AND resource = $3 AND action = $4",
    )
    .bind(&realm_id)
    .bind(target_user_id)
    .bind(&payload.resource)
    .bind(&payload.action)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!(
            user_id = %target_user_id,
            resource = %payload.resource,
            action = %payload.action,
            error = %e,
            "Failed to check existing permission"
        );
        ApiError::internal("Failed to check existing permission")
    })?;

    if existing.is_some() {
        tracing::debug!(
            user_id = %target_user_id,
            resource = %payload.resource,
            action = %payload.action,
            "Permission already exists"
        );
        return Ok(ApiResult::ok(()));
    }

    // Add the permission policy to role_policies table
    // Direct user permissions are stored with user_id as role_id
    let policy_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO role_policies (id, role_id, realm_id, resource, action)
         VALUES ($1, $2::uuid, $3, $4, $5)",
    )
    .bind(policy_id)
    .bind(target_user_id)
    .bind(&realm_id)
    .bind(&payload.resource)
    .bind(&payload.action)
    .execute(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!(
            user_id = %target_user_id,
            resource = %payload.resource,
            action = %payload.action,
            error = %e,
            "Failed to add permission policy"
        );
        ApiError::internal("Failed to add permission policy")
    })?;

    // Invalidate cache for the user
    let _ = permission_checker
        .invalidate_user_role_cache(&realm_id, &target_user_id.to_string())
        .await;

    tracing::info!(
        user_id = %target_user_id,
        resource = %payload.resource,
        action = %payload.action,
        realm_id = %realm_id,
        "Direct permission assigned to user successfully"
    );

    Ok(ApiResult::ok(()))
}

/// Remove direct permission from user
///
/// Removes a directly assigned permission from a user.
#[utoipa::path(
    delete,
    path = "/api/users/{realmId}/{userId}/permissions",
    tag = "users",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("userId" = Uuid, Path, description = "User ID")
    ),
    request_body = AssignPermissionRequest,
    responses(
        (status = 200, description = "Permission removed"),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn remove_user_permission(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path((realm_id, target_user_id)): Path<(String, Uuid)>,
    _headers: HeaderMap,
    Valid(Json(payload)): Valid<Json<AssignPermissionRequest>>,
) -> Result<ApiResult<()>, ApiError> {
    let admin = AdminIdentity::require(identity, &realm_id, "user permission management")?;

    tracing::info!(
        realm_id = %realm_id,
        target_user_id = %target_user_id,
        resource = %payload.resource,
        action = %payload.action,
        "Removing direct permission from user"
    );

    let permission_checker = &state.permission_checker;

    // Check policies.manage permission
    admin
        .require_permission(&state, "policies", "manage")
        .await?;

    // Security: the target user must exist in this realm, matching the
    // assignment path (policy rows are only ever removed for realm users).
    let target_exists =
        sqlx::query_scalar::<_, Uuid>("SELECT id FROM account WHERE id = $1 AND realm_id = $2")
            .bind(target_user_id)
            .bind(&realm_id)
            .fetch_optional(&state.pool)
            .await
            .map_err(|e| {
                tracing::error!(
                    realm_id = %realm_id,
                    target_user_id = %target_user_id,
                    error = %e,
                    "Failed to check target user for permission removal"
                );
                ApiError::internal("Failed to check target user")
            })?;
    if target_exists.is_none() {
        return Err(ApiError::not_found("User not found in this realm"));
    }

    // Delete the permission policy from role_policies table
    let result = sqlx::query(
        "DELETE FROM role_policies
         WHERE role_id = $1::uuid AND realm_id = $2 AND resource = $3 AND action = $4",
    )
    .bind(target_user_id)
    .bind(&realm_id)
    .bind(&payload.resource)
    .bind(&payload.action)
    .execute(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!(
            user_id = %target_user_id,
            resource = %payload.resource,
            action = %payload.action,
            error = %e,
            "Failed to remove permission policy"
        );
        ApiError::internal("Failed to remove permission policy")
    })?;

    if result.rows_affected() == 0 {
        tracing::debug!(
            user_id = %target_user_id,
            resource = %payload.resource,
            action = %payload.action,
            "Permission not found (already removed or never existed)"
        );
    }

    // Invalidate cache for the user
    let _ = permission_checker
        .invalidate_user_role_cache(&realm_id, &target_user_id.to_string())
        .await;

    tracing::info!(
        user_id = %target_user_id,
        resource = %payload.resource,
        action = %payload.action,
        realm_id = %realm_id,
        "Direct permission removed from user successfully"
    );

    Ok(ApiResult::ok(()))
}

/// Get user's effective permissions
///
/// Returns all permissions a user has, including:
/// - Permissions directly assigned to the user
/// - Permissions inherited from roles
///
/// Each permission includes its source (role name or "direct").
#[utoipa::path(
    get,
    path = "/api/users/{realmId}/{userId}/effective-permissions",
    tag = "users",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("userId" = Uuid, Path, description = "User ID")
    ),
    responses(
        (status = 200, description = "User effective permissions", body = EffectivePermissionsResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn get_effective_permissions(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path((realm_id, target_user_id)): Path<(String, Uuid)>,
    _headers: HeaderMap,
) -> Result<ApiResult<EffectivePermissionsResponse>, ApiError> {
    let admin = AdminIdentity::require(identity, &realm_id, "user permission management")?;
    admin.require_permission(&state, "users", "view").await?;

    let user_permission_service = &state.user_permission_service;

    // Call service layer to get effective permissions
    let permissions = user_permission_service
        .get_effective_permissions(admin.identity().clone(), &realm_id, target_user_id)
        .await
        .map_err(|e| match e {
            UserAdminError::PermissionDenied(msg) => ApiError::forbidden(msg),
            UserAdminError::UserNotFound(id) => {
                ApiError::not_found(format!("User not found: {}", id))
            }
            UserAdminError::DatabaseError(msg) => {
                tracing::error!(
                    realm_id = %realm_id,
                    user_id = %target_user_id,
                    error = %msg,
                    "Database error fetching effective permissions"
                );
                ApiError::internal(format!("Database error: {}", msg))
            }
            UserAdminError::InternalError(msg) => {
                tracing::error!(
                    realm_id = %realm_id,
                    user_id = %target_user_id,
                    error = %msg,
                    "Internal error fetching effective permissions"
                );
                ApiError::internal(msg)
            }
            _ => {
                tracing::error!(
                    realm_id = %realm_id,
                    user_id = %target_user_id,
                    error = ?e,
                    "Unexpected error fetching effective permissions"
                );
                ApiError::internal("Unexpected error")
            }
        })?;

    // Convert PermissionDetail to EffectivePermission
    let effective_permissions: Vec<EffectivePermission> = permissions
        .into_iter()
        .map(|p| {
            let (source, source_name) = match p.source {
                herald_core::domain::user::admin_dtos::PermissionSource::Role {
                    role_id: _,
                    role_name,
                } => ("role".to_string(), Some(role_name)),
                herald_core::domain::user::admin_dtos::PermissionSource::Direct => {
                    ("direct".to_string(), None)
                }
            };

            EffectivePermission {
                name: format!("{}.{}", p.resource, p.action),
                source,
                source_name,
            }
        })
        .collect();

    Ok(ApiResult::ok(EffectivePermissionsResponse {
        permissions: effective_permissions,
    }))
}
