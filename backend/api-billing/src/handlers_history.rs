use axum::{
    Json,
    extract::{Extension, Path, Query, State},
};
use std::str::FromStr;
use uuid::Uuid;
use validator::Validate;

use crate::types_history::{
    SubscriptionHistoryEventResponse, SubscriptionHistoryEventWithUser,
    SubscriptionHistoryListQuery, SubscriptionHistoryListResponse, SubscriptionHistoryResponse,
    SubscriptionSummary,
};
use herald_api_base::application::http::common::auth_utils::{
    require_authenticated_user_in_realm_with_token, require_token_scope,
};
use herald_api_base::application::http::common::pagination::{
    PaginationMeta, calculate_total_pages,
};
use herald_api_base::application::http::server::api_entities::{ApiError, ErrorResponse};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::authentication::{CredentialScope, Identity, TokenCredentialContext};
use herald_core::domain::billing::{
    BillingRepository, HistoryEventType, SUBSCRIPTION_STATUS_UNKNOWN, SortOrder,
    SubscriptionHistoryQuery,
};
use herald_core::domain::common::entities::app_errors::CoreError;

/// Get subscription history for a specific subscription
#[utoipa::path(
    get,
    path = "/api/bill/{realmId}/subscriptions/{subscriptionId}/history",
    tag = "billing",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("subscriptionId" = Uuid, Path, description = "Subscription ID")
    ),
    responses(
        (status = 200, description = "Subscription history retrieved", body = SubscriptionHistoryResponse),
        (status = 401, description = "Unauthorized - Invalid or missing authentication", body = ErrorResponse),
        (status = 403, description = "Forbidden - Insufficient permissions", body = ErrorResponse),
        (status = 404, description = "Subscription not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("session_token" = []))
)]
pub async fn get_subscription_history(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path((realm_id, subscription_id)): Path<(String, Uuid)>,
) -> Result<Json<SubscriptionHistoryResponse>, ApiError> {
    if !identity.has_access_to_realm(&realm_id) {
        return Err(ApiError::forbidden(
            "Access denied: cannot access billing from a different realm".to_string(),
        ));
    }

    crate::handlers::require_billing_permission(&state, &identity, &realm_id, "view").await?;

    let subscription = match state
        .billing_repository
        .find_subscription_by_id(subscription_id)
        .await?
    {
        Some(sub) => Ok(sub),
        None => Err(CoreError::NotFound),
    }?;

    if subscription.realm_id != realm_id {
        return Err(ApiError::not_found("Subscription not found"));
    }

    let events = state
        .billing_repository
        .get_subscription_history(&realm_id, &subscription_id)
        .await?;

    let events_response: Vec<SubscriptionHistoryEventResponse> =
        events.into_iter().map(Into::into).collect();

    let total = events_response.len();

    Ok(Json(SubscriptionHistoryResponse {
        subscription_id,
        events: events_response,
        total,
    }))
}

/// Get current user's subscription history for a specific subscription
#[utoipa::path(
    get,
    path = "/api/bill/{realmId}/my/subscriptions/{subscriptionId}/history",
    tag = "billing",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("subscriptionId" = Uuid, Path, description = "Subscription ID")
    ),
    responses(
        (status = 200, description = "Subscription history retrieved", body = SubscriptionHistoryResponse),
        (status = 401, description = "Unauthorized - Invalid or missing authentication", body = ErrorResponse),
        (status = 403, description = "Forbidden - Authenticated user required", body = ErrorResponse),
        (status = 404, description = "Subscription not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_my_subscription_history(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Extension(context): Extension<TokenCredentialContext>,
    Path((realm_id, subscription_id)): Path<(String, Uuid)>,
) -> Result<Json<SubscriptionHistoryResponse>, ApiError> {
    require_token_scope(&identity, &context, CredentialScope::SubscriptionRead)?;
    let user_id = require_authenticated_user_in_realm_with_token(
        &identity,
        &context,
        &realm_id,
        "subscription history",
    )?;

    let subscription = state
        .billing_repository
        .find_subscription_by_id(subscription_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Subscription not found"))?;

    require_subscription_history_ownership(
        &subscription.realm_id,
        subscription.user_id,
        &realm_id,
        user_id,
    )?;

    let events = state
        .billing_repository
        .get_subscription_history(&realm_id, &subscription_id)
        .await?;

    let events_response: Vec<SubscriptionHistoryEventResponse> =
        events.into_iter().map(Into::into).collect();

    let total = events_response.len();

    Ok(Json(SubscriptionHistoryResponse {
        subscription_id,
        events: events_response,
        total,
    }))
}

/// List subscription history with filtering and pagination
#[utoipa::path(
    get,
    path = "/api/bill/{realmId}/subscriptions/history",
    tag = "billing",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("userId" = Option<Uuid>, Query, description = "Filter by user ID (client_app_id)"),
        ("entitlementKey" = Option<String>, Query, description = "Filter by entitlement key"),
        ("eventType" = Option<String>, Query, description = "Filter by event type"),
        ("subscriptionStatus" = Option<String>, Query, description = "Filter by subscription status"),
        ("fromDate" = Option<String>, Query, description = "Filter by date range start (ISO 8601)"),
        ("toDate" = Option<String>, Query, description = "Filter by date range end (ISO 8601)"),
        ("page" = Option<u64>, Query, description = "Page number (1-based, default: 1)"),
        ("pageSize" = Option<u64>, Query, description = "Items per page (default: 20, max: 100)"),
        ("sortBy" = Option<String>, Query, description = "Sort field (default: timestamp)"),
        ("sortOrder" = Option<String>, Query, description = "Sort order - asc or desc (default: desc)")
    ),
    responses(
        (status = 200, description = "Subscription history list retrieved", body = SubscriptionHistoryListResponse),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Unauthorized - Invalid or missing authentication", body = ErrorResponse),
        (status = 403, description = "Forbidden - Realm Admin required", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("session_token" = []))
)]
pub async fn list_subscription_history(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(realm_id): Path<String>,
    Query(query): Query<SubscriptionHistoryListQuery>,
) -> Result<Json<SubscriptionHistoryListResponse>, ApiError> {
    if !identity.has_access_to_realm(&realm_id) {
        return Err(ApiError::forbidden(
            "Access denied: cannot access billing from a different realm".to_string(),
        ));
    }

    crate::handlers::require_billing_permission(&state, &identity, &realm_id, "view").await?;

    if let Err(validation_errors) = query.validate() {
        return Err(ApiError::bad_request(format!(
            "Invalid query parameters: {}",
            validation_errors
        )));
    }

    let event_type = query
        .event_type
        .and_then(|t| HistoryEventType::from_str(&t).ok());

    let sort_order = match query.sort_order.to_lowercase().as_str() {
        "asc" => Some(SortOrder::Asc),
        "desc" => Some(SortOrder::Desc),
        _ => None,
    };

    let repo_query = SubscriptionHistoryQuery {
        user_id: query.user_id,
        entitlement_key: query.entitlement_key,
        event_type,
        subscription_status: query.subscription_status,
        from_date: query.from_date,
        to_date: query.to_date,
        page: Some(query.page),
        page_size: Some(query.page_size),
        sort_by: Some(query.sort_by),
        sort_order,
    };

    let (events, total) = state
        .billing_repository
        .list_subscription_history(&realm_id, repo_query)
        .await?;

    let events_with_user = convert_events_with_details(events, &state).await;

    let total_pages = calculate_total_pages(total as i64, query.page_size as i64);

    let pagination = PaginationMeta {
        page: query.page as i64,
        page_size: query.page_size as i64,
        total_count: total as i64,
        total_pages,
    };

    Ok(Json(SubscriptionHistoryListResponse {
        events: events_with_user,
        pagination,
    }))
}

/// List current user's subscription history with filtering and pagination
#[utoipa::path(
    get,
    path = "/api/bill/{realmId}/my/subscriptions/history",
    tag = "billing",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("entitlementKey" = Option<String>, Query, description = "Filter by entitlement key"),
        ("eventType" = Option<String>, Query, description = "Filter by event type"),
        ("subscriptionStatus" = Option<String>, Query, description = "Filter by subscription status"),
        ("fromDate" = Option<String>, Query, description = "Filter by date range start (ISO 8601)"),
        ("toDate" = Option<String>, Query, description = "Filter by date range end (ISO 8601)"),
        ("page" = Option<u64>, Query, description = "Page number (1-based, default: 1)"),
        ("pageSize" = Option<u64>, Query, description = "Items per page (default: 20, max: 100)"),
        ("sortBy" = Option<String>, Query, description = "Sort field (default: timestamp)"),
        ("sortOrder" = Option<String>, Query, description = "Sort order - asc or desc (default: desc)")
    ),
    responses(
        (status = 200, description = "Subscription history list retrieved", body = SubscriptionHistoryListResponse),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Unauthorized - Invalid or missing authentication", body = ErrorResponse),
        (status = 403, description = "Forbidden - Authenticated user required", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_my_subscription_history(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Extension(context): Extension<TokenCredentialContext>,
    Path(realm_id): Path<String>,
    Query(query): Query<SubscriptionHistoryListQuery>,
) -> Result<Json<SubscriptionHistoryListResponse>, ApiError> {
    require_token_scope(&identity, &context, CredentialScope::SubscriptionRead)?;
    let user_id = require_authenticated_user_in_realm_with_token(
        &identity,
        &context,
        &realm_id,
        "subscription history",
    )?;

    if let Some(requested_user_id) = query.user_id
        && requested_user_id != user_id
    {
        return Err(ApiError::forbidden(
            "You can only view your own subscription history",
        ));
    }

    if let Err(validation_errors) = query.validate() {
        return Err(ApiError::bad_request(format!(
            "Invalid query parameters: {}",
            validation_errors
        )));
    }

    let event_type = query
        .event_type
        .and_then(|t| HistoryEventType::from_str(&t).ok());

    let sort_order = match query.sort_order.to_lowercase().as_str() {
        "asc" => Some(SortOrder::Asc),
        "desc" => Some(SortOrder::Desc),
        _ => None,
    };

    let repo_query = SubscriptionHistoryQuery {
        user_id: Some(user_id),
        entitlement_key: query.entitlement_key,
        event_type,
        subscription_status: query.subscription_status,
        from_date: query.from_date,
        to_date: query.to_date,
        page: Some(query.page),
        page_size: Some(query.page_size),
        sort_by: Some(query.sort_by),
        sort_order,
    };

    let (events, total) = state
        .billing_repository
        .list_subscription_history(&realm_id, repo_query)
        .await?;

    let events_with_user = convert_events_with_details(events, &state).await;

    let total_pages = calculate_total_pages(total as i64, query.page_size as i64);

    let pagination = PaginationMeta {
        page: query.page as i64,
        page_size: query.page_size as i64,
        total_count: total as i64,
        total_pages,
    };

    Ok(Json(SubscriptionHistoryListResponse {
        events: events_with_user,
        pagination,
    }))
}

async fn convert_events_with_details(
    events: Vec<herald_core::domain::billing::SubscriptionHistoryEvent>,
    state: &AppState,
) -> Vec<SubscriptionHistoryEventWithUser> {
    let mut events_with_details = Vec::new();

    for event in events {
        let subscription_summary = if let Ok(Some(subscription)) = state
            .billing_repository
            .find_subscription_by_id(event.subscription_id)
            .await
        {
            SubscriptionSummary {
                id: subscription.id,
                status: subscription.status.as_str().to_string(),
                entitlement_key: Some(subscription.entitlement_key),
            }
        } else {
            SubscriptionSummary {
                id: event.subscription_id,
                status: SUBSCRIPTION_STATUS_UNKNOWN.to_string(),
                entitlement_key: None,
            }
        };

        events_with_details.push(SubscriptionHistoryEventWithUser {
            id: event.id,
            subscription_id: event.subscription_id,
            event_type: event.event_type.as_str().to_string(),
            timestamp: event.timestamp,
            actor: event.actor,
            changes: event.changes,
            previous_state: event.previous_state,
            new_state: event.new_state,
            user: None,
            subscription: subscription_summary,
        });
    }

    events_with_details
}

fn require_subscription_history_ownership(
    subscription_realm_id: &str,
    subscription_user_id: Uuid,
    realm_id: &str,
    user_id: Uuid,
) -> Result<(), ApiError> {
    if subscription_realm_id != realm_id || subscription_user_id != user_id {
        return Err(ApiError::not_found("Subscription not found"));
    }
    Ok(())
}

#[cfg(test)]
mod browser_scope_subscription_tests {
    use super::*;

    #[test]
    fn browser_scope_subscription_rejects_cross_user_history() {
        assert!(
            require_subscription_history_ownership(
                "realm",
                Uuid::now_v7(),
                "realm",
                Uuid::now_v7(),
            )
            .is_err()
        );
    }
}
