use axum::Json;
use axum::extract::{Extension, Path, State};
use axum_valid::Valid;
use herald_api_base::application::http::common::auth_utils::AdminIdentity;
use herald_api_base::application::http::server::api_entities::{ApiError, ApiResult};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::authentication::Identity;
use herald_core::domain::user::admin_errors::UserAdminError;
use herald_core::domain::user::admin_ports::RoleAssignmentService;

use crate::api_keys::types::{ApiKeyRoleDetail, ApiKeyRolesResponse, UpdateApiKeyRolesRequest};

/// Get API Key roles
///
/// Returns the list of roles assigned to a specific API Key.
#[utoipa::path(
    get,
    path = "/api/api-keys/{apiKeyId}/roles",
    tag = "api-keys",
    operation_id = "adminGetApiKeyRoles",
    params(
        ("apiKeyId" = String, Path, description = "API Key ID")
    ),
    responses(
        (status = 200, description = "API Key roles", body = ApiKeyRolesResponse),
        (status = 401, description = "Unauthorized", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 403, description = "Forbidden", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 404, description = "API Key not found", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 500, description = "Internal server error", body = herald_api_base::application::http::server::api_entities::ErrorResponse)
    )
)]
pub async fn get_api_key_roles(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(api_key_id): Path<String>,
) -> Result<ApiResult<ApiKeyRolesResponse>, ApiError> {
    let admin = AdminIdentity::require(identity, "api keys")?;
    let realm_id = admin.realm_id().to_string();
    admin.require_permission(&state, "api_keys", "view").await?;

    // Verify API Key exists and belongs to realm
    let api_key = state
        .api_key_repo
        .find_by_id(&api_key_id)
        .await
        .map_err(|e| {
            tracing::error!("Failed to get API key: {e}");
            ApiError::internal("Failed to get API key")
        })?
        .ok_or_else(|| ApiError::not_found("API key not found"))?;

    if api_key.realm_id != realm_id {
        return Err(ApiError::not_found("API key not found"));
    }

    let roles = state
        .role_assignment_service
        .get_api_key_roles(admin.identity().clone(), &realm_id, &api_key_id)
        .await
        .map_err(map_admin_error)?;

    let role_details: Vec<ApiKeyRoleDetail> = roles
        .into_iter()
        .map(|r| ApiKeyRoleDetail {
            id: r.id,
            name: r.name,
            description: r.description,
        })
        .collect();

    Ok(ApiResult::ok(ApiKeyRolesResponse {
        roles: role_details,
    }))
}

/// Update API Key roles
///
/// Replaces all roles assigned to an API Key. Empty array clears all roles.
/// Builtin roles cannot be assigned to API Keys (returns 400).
#[utoipa::path(
    put,
    path = "/api/api-keys/{apiKeyId}/roles",
    tag = "api-keys",
    operation_id = "adminUpdateApiKeyRoles",
    params(
        ("apiKeyId" = String, Path, description = "API Key ID")
    ),
    request_body = UpdateApiKeyRolesRequest,
    responses(
        (status = 200, description = "Roles updated"),
        (status = 400, description = "Bad request - builtin role or role not found", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 401, description = "Unauthorized", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 403, description = "Forbidden", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 404, description = "API Key not found", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 500, description = "Internal server error", body = herald_api_base::application::http::server::api_entities::ErrorResponse)
    )
)]
pub async fn update_api_key_roles(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(api_key_id): Path<String>,
    Valid(Json(payload)): Valid<axum::Json<UpdateApiKeyRolesRequest>>,
) -> Result<ApiResult<()>, ApiError> {
    let admin = AdminIdentity::require(identity, "api keys")?;
    let realm_id = admin.realm_id().to_string();
    admin.require_permission(&state, "roles", "manage").await?;

    // Verify API Key exists and belongs to realm
    let api_key = state
        .api_key_repo
        .find_by_id(&api_key_id)
        .await
        .map_err(|e| {
            tracing::error!("Failed to get API key: {e}");
            ApiError::internal("Failed to get API key")
        })?
        .ok_or_else(|| ApiError::not_found("API key not found"))?;

    if api_key.realm_id != realm_id {
        return Err(ApiError::not_found("API key not found"));
    }

    state
        .role_assignment_service
        .assign_api_key_roles(
            admin.identity().clone(),
            &realm_id,
            &api_key_id,
            payload.role_ids,
        )
        .await
        .map_err(map_admin_error)?;

    Ok(ApiResult::ok(()))
}

fn map_admin_error(e: UserAdminError) -> ApiError {
    match e {
        UserAdminError::PermissionDenied(msg) => ApiError::forbidden(msg),
        UserAdminError::RoleNotFound(id) => {
            ApiError::bad_request(format!("Role not found: {}", id))
        }
        UserAdminError::InvalidRoleAssignment(msg) => ApiError::bad_request(msg),
        UserAdminError::DatabaseError(msg) => {
            ApiError::internal(format!("Database error: {}", msg))
        }
        UserAdminError::InternalError(msg) => ApiError::internal(msg),
        _ => ApiError::internal("Unexpected error"),
    }
}
