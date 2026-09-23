use crate::role_definitions::types::{ErrorResponse, RoleResponse};
use axum::{
    Extension,
    extract::{Path, State},
};
use herald_api_base::application::http::common::auth_utils::AdminIdentity;
use herald_api_base::application::http::server::api_entities::{ApiError, ApiResult};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::authentication::Identity;
use uuid::Uuid;

/// Get role by ID
#[utoipa::path(
    get,
    path = "/api/roles/define/{roleId}",
    tag = "role-definitions",
    params(
        ("roleId" = Uuid, Path, description = "Role ID")
    ),
    responses(
        (status = 200, description = "Role found", body = RoleResponse),
        (status = 403, description = "Forbidden - Insufficient permissions", body = ErrorResponse),
        (status = 404, description = "Role not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn get_role(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(id): Path<Uuid>,
) -> Result<ApiResult<RoleResponse>, ApiError> {
    let admin = AdminIdentity::require(identity, "role definitions")?;
    let realm_id = admin.realm_id().to_string();
    admin.require_permission(&state, "roles", "view").await?;
    let row = sqlx::query_as::<_, RoleResponse>(
        r#"
        SELECT id, name, description, realm_id, client_id, is_builtin
        FROM roles
        WHERE id = $1 AND realm_id = $2
        "#,
    )
    .bind(id)
    .bind(&realm_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!("Failed to get role: {e}");
        ApiError::internal("Failed to get role")
    })?;

    let row = row.ok_or_else(|| ApiError::not_found("Role not found"))?;

    Ok(ApiResult::ok(row))
}
