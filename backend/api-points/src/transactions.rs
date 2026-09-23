// Points Transaction Handlers

use axum::extract::{Extension, Query, State};
use uuid::Uuid;

use crate::types::{ListTransactionsQuery, PointsTransactionResponse, UserTransactionsQuery};
use herald_api_base::application::http::auth::util::check_permission_with_timeout;
use herald_api_base::application::http::common::auth_utils::{
    require_authenticated_user_in_realm, require_token_scope,
};
use herald_api_base::application::http::server::api_entities::{
    ApiError, ApiResult, ErrorResponse, PageResponse,
};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::authentication::{CredentialScope, Identity, TokenCredentialContext};
use herald_core::domain::points::ports::TransactionFilters;

/// List points transactions with filters
#[utoipa::path(
    get,
    path = "/api/points/transactions",
    params(
        ("userId" = Option<String>, Query, description = "Filter by user ID"),
        ("transactionType" = Option<String>, Query, description = "Filter by transaction type"),
        ("clientAppId" = Option<String>, Query, description = "Filter by client app ID"),
        ("subscriptionId" = Option<String>, Query, description = "Filter by subscription ID"),
        ("bucketId" = Option<String>, Query, description = "Filter by Credit Bucket ID"),
        ("startTime" = Option<String>, Query, description = "Filter by start time (ISO 8601)"),
        ("endTime" = Option<String>, Query, description = "Filter by end time (ISO 8601)"),
        ("page" = Option<u64>, Query, description = "Page number (0-based, default: 0)"),
        ("pageSize" = Option<u64>, Query, description = "Page size (default: 20, max: 100)")
    ),
    responses(
        (status = 200, description = "Transactions retrieved successfully", body = PageResponse<PointsTransactionResponse>),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "points"
)]
#[tracing::instrument(
    skip_all,
    fields(db.operation = "list_transactions")
)]
pub async fn list_transactions(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Query(query): Query<ListTransactionsQuery>,
) -> Result<ApiResult<PageResponse<PointsTransactionResponse>>, ApiError> {
    let realm_id = identity.realm_id();
    let user_id = require_authenticated_user_in_realm(&identity, &realm_id, "points transactions")?;
    state
        .points_service
        .ensure_can_manage_points(identity.clone())
        .await
        .map_err(ApiError::from)?;
    // Authenticated caller id, captured before the filter `user_id` (Option<Uuid>)
    // shadows the outer binding below. Used for the `points.manage` probe that
    // gates `effective_at` visibility.
    let caller_user_id = user_id;

    // Parse filters
    let user_id = query.user_id.and_then(|s| s.parse::<Uuid>().ok());

    let client_app_id = query.client_app_id.and_then(|s| s.parse::<Uuid>().ok());

    let subscription_id = query.subscription_id.and_then(|s| s.parse::<Uuid>().ok());

    let bucket_id = match query.bucket_id.as_deref().map(str::trim) {
        Some(s) if !s.is_empty() => Some(
            s.parse::<Uuid>()
                .map_err(|_| ApiError::bad_request("Invalid bucketId format"))?,
        ),
        _ => None,
    };

    let transaction_type = match query.transaction_type {
        Some(s) => Some(
            s.parse::<herald_core::domain::points::TransactionType>()
                .map_err(|_| ApiError::bad_request(format!("Invalid transaction_type: {}", s)))?,
        ),
        None => None,
    };

    let start_time = match query.start_time {
        Some(s) => Some(
            chrono::DateTime::parse_from_rfc3339(&s)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .map_err(|_| ApiError::bad_request(format!("Invalid start_time: {}", s)))?,
        ),
        None => None,
    };

    let end_time = match query.end_time {
        Some(s) => Some(
            chrono::DateTime::parse_from_rfc3339(&s)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .map_err(|_| ApiError::bad_request(format!("Invalid end_time: {}", s)))?,
        ),
        None => None,
    };

    let filters = TransactionFilters {
        user_id,
        bucket_id,
        transaction_type,
        client_app_id,
        subscription_id,
        external_ref_id: query.external_ref_id.unwrap_or_default(),
        start_time,
        end_time,
        page: query.page,
        page_size: query.page_size,
    };

    match state
        .points_service
        .list_transactions(identity, &realm_id, filters)
        .await
    {
        Ok(paginated) => {
            // `effective_at` is admin/audit-only. Regular users must never see
            // it. Probe `points.manage` non-erroringly — deny resolves to
            // `false`, which forces `effective_at = None` for every row below.
            // Combined with `#[serde(skip_serializing_if = "Option::is_none")]`
            // on the response type, this guarantees the key is absent from regular-user
            // JSON. A `false` here means "not a points manager" (view-only or
            // self-view).
            let can_manage = check_permission_with_timeout(
                &state,
                &realm_id,
                &caller_user_id.to_string(),
                "points",
                "manage",
            )
            .await
            .unwrap_or(false);

            let data = paginated
                .data
                .into_iter()
                .map(|transaction| PointsTransactionResponse {
                    id: transaction.id,
                    wallet_id: transaction.wallet_id,
                    user_id: transaction.user_id,
                    realm_id: transaction.realm_id,
                    bucket_id: Some(transaction.bucket_id),
                    transaction_type: transaction.transaction_type.as_str().to_string(),
                    amount: transaction.amount,
                    balance_after: transaction.balance_after,
                    description: transaction.description,
                    client_app_id: transaction.client_app_id,
                    subscription_id: transaction.subscription_id,
                    external_ref_id: transaction.external_ref_id,
                    created_at: transaction.created_at.to_rfc3339(),
                    // `points.view`-only callers get `None` (serialized away by
                    // `skip_serializing_if`); `points.manage` callers get the real
                    // ledger `effective_at`.
                    effective_at: if can_manage {
                        transaction.effective_at
                    } else {
                        None
                    },
                })
                .collect();

            Ok(ApiResult::ok(PageResponse {
                items: data,
                total: paginated.total as i64,
                page: paginated.page as i64,
                page_size: paginated.page_size as i64,
            }))
        }
        Err(e) => Err(match e {
            herald_core::domain::common::entities::app_errors::CoreError::Unauthorized => {
                ApiError::unauthorized("Unauthorized")
            }
            herald_core::domain::common::entities::app_errors::CoreError::Forbidden(msg) => {
                ApiError::forbidden(msg)
            }
            _ => ApiError::internal("Internal server error"),
        }),
    }
}

#[utoipa::path(
    get,
    path = "/api/user/transactions",
    params(UserTransactionsQuery),
    responses(
        (status = 200, description = "Current user's transactions", body = PageResponse<PointsTransactionResponse>),
        (status = 401, description = "Unauthorized", body = ErrorResponse)
    ),
    tag = "user",
    security(("bearer_auth" = []))
)]
pub async fn list_user_transactions(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Extension(context): Extension<TokenCredentialContext>,
    Query(query): Query<UserTransactionsQuery>,
) -> Result<ApiResult<PageResponse<PointsTransactionResponse>>, ApiError> {
    require_token_scope(&identity, &context, CredentialScope::PointsTransactionsRead)?;
    let realm_id = identity.realm_id();
    let user_id = identity
        .user_id()
        .parse::<Uuid>()
        .map_err(|_| ApiError::unauthorized("User token required"))?;
    let parse_uuid = |value: Option<String>, name: &str| -> Result<Option<Uuid>, ApiError> {
        value
            .map(|s| {
                s.parse::<Uuid>()
                    .map_err(|_| ApiError::bad_request(format!("Invalid {name} format")))
            })
            .transpose()
    };
    let transaction_type = query
        .transaction_type
        .map(|s| {
            s.parse()
                .map_err(|_| ApiError::bad_request(format!("Invalid transaction_type: {s}")))
        })
        .transpose()?;
    let parse_time = |value: Option<String>, name: &str| {
        value
            .map(|s| {
                chrono::DateTime::parse_from_rfc3339(&s)
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .map_err(|_| ApiError::bad_request(format!("Invalid {name}: {s}")))
            })
            .transpose()
    };
    let filters = TransactionFilters {
        user_id: Some(user_id),
        bucket_id: parse_uuid(query.bucket_id, "bucketId")?,
        transaction_type,
        client_app_id: parse_uuid(query.client_app_id, "clientAppId")?,
        subscription_id: parse_uuid(query.subscription_id, "subscriptionId")?,
        external_ref_id: query.external_ref_id.unwrap_or_default(),
        start_time: parse_time(query.start_time, "startTime")?,
        end_time: parse_time(query.end_time, "endTime")?,
        page: query.page,
        page_size: query.page_size,
    };
    let paginated = state
        .points_service
        .list_transactions(identity, &realm_id, filters)
        .await
        .map_err(ApiError::from)?;
    let items = paginated
        .data
        .into_iter()
        .map(|transaction| PointsTransactionResponse {
            id: transaction.id,
            wallet_id: transaction.wallet_id,
            user_id: transaction.user_id,
            realm_id: transaction.realm_id,
            bucket_id: Some(transaction.bucket_id),
            transaction_type: transaction.transaction_type.as_str().to_string(),
            amount: transaction.amount,
            balance_after: transaction.balance_after,
            description: transaction.description,
            client_app_id: transaction.client_app_id,
            subscription_id: transaction.subscription_id,
            external_ref_id: transaction.external_ref_id,
            created_at: transaction.created_at.to_rfc3339(),
            effective_at: None,
        })
        .collect();
    Ok(ApiResult::ok(PageResponse {
        items,
        total: paginated.total as i64,
        page: paginated.page as i64,
        page_size: paginated.page_size as i64,
    }))
}
