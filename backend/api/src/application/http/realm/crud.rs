// Realm CRUD handlers

use axum::{
    Json,
    extract::{Extension, Path, Query, State},
    http::HeaderMap,
};
use herald_api_base::application::http::auth::util::{ClientIp, user_agent_from_headers};
use herald_core::domain::audit::AuditContext;
use herald_core::domain::authentication::Identity;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use validator::Validate;

use crate::application::http::realm::validators::{
    CreateRealmValidator, InitialAdminUserValidator, ListRealmsPaginatedQuery, UpdateRealmValidator,
};
use crate::application::http::server::api_entities::{ApiError, ApiResult, PageResponse};
use crate::application::http::state::AppState;
use herald_core::domain::realm::{CreateRealmRequest, Realm, RealmService, UpdateRealmRequest};

/// Admin user information returned in realm responses
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AdminUserResponse {
    pub id: String,
    pub email: String,
    pub role: String,
}

/// Realm response DTO for API responses
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RealmResponse {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub admin_user: Option<AdminUserResponse>,
}

impl From<herald_core::domain::realm::CreatedAdminUser> for AdminUserResponse {
    fn from(user: herald_core::domain::realm::CreatedAdminUser) -> Self {
        Self {
            id: user.id,
            email: user.email,
            role: user.role,
        }
    }
}

impl From<Realm> for RealmResponse {
    fn from(realm: Realm) -> Self {
        Self {
            id: realm.id,
            name: realm.name,
            description: realm.description,
            created_at: realm.created_at.to_rfc3339(),
            updated_at: realm.updated_at.to_rfc3339(),
            admin_user: realm.admin_user.map(AdminUserResponse::from),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ListRealmsResponse {
    pub realms: Vec<RealmResponse>,
}

impl From<InitialAdminUserValidator> for herald_core::domain::realm::InitialAdminUser {
    fn from(val: InitialAdminUserValidator) -> Self {
        Self {
            email: val.email,
            password: val.password,
        }
    }
}

/// List all realms (non-paginated, deprecated - use list_realms_paginated)
#[utoipa::path(
    get,
    path = "/api/realms",
    tag = "realms",
    responses(
        (status = 200, description = "List of realms", body = ListRealmsResponse),
        (status = 401, description = "Unauthorized", body = crate::application::http::server::api_entities::ErrorResponse),
        (status = 500, description = "Internal server error", body = crate::application::http::server::api_entities::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_realms(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    _headers: HeaderMap,
) -> Result<ApiResult<ListRealmsResponse>, ApiError> {
    let realm_service = state.service.realm_service();

    let identity_realm_id = identity.realm_id();
    let current_user_id = identity.user_id();

    tracing::debug!(
        realm_id = %identity_realm_id,
        user_id = %current_user_id,
        "Listing realms"
    );

    let realms = realm_service.list_realms(identity).await.map_err(|e| {
        tracing::error!("Failed to list realms: {e}");
        match e {
            herald_core::domain::common::entities::app_errors::CoreError::Forbidden(msg) => {
                ApiError::forbidden(msg)
            }
            _ => ApiError::internal("Failed to list realms"),
        }
    })?;

    Ok(ApiResult::ok(ListRealmsResponse {
        realms: realms.into_iter().map(RealmResponse::from).collect(),
    }))
}

/// List realms with pagination
#[utoipa::path(
    get,
    path = "/api/realms/paginated",
    tag = "realms",
    params(
        ("page" = Option<i32>, Query, description = "Page number (0-based, default 0)"),
        ("pageSize" = Option<i32>, Query, description = "Page size (default 25, max 100)"),
        ("search" = Option<String>, Query, description = "Search term for realm_id or name"),
        ("sortBy" = Option<String>, Query, description = "Sort column (realm_id, name, created_at, updated_at)"),
        ("sortOrder" = Option<String>, Query, description = "Sort order (asc, desc)"),
    ),
    responses(
        (status = 200, description = "Paginated list of realms", body = PageResponse<RealmResponse>),
        (status = 401, description = "Unauthorized", body = crate::application::http::server::api_entities::ErrorResponse),
        (status = 500, description = "Internal server error", body = crate::application::http::server::api_entities::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_realms_paginated(
    Query(query): Query<ListRealmsPaginatedQuery>,
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    _headers: HeaderMap,
) -> Result<ApiResult<PageResponse<RealmResponse>>, ApiError> {
    let realm_service = state.service.realm_service();

    let identity_realm_id = identity.realm_id();
    let current_user_id = identity.user_id();

    tracing::debug!(
        realm_id = %identity_realm_id,
        user_id = %current_user_id,
        "Listing realms with pagination"
    );

    // Convert query to filters
    let filters = query.to_filters();

    let response = realm_service
        .list_realms_paginated(identity, filters)
        .await
        .map_err(|e| {
            tracing::error!("Failed to list realms: {e}");
            match e {
                herald_core::domain::common::entities::app_errors::CoreError::Forbidden(msg) => {
                    ApiError::forbidden(msg)
                }
                _ => ApiError::internal("Failed to list realms"),
            }
        })?;

    Ok(ApiResult::ok(PageResponse {
        items: response
            .realms
            .into_iter()
            .map(RealmResponse::from)
            .collect(),
        page: response.pagination.page as i64,
        page_size: response.pagination.page_size as i64,
        total: response.pagination.total,
    }))
}

/// Get realm by ID
#[utoipa::path(
    get,
    path = "/api/realms/{realmId}",
    tag = "realms",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
    ),
    responses(
        (status = 200, description = "Realm details", body = RealmResponse),
        (status = 401, description = "Unauthorized", body = crate::application::http::server::api_entities::ErrorResponse),
        (status = 404, description = "Realm not found", body = crate::application::http::server::api_entities::ErrorResponse),
        (status = 500, description = "Internal server error", body = crate::application::http::server::api_entities::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_realm(
    Path(realm_id): Path<String>,
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    _headers: HeaderMap,
) -> Result<ApiResult<RealmResponse>, ApiError> {
    let realm_service = state.service.realm_service();

    let identity_realm_id = identity.realm_id();
    let current_user_id = identity.user_id();

    tracing::debug!(
        realm_id = %identity_realm_id,
        user_id = %current_user_id,
        "Getting realm"
    );

    let realm = realm_service
        .get_realm(identity, realm_id)
        .await
        .map_err(|e| {
            tracing::error!("Failed to get realm: {e}");
            match e {
                herald_core::domain::common::entities::app_errors::CoreError::Forbidden(msg) => {
                    ApiError::forbidden(msg)
                }
                herald_core::domain::common::entities::app_errors::CoreError::NotFound => {
                    ApiError::not_found("Realm not found")
                }
                _ => ApiError::internal("Internal server error"),
            }
        })?;

    Ok(ApiResult::ok(RealmResponse::from(realm)))
}

/// Create a new realm
#[utoipa::path(
    post,
    path = "/api/realms",
    tag = "realms",
    request_body = CreateRealmValidator,
    responses(
        (status = 201, description = "Realm created", body = RealmResponse),
        (status = 400, description = "Bad request - invalid ID or ID already exists", body = crate::application::http::server::api_entities::ErrorResponse),
        (status = 401, description = "Unauthorized", body = crate::application::http::server::api_entities::ErrorResponse),
        (status = 403, description = "Forbidden - admin only", body = crate::application::http::server::api_entities::ErrorResponse),
        (status = 500, description = "Internal server error", body = crate::application::http::server::api_entities::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_realm(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Json(payload): Json<CreateRealmValidator>,
) -> Result<ApiResult<RealmResponse>, ApiError> {
    // Validate request
    payload
        .validate()
        .map_err(|e| ApiError::bad_request(format!("Validation error: {e}")))?;

    let realm_service = state.service.realm_service();

    let identity_realm_id = identity.realm_id();
    let current_user_id = identity.user_id();

    tracing::debug!(
        realm_id = %identity_realm_id,
        user_id = %current_user_id,
        "Creating realm"
    );

    let request = CreateRealmRequest {
        id: payload.id.clone(),
        name: payload.name.clone(),
        description: payload.description.clone(),
        admin_user: payload.admin_user.into(),
    };

    // Debug logging
    tracing::info!(
        realm_id_input = ?payload.id,
        realm_name = ?payload.name,
        "create_realm: Received request"
    );

    let ctx = AuditContext::admin(&identity, ip, user_agent_from_headers(&headers));
    let realm = realm_service
        .create_realm(identity, ctx, request)
        .await
        .map_err(|e| {
            tracing::error!("Failed to create realm: {e}");
            match e {
                herald_core::domain::common::entities::app_errors::CoreError::BadRequest(msg) => {
                    ApiError::bad_request(msg)
                }
                herald_core::domain::common::entities::app_errors::CoreError::Forbidden(msg) => {
                    ApiError::forbidden(msg)
                }
                _ => ApiError::internal("Internal server error"),
            }
        })?;

    Ok(ApiResult::created(RealmResponse::from(realm)))
}

/// Update realm
#[utoipa::path(
    put,
    path = "/api/realms/{realmId}",
    tag = "realms",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
    ),
    request_body = UpdateRealmValidator,
    responses(
        (status = 200, description = "Realm updated", body = RealmResponse),
        (status = 400, description = "Bad request", body = crate::application::http::server::api_entities::ErrorResponse),
        (status = 401, description = "Unauthorized", body = crate::application::http::server::api_entities::ErrorResponse),
        (status = 403, description = "Forbidden - realm admin only", body = crate::application::http::server::api_entities::ErrorResponse),
        (status = 404, description = "Realm not found", body = crate::application::http::server::api_entities::ErrorResponse),
        (status = 500, description = "Internal server error", body = crate::application::http::server::api_entities::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_realm(
    Path(realm_id): Path<String>,
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    _headers: HeaderMap,
    Json(payload): Json<UpdateRealmValidator>,
) -> Result<ApiResult<RealmResponse>, ApiError> {
    // Validate request
    payload
        .validate()
        .map_err(|e| ApiError::bad_request(format!("Validation error: {e}")))?;

    let realm_service = state.service.realm_service();

    let identity_realm_id = identity.realm_id();
    let current_user_id = identity.user_id();

    tracing::debug!(
        realm_id = %identity_realm_id,
        user_id = %current_user_id,
        "Updating realm"
    );

    let request = UpdateRealmRequest {
        name: Some(payload.name),
        description: payload.description,
    };

    let realm = realm_service
        .update_realm(identity, realm_id, request)
        .await
        .map_err(|e| {
            tracing::error!("Failed to update realm: {e}");
            match e {
                herald_core::domain::common::entities::app_errors::CoreError::Forbidden(msg) => {
                    ApiError::forbidden(msg)
                }
                herald_core::domain::common::entities::app_errors::CoreError::NotFound => {
                    ApiError::not_found("Realm not found")
                }
                herald_core::domain::common::entities::app_errors::CoreError::BadRequest(msg) => {
                    ApiError::bad_request(msg)
                }
                _ => ApiError::internal("Internal server error"),
            }
        })?;

    Ok(ApiResult::ok(RealmResponse::from(realm)))
}
