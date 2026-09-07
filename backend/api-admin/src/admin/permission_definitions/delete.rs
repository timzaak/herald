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

    let Some(permission) = super::fetch_permission_definition(&state, id, &realm_id).await? else {
        return Err(ApiError::not_found("Permission not found"));
    };

    if permission.is_builtin {
        tracing::warn!(
            user_id = %admin.user_id_string(),
            permission_id = %id,
            permission_name = %permission.name,
            "Attempted to delete built-in permission"
        );
        return Err(ApiError::forbidden("Cannot delete built-in permission"));
    }

    // Refuse while any role still references the permission — via a
    // role_permissions assignment or a role_policies runtime mirror (same
    // guard as the update path).
    if super::permission_in_use(
        &state,
        id,
        &realm_id,
        &permission.resource,
        &permission.action,
    )
    .await?
    {
        return Err(ApiError::conflict(
            "Cannot delete permission that is assigned to roles",
        ));
    }

    let result = sqlx::query("DELETE FROM permissions WHERE id = $1 AND realm_id = $2")
        .bind(id)
        .bind(&realm_id)
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
        .invalidate_realm_cache(&realm_id)
        .await;

    // Record audit event (mirrors role-definitions delete; permissions.md
    // [US-AU-005] requires permission-definition changes to be audited).
    super::record_permission_audit(
        &state,
        &admin,
        &realm_id,
        AuditAction::PermissionDelete,
        id.to_string(),
        None,
        None,
    )
    .await;

    Ok(ApiResult::no_content())
}
