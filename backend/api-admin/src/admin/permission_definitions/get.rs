use axum::{
    Extension,
    extract::{Path, State},
};
use herald_api_base::application::http::common::auth_utils::AdminIdentity;
use herald_core::domain::authentication::Identity;
use uuid::Uuid;

use crate::admin::permission_definitions::types::{ErrorResponse, PermissionResponse};
use herald_api_base::application::http::server::api_entities::{ApiError, ApiResult};
use herald_api_base::application::http::state::AppState;

/// Get permission by ID
#[utoipa::path(
    get,
    path = "/api/permission/define/{permissionDefinitionId}",
    tag = "permission-definitions",
    params(
        ("permissionDefinitionId" = Uuid, Path, description = "Permission ID")
    ),
    responses(
        (status = 200, description = "Permission found", body = PermissionResponse),
        (status = 404, description = "Permission not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn get_permission(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(id): Path<Uuid>,
) -> Result<ApiResult<PermissionResponse>, ApiError> {
    let admin = AdminIdentity::require(identity, "permission definitions")?;
    let realm_id = admin.realm_id().to_string();
    admin
        .require_permission(&state, "permissions", "view")
        .await?;

    let row = sqlx::query_as::<_, PermissionResponse>(
        r#"
        SELECT id, name, resource, action, description, realm_id, is_builtin
        FROM permissions
        WHERE id = $1 AND realm_id = $2
        "#,
    )
    .bind(id)
    .bind(&realm_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!("Failed to get permission: {e}");
        ApiError::internal("Failed to get permission")
    })?;

    let row = row.ok_or_else(|| ApiError::not_found("Permission not found"))?;

    Ok(ApiResult::ok(row))
}
