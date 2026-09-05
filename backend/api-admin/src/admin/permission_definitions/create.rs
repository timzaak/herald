use axum::{
    Extension, Json,
    extract::{Path, State},
};
use axum_valid::Valid;
use herald_api_base::application::http::common::auth_utils::AdminIdentity;
use herald_core::domain::authentication::Identity;

use crate::admin::permission_definitions::types::{
    ErrorResponse, PermissionCreateRequest, PermissionResponse,
};
use herald_api_base::application::http::server::api_entities::{ApiError, ApiResult};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::audit::AuditAction;

/// Create a new permission
#[utoipa::path(
    post,
    path = "/api/permission/{realmId}/define",
    tag = "permission-definitions",
    operation_id = "create_permission_definition",
    summary = "Create a new permission",
    description = "Create a new permission definition. Permission name must be in format 'resource.action' (e.g., 'users.manage'). Requires `permissions.manage` permission.",
    params(
        ("realmId" = String, Path, description = "Realm ID")
    ),
    request_body = PermissionCreateRequest,
    responses(
        (status = 201, description = "Permission created", body = PermissionResponse),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 403, description = "Forbidden - Insufficient permissions (requires permissions.manage)", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_permission(
    Path(realm_id): Path<String>,
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Valid(Json(payload)): Valid<Json<PermissionCreateRequest>>,
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

    // 验证 resource 和 action 都不为空
    if resource.is_empty() || action.is_empty() {
        return Err(ApiError::bad_request(
            "Permission name must be in format 'resource.action' with both resource and action non-empty",
        ));
    }

    // Validate sensitive permissions can only be created in admin realm
    super::super::middleware::validate_sensitive_permission_creation(&payload.name, &realm_id)?;

    let row = sqlx::query_as::<_, PermissionResponse>(
        r#"
        INSERT INTO permissions (name, resource, action, description, realm_id, is_builtin)
        VALUES ($1, $2, $3, $4, $5, false)
        RETURNING id, name, resource, action, description, realm_id, is_builtin
        "#,
    )
    .bind(&payload.name)
    .bind(resource)
    .bind(action)
    .bind(&payload.description)
    .bind(&realm_id)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!("Failed to create permission: {e}");
        if let sqlx::Error::Database(db_err) = &e
            && db_err.code().as_deref() == Some("23505")
        // PostgreSQL unique constraint violation
        {
            ApiError::bad_request("Permission name already exists in this realm")
        } else {
            ApiError::internal("Failed to create permission")
        }
    })?;

    // Record audit event (mirrors role-definitions create; permissions.md
    // [US-AU-005] requires permission-definition changes to be audited).
    super::record_permission_audit(
        &state,
        &admin,
        &realm_id,
        AuditAction::PermissionCreate,
        row.id.to_string(),
        Some(row.name.clone()),
        Some(serde_json::json!({"name": row.name})),
    )
    .await;

    Ok(ApiResult::created(row))
}
