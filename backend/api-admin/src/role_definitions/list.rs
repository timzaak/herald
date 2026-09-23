use crate::role_definitions::types::{ErrorResponse, RoleResponse};
use axum::{Extension, extract::State};
use herald_api_base::application::http::common::auth_utils::AdminIdentity;
use herald_api_base::application::http::server::api_entities::{ApiError, ApiResult};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::authentication::Identity;

/// List roles by realm_id for admin-web-console client
#[utoipa::path(
    get,
    path = "/api/roles/define",
    tag = "role-definitions",
    summary = "List roles in the realm",
    description = "List all role definitions for the admin-web-console client. Requires `roles.view` permission.",
        responses(
        (status = 200, description = "List of roles", body = Vec<RoleResponse>),
        (status = 403, description = "Forbidden - Insufficient permissions (requires roles.view)", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_roles(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
) -> Result<ApiResult<Vec<RoleResponse>>, ApiError> {
    let admin = AdminIdentity::require(identity, "role definitions")?;
    let realm_id = admin.realm_id().to_string();
    admin.require_permission(&state, "roles", "view").await?;
    // Use the client_id string directly (not the UUID)
    // roles.client_id stores the client identifier string (e.g., 'admin-web-console')
    let client_id = "admin-web-console";

    let rows = sqlx::query_as::<_, RoleResponse>(
        r#"
        SELECT id, name, description, realm_id, client_id, is_builtin
        FROM roles
        WHERE realm_id = $1 AND client_id = $2
        ORDER BY created_at DESC
        "#,
    )
    .bind(&realm_id)
    .bind(client_id)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!("Failed to list roles for realm {}: {e}", realm_id);
        ApiError::internal("Failed to list roles")
    })?;

    tracing::info!(
        "Listed {} roles for realm {} with client_id={}",
        rows.len(),
        realm_id,
        client_id
    );

    if rows.is_empty() {
        tracing::warn!(
            "No roles found for realm {} with client_id={}. \
             This may indicate RBAC initialization did not complete successfully.",
            realm_id,
            client_id
        );
    }

    Ok(ApiResult::ok(rows))
}
