use herald_api_base::application::http::server::api_entities::ApiError;
use herald_api_base::application::http::state::AppState;
use herald_core::domain::authentication::Identity;
use herald_core::domain::client_api_keys::constants::ADMIN_API_CLIENT_ID;
use herald_core::domain::user::RoleAssignmentService;
use herald_core::domain::user::admin_errors::UserAdminError;
use herald_core::entity::client_app;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use uuid::Uuid;

use crate::api_keys::types::ApiKeyRoleSummary;

pub async fn resolve_client_app_for_create(
    state: &AppState,
    realm_id: &str,
    client_app_id: Option<Uuid>,
) -> Result<client_app::Model, ApiError> {
    let query = client_app::Entity::find().filter(client_app::Column::RealmId.eq(realm_id));
    let app = match client_app_id {
        Some(id) => {
            query
                .filter(client_app::Column::Id.eq(id))
                .one(state.db.as_ref())
                .await
        }
        None => {
            query
                .filter(client_app::Column::ClientId.eq(ADMIN_API_CLIENT_ID))
                .one(state.db.as_ref())
                .await
        }
    }
    .map_err(|e| {
        tracing::error!("Failed to query Client App for API key: {e}");
        ApiError::internal("Failed to create API key")
    })?;

    app.ok_or_else(|| {
        if client_app_id.is_some() {
            ApiError::bad_request("Client App not found in this realm")
        } else {
            ApiError::bad_request(
                "Realm is missing the built-in API Key Client App. Please contact support.",
            )
        }
    })
}

pub async fn client_app_name(
    state: &AppState,
    client_app_id: Option<Uuid>,
) -> Result<Option<String>, ApiError> {
    let Some(id) = client_app_id else {
        return Ok(None);
    };

    let app = client_app::Entity::find_by_id(id)
        .one(state.db.as_ref())
        .await
        .map_err(|e| {
            tracing::error!("Failed to query API key Client App name: {e}");
            ApiError::internal("Failed to load API key Client App")
        })?;

    Ok(app.map(|app| app.name))
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
