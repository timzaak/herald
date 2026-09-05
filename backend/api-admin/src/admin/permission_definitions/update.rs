use axum::{
    Extension, Json,
    extract::{Path, State},
};
use axum_valid::Valid;
use herald_api_base::application::http::common::auth_utils::AdminIdentity;
use herald_core::domain::authentication::Identity;
use uuid::Uuid;

use crate::admin::permission_definitions::types::{
    ErrorResponse, PermissionResponse, PermissionUpdateRequest,
};
use herald_api_base::application::http::server::api_entities::{ApiError, ApiResult};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::audit::AuditAction;

/// Update permission
#[utoipa::path(
    put,
    path = "/api/permission/{realmId}/define/{permissionDefinitionId}",
    tag = "permission-definitions",
    summary = "Update a permission",
    description = "Update permission definition. Built-in permissions cannot be modified. Requires `permissions.manage` permission.",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("permissionDefinitionId" = Uuid, Path, description = "Permission ID")
    ),
    request_body = PermissionUpdateRequest,
    responses(
        (status = 200, description = "Permission updated", body = PermissionResponse),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 403, description = "Forbidden - Insufficient permissions (requires permissions.manage) or attempting to modify built-in permission", body = ErrorResponse),
        (status = 404, description = "Permission not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_permission(
    State(state): State<AppState>,
    Path((realm_id, id)): Path<(String, Uuid)>,
    Extension(identity): Extension<Identity>,
    Valid(Json(payload)): Valid<Json<PermissionUpdateRequest>>,
) -> Result<ApiResult<PermissionResponse>, ApiError> {
    let admin = AdminIdentity::require(identity, &realm_id, "permission definitions")?;
    admin
        .require_permission(&state, "permissions", "manage")
        .await?;
    // 从 name 中解析 resource 和 action
    // 格式: "resource.action" (如 "users.manage")
    let parts: Vec<&str> = payload.name.split('.').collect();
    if parts.len() != 2 {
        return Err(ApiError::bad_request(
            "Permission name must be in format 'resource.action' (e.g., 'users.manage')",
        ));
    }
    let resource = parts[0];
    let action = parts[1];

    let permission_check: Option<(bool,)> =
        sqlx::query_as("SELECT is_builtin FROM permissions WHERE id = $1 AND realm_id = $2")
            .bind(id)
            .bind(&realm_id)
            .fetch_optional(&state.pool)
            .await
            .map_err(|e| {
                tracing::error!("Failed to check permission: {e}");
                ApiError::internal("Failed to check permission")
            })?;

    if let Some((is_builtin,)) = permission_check
        && is_builtin
    {
        return Err(ApiError::forbidden(
            "Cannot modify built-in permission definition",
        ));
    }

    let row = sqlx::query_as::<_, PermissionResponse>(
        r#"
        UPDATE permissions
        SET name = $1, resource = $2, action = $3, description = $4, updated_at = CURRENT_TIMESTAMP
        WHERE id = $5 AND realm_id = $6
        RETURNING id, name, resource, action, description, realm_id, is_builtin
        "#,
    )
    .bind(&payload.name)
    .bind(resource)
    .bind(action)
    .bind(&payload.description)
    .bind(id)
    .bind(&realm_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!("Failed to update permission: {e}");
        if let sqlx::Error::Database(db_err) = &e
            && db_err.code().as_deref() == Some("23505")
        // PostgreSQL unique constraint violation
        {
            ApiError::bad_request("Permission name already exists in this realm")
        } else {
            ApiError::internal("Failed to update permission")
        }
    })?;

    let row = row.ok_or_else(|| ApiError::not_found("Permission not found"))?;

    // Record audit event (mirrors role-definitions update; permissions.md
    // [US-AU-005] requires permission-definition changes to be audited).
    super::record_permission_audit(
        &state,
        &admin,
        &realm_id,
        AuditAction::PermissionUpdate,
        row.id.to_string(),
        Some(row.name.clone()),
        Some(serde_json::json!({"name": row.name})),
    )
    .await;

    Ok(ApiResult::ok(row))
}
