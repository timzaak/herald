// Points API for Third-Party Integration
//
// Allows third-party apps to query and consume points using API Key authentication.

use axum::{
    Json,
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::authz::require_principal_permission;
use crate::client_app_scope::{
    ensure_bucket_in_client_app_scope, ensure_client_app_scope, is_admin_api_key,
};
use herald_api_base::application::http::common::error_codes::ErrorCode;
use herald_api_base::application::http::common::error_helpers::json_error;
use herald_api_base::application::http::rate_limit::{RateLimitConfig, rate_limit};
use herald_api_base::application::http::server::api_entities::ApiError;
use herald_api_base::application::http::server::api_entities::ErrorResponse;
use herald_api_base::application::http::state::AppState;
use herald_core::domain::authentication::Identity;
use herald_core::domain::common::entities::app_errors::CoreError;
use herald_core::domain::points::dtos::{ConsumePointsInput, GrantPointsInput};
use herald_core::domain::points::entities::{
    ConsumptionAllocationView, CreditSourceType, PointsTransaction,
};
use herald_core::domain::points::ports::TransactionFilters;

const REALM_RATE_LIMIT_PREFIX: &str = "points:realm:";
const USER_RATE_LIMIT_PREFIX: &str = "points:user:";
const REALM_RATE_LIMIT: RateLimitConfig = RateLimitConfig {
    max_requests: 100,
    window_secs: 60,
    enforce_in_dev: true,
};
const USER_RATE_LIMIT: RateLimitConfig = RateLimitConfig {
    max_requests: 20,
    window_secs: 60,
    enforce_in_dev: true,
};

/// Balance response (SDK-compatible)
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExtPointsBalanceResponse {
    pub user_id: String,
    pub balance: i64,
    pub topup_balance: i64,
    pub subscription_balance: i64,
    pub granted_balance: i64,
    pub registration_balance: i64,
    pub free_periodic_balance: i64,
    /// Total points granted through paid topups and subscription entitlements.
    pub total_paid_granted: i64,
    /// Deprecated compatibility alias for total_paid_granted.
    pub total_recharged: i64,
    pub total_consumed: i64,
    pub unit: String,
    pub currency: String,
    pub updated_at: String,
}

/// Consume points request (SDK-compatible)
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExtConsumePointsRequest {
    pub user_id: String,
    pub client_app_id: String,
    pub amount: i64,
    pub description: Option<String>,
    pub idempotency_key: Option<String>,
}

/// Per-bucket transaction inside a multi-bucket consume response.
///
/// Single-pool consume → `transactions` has length 1 (structure unified with the
/// multi-bucket case). `amount` is the points deducted from this pool (negative
/// sign stripped — always a positive deduction magnitude).
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BucketTransaction {
    pub transaction_id: String,
    pub bucket_id: String,
    pub wallet_id: String,
    pub user_id: String,
    pub amount: i64,
    pub balance_after: i64,
}

/// Ledger-level truth source for a consume.
///
/// Populated from `points_consumption_allocations` joined with its ledger's
/// credit_type via the consume `correlation_id`. Empty for legacy single-pool
/// rows (NULL correlation_id) and when the allocation lookup fails (the
/// deduction has already succeeded; we surface a partial result rather than
/// failing an already-completed consume).
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AllocationDetail {
    pub bucket_id: String,
    pub wallet_id: String,
    pub ledger_id: String,
    pub credit_type: String,
    pub allocated_amount: i64,
}

/// Consume points response (SDK-compatible).
///
/// Breaking change: the previous single-transaction shape is replaced by a
/// per-bucket multi-transaction shape. `correlation_id` groups the N
/// transactions of one consume (DB `points_transactions.correlation_id`);
/// single-pool hits still produce exactly one transaction.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExtConsumePointsResponse {
    pub user_id: String,
    /// Total points consumed in this request (sum across all affected buckets).
    pub amount: i64,
    /// Grouping key shared by the N transactions of this consume. Falls back to
    /// the primary transaction id when the underlying row has no correlation_id
    /// (legacy single-pool replay).
    pub correlation_id: String,
    /// One entry per affected bucket, sorted by `bucket_id` ASC. Length 1 for a
    /// single-pool hit.
    pub transactions: Vec<BucketTransaction>,
    /// Ledger-level allocations. See [`AllocationDetail`].
    pub allocations: Vec<AllocationDetail>,
}

/// Grant points request (SDK-compatible).
///
/// `bucket_id` is REQUIRED: every grant must target an
/// explicit Credit Bucket. Missing → 400 `grant_bucket_required`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExtGrantPointsRequest {
    pub user_id: String,
    /// Required target Credit Bucket. Deserialized as `Option` so a missing
    /// field yields a structured 400 `grant_bucket_required` body instead of
    /// Axum's generic JSON parse error.
    pub bucket_id: Option<String>,
    pub amount: i64,
    pub reason: String,
    pub validity_days: Option<i64>,
}

/// Grant points response (SDK-compatible)
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExtGrantPointsResponse {
    pub transaction_id: String,
    pub user_id: String,
    pub bucket_id: String,
    pub amount: i64,
    pub granted_balance: i64,
    pub balance: i64,
    pub expires_at: Option<String>,
}

/// Get user points balance
///
/// Returns the current points balance for a user in the specified realm.
///
/// # Authentication
/// Requires valid API Key via X-API-Key header
///
/// # Realm Isolation
/// The API key must belong to the same realm as the requested realm.
/// Cross-realm requests will return 403 Forbidden.
///
/// # Example
/// ```bash
/// curl -X GET \
///   https://api.example.com/api/ext/points/realm123/balance?userId=user-123 \
///   -H "X-API-Key: your-api-key"
/// ```
#[utoipa::path(
    get,
    path = "/api/ext/points/{realmId}/balance",
    tag = "ext",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("userId" = Option<String>, Query, description = "User ID (optional)")
    ),
    responses(
        (status = 200, description = "Balance retrieved successfully", body = ExtPointsBalanceResponse),
        (status = 400, description = "Bad request - Invalid user ID format", body = ErrorResponse),
        (status = 401, description = "Unauthorized - Invalid or missing API Key", body = ErrorResponse),
        (status = 403, description = "Forbidden - Cross-realm access attempt", body = ErrorResponse),
        (status = 404, description = "Not found - User or account not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("api_key" = []))
)]
pub async fn get_balance_ext(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(realm_id): Path<String>,
    Query(query): Query<std::collections::HashMap<String, String>>,
) -> Response {
    let api_key_realm_id = identity.realm_id();

    tracing::info!(
        api_key_realm_id = %api_key_realm_id,
        request_realm_id = %realm_id,
        "Balance query requested"
    );

    // 1. Check realm isolation - API key must be for the requested realm
    if !identity.has_access_to_realm(&realm_id) {
        tracing::warn!(
            api_key_realm_id = %api_key_realm_id,
            request_realm_id = %realm_id,
            "Cross-realm access attempt blocked"
        );
        return json_error(StatusCode::FORBIDDEN, ErrorCode::CrossRealmAccessForbidden);
    }

    if let Err(resp) =
        require_principal_permission(&state, &identity, &realm_id, "points", "view").await
    {
        return resp.into_response();
    }

    // 2. Extract user_id from query parameter
    let user_id = match query.get("userId") {
        Some(user_id_str) => match user_id_str.parse::<Uuid>() {
            Ok(uuid) => uuid,
            Err(_) => {
                tracing::warn!("Invalid user ID format: {}", user_id_str);
                return json_error(StatusCode::BAD_REQUEST, ErrorCode::InvalidUserIdFormat);
            }
        },
        None => {
            tracing::warn!("Missing userId parameter");
            return json_error(StatusCode::BAD_REQUEST, ErrorCode::MissingUserId);
        }
    };

    // 3. Query balance. A client-app-bound key is scoped to the buckets its
    // app covers (same coverage set it can consume from); admin-api keys and
    // unbound keys see the realm-wide total.
    let bound_client_app_id = identity
        .as_third_party()
        .and_then(|api_key| api_key.client_app_id);
    let admin_key = match is_admin_api_key(&state, &identity).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };

    let balance_result = match bound_client_app_id {
        Some(client_app_id) if !admin_key => {
            state
                .points_service
                .get_balance_for_client_app(identity, &realm_id, user_id, client_app_id)
                .await
        }
        _ => {
            state
                .points_service
                .get_balance(identity, &realm_id, user_id)
                .await
        }
    };
    let balance = match balance_result {
        Ok(balance) => balance,
        Err(e) => {
            tracing::error!("Failed to query balance: {}", e);
            return match e {
                herald_core::domain::common::entities::app_errors::CoreError::Unauthorized => {
                    json_error(StatusCode::UNAUTHORIZED, ErrorCode::Unauthorized)
                }
                herald_core::domain::common::entities::app_errors::CoreError::Forbidden(_) => {
                    json_error(StatusCode::FORBIDDEN, ErrorCode::Forbidden)
                }
                herald_core::domain::common::entities::app_errors::CoreError::NotFound => {
                    json_error(StatusCode::NOT_FOUND, ErrorCode::WalletNotFound)
                }
                _ => json_error(StatusCode::INTERNAL_SERVER_ERROR, ErrorCode::InternalError),
            };
        }
    };

    // 4. Build response
    let response = ExtPointsBalanceResponse {
        user_id: balance.user_id.to_string(),
        balance: balance.balance,
        topup_balance: balance.topup_balance,
        subscription_balance: balance.subscription_balance,
        granted_balance: balance.granted_balance,
        registration_balance: balance.registration_balance,
        free_periodic_balance: balance.free_periodic_balance,
        total_paid_granted: balance.total_recharged,
        total_recharged: balance.total_recharged,
        total_consumed: balance.total_consumed,
        unit: balance.unit.clone(),
        currency: balance.unit,
        updated_at: balance.updated_at.to_rfc3339(),
    };

    tracing::info!(
        user_id = %balance.user_id,
        balance = %balance.balance,
        "Balance retrieved successfully"
    );

    Json(response).into_response()
}

/// Consume points from user account
///
/// Consumes points from a user's account for paid operations.
/// Returns transaction details including the new balance.
///
/// # Authentication
/// Requires valid API Key via X-API-Key header
///
/// # Realm Isolation
/// The API key must belong to the same realm as the requested realm.
/// Cross-realm requests will return 403 Forbidden.
///
/// # Example
/// ```bash
/// curl -X POST \
///   https://api.example.com/api/ext/points/realm123/consume \
///   -H "X-API-Key: your-api-key" \
///   -H "Content-Type: application/json" \
///   -d '{
///     "userId": "user-123",
///     "clientAppId": "app-abc",
///     "amount": 100,
///     "description": "AI API call"
///   }'
/// ```
#[utoipa::path(
    post,
    path = "/api/ext/points/{realmId}/consume",
    tag = "ext",
    params(
        ("realmId" = String, Path, description = "Realm ID")
    ),
    request_body = ExtConsumePointsRequest,
    responses(
        (status = 200, description = "Points consumed successfully", body = ExtConsumePointsResponse),
        (status = 400, description = "Bad request (invalid amount, frozen/closed account)", body = ErrorResponse),
        (status = 401, description = "Unauthorized - Invalid or missing API Key", body = ErrorResponse),
        (status = 403, description = "Forbidden - Cross-realm access attempt", body = ErrorResponse),
        (status = 404, description = "Not found - User or account not found", body = ErrorResponse),
        (status = 409, description = "Conflict - no covered credit pool for client_app (code=no_covered_pool) or insufficient points (code=insufficient_points, includes have/need)", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("api_key" = []))
)]
pub async fn consume_points_ext(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(realm_id): Path<String>,
    Json(request): Json<ExtConsumePointsRequest>,
) -> Response {
    let api_key_realm_id = identity.realm_id();

    tracing::info!(
        api_key_realm_id = %api_key_realm_id,
        request_realm_id = %realm_id,
        user_id = %request.user_id,
        amount = request.amount,
        "Points consumption requested"
    );

    // 0. Check realm isolation - API key must be for the requested realm.
    // This must run BEFORE any rate limiting: the limit keys are derived from
    // the caller's validated realm, otherwise a foreign-realm caller could
    // exhaust another realm's shared budget (cross-realm DoS).
    if !identity.has_access_to_realm(&realm_id) {
        tracing::warn!(
            api_key_realm_id = %api_key_realm_id,
            request_realm_id = %realm_id,
            "Cross-realm access attempt blocked"
        );
        return json_error(StatusCode::FORBIDDEN, ErrorCode::CrossRealmAccessForbidden);
    }

    // 1. Apply rate limiting (parallel checks for better performance),
    // keyed on the API key's own (already validated) realm.
    let (realm_result, user_result) = tokio::join!(
        rate_limit(
            &state,
            format!("{}{}", REALM_RATE_LIMIT_PREFIX, api_key_realm_id),
            REALM_RATE_LIMIT
        ),
        rate_limit(
            &state,
            format!(
                "{}{}:{}",
                USER_RATE_LIMIT_PREFIX, api_key_realm_id, request.user_id
            ),
            USER_RATE_LIMIT
        )
    );

    if let Err(e) = realm_result {
        tracing::warn!(
            realm_id = %api_key_realm_id,
            error = %e,
            "Realm-level rate limit exceeded"
        );
        return json_error(StatusCode::TOO_MANY_REQUESTS, ErrorCode::RateLimitExceeded);
    }

    if let Err(e) = user_result {
        tracing::warn!(
            realm_id = %api_key_realm_id,
            user_id = %request.user_id,
            error = %e,
            "User-level rate limit exceeded"
        );
        return json_error(StatusCode::TOO_MANY_REQUESTS, ErrorCode::RateLimitExceeded);
    }

    if let Err(resp) =
        require_principal_permission(&state, &identity, &realm_id, "points", "manage").await
    {
        return resp.into_response();
    }

    let client_app_id = match request.client_app_id.parse::<Uuid>() {
        Ok(uuid) => uuid,
        Err(_) => {
            tracing::warn!("Invalid client_app_id format: {}", request.client_app_id);
            return json_error(StatusCode::BAD_REQUEST, ErrorCode::InvalidClientAppIdFormat);
        }
    };

    if let Err(resp) = ensure_client_app_scope(&state, &identity, client_app_id).await {
        return resp;
    }

    // 2. Check idempotency if key is provided
    if let Some(ref idempotency_key) = request.idempotency_key {
        let idempotency_service = &state.idempotency_service;
        // Scope idempotency keys to the calling principal, not just the realm:
        // multiple API keys (potentially bound to different client apps) share
        // a realm, and a realm-wide namespace would let one key replay or
        // suppress another key's transaction by colliding on the key string.
        let idempotency_scope = format!("{}:{}", realm_id, identity.id());
        let request_data = serde_json::to_string(&request).unwrap_or_else(|_| {
            tracing::warn!(
                idempotency_key = %idempotency_key,
                "Failed to serialize idempotency request data, using empty string"
            );
            String::new()
        });

        match idempotency_service
            .check_or_create(&idempotency_scope, idempotency_key, &request_data)
            .await
        {
            Ok(herald_core::domain::points::IdempotencyResult::Cached { transaction }) => {
                // Idempotent replay. The cached `transaction` is
                // the primary transaction (first by bucket_id ASC). To return the
                // FULL original result set across the correlation_id group
                // (multi-pool consume shares one correlation_id across N
                // per-bucket transactions), reassemble the siblings from the
                // primary WITHOUT re-deducting. Legacy single-pool rows
                // (correlation_id = NULL) replay as the single primary.
                let response = match state
                    .points_service
                    .replay_consume(&realm_id, transaction.id)
                    .await
                {
                    Ok(siblings) => {
                        let mut sorted = siblings;
                        sorted.sort_by_key(|t| t.bucket_id);
                        let primary = sorted.first().unwrap_or(&transaction);

                        // Surface the original consume's ledger-level
                        // allocations. Multi-pool replays
                        // share a correlation_id; legacy single-pool rows have
                        // none and surface an empty slice.
                        let allocations = match primary.correlation_id.as_deref() {
                            Some(cid) => {
                                match state
                                    .points_service
                                    .find_consumption_allocations_by_correlation_id(&realm_id, cid)
                                    .await
                                {
                                    Ok(views) => {
                                        views.into_iter().map(allocation_view_to_detail).collect()
                                    }
                                    Err(e) => {
                                        tracing::warn!(
                                            correlation_id = %cid,
                                            error = %e,
                                            "Failed to load replay allocations; \
                                             returning empty slice"
                                        );
                                        Vec::new()
                                    }
                                }
                            }
                            None => Vec::new(),
                        };

                        ExtConsumePointsResponse {
                            user_id: primary.user_id.to_string(),
                            amount: request.amount,
                            correlation_id: primary
                                .correlation_id
                                .clone()
                                .unwrap_or_else(|| primary.id.to_string()),
                            transactions: sorted
                                .iter()
                                .map(|t| BucketTransaction {
                                    transaction_id: t.id.to_string(),
                                    bucket_id: t.bucket_id.to_string(),
                                    wallet_id: t.wallet_id.to_string(),
                                    user_id: t.user_id.to_string(),
                                    amount: t.amount.abs(),
                                    balance_after: t.balance_after,
                                })
                                .collect(),
                            allocations,
                        }
                    }
                    Err(e) => {
                        tracing::error!(
                            idempotency_key = %idempotency_key,
                            primary_transaction_id = %transaction.id,
                            error = %e,
                            "Failed to reassemble consume replay from primary transaction"
                        );
                        // Fall back to the cached primary-only response rather
                        // than failing the replay outright — the deduction has
                        // already happened; surfacing a partial result is safer
                        // than erroring on an already-completed operation.
                        build_consume_response_from_primary(&transaction, Vec::new())
                    }
                };

                tracing::info!(
                    idempotency_key = %idempotency_key,
                    primary_transaction_id = %transaction.id,
                    bucket_count = response.transactions.len(),
                    "Returning cached idempotent response"
                );

                return Json(response).into_response();
            }
            Ok(herald_core::domain::points::IdempotencyResult::New) => {
                // Proceed with normal processing
            }
            Err(e) => {
                tracing::error!(
                    idempotency_key = %idempotency_key,
                    error = %e,
                    "Idempotency check failed"
                );
                return match e {
                    herald_core::domain::common::entities::app_errors::CoreError::Conflict(_) => {
                        json_error(StatusCode::CONFLICT, ErrorCode::IdempotencyConflict)
                    }
                    _ => json_error(StatusCode::INTERNAL_SERVER_ERROR, ErrorCode::InternalError),
                };
            }
        }
    }

    // 3. Validate amount range (1 point to 1,000,000 points)
    if request.amount <= 0 || request.amount > 1_000_000 {
        return json_error(StatusCode::BAD_REQUEST, ErrorCode::InvalidAmount);
    }

    // 3. Parse user_id
    let user_id = match request.user_id.parse::<Uuid>() {
        Ok(uuid) => uuid,
        Err(_) => {
            tracing::warn!("Invalid user ID format: {}", request.user_id);
            return json_error(StatusCode::BAD_REQUEST, ErrorCode::InvalidUserIdFormat);
        }
    };

    // 5. Create input DTO
    let input = ConsumePointsInput {
        user_id: user_id.to_string(),
        client_app_id: client_app_id.to_string(),
        amount: request.amount,
        description: request.description.clone(),
    };

    // 6. Consume points — returns one transaction per affected bucket.
    // All share one correlation_id; each carries its own
    // wallet_id/bucket_id/balance_after.
    let transactions = match state
        .points_service
        .consume_points(identity.clone(), &realm_id, input)
        .await
    {
        Ok(transactions) => transactions,
        Err(e) => {
            tracing::error!("Failed to consume points: {}", e);
            return map_consume_error(e);
        }
    };

    // 7. Sort transactions by bucket_id ASC (deterministic output, mirrors the
    // infra write order; transaction.amount is stored negative — deduction
    // magnitude is its absolute value).
    let mut sorted = transactions.clone();
    sorted.sort_by_key(|t| t.bucket_id);

    let primary = match sorted.first() {
        Some(t) => t,
        None => {
            tracing::error!("Consume returned no transactions");
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, ErrorCode::InternalError);
        }
    };

    // Surface the ledger-level allocations of this consume.
    // Multi-bucket consumes share one correlation_id across their N transactions;
    // legacy single-pool rows (NULL correlation_id) have no grouping key and
    // surface an empty slice.
    let allocations = match primary.correlation_id.as_deref() {
        Some(cid) => match state
            .points_service
            .find_consumption_allocations_by_correlation_id(&realm_id, cid)
            .await
        {
            Ok(views) => views.into_iter().map(allocation_view_to_detail).collect(),
            Err(e) => {
                tracing::warn!(
                    correlation_id = %cid,
                    error = %e,
                    "Failed to load consume allocations; returning empty slice"
                );
                Vec::new()
            }
        },
        None => Vec::new(),
    };

    let response = ExtConsumePointsResponse {
        user_id: primary.user_id.to_string(),
        amount: request.amount,
        correlation_id: primary
            .correlation_id
            .clone()
            .unwrap_or_else(|| primary.id.to_string()),
        transactions: sorted
            .iter()
            .map(|t| BucketTransaction {
                transaction_id: t.id.to_string(),
                bucket_id: t.bucket_id.to_string(),
                wallet_id: t.wallet_id.to_string(),
                user_id: t.user_id.to_string(),
                amount: t.amount.abs(),
                balance_after: t.balance_after,
            })
            .collect(),
        allocations,
    };

    tracing::info!(
        primary_transaction_id = %primary.id,
        correlation_id = %response.correlation_id,
        user_id = %primary.user_id,
        amount = request.amount,
        bucket_count = response.transactions.len(),
        "Points consumed successfully (per-bucket transactions)"
    );

    // Audit trail: distinguish the API key that performed the deduction
    // (actor) from the user whose points were spent (subject). Enables
    // post-incident attribution and key-scoped revocation.
    tracing::info!(
        target: "herald.audit.points_consume",
        actor_api_key_id = %identity.id(),
        actor_client_app_id = %client_app_id,
        subject_user_id = %user_id,
        amount = request.amount,
        correlation_id = %response.correlation_id,
        realm_id = %realm_id,
        "points consumed"
    );

    // 8. Save idempotency result if key was provided — cache the primary
    // transaction (first by bucket_id ASC), matching the infra primary_txn_id
    // semantics.
    if let Some(ref idempotency_key) = request.idempotency_key {
        let idempotency_service = &state.idempotency_service;
        // Must match the scope used by check_or_create above.
        let idempotency_scope = format!("{}:{}", realm_id, identity.id());
        if let Err(e) = idempotency_service
            .save_result(&idempotency_scope, idempotency_key, primary)
            .await
        {
            tracing::error!(
                idempotency_key = %idempotency_key,
                transaction_id = %primary.id,
                error = %e,
                "Failed to save idempotency result"
            );
            // Non-critical error, don't fail the request
        }
    }

    Json(response).into_response()
}

/// Grant points to a user account (SDK)
///
/// Grants points to a user's account from a third-party application.
/// Returns transaction details including the new balance.
///
/// # Authentication
/// Requires valid API Key via X-API-Key header
///
/// # Realm Isolation
/// The API key must belong to the same realm as the requested realm.
/// Cross-realm requests will return 403 Forbidden.
///
/// # Example
/// ```bash
/// curl -X POST \
///   https://api.example.com/api/ext/points/realm123/grant \
///   -H "X-API-Key: your-api-key" \
///   -H "Content-Type: application/json" \
///   -d '{
///     "userId": "user-123",
///     "amount": 100,
///     "reason": "Promotional grant"
///   }'
/// ```
#[utoipa::path(
    post,
    path = "/api/ext/points/{realmId}/grant",
    tag = "ext",
    params(
        ("realmId" = String, Path, description = "Realm ID")
    ),
    request_body = ExtGrantPointsRequest,
    responses(
        (status = 200, description = "Points granted successfully", body = ExtGrantPointsResponse),
        (status = 400, description = "Bad request (invalid amount, invalid user ID, empty reason, missing/invalid bucketId → code=grant_bucket_required)", body = ErrorResponse),
        (status = 401, description = "Unauthorized - Invalid or missing API Key", body = ErrorResponse),
        (status = 403, description = "Forbidden - Cross-realm access attempt", body = ErrorResponse),
        (status = 404, description = "Not found - User or account not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("api_key" = []))
)]
pub async fn grant_points_ext(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(realm_id): Path<String>,
    Json(request): Json<ExtGrantPointsRequest>,
) -> Response {
    let api_key_realm_id = identity.realm_id();

    tracing::info!(
        api_key_realm_id = %api_key_realm_id,
        request_realm_id = %realm_id,
        user_id = %request.user_id,
        amount = request.amount,
        "Points grant requested"
    );

    // 0. Check realm isolation. Runs BEFORE rate limiting for the same
    // cross-realm DoS reason as the consume path.
    if !identity.has_access_to_realm(&realm_id) {
        tracing::warn!(
            api_key_realm_id = %api_key_realm_id,
            request_realm_id = %realm_id,
            "Cross-realm access attempt blocked"
        );
        return json_error(StatusCode::FORBIDDEN, ErrorCode::CrossRealmAccessForbidden);
    }

    // 1. Apply rate limiting (realm 100/min, user 20/min), keyed on the API
    // key's own (already validated) realm.
    let (realm_result, user_result) = tokio::join!(
        rate_limit(
            &state,
            format!("{}{}", REALM_RATE_LIMIT_PREFIX, api_key_realm_id),
            REALM_RATE_LIMIT
        ),
        rate_limit(
            &state,
            format!(
                "{}{}:{}",
                USER_RATE_LIMIT_PREFIX, api_key_realm_id, request.user_id
            ),
            USER_RATE_LIMIT
        )
    );

    if let Err(e) = realm_result {
        tracing::warn!(
            realm_id = %api_key_realm_id,
            error = %e,
            "Realm-level rate limit exceeded"
        );
        return json_error(StatusCode::TOO_MANY_REQUESTS, ErrorCode::RateLimitExceeded);
    }

    if let Err(e) = user_result {
        tracing::warn!(
            realm_id = %api_key_realm_id,
            user_id = %request.user_id,
            error = %e,
            "User-level rate limit exceeded"
        );
        return json_error(StatusCode::TOO_MANY_REQUESTS, ErrorCode::RateLimitExceeded);
    }

    if let Err(resp) =
        require_principal_permission(&state, &identity, &realm_id, "points", "manage").await
    {
        return resp.into_response();
    }

    // 2. Validate amount (1 to 1,000,000)
    if request.amount <= 0 || request.amount > 1_000_000 {
        return json_error(StatusCode::BAD_REQUEST, ErrorCode::InvalidAmount);
    }

    // 3. Parse user_id as UUID
    let user_id = match request.user_id.parse::<Uuid>() {
        Ok(uuid) => uuid,
        Err(_) => {
            tracing::warn!("Invalid user ID format: {}", request.user_id);
            return json_error(StatusCode::BAD_REQUEST, ErrorCode::InvalidUserIdFormat);
        }
    };

    // 4. bucketId is REQUIRED: every grant must target an
    // explicit Credit Bucket. Missing → 400 `grant_bucket_required`; present
    // but malformed → 400 grant_bucket_required as well (consumers should fix
    // the request either way).
    let bucket_id = match request.bucket_id.as_deref().map(str::trim) {
        None | Some("") => {
            tracing::warn!("Missing required bucketId for points grant");
            return grant_bucket_required_error();
        }
        Some(raw) => match raw.parse::<Uuid>() {
            Ok(uuid) => uuid,
            Err(_) => {
                tracing::warn!("Invalid bucketId format: {}", raw);
                return grant_bucket_required_error();
            }
        },
    };

    // 4b. Client-app scope: a key bound to one app may only grant into
    // buckets covered by that app (consumption draws exclusively from
    // covered buckets; grants must respect the same boundary, otherwise a
    // bound key could mint points into other apps' pools).
    if let Err(resp) =
        ensure_bucket_in_client_app_scope(&state, &identity, &realm_id, bucket_id).await
    {
        return resp;
    }

    // 5. Validate reason non-empty
    if request.reason.trim().is_empty() {
        return json_error(StatusCode::BAD_REQUEST, ErrorCode::ValidationError);
    }

    // 6. Validate validity_days is None or > 0
    if let Some(days) = request.validity_days
        && days <= 0
    {
        return json_error(StatusCode::BAD_REQUEST, ErrorCode::ValidationError);
    }

    // 7. Determine source_id from API key identity
    let source_id = identity
        .as_third_party()
        .map(|api_key| {
            api_key
                .client_app_id
                .map(|id| id.to_string())
                .unwrap_or_else(|| api_key.id.clone())
        })
        .unwrap_or_default();

    // 8. Build input and call service
    let input = GrantPointsInput {
        user_id,
        bucket_id,
        amount: request.amount,
        reason: request.reason,
        validity_days: request.validity_days,
        source_type: CreditSourceType::SdkGrant,
        source_id,
    };

    let output = match state
        .points_service
        .grant_points_for_sdk(&realm_id, input)
        .await
    {
        Ok(output) => output,
        Err(e) => {
            tracing::error!("Failed to grant points: {}", e);
            return match e {
                herald_core::domain::common::entities::app_errors::CoreError::Unauthorized => {
                    json_error(StatusCode::UNAUTHORIZED, ErrorCode::Unauthorized)
                }
                herald_core::domain::common::entities::app_errors::CoreError::Forbidden(_) => {
                    json_error(StatusCode::FORBIDDEN, ErrorCode::Forbidden)
                }
                herald_core::domain::common::entities::app_errors::CoreError::NotFound => {
                    json_error(StatusCode::NOT_FOUND, ErrorCode::UserNotFound)
                }
                herald_core::domain::common::entities::app_errors::CoreError::GrantBucketRequired => {
                    grant_bucket_required_error()
                }
                herald_core::domain::common::entities::app_errors::CoreError::BadRequest(_) => {
                    json_error(StatusCode::BAD_REQUEST, ErrorCode::ValidationError)
                }
                _ => json_error(StatusCode::INTERNAL_SERVER_ERROR, ErrorCode::InternalError),
            };
        }
    };

    // 9. Build response (ext convention: balance instead of totalBalance).
    // bucketId echoes the request target (service output doesn't carry it).
    let response = ExtGrantPointsResponse {
        transaction_id: output.transaction_id.to_string(),
        user_id: output.user_id.to_string(),
        bucket_id: bucket_id.to_string(),
        amount: output.amount,
        granted_balance: output.granted_balance,
        balance: output.total_balance,
        expires_at: output.expires_at.map(|dt| dt.to_rfc3339()),
    };

    tracing::info!(
        transaction_id = %output.transaction_id,
        user_id = %output.user_id,
        bucket_id = %bucket_id,
        amount = output.amount,
        balance = output.total_balance,
        "Points granted successfully"
    );

    Json(response).into_response()
}

// =============================================================================
// Consume / grant helpers
// =============================================================================

/// Map a repository consumption-allocation view (allocation + ledger credit
/// type) to the SDK response `AllocationDetail`.
fn allocation_view_to_detail(view: ConsumptionAllocationView) -> AllocationDetail {
    AllocationDetail {
        bucket_id: view.allocation.bucket_id.to_string(),
        wallet_id: view
            .allocation
            .wallet_id
            .map(|w| w.to_string())
            .unwrap_or_default(),
        ledger_id: view.allocation.ledger_id.to_string(),
        credit_type: view.credit_type.to_string(),
        allocated_amount: view.allocation.allocated_amount,
    }
}

/// Build a consume response from a single primary transaction (used by the
/// idempotency replay path, which only surfaces the cached primary transaction
/// — the infra `primary_txn_id`).
///
/// `amount` is the deduction magnitude (transaction rows store it negative).
fn build_consume_response_from_primary(
    transaction: &PointsTransaction,
    allocations: Vec<AllocationDetail>,
) -> ExtConsumePointsResponse {
    ExtConsumePointsResponse {
        user_id: transaction.user_id.to_string(),
        amount: transaction.amount.abs(),
        correlation_id: transaction
            .correlation_id
            .clone()
            .unwrap_or_else(|| transaction.id.to_string()),
        transactions: vec![BucketTransaction {
            transaction_id: transaction.id.to_string(),
            bucket_id: transaction.bucket_id.to_string(),
            wallet_id: transaction.wallet_id.to_string(),
            user_id: transaction.user_id.to_string(),
            amount: transaction.amount.abs(),
            balance_after: transaction.balance_after,
        }],
        allocations,
    }
}

/// Structured error body used by the consume error contract.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConsumeErrorBody {
    code: &'static str,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    have: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    need: Option<i64>,
}

/// Map a consume `CoreError` to the consume error contract:
/// - `NoCoveredPointsPool` → 409 `no_covered_pool`
/// - `insufficient_points` (materialized as `BadRequest("Insufficient points
///   balance. Required: {need}, Available: {have}")`) → 409 `insufficient_points`
///   with `have`/`need`
/// - frozen/closed wallet → 400 `wallet_frozen_or_closed`
/// - other `BadRequest` → 400 `invalid_amount`
fn map_consume_error(e: CoreError) -> Response {
    use herald_core::domain::common::entities::app_errors::CoreError;
    match e {
        CoreError::Unauthorized => json_error(StatusCode::UNAUTHORIZED, ErrorCode::Unauthorized),
        CoreError::Forbidden(_) => json_error(StatusCode::FORBIDDEN, ErrorCode::Forbidden),
        CoreError::NotFound => json_error(StatusCode::NOT_FOUND, ErrorCode::WalletNotFound),
        CoreError::NoCoveredPointsPool { client_app_id } => {
            tracing::warn!(
                client_app_id = %client_app_id,
                "Consume rejected: no covered credit pool"
            );
            ApiError::conflict_json(ConsumeErrorBody {
                code: "no_covered_pool",
                message: format!(
                    "Client app {client_app_id} does not cover any available credit bucket"
                ),
                have: None,
                need: None,
            })
            .into_response()
        }
        CoreError::BadRequest(ref msg) if msg.starts_with("Insufficient points balance") => {
            let (have, need) = parse_have_need(msg).unwrap_or((None, None));
            ApiError::conflict_json(ConsumeErrorBody {
                code: "insufficient_points",
                message: "Insufficient points balance for the covered credit pools".to_string(),
                have,
                need,
            })
            .into_response()
        }
        CoreError::BadRequest(ref msg) if msg.contains("Cannot consume points from") => {
            json_error(StatusCode::BAD_REQUEST, ErrorCode::WalletFrozenOrClosed)
        }
        CoreError::BadRequest(_) => json_error(StatusCode::BAD_REQUEST, ErrorCode::InvalidAmount),
        CoreError::Conflict(_) => {
            json_error(StatusCode::CONFLICT, ErrorCode::ConcurrentModification)
        }
        _ => json_error(StatusCode::INTERNAL_SERVER_ERROR, ErrorCode::InternalError),
    }
}

/// Parse `have`/`need` out of the `insufficient_points` message produced by
/// `PointsErrorExt::insufficient_points`:
/// `"Insufficient points balance. Required: {need}, Available: {have}"`.
fn parse_have_need(msg: &str) -> Option<(Option<i64>, Option<i64>)> {
    let need = msg
        .split("Required:")
        .nth(1)
        .and_then(|s| s.split(',').next())
        .and_then(|s| s.trim().parse::<i64>().ok());
    let have = msg
        .split("Available:")
        .nth(1)
        .and_then(|s| s.trim().parse::<i64>().ok());
    Some((have, need))
}

/// Structured 400 `grant_bucket_required` body.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GrantBucketRequiredBody {
    code: &'static str,
    message: &'static str,
}

fn grant_bucket_required_error() -> Response {
    ApiError::bad_request_json(GrantBucketRequiredBody {
        code: "grant_bucket_required",
        message: "Points grant requires an explicit target bucket",
    })
    .into_response()
}

/// Transaction response (SDK-compatible)
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExtTransactionResponse {
    pub transaction_id: String,
    pub wallet_id: String,
    pub user_id: String,
    pub transaction_type: String,
    pub amount: i64,
    pub balance_after: i64,
    pub description: Option<String>,
    pub client_app_id: Option<String>,
    pub subscription_id: Option<String>,
    pub external_ref_id: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct ExtTransactionByRefQuery {
    pub user_id: Option<String>,
}

fn transaction_to_response(
    transaction: &herald_core::domain::points::entities::PointsTransaction,
) -> ExtTransactionResponse {
    ExtTransactionResponse {
        transaction_id: transaction.id.to_string(),
        wallet_id: transaction.wallet_id.to_string(),
        user_id: transaction.user_id.to_string(),
        transaction_type: transaction.transaction_type.to_string(),
        amount: transaction.amount,
        balance_after: transaction.balance_after,
        description: transaction.description.clone(),
        client_app_id: transaction.client_app_id.map(|id| id.to_string()),
        subscription_id: transaction.subscription_id.map(|id| id.to_string()),
        external_ref_id: transaction.external_ref_id.clone(),
        created_at: transaction.created_at.to_rfc3339(),
    }
}

/// Get transaction by ID
///
/// Returns details of a specific points transaction.
///
/// # Authentication
/// Requires valid API Key via X-API-Key header
///
/// # Realm Isolation
/// The API key must belong to the same realm as the requested realm.
/// Cross-realm requests will return 403 Forbidden.
///
/// # Example
/// ```bash
/// curl -X GET \
///   https://api.example.com/api/ext/points/realm123/transactions/uuid \
///   -H "X-API-Key: your-api-key"
/// ```
#[utoipa::path(
    get,
    path = "/api/ext/points/{realmId}/transactions/{transactionId}",
    tag = "ext",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("transactionId" = String, Path, description = "Transaction ID")
    ),
    responses(
        (status = 200, description = "Transaction retrieved successfully", body = ExtTransactionResponse),
        (status = 400, description = "Bad request - Invalid ID format", body = ErrorResponse),
        (status = 401, description = "Unauthorized - Invalid or missing API Key", body = ErrorResponse),
        (status = 403, description = "Forbidden - Cross-realm access attempt", body = ErrorResponse),
        (status = 404, description = "Not found - Transaction not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("api_key" = []))
)]
pub async fn get_transaction_ext(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path((realm_id, transaction_id_str)): Path<(String, String)>,
) -> Response {
    let api_key_realm_id = identity.realm_id();

    tracing::info!(
        api_key_realm_id = %api_key_realm_id,
        request_realm_id = %realm_id,
        transaction_id = %transaction_id_str,
        "Transaction query requested"
    );

    // 1. Check realm isolation - API key must be for the requested realm
    if !identity.has_access_to_realm(&realm_id) {
        tracing::warn!(
            api_key_realm_id = %api_key_realm_id,
            request_realm_id = %realm_id,
            "Cross-realm access attempt blocked"
        );
        return json_error(StatusCode::FORBIDDEN, ErrorCode::CrossRealmAccessForbidden);
    }

    if let Err(resp) =
        require_principal_permission(&state, &identity, &realm_id, "points", "view").await
    {
        return resp.into_response();
    }

    // 2. Parse transaction_id
    let transaction_id = match transaction_id_str.parse::<Uuid>() {
        Ok(uuid) => uuid,
        Err(_) => {
            tracing::warn!("Invalid transaction ID format: {}", transaction_id_str);
            return json_error(
                StatusCode::BAD_REQUEST,
                ErrorCode::InvalidTransactionIdFormat,
            );
        }
    };

    // 3. Get transaction
    let transaction = match state
        .points_service
        .get_transaction(identity.clone(), &realm_id, transaction_id)
        .await
    {
        Ok(transaction) => transaction,
        Err(e) => {
            tracing::error!("Failed to get transaction: {}", e);
            return match e {
                herald_core::domain::common::entities::app_errors::CoreError::Unauthorized => {
                    json_error(StatusCode::UNAUTHORIZED, ErrorCode::Unauthorized)
                }
                herald_core::domain::common::entities::app_errors::CoreError::Forbidden(_) => {
                    json_error(StatusCode::FORBIDDEN, ErrorCode::Forbidden)
                }
                herald_core::domain::common::entities::app_errors::CoreError::NotFound => {
                    json_error(StatusCode::NOT_FOUND, ErrorCode::TransactionNotFound)
                }
                _ => json_error(StatusCode::INTERNAL_SERVER_ERROR, ErrorCode::InternalError),
            };
        }
    };

    match transaction.client_app_id {
        Some(client_app_id) => {
            if let Err(resp) = ensure_client_app_scope(&state, &identity, client_app_id).await {
                return resp;
            }
        }
        // Transactions without client-app attribution (admin/SDK grants,
        // registration credits) are realm-level records: a key bound to one
        // client app must not read them — only realm-admin (unbound) keys can.
        None => match is_admin_api_key(&state, &identity).await {
            Ok(true) => {}
            Ok(false) => return json_error(StatusCode::FORBIDDEN, ErrorCode::Forbidden),
            Err(resp) => return resp,
        },
    }

    let response = transaction_to_response(&transaction);

    tracing::info!(
        transaction_id = %transaction.id,
        "Transaction retrieved successfully"
    );

    Json(response).into_response()
}

/// Get transaction by external reference ID
///
/// Returns details of a specific points transaction by `externalRefId`.
#[utoipa::path(
    get,
    path = "/api/ext/points/{realmId}/transactions/by-external-ref/{externalRefId}",
    tag = "ext",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("externalRefId" = String, Path, description = "External reference ID"),
        ExtTransactionByRefQuery
    ),
    responses(
        (status = 200, description = "Transaction retrieved successfully", body = ExtTransactionResponse),
        (status = 400, description = "Bad request - invalid user ID or non-unique external reference", body = ErrorResponse),
        (status = 401, description = "Unauthorized - Invalid or missing API Key", body = ErrorResponse),
        (status = 403, description = "Forbidden - Cross-realm access attempt", body = ErrorResponse),
        (status = 404, description = "Not found - Transaction not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("api_key" = []))
)]
pub async fn get_transaction_by_external_ref_ext(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path((realm_id, external_ref_id)): Path<(String, String)>,
    Query(query): Query<ExtTransactionByRefQuery>,
) -> Response {
    let api_key_realm_id = identity.realm_id();

    tracing::info!(
        api_key_realm_id = %api_key_realm_id,
        request_realm_id = %realm_id,
        external_ref_id = %external_ref_id,
        "Transaction query by external ref requested"
    );

    if external_ref_id.trim().is_empty() {
        return json_error(StatusCode::BAD_REQUEST, ErrorCode::ValidationError);
    }

    if !identity.has_access_to_realm(&realm_id) {
        return json_error(StatusCode::FORBIDDEN, ErrorCode::CrossRealmAccessForbidden);
    }

    if let Err(resp) =
        require_principal_permission(&state, &identity, &realm_id, "points", "view").await
    {
        return resp.into_response();
    }

    let user_id = match query.user_id {
        Some(raw_user_id) => match raw_user_id.parse::<Uuid>() {
            Ok(uuid) => Some(uuid),
            Err(_) => return json_error(StatusCode::BAD_REQUEST, ErrorCode::InvalidUserIdFormat),
        },
        None => None,
    };

    let filters = TransactionFilters {
        user_id,
        external_ref_id,
        page: Some(1),
        page_size: Some(2),
        ..Default::default()
    };

    let result = match state
        .points_service
        .list_transactions(identity.clone(), &realm_id, filters)
        .await
    {
        Ok(result) => result,
        Err(e) => {
            tracing::error!("Failed to get transaction by external ref: {}", e);
            return match e {
                herald_core::domain::common::entities::app_errors::CoreError::Unauthorized => {
                    json_error(StatusCode::UNAUTHORIZED, ErrorCode::Unauthorized)
                }
                herald_core::domain::common::entities::app_errors::CoreError::Forbidden(_) => {
                    json_error(StatusCode::FORBIDDEN, ErrorCode::Forbidden)
                }
                _ => json_error(StatusCode::INTERNAL_SERVER_ERROR, ErrorCode::InternalError),
            };
        }
    };

    if result.data.is_empty() {
        return json_error(StatusCode::NOT_FOUND, ErrorCode::TransactionNotFound);
    }
    if result.data.len() > 1 {
        return json_error(StatusCode::BAD_REQUEST, ErrorCode::ValidationError);
    }

    let transaction = result.data.into_iter().next().expect("checked non-empty");
    match transaction.client_app_id {
        Some(client_app_id) => {
            if let Err(resp) = ensure_client_app_scope(&state, &identity, client_app_id).await {
                return resp;
            }
        }
        // Unattributed (admin/SDK grant) transactions are realm-level records;
        // restrict them to realm-admin (unbound) keys, same as get_transaction.
        None => match is_admin_api_key(&state, &identity).await {
            Ok(true) => {}
            Ok(false) => return json_error(StatusCode::FORBIDDEN, ErrorCode::Forbidden),
            Err(resp) => return resp,
        },
    }

    Json(transaction_to_response(&transaction)).into_response()
}
