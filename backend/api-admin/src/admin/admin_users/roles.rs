use crate::admin::admin_users::types::{
    ErrorResponse, UpdateUserRolesRequest, UserRoleDetail, UserRolesResponse,
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
use herald_core::domain::user::admin_errors::UserAdminError;
use herald_core::domain::user::admin_ports::RoleAssignmentService;
use uuid::Uuid;

/// Get user roles
///
/// Returns the list of roles assigned to a specific user.
#[utoipa::path(
    get,
    path = "/api/users/{userId}/roles",
    tag = "users",
    operation_id = "adminGetUserRoles",
    params(
        ("userId" = Uuid, Path, description = "User ID")
    ),
    responses(
        (status = 200, description = "User roles", body = UserRolesResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 404, description = "User not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn get_user_roles(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(user_id): Path<Uuid>,
    _headers: HeaderMap,
) -> Result<ApiResult<UserRolesResponse>, ApiError> {
    let admin = AdminIdentity::require(identity, "user role management")?;
    let realm_id = admin.realm_id().to_string();
    admin.require_permission(&state, "users", "view").await?;

    let role_assignment_service = &state.role_assignment_service;

    let roles = role_assignment_service
        .get_user_roles(admin.identity().clone(), &realm_id, user_id)
        .await
        .map_err(|e| match e {
            UserAdminError::PermissionDenied(msg) => ApiError::forbidden(msg),
            UserAdminError::UserNotFound(id) => {
                ApiError::not_found(format!("User not found: {}", id))
            }
            UserAdminError::DatabaseError(msg) => {
                ApiError::internal(format!("Database error: {}", msg))
            }
            UserAdminError::InternalError(msg) => ApiError::internal(msg),
            _ => ApiError::internal("Unexpected error"),
        })?;

    // Convert RoleDetail to UserRoleDetail
    let role_details: Vec<UserRoleDetail> = roles
        .into_iter()
        .map(|r| UserRoleDetail {
            id: r.id,
            name: r.name,
            description: r.description,
            source: r.source,
            source_id: r.source_id,
            expires_at: r.expires_at,
        })
        .collect();

    Ok(ApiResult::ok(UserRolesResponse {
        roles: role_details,
    }))
}

/// Update user roles
///
/// Updates the roles assigned to a user. Replaces all existing roles with the new list.
///
/// **Security constraints**:
/// - Realm-admin can only assign "user" role, not "realm-admin"
#[utoipa::path(
    put,
    path = "/api/users/{userId}/roles",
    tag = "users",
    params(
        ("userId" = Uuid, Path, description = "User ID")
    ),
    request_body = UpdateUserRolesRequest,
    responses(
        (status = 200, description = "Roles updated"),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden - cannot assign admin roles", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn update_user_roles(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(target_user_id): Path<Uuid>,
    _headers: HeaderMap,
    Valid(Json(payload)): Valid<Json<UpdateUserRolesRequest>>,
) -> Result<ApiResult<()>, ApiError> {
    let admin = AdminIdentity::require(identity, "user role management")?;
    let realm_id = admin.realm_id().to_string();
    admin.require_permission(&state, "roles", "manage").await?;

    let role_assignment_service = &state.role_assignment_service;

    role_assignment_service
        .assign_user_roles(
            admin.identity().clone(),
            &realm_id,
            target_user_id,
            payload.role_ids,
        )
        .await
        .map_err(|e| match e {
            UserAdminError::PermissionDenied(msg) => ApiError::forbidden(msg),
            UserAdminError::RoleNotFound(id) => {
                ApiError::bad_request(format!("Role not found: {}", id))
            }
            UserAdminError::InvalidRoleAssignment(msg) => {
                ApiError::bad_request(format!("Invalid role assignment: {}", msg))
            }
            UserAdminError::DatabaseError(msg) => {
                ApiError::internal(format!("Database error: {}", msg))
            }
            UserAdminError::InternalError(msg) => ApiError::internal(msg),
            _ => ApiError::internal("Unexpected error"),
        })?;

    Ok(ApiResult::ok(()))
}
