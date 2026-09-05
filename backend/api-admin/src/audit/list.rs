use axum::extract::{Extension, Path, Query, State};
use chrono::{DateTime, NaiveDate};
use herald_core::domain::audit::{
    AuditAction, AuditCategory, AuditEventFilters, AuditEventRepository,
};
use herald_core::domain::authentication::Identity;

use super::types::{AuditEventListResponse, AuditEventQueryParams, AuditEventResponse};
use herald_api_base::application::http::common::auth_utils::AdminIdentity;
use herald_api_base::application::http::server::api_entities::{ApiError, ApiResult};
use herald_api_base::application::http::state::AppState;

/// List audit events with optional filters and pagination
#[utoipa::path(
    get,
    path = "/api/audit/{realmId}",
    tag = "audit",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("category" = Option<String>, Query, description = "Filter by audit category (e.g. user_management, rbac, realm_management, auth)"),
        ("action" = Option<String>, Query, description = "Filter by action (e.g. user.create, auth.login)"),
        ("actorId" = Option<String>, Query, description = "Filter by actor ID"),
        ("startTime" = Option<String>, Query, description = "Start time (ISO 8601 / RFC 3339)"),
        ("endTime" = Option<String>, Query, description = "End time (ISO 8601 / RFC 3339)"),
        ("page" = Option<u64>, Query, description = "Page number (0-based, default 0)"),
        ("pageSize" = Option<u64>, Query, description = "Page size (default 20, max 100)"),
    ),
    responses(
        (status = 400, description = "Invalid filter", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 200, description = "Paginated list of audit events", body = AuditEventListResponse),
        (status = 401, description = "Unauthorized", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 403, description = "Forbidden - Insufficient permissions", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 500, description = "Internal server error", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_audit_events(
    Path(realm_id): Path<String>,
    Query(params): Query<AuditEventQueryParams>,
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
) -> Result<ApiResult<AuditEventListResponse>, ApiError> {
    let admin = AdminIdentity::require(identity, &realm_id, "audit logs")?;
    admin.require_permission(&state, "audit", "view").await?;

    let category: Option<AuditCategory> = params
        .category
        .as_deref()
        .map(|s| serde_json::from_str(&format!("\"{s}\"")))
        .transpose()
        .map_err(|_| ApiError::bad_request("Invalid audit category"))?;

    let action: Option<AuditAction> = params
        .action
        .as_deref()
        .map(|s| serde_json::from_str(&format!("\"{s}\"")))
        .transpose()
        .map_err(|_| ApiError::bad_request("Invalid audit action"))?;

    let parse_query_time = |value: Option<&str>| {
        value
            .and_then(|s| {
                DateTime::parse_from_rfc3339(s)
                    .or_else(|_| DateTime::parse_from_rfc3339(&s.replace(' ', "+")))
                    .ok()
            })
            .map(|dt| dt.to_utc())
            .or_else(|| {
                value.and_then(|s| {
                    NaiveDate::parse_from_str(s, "%Y-%m-%d")
                        .ok()
                        .map(|d| d.and_hms_opt(0, 0, 0).unwrap().and_utc())
                })
            })
    };

    let start_time = parse_query_time(params.start_time.as_deref());
    let end_time = parse_query_time(params.end_time.as_deref());

    let filters = AuditEventFilters {
        category,
        action,
        actor_id: params.actor_id,
        start_time,
        end_time,
        page: params.page.unwrap_or(0),
        page_size: params.page_size.unwrap_or(20).min(100),
    };

    let result = state
        .audit_event_repository
        .list_paginated(&realm_id, filters)
        .await
        .map_err(|e| {
            tracing::error!("Failed to list audit events: {e}");
            ApiError::internal("Failed to list audit events")
        })?;

    let items = result
        .items
        .into_iter()
        .map(AuditEventResponse::from_event)
        .collect();

    Ok(ApiResult::ok(AuditEventListResponse {
        items,
        page: result.page as i64,
        page_size: result.page_size as i64,
        total: result.total as i64,
    }))
}
