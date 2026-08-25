use axum::extract::{Extension, State};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

pub use crate::application::http::server::api_entities::ErrorResponse;
use crate::application::http::server::api_entities::{ApiError, ApiResult};
use crate::application::http::state::AppState;
use herald_api_base::application::http::common::auth_utils::{SelfIdentity, require_token_scope};
use herald_core::domain::authentication::{CredentialScope, Identity, TokenCredentialContext};
use herald_core::domain::authorization::{RoleRepository, permission_service::PermissionService};
use herald_core::infrastructure::authorization::PostgresRoleRepository;

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct UserProfileRolesResponse {
    pub roles: Vec<String>,
    pub permissions: Vec<String>,
}

/// Get the current user's roles and permissions
///
/// Returns all roles (role names) and permissions (permission names) of the
/// currently logged-in user. Returns the roles and permissions the user
/// actually holds, without expanding the full set of realm permission definitions.
#[utoipa::path(
    get,
    path = "/api/user/roles",
    tag = "user",
    responses(
        (status = 200, description = "User roles and permissions retrieved", body = UserProfileRolesResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn get_user_roles(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Extension(context): Extension<TokenCredentialContext>,
) -> Result<ApiResult<UserProfileRolesResponse>, ApiError> {
    require_token_scope(&identity, &context, CredentialScope::ProfileRead)?;
    let self_identity = SelfIdentity::require(identity)?;
    let realm_id = self_identity.realm_id();
    let user_id = self_identity.user_id_string();

    let permission_checker = &state.permission_checker;
    let role_repo = PostgresRoleRepository::new(state.db.clone());

    // Get user's roles
    let role_ids = permission_checker
        .get_user_roles(&realm_id, &user_id)
        .await
        .map_err(|e| {
            tracing::error!(
                error = %e,
                user_id = %user_id,
                realm_id = %realm_id,
                "Failed to fetch user roles"
            );
            ApiError::internal("Internal server error")
        })?;

    tracing::debug!(
        role_ids = ?role_ids,
        "Found roles for user"
    );

    // Get role names
    let role_names = if !role_ids.is_empty() {
        let role_uuids: Vec<Uuid> = role_ids
            .iter()
            .filter_map(|id| Uuid::parse_str(id).ok())
            .collect();

        if role_uuids.is_empty() {
            Vec::new()
        } else {
            match role_repo.find_by_ids(&realm_id, role_uuids).await {
                Ok(roles) => roles.into_iter().map(|r| r.name).collect(),
                Err(e) => {
                    tracing::error!("Failed to fetch role details: {}", e);
                    Vec::new()
                }
            }
        }
    } else {
        Vec::new()
    };

    let permissions = permission_checker
        .get_user_permissions(&realm_id, &user_id)
        .await
        .map_err(|e| {
            tracing::error!(
                error = %e,
                user_id = %user_id,
                realm_id = %realm_id,
                "Failed to fetch user permissions"
            );
            ApiError::internal("Failed to fetch user permissions")
        })?;

    Ok(ApiResult::ok(UserProfileRolesResponse {
        roles: role_names,
        permissions,
    }))
}
