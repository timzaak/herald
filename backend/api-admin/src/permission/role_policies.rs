// =============================================================================
// Role Policies Management API
// =============================================================================
//
// Provides endpoints for managing role permission policies
// GET /api/permission/roles/{roleId}/policies - Get role's policies
// POST /api/permission/roles/{roleId}/policies - Add policy to role
// DELETE /api/permission/roles/{roleId}/policies/{policyId} - Remove policy from role
//
// =============================================================================

use axum::{
    Extension,
    extract::{Path, State},
};
use herald_core::domain::authentication::Identity;
use sea_orm::{ActiveModelTrait, ActiveValue, ColumnTrait, EntityTrait, QueryFilter};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;
use validator::Validate;

use herald_api_base::application::http::common::auth_utils::AdminIdentity;
use herald_api_base::application::http::common::validation;
use herald_api_base::application::http::server::api_entities::{ApiError, ApiResult};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::authorization::permission_service::PermissionService;
use herald_core::entity::role_policies;

// ============================================================================
// Request/Response Types
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PolicyResponse {
    pub id: Uuid,
    pub resource: String,
    pub action: String,
    pub realm_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RolePoliciesResponse {
    pub policies: Vec<PolicyResponse>,
}

pub use herald_api_base::application::http::server::api_entities::ErrorResponse;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Validate)]
pub struct AddPolicyRequest {
    #[validate(length(min = 1))]
    pub resource: String,
    #[validate(length(min = 1))]
    pub action: String,
}

// ============================================================================
// Handlers
// ============================================================================

/// Get role's policies
///
/// Returns all permission policies for a specific role
#[utoipa::path(
    get,
    path = "/api/permission/roles/{roleId}/policies",
    params(
        ("roleId" = Uuid, Path, description = "Role ID")
    ),
    responses(
        (status = 200, description = "Role policies retrieved successfully", body = RolePoliciesResponse),
        (status = 403, description = "Forbidden - Insufficient permissions", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "permission"
)]
pub async fn get_role_policies(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(role_uuid): Path<Uuid>,
) -> Result<ApiResult<RolePoliciesResponse>, ApiError> {
    // First, get the role to determine realm_id
    let role = herald_core::entity::roles::Entity::find()
        .filter(herald_core::entity::roles::Column::Id.eq(role_uuid))
        .one(state.db.as_ref())
        .await
        .map_err(|e| {
            tracing::error!(error = %e, role_id = %role_uuid, "Failed to query role");
            ApiError::internal("Failed to query role")
        })?
        .ok_or_else(|| ApiError::not_found("Role not found"))?;

    let realm_id = role.realm_id;

    let admin = AdminIdentity::require(identity, &realm_id, "role policies")?;
    admin.require_permission(&state, "policies", "view").await?;
    // Query role_policies
    let policies = role_policies::Entity::find()
        .filter(role_policies::Column::RoleId.eq(role_uuid))
        .all(state.db.as_ref())
        .await
        .map_err(|e| {
            tracing::error!(error = %e, role_id = %role_uuid, "Failed to query role policies");
            ApiError::internal("Failed to query role policies")
        })?;

    let policies: Vec<PolicyResponse> = policies
        .into_iter()
        .map(|p| PolicyResponse {
            id: p.id,
            resource: p.resource,
            action: p.action,
            realm_id: p.realm_id,
        })
        .collect();

    Ok(ApiResult::ok(RolePoliciesResponse { policies }))
}

/// Add policy to role
///
/// Adds a permission policy to a role
#[utoipa::path(
    post,
    path = "/api/permission/roles/{roleId}/policies",
    params(
        ("roleId" = Uuid, Path, description = "Role ID")
    ),
    request_body = AddPolicyRequest,
    responses(
        (status = 201, description = "Policy added successfully", body = PolicyResponse),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 403, description = "Forbidden - Insufficient permissions", body = ErrorResponse),
        (status = 404, description = "Role not found", body = ErrorResponse),
        (status = 409, description = "Policy already exists", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "permission"
)]
pub async fn add_policy_to_role(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(role_id): Path<Uuid>,
    axum::Json(request): axum::Json<AddPolicyRequest>,
) -> Result<ApiResult<PolicyResponse>, ApiError> {
    // Validate request
    if let Err(errors) = request.validate() {
        return Err(ApiError::bad_request(format!(
            "Invalid request: {}",
            errors
        )));
    }

    // Get role to determine realm_id
    let role = herald_core::entity::roles::Entity::find()
        .filter(herald_core::entity::roles::Column::Id.eq(role_id))
        .one(state.db.as_ref())
        .await
        .map_err(|e| {
            tracing::error!(error = %e, role_id = %role_id, "Failed to query role");
            ApiError::internal("Failed to query role")
        })?
        .ok_or_else(|| ApiError::not_found("Role not found"))?;

    let realm_id = role.realm_id;

    let admin = AdminIdentity::require(identity, &realm_id, "role policies")?;
    admin
        .require_permission(&state, "policies", "manage")
        .await?;

    // Security: wildcard policies are reserved for the platform; mirror the
    // guard used by direct user-permission assignment so the two policy-creation
    // surfaces stay consistent (currently inert — matches_policy is exact-match
    // — but a future matcher change would otherwise make this a bypass).
    if request.resource == "All" || request.resource.contains("*") {
        tracing::warn!(
            role_id = %role_id,
            realm_id = %realm_id,
            resource = %request.resource,
            "Attempted to create privileged role policy"
        );
        return Err(ApiError::forbidden("Cannot create privileged policies"));
    }

    // Security: a delegated policies.manage holder must not attach a
    // permission they do not hold themselves to any role — otherwise a
    // sub-admin could grant e.g. ("users","manage") to the builtin "user"
    // role and escalate every account in the realm. Mirrors the guard on
    // direct user-permission assignment.
    admin
        .require_permission(&state, &request.resource, &request.action)
        .await?;

    // Create new policy
    let policy = role_policies::ActiveModel {
        id: ActiveValue::Set(Uuid::now_v7()),
        realm_id: ActiveValue::Set(realm_id.clone()),
        role_id: ActiveValue::Set(role_id), // Use the parsed UUID
        resource: ActiveValue::Set(request.resource),
        action: ActiveValue::Set(request.action),
        created_at: ActiveValue::Set(chrono::Utc::now().into()),
    };

    let policy = policy.insert(state.db.as_ref()).await.map_err(|e| {
        tracing::error!(
            error = %e,
            role_id = %role_id,
            "Failed to add policy"
        );

        if validation::is_duplicate_key_error(&e) {
            return ApiError::conflict("Policy already exists for this role");
        }

        ApiError::internal("Failed to add policy")
    })?;

    // Invalidate role policy cache
    let _ = state
        .permission_checker
        .invalidate_role_policy_cache(&realm_id, &role_id.to_string())
        .await;

    tracing::info!(
        role_id = %role_id,
        resource = %policy.resource,
        action = %policy.action,
        "Policy added to role"
    );

    Ok(ApiResult::created(PolicyResponse {
        id: policy.id,
        resource: policy.resource,
        action: policy.action,
        realm_id: policy.realm_id,
    }))
}

/// Remove policy from role
///
/// Removes a specific permission policy from a role
#[utoipa::path(
    delete,
    path = "/api/permission/roles/{roleId}/policies/{policyId}",
    params(
        ("roleId" = Uuid, Path, description = "Role ID"),
        ("policyId" = Uuid, Path, description = "Policy ID")
    ),
    responses(
        (status = 204, description = "Policy removed successfully"),
        (status = 403, description = "Forbidden - Insufficient permissions", body = ErrorResponse),
        (status = 404, description = "Role, policy, or assignment not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "permission"
)]
pub async fn remove_policy_from_role(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path((role_id, policy_id)): Path<(Uuid, Uuid)>,
) -> Result<ApiResult<()>, ApiError> {
    let policy = role_policies::Entity::find()
        .filter(role_policies::Column::Id.eq(policy_id))
        .one(state.db.as_ref())
        .await
        .map_err(|e| {
            tracing::error!(
                error = %e,
                role_id = %role_id,
                policy_id = %policy_id,
                "Failed to query policy"
            );
            ApiError::internal("Failed to query policy")
        })?;

    let realm_id = policy
        .ok_or_else(|| ApiError::not_found("Policy not found"))?
        .realm_id;

    let admin = AdminIdentity::require(identity, &realm_id, "role policies")?;
    admin
        .require_permission(&state, "policies", "manage")
        .await?;

    // The policy must actually be attached to the path role: otherwise a
    // caller could delete a realm-mate role's policy while the cache
    // invalidation below targets the unverified path roleId — the affected
    // role would keep serving the removed permission from cache until TTL.
    let result = role_policies::Entity::delete_many()
        .filter(role_policies::Column::Id.eq(policy_id))
        .filter(role_policies::Column::RoleId.eq(role_id))
        .exec(state.db.as_ref())
        .await
        .map_err(|e| {
            tracing::error!(
                error = %e,
                role_id = %role_id,
                policy_id = %policy_id,
                "Failed to remove policy"
            );
            ApiError::internal("Failed to remove policy")
        })?;

    if result.rows_affected == 0 {
        return Err(ApiError::not_found("Policy not found"));
    }

    // Invalidate role policy cache
    invalidate_role_policy_cache(&state, &realm_id, &role_id).await;

    tracing::info!(
        role_id = %role_id,
        policy_id = %policy_id,
        "Policy removed from role"
    );

    Ok(ApiResult::no_content())
}

// ============================================================================
// Router
// ============================================================================

pub fn router() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/{roleId}/policies", axum::routing::get(get_role_policies))
        .route(
            "/{roleId}/policies",
            axum::routing::post(add_policy_to_role),
        )
        .route(
            "/{roleId}/policies/{policyId}",
            axum::routing::delete(remove_policy_from_role),
        )
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Invalidate role policy cache after policy changes
///
/// This helper function reduces nesting by extracting the cache invalidation logic
async fn invalidate_role_policy_cache(state: &AppState, realm_id: &str, role_id: &Uuid) {
    let _ = state
        .permission_checker
        .invalidate_role_policy_cache(realm_id, &role_id.to_string())
        .await;
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {

    // TODO: Add integration tests with test database
}
