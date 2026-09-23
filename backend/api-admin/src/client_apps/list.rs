use axum::{
    extract::{Extension, Query, State},
    http::HeaderMap,
};
use herald_core::domain::authentication::Identity;

use crate::client_apps::types::{ClientAppItem, ListQuery};
use herald_api_base::application::http::common::auth_utils::AdminIdentity;
use herald_api_base::application::http::server::api_entities::{ApiError, ApiResult, PageResponse};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::client::ports::ClientService;

/// List all client apps for a realm
///
/// Returns a list of OAuth client applications configured for the specified realm.
#[utoipa::path(
    get,
    path = "/api/client",
    tag = "client",
    summary = "List client applications",
    description = "List all OAuth client applications configured for the specified realm with pagination. Requires `clients.view` permission.",
    params(
        ("page" = Option<i64>, Query, description = "Page number (0-based, default 0)"),
        ("pageSize" = Option<i64>, Query, description = "Page size (default 20)"),
    ),
    responses(
        (status = 200, description = "ClientApp list", body = PageResponse<ClientAppItem>),
        (status = 403, description = "Forbidden - Insufficient permissions (requires clients.view)", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 500, description = "Internal server error", body = herald_api_base::application::http::server::api_entities::ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_client_apps(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Query(query): Query<ListQuery>,
    _headers: HeaderMap,
) -> Result<ApiResult<PageResponse<ClientAppItem>>, ApiError> {
    let admin = AdminIdentity::require(identity, "client applications")?;
    let realm_id = admin.realm_id().to_string();
    admin.require_permission(&state, "clients", "view").await?;

    tracing::debug!(
        realm_id = %realm_id,
        user_id = %admin.user_id_string(),
        "Listing client apps"
    );

    // Call service layer with pagination
    let client_service = state.service.client_service();
    let page = query.page as u64;
    let page_size = query.page_size as u64;
    let (client_apps, total_count) = client_service
        .list_client_apps_paginated(admin.identity().clone(), realm_id, page, page_size)
        .await
        .map_err(|e| {
            tracing::error!("Failed to list client apps: {}", e);
            ApiError::internal(format!("Failed to list client apps: {e}"))
        })?;

    // Convert domain models to API response models
    let data: Vec<ClientAppItem> = client_apps.into_iter().map(ClientAppItem::from).collect();

    Ok(ApiResult::ok(PageResponse {
        items: data,
        page: query.page,
        page_size: query.page_size,
        total: total_count as i64,
    }))
}
