use axum::{Extension, extract::Path, extract::State};
use herald_api_base::application::http::common::auth_utils::AdminIdentity;
use herald_core::domain::authentication::Identity;
use herald_core::domain::authorization::PermissionService;
use uuid::Uuid;

use super::types::ErrorResponse;
use herald_api_base::application::http::server::api_entities::{ApiError, ApiResult};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::audit::AuditAction;

/// Delete permission
#[utoipa::path(
    delete,
    path = "/api/permission/{realmId}/define/{permissionDefinitionId}",
    tag = "permission-definitions",
    summary = "Delete a permission",
    description = "Delete a permission definition. Built-in permissions cannot be deleted. Requires `permissions.manage` permission.",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("permissionDefinitionId" = Uuid, Path, description = "Permission ID")
    ),
    responses(
        (status = 204, description = "Permission deleted"),
        (status = 403, description = "Forbidden - Insufficient permissions (requires permissions.manage) or attempting to delete built-in permission", body = ErrorResponse),
        (status = 404, description = "Permission not found", body = ErrorResponse),
        (status = 409, description = "Conflict - Permission is assigned to roles and cannot be deleted", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn delete_permission(
    State(state): State<AppState>,
    Path((realm_id, id)): Path<(String, Uuid)>,
    Extension(identity): Extension<Identity>,
) -> Result<ApiResult<()>, ApiError> {
    let admin = AdminIdentity::require(identity, &realm_id, "permission definitions")?;
    admin
        .require_permission(&state, "permissions", "manage")
        .await?;

    // 3. Check if permission is built-in
    let permission: Option<(bool, String, String)> = sqlx::query_as(
        "SELECT is_builtin, name, realm_id FROM permissions WHERE id = $1 AND realm_id = $2",
    )
    .bind(id)
    .bind(&realm_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!("Failed to check permission: {e}");
        ApiError::internal("Failed to check permission")
    })?;

    let permission_realm_id = match permission {
        Some((is_builtin, permission_name, realm_id)) => {
            if is_builtin {
                tracing::warn!(
                    user_id = %admin.user_id_string(),
                    permission_id = %id,
                    permission_name = %permission_name,
                    "Attempted to delete built-in permission"
                );
                return Err(ApiError::forbidden("Cannot delete built-in permission"));
            }
            realm_id
        }
        None => {
            return Err(ApiError::not_found("Permission not found"));
        }
    };

    // Check if permission is assigned to any roles
    let permission_in_use: Option<(bool,)> =
        sqlx::query_as("SELECT EXISTS(SELECT 1 FROM role_permissions WHERE permission_id = $1)")
            .bind(id)
            .fetch_optional(&state.pool)
            .await
            .map_err(|e| {
                tracing::error!("Failed to check permission usage: {e}");
                ApiError::internal("Failed to check permission usage")
            })?;

    if matches!(permission_in_use, Some((true,))) {
        return Err(ApiError::conflict(
            "Cannot delete permission that is assigned to roles",
        ));
    }

    // 4. Execute deletion
    let result = sqlx::query("DELETE FROM permissions WHERE id = $1 AND realm_id = $2")
        .bind(id)
        .bind(&permission_realm_id)
        .execute(&state.pool)
        .await
        .map_err(|e| {
            tracing::error!("Failed to delete permission: {e}");
            ApiError::internal("Failed to delete permission")
        })?;

    if result.rows_affected() == 0 {
        return Err(ApiError::not_found("Permission not found"));
    }

    let _ = state
        .permission_checker
        .invalidate_realm_cache(&permission_realm_id)
        .await;

    // Record audit event (mirrors role-definitions delete; permissions.md
    // [US-AU-005] requires permission-definition changes to be audited).
    super::record_permission_audit(
        &state,
        &admin,
        &permission_realm_id,
        AuditAction::PermissionDelete,
        id.to_string(),
        None,
        None,
    )
    .await;

    Ok(ApiResult::no_content())
}
