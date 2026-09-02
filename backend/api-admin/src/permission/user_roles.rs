// =============================================================================
// User Roles Management API
// =============================================================================
//
// Provides endpoints for managing user role assignments
// GET /api/permission/users/{userId}/roles - Get user's roles
// POST /api/permission/users/{userId}/roles - Assign roles to user
// DELETE /api/permission/users/{userId}/roles/{roleId} - Remove role from user
//
// =============================================================================

use axum::{
    Json,
    extract::{Extension, Path, State},
};
use sea_orm::{ActiveModelTrait, ActiveValue, ColumnTrait, EntityTrait, QueryFilter};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use utoipa::ToSchema;
use uuid::Uuid;
use validator::Validate;

use herald_api_base::application::http::common::{auth_utils::AdminIdentity, validation};
use herald_api_base::application::http::server::api_entities::{ApiError, ApiResult};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::audit::{
    ActorType, AuditAction, AuditCategory, AuditEventRepository, AuditResult, AuditTargetType,
    NewAuditEvent,
};
use herald_core::domain::authentication::Identity;
use herald_core::domain::authorization::permission_service::PermissionService;
use herald_core::domain::authorization::principal_types;
use herald_core::entity::{account, role_policies, roles, user_roles};

pub use herald_api_base::application::http::server::api_entities::ErrorResponse;

// ============================================================================
// Request/Response Types
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RoleResponse {
    pub id: Uuid,
    pub name: String,
    pub realm_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserRolesResponse {
    pub roles: Vec<RoleResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Validate)]
#[serde(rename_all = "camelCase")]
pub struct AssignRolesRequest {
    #[validate(length(min = 1))]
    pub role_ids: Vec<Uuid>,
}

// ============================================================================
// Handlers
// ============================================================================

/// Get user's roles
///
/// Returns all roles assigned to a specific user
#[utoipa::path(
    get,
    path = "/api/permission/users/{userId}/roles",
    params(
        ("userId" = Uuid, Path, description = "User ID")
    ),
    responses(
        (status = 200, description = "User's roles retrieved successfully", body = UserRolesResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "User not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "permission"
)]
pub async fn get_user_roles(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(user_id): Path<Uuid>,
) -> Result<ApiResult<UserRolesResponse>, ApiError> {
    let user = account::Entity::find()
        .filter(account::Column::Id.eq(user_id))
        .one(state.db.as_ref())
        .await
        .map_err(|e| {
            tracing::error!(error = %e, user_id = %user_id, "Failed to query user");
            ApiError::internal(format!("Failed to query user: {}", e))
        })?
        .ok_or_else(|| ApiError::not_found("User not found"))?;

    let realm_id = user
        .realm_id
        .as_ref()
        .ok_or_else(|| ApiError::bad_request("User has no realm"))?;

    let admin = AdminIdentity::require(identity, realm_id, "user roles")?;
    admin.require_permission(&state, "users", "view").await?;

    // Query user_roles with join to roles table
    let user_roles_data = user_roles::Entity::find()
        .filter(user_roles::Column::UserId.eq(user_id))
        .filter(user_roles::Column::RealmId.eq(realm_id.clone()))
        .find_also_related(roles::Entity)
        .all(state.db.as_ref())
        .await
        .map_err(|e| {
            tracing::error!(error = %e, user_id = %user_id, "Failed to query user roles");
            ApiError::internal(format!("Failed to query user roles: {}", e))
        })?;

    let roles: Vec<RoleResponse> = user_roles_data
        .into_iter()
        .filter_map(|(_user_role, role_opt)| {
            role_opt.map(|role| RoleResponse {
                id: role.id,
                name: role.name,
                realm_id: role.realm_id,
            })
        })
        .collect();

    Ok(ApiResult::ok(UserRolesResponse { roles }))
}

/// Assign roles to user
///
/// Assigns one or more roles to a user
#[utoipa::path(
    post,
    path = "/api/permission/users/{userId}/roles",
    params(
        ("userId" = Uuid, Path, description = "User ID")
    ),
    request_body = AssignRolesRequest,
    responses(
        (status = 201, description = "Roles assigned successfully", body = UserRolesResponse),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "User or role not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "permission"
)]
pub async fn assign_roles_to_user(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(user_id): Path<Uuid>,
    Json(request): Json<AssignRolesRequest>,
) -> Result<ApiResult<UserRolesResponse>, ApiError> {
    // Validate request
    if let Err(errors) = request.validate() {
        return Err(ApiError::bad_request(format!(
            "Invalid request: {}",
            errors
        )));
    }

    // Get user's realm_id
    let user = account::Entity::find()
        .filter(account::Column::Id.eq(user_id))
        .one(state.db.as_ref())
        .await
        .map_err(|e| {
            tracing::error!(error = %e, user_id = %user_id, "Failed to query user");
            ApiError::internal(format!("Failed to query user: {}", e))
        })?
        .ok_or_else(|| ApiError::not_found("User not found"))?;

    let realm_id = user
        .realm_id
        .as_ref()
        .ok_or_else(|| ApiError::bad_request("User has no realm"))?;

    let admin = AdminIdentity::require(identity.clone(), realm_id, "user roles")?;
    admin.require_permission(&state, "roles", "manage").await?;

    let mut seen_role_ids = HashSet::new();
    let unique_role_ids: Vec<Uuid> = request
        .role_ids
        .iter()
        .copied()
        .filter(|role_id| seen_role_ids.insert(*role_id))
        .collect();

    let matching_roles = roles::Entity::find()
        .filter(roles::Column::RealmId.eq(realm_id.clone()))
        .filter(roles::Column::Id.is_in(unique_role_ids.clone()))
        .all(state.db.as_ref())
        .await
        .map_err(|e| {
            tracing::error!(error = %e, realm_id = %realm_id, "Failed to query roles");
            ApiError::internal(format!("Failed to query roles: {}", e))
        })?;

    if matching_roles.len() != unique_role_ids.len() {
        return Err(ApiError::bad_request(
            "One or more roles do not exist in the target realm",
        ));
    }

    // Security: a delegated roles.manage holder must not reach primary-admin
    // level by assigning a privileged builtin role (e.g. realm-admin) to
    // themselves. Assigning such a role requires holding every permission it
    // grants — only satisfied by callers already at that level. The plain
    // builtin "user" role is exempt (it is the default end-user role).
    for role in &matching_roles {
        if !role.is_builtin || role.name == "user" {
            continue;
        }
        let role_policies = role_policies::Entity::find()
            .filter(role_policies::Column::RealmId.eq(realm_id.clone()))
            .filter(role_policies::Column::RoleId.eq(role.id))
            .all(state.db.as_ref())
            .await
            .map_err(|e| {
                tracing::error!(error = %e, role_id = %role.id, "Failed to query role policies");
                ApiError::internal(format!("Failed to query role policies: {}", e))
            })?;
        for policy in &role_policies {
            admin
                .require_permission(&state, &policy.resource, &policy.action)
                .await?;
        }
    }

    // Assign each role
    for role_id in &unique_role_ids {
        let user_role = user_roles::ActiveModel {
            id: ActiveValue::Set(Uuid::now_v7()),
            realm_id: ActiveValue::Set(realm_id.clone()),
            user_id: ActiveValue::Set(Some(user_id)),
            role_id: ActiveValue::Set(*role_id),
            client_id: ActiveValue::Set(Some(identity.client_id())),
            principal_type: ActiveValue::Set(principal_types::USER.to_string()),
            principal_id: ActiveValue::Set(user_id.to_string()),
            // Admin-assign path is a manual grant (no payment
            // origin, no subscription expiry).
            source: ActiveValue::Set("manual".to_string()),
            source_id: ActiveValue::Set(None),
            expires_at: ActiveValue::Set(None),
            created_at: ActiveValue::Set(chrono::Utc::now().into()),
        };

        // Try to insert
        match user_role.insert(state.db.as_ref()).await {
            Ok(_) => {
                tracing::debug!(user_id = %user_id, role_id = %role_id, "Role assigned to user");
            }
            Err(e) => {
                // Check if this is a duplicate key error (idempotent)
                if validation::is_duplicate_key_error(&e) {
                    tracing::debug!(
                        user_id = %user_id,
                        role_id = %role_id,
                        "Role already assigned (idempotent)"
                    );
                } else {
                    tracing::error!(
                        error = %e,
                        user_id = %user_id,
                        role_id = %role_id,
                        error_type = %std::any::type_name::<sea_orm::DbErr>(),
                        "Failed to assign role to user"
                    );
                    return Err(ApiError::internal(format!("Failed to assign role: {}", e)));
                }
            }
        }
    }

    // Invalidate user role cache
    let _ = state
        .permission_checker
        .invalidate_user_role_cache(realm_id, &user_id.to_string())
        .await;

    // Query and return updated user roles list
    let updated_roles = user_roles::Entity::find()
        .filter(user_roles::Column::UserId.eq(user_id))
        .find_also_related(roles::Entity)
        .all(state.db.as_ref())
        .await
        .map_err(|e| {
            tracing::error!(error = %e, user_id = %user_id, "Failed to query updated user roles");
            ApiError::internal(format!("Failed to query updated user roles: {}", e))
        })?;

    let roles: Vec<RoleResponse> = updated_roles
        .into_iter()
        .filter_map(|(_user_role, role_opt)| {
            role_opt.map(|role| RoleResponse {
                id: role.id,
                name: role.name,
                realm_id: role.realm_id,
            })
        })
        .collect();

    tracing::info!(
        user_id = %user_id,
        role_ids = ?request.role_ids,
        total_roles = roles.len(),
        "Roles assigned to user"
    );

    if let Err(e) = state
        .audit_event_repository
        .create(NewAuditEvent {
            realm_id: realm_id.clone(),
            category: AuditCategory::Rbac,
            action: AuditAction::RoleAssign,
            actor_id: identity.user_id().to_string(),
            actor_type: Some(ActorType::Admin),
            actor_name: identity.as_user().map(|u| u.email.clone()),
            target_type: AuditTargetType::User,
            target_id: user_id.to_string(),
            target_name: None,
            result: AuditResult::Success,
            details: Some(serde_json::json!({"role_ids": request.role_ids})),
            ip_address: None,
            user_agent: None,
            trace_id: None,
        })
        .await
    {
        tracing::warn!(error = %e, "Failed to record audit event");
    }

    Ok(ApiResult::created(UserRolesResponse { roles }))
}

/// Remove role from user
///
/// Removes a specific role from a user
#[utoipa::path(
    delete,
    path = "/api/permission/users/{userId}/roles/{roleId}",
    params(
        ("userId" = Uuid, Path, description = "User ID"),
        ("roleId" = Uuid, Path, description = "Role ID")
    ),
    responses(
        (status = 204, description = "Role removed successfully"),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "User, role, or assignment not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "permission"
)]
pub async fn remove_role_from_user(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path((user_id, role_id)): Path<(Uuid, Uuid)>,
) -> Result<ApiResult<()>, ApiError> {
    let user = account::Entity::find()
        .filter(account::Column::Id.eq(user_id))
        .one(state.db.as_ref())
        .await
        .map_err(|e| {
            tracing::error!(error = %e, user_id = %user_id, "Failed to query user");
            ApiError::internal(format!("Failed to query user: {}", e))
        })?
        .ok_or_else(|| ApiError::not_found("User not found"))?;

    let realm_id = user
        .realm_id
        .as_ref()
        .ok_or_else(|| ApiError::bad_request("User has no realm"))?;

    let admin = AdminIdentity::require(identity.clone(), realm_id, "user roles")?;
    admin.require_permission(&state, "roles", "manage").await?;

    // Find and delete the user_role assignment
    let result = user_roles::Entity::delete_many()
        .filter(user_roles::Column::UserId.eq(user_id))
        .filter(user_roles::Column::RoleId.eq(role_id))
        .filter(user_roles::Column::RealmId.eq(realm_id.clone()))
        .exec(state.db.as_ref())
        .await
        .map_err(|e| {
            tracing::error!(
                error = %e,
                user_id = %user_id,
                role_id = %role_id,
                "Failed to remove role from user"
            );
            ApiError::internal(format!("Failed to remove role: {}", e))
        })?;

    if result.rows_affected == 0 {
        return Err(ApiError::not_found("User role assignment not found"));
    }

    // Invalidate user role cache
    invalidate_user_role_cache(&state, &user_id.to_string()).await;

    tracing::info!(
        user_id = %user_id,
        role_id = %role_id,
        "Role removed from user"
    );

    if let Err(e) = state
        .audit_event_repository
        .create(NewAuditEvent {
            realm_id: realm_id.clone(),
            category: AuditCategory::Rbac,
            action: AuditAction::RoleUnassign,
            actor_id: identity.user_id().to_string(),
            actor_type: Some(ActorType::Admin),
            actor_name: identity.as_user().map(|u| u.email.clone()),
            target_type: AuditTargetType::User,
            target_id: user_id.to_string(),
            target_name: None,
            result: AuditResult::Success,
            details: Some(serde_json::json!({"role_id": role_id})),
            ip_address: None,
            user_agent: None,
            trace_id: None,
        })
        .await
    {
        tracing::warn!(error = %e, "Failed to record audit event");
    }

    Ok(ApiResult::no_content())
}

// ============================================================================
// Router
// ============================================================================

pub fn router() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/{userId}/roles", axum::routing::get(get_user_roles))
        .route("/{userId}/roles", axum::routing::post(assign_roles_to_user))
        .route(
            "/{userId}/roles/{roleId}",
            axum::routing::delete(remove_role_from_user),
        )
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Invalidate user role cache after role changes
///
/// This helper function reduces nesting by extracting the cache invalidation logic
async fn invalidate_user_role_cache(state: &AppState, user_id: &str) {
    // Parse user_id once, reuse for both query and cache invalidation
    if let Ok(uuid) = Uuid::parse_str(user_id)
        && let Ok(Some(user)) = account::Entity::find()
            .filter(account::Column::Id.eq(uuid))
            .one(state.db.as_ref())
            .await
        && let Some(realm_id) = user.realm_id
    {
        let _ = state
            .permission_checker
            .invalidate_user_role_cache(&realm_id, user_id)
            .await;
    }
}
