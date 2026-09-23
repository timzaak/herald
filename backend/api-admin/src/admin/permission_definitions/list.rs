use axum::{Extension, extract::State};
use herald_api_base::application::http::common::auth_utils::AdminIdentity;
use herald_core::domain::authentication::Identity;

use crate::admin::permission_definitions::types::{ErrorResponse, PermissionResponse};
use herald_api_base::application::http::server::api_entities::{ApiError, ApiResult};
use herald_api_base::application::http::state::AppState;

/// List permissions by realm_id
#[utoipa::path(
    get,
    path = "/api/permission/define",
    tag = "permission-definitions",
    summary = "List permissions in the realm",
    description = "List all permission definitions in the realm. Requires `permissions.view` permission.",
        responses(
        (status = 200, description = "List of permissions", body = Vec<PermissionResponse>),
        (status = 403, description = "Forbidden - Insufficient permissions (requires permissions.view)", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_permissions(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
) -> Result<ApiResult<Vec<PermissionResponse>>, ApiError> {
    let admin = AdminIdentity::require(identity, "permission definitions")?;
    let realm_id = admin.realm_id().to_string();
    admin
        .require_permission(&state, "permissions", "view")
        .await?;

    let rows = sqlx::query_as::<_, PermissionResponse>(
        r#"
        SELECT id, name, resource, action, description, realm_id, is_builtin
        FROM permissions
        WHERE realm_id = $1
        ORDER BY created_at DESC
        "#,
    )
    .bind(&realm_id)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!("Failed to list permissions: {e}");
        ApiError::internal("Failed to list permissions")
    })?;

    Ok(ApiResult::ok(rows))
}
