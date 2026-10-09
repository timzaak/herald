use herald_api_base::application::http::server::api_entities::ApiError;
use herald_api_base::application::http::state::AppState;
use herald_core::domain::authentication::Identity;
use herald_core::domain::user::RoleAssignmentService;
use herald_core::domain::user::admin_errors::UserAdminError;
use herald_core::entity::client_app;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use uuid::Uuid;

use crate::api_keys::types::ApiKeyRoleSummary;

/// Resolve the Client App an API key creation must bind to. The app is
/// always explicit — there is no default binding — and must exist in the
/// key's realm.
pub async fn resolve_client_app_for_create(
    state: &AppState,
    realm_id: &str,
    client_app_id: Uuid,
) -> Result<client_app::Model, ApiError> {
    client_app::Entity::find()
        .filter(client_app::Column::RealmId.eq(realm_id))
        .filter(client_app::Column::Id.eq(client_app_id))
        .one(state.db.as_ref())
        .await
        .map_err(|e| {
            tracing::error!("Failed to query Client App for API key: {e}");
            ApiError::internal("Failed to create API key")
        })?
        .ok_or_else(|| ApiError::bad_request("Client App not found in this realm"))
}

/// Load the bound Client App's display `(name, enabled)` for API key
/// responses: `(None, None)` when the key has no bound app (legacy rows).
/// The enabled flag drives the admin-UI "bound app disabled" warning.
pub async fn client_app_name_and_enabled(
    state: &AppState,
    client_app_id: Option<Uuid>,
) -> Result<(Option<String>, Option<bool>), ApiError> {
    let Some(id) = client_app_id else {
        return Ok((None, None));
    };

    let app = client_app::Entity::find_by_id(id)
        .one(state.db.as_ref())
        .await
        .map_err(|e| {
            tracing::error!("Failed to query API key Client App name: {e}");
            ApiError::internal("Failed to load API key Client App")
        })?;

    Ok(app
        .map(|app| (Some(app.name), Some(app.enabled)))
        .unwrap_or((None, None)))
}

/// Load the role summaries embedded in `ApiKeyListItem` responses, shared by
/// the get and update handlers so their error mapping cannot drift apart.
pub async fn api_key_role_summaries(
    state: &AppState,
    identity: &Identity,
    realm_id: &str,
    api_key_id: &str,
) -> Result<Vec<ApiKeyRoleSummary>, ApiError> {
    let roles = state
        .role_assignment_service
        .get_api_key_roles(identity.clone(), realm_id, api_key_id)
        .await
        .map_err(|e| match e {
            UserAdminError::PermissionDenied(msg) => ApiError::forbidden(msg),
            other => ApiError::internal(format!("Failed to load API key roles: {other}")),
        })?;
    Ok(roles
        .into_iter()
        .map(|role| ApiKeyRoleSummary {
            id: role.id,
            name: role.name,
        })
        .collect())
}
