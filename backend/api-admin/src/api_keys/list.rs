use axum::extract::{Extension, Query, State};
use herald_api_base::application::http::common::auth_utils::AdminIdentity;
use herald_api_base::application::http::server::api_entities::{ApiError, ApiResult, PageResponse};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::authentication::Identity;
use herald_core::domain::user::UserRoleRepository;
use herald_core::entity::client_app;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

use crate::api_keys::types::{ApiKeyListItem, ApiKeyRoleSummary, ListQuery};

/// List all API Keys for a realm
///
/// Returns a paginated list of API keys. Hash and plaintext are never exposed.
#[utoipa::path(
    get,
    path = "/api/api-keys",
    tag = "api-keys",
    params(
        ("page" = Option<i64>, Query, description = "Page number (0-based, default 0)"),
        ("pageSize" = Option<i64>, Query, description = "Page size (default 20)"),
    ),
    responses(
        (status = 200, description = "API Key list", body = PageResponse<ApiKeyListItem>),
        (status = 403, description = "Forbidden", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 500, description = "Internal server error", body = herald_api_base::application::http::server::api_entities::ErrorResponse)
    )
)]
pub async fn list_api_keys(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Query(query): Query<ListQuery>,
) -> Result<ApiResult<PageResponse<ApiKeyListItem>>, ApiError> {
    let admin = AdminIdentity::require(identity, "api keys")?;
    let realm_id = admin.realm_id().to_string();
    admin.require_permission(&state, "api_keys", "view").await?;

    let offset = (query.page * query.page_size) as u64;
    let limit = query.page_size as u64;

    let (api_keys, total_count) = {
        let keys = state
            .api_key_repo
            .list(&realm_id, offset, limit)
            .await
            .map_err(|e| {
                tracing::error!("Failed to list API keys: {e}");
                ApiError::internal("Failed to list API keys")
            })?;

        let count = state.api_key_repo.count(&realm_id).await.map_err(|e| {
            tracing::error!("Failed to count API keys: {e}");
            ApiError::internal("Failed to count API keys")
        })?;

        (keys, count)
    };

    let items: Vec<ApiKeyListItem> = {
        // Batch load role summaries for the page's API key IDs
        let api_key_ids: Vec<String> = api_keys.iter().map(|k| k.id.clone()).collect();
        let role_summaries = if api_key_ids.is_empty() {
            Vec::new()
        } else {
            state
                .user_role_repository
                .get_api_key_role_summaries_batch(&api_key_ids)
                .await
                .map_err(|e| {
                    tracing::error!("Failed to batch load API key roles: {e}");
                    ApiError::internal("Failed to load API key roles")
                })?
        };

        let mut role_map: std::collections::HashMap<String, Vec<(uuid::Uuid, String)>> =
            role_summaries.into_iter().collect();

        let client_app_ids: Vec<uuid::Uuid> =
            api_keys.iter().filter_map(|k| k.client_app_id).collect();
        let client_apps = if client_app_ids.is_empty() {
            Vec::new()
        } else {
            client_app::Entity::find()
                .filter(client_app::Column::Id.is_in(client_app_ids))
                .all(state.db.as_ref())
                .await
                .map_err(|e| {
                    tracing::error!("Failed to batch load API key Client Apps: {e}");
                    ApiError::internal("Failed to load API key Client Apps")
                })?
        };
        let client_app_name_by_id: std::collections::HashMap<uuid::Uuid, String> = client_apps
            .into_iter()
            .map(|app| (app.id, app.name))
            .collect();

        api_keys
            .into_iter()
            .map(|k| {
                let roles = role_map.remove(&k.id).unwrap_or_default();
                let client_app_name = k
                    .client_app_id
                    .and_then(|id| client_app_name_by_id.get(&id).cloned());
                ApiKeyListItem {
                    id: k.id,
                    name: k.name,
                    realm_id: k.realm_id,
                    client_app_id: k.client_app_id,
                    client_app_name,
                    enabled: k.enabled,
                    expires_at: k.expires_at.map(|dt| dt.to_rfc3339()),
                    last_used_at: k.last_used_at.map(|dt| dt.to_rfc3339()),
                    created_at: k.created_at.to_rfc3339(),
                    roles: roles
                        .into_iter()
                        .map(|(id, name)| ApiKeyRoleSummary { id, name })
                        .collect(),
                }
            })
            .collect()
    };

    Ok(ApiResult::ok(PageResponse {
        items,
        page: query.page,
        page_size: query.page_size,
        total: total_count as i64,
    }))
}
