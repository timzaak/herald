//! Internal quota-entitlement endpoints (demo/test-only).
//!
//! These routes bypass normal user authentication and are guarded solely by
//! `internal_api_key_middleware` (the shared `X-Internal-API-Key` /
//! `INTERNAL_API_KEY` secret). They let fast demo E2E tests construct a
//! `PointsQuotaEntitlement` directly — replicating what the Stripe webhook path
//! (`handle_subscription_paid` → `subscription_service.grant_quota_entitlement`)
//! would produce — without driving a real Stripe checkout.
//!
//! See `.ai/task/support-multiple-price/demo/dev/DE-D01-shared-infra-and-seed.md`
//! §27: seeded Stripe price IDs are placeholders and must never be driven through
//! real Stripe. Fast demos therefore grant quota via these endpoints instead of
//! the purchase flow.

use axum::{
    Json,
    extract::{Path, State},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;
use validator::Validate;

use herald_api_base::application::http::server::api_entities::{ApiError, ErrorResponse};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::common::entities::now_utc;
use herald_core::domain::points::entities::{
    CreditType, QuotaEntitlementStatus, QuotaSourceType, QuotaWindow,
};

/// A single window in a grant request. Mirrors `QuotaWindow` but accepts raw
/// seconds + limit + the config-derived stable display `key`.
#[derive(Debug, Deserialize, Serialize, ToSchema, Validate)]
#[serde(rename_all = "camelCase")]
pub struct InternalQuotaWindowInput {
    pub key: String,
    #[validate(range(min = 1))]
    pub window_seconds: i64,
    #[validate(range(min = 0))]
    pub limit: i64,
}

#[derive(Debug, Deserialize, Serialize, ToSchema, Validate)]
#[serde(rename_all = "camelCase")]
pub struct GrantQuotaEntitlementRequest {
    pub user_id: Uuid,
    pub bucket_id: Uuid,
    /// `subscription_credit` or `free_periodic_credit` (only those two feed the
    /// dashboard window view). Defaults to `subscription_credit`.
    pub credit_type: Option<String>,
    /// Defaults to `subscription_initial`.
    pub source_type: Option<String>,
    /// Stable anchor identifying this grant. Revoke targets the same value, so
    /// callers use a fixed `source_id` across the grant/revoke pair to obtain a
    /// clean baseline between tests.
    pub source_id: String,
    #[validate(nested)]
    pub windows: Vec<InternalQuotaWindowInput>,
    /// RFC3339 timestamp; omit/leave null for an entitlement that never expires.
    pub effective_until: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GrantQuotaEntitlementResponse {
    pub entitlement_id: Uuid,
    pub status: String,
}

#[derive(Debug, Deserialize, Serialize, ToSchema, Validate)]
#[serde(rename_all = "camelCase")]
pub struct RevokeQuotaEntitlementRequest {
    pub user_id: Uuid,
    pub bucket_id: Uuid,
    pub credit_type: Option<String>,
    pub source_id: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RevokeQuotaEntitlementResponse {
    pub revoked: bool,
}

/// Reject requests whose user or credit bucket does not exist in the path
/// realm. The internal key is global, so without this check a caller could
/// seed quota entitlements for arbitrary (cross-realm or nonexistent) users
/// and buckets — the domain grant path itself writes rows verbatim.
async fn ensure_user_and_bucket_in_realm(
    state: &AppState,
    realm_id: &str,
    user_id: Uuid,
    bucket_id: Uuid,
) -> Result<(), ApiError> {
    let user_ok = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM account WHERE id = $1 AND realm_id = $2",
    )
    .bind(user_id)
    .bind(realm_id)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, realm_id, "internal quota: user lookup failed");
        ApiError::internal("Internal server error")
    })?;
    if user_ok == 0 {
        return Err(ApiError::bad_request(format!(
            "user {user_id} does not exist in realm {realm_id}"
        )));
    }

    let bucket_ok = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM credit_buckets WHERE id = $1 AND realm_id = $2",
    )
    .bind(bucket_id)
    .bind(realm_id)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, realm_id, "internal quota: bucket lookup failed");
        ApiError::internal("Internal server error")
    })?;
    if bucket_ok == 0 {
        return Err(ApiError::bad_request(format!(
            "credit bucket {bucket_id} does not exist in realm {realm_id}"
        )));
    }

    Ok(())
}

/// Grant (or idempotently re-grant) a quota entitlement.
///
/// `idempotency_key` is derived as `internal:{source_id}` so a replayed grant
/// converges on the same row (matching the `UNIQUE(realm_id, user_id, bucket_id,
/// credit_type, idempotency_key)` constraint).
#[utoipa::path(
    post,
    path = "/api/internal/points/{realmId}/quota-entitlement/grant",
    params(
        ("realmId" = String, Path, description = "Realm ID")
    ),
    request_body = GrantQuotaEntitlementRequest,
    responses(
        (status = 200, description = "Quota entitlement granted", body = GrantQuotaEntitlementResponse),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Unauthorized — missing/invalid X-Internal-API-Key", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "InternalPoints"
)]
#[tracing::instrument(skip_all, fields(db.operation = "internal_grant_quota_entitlement"))]
pub async fn grant_quota_entitlement(
    State(state): State<AppState>,
    Path(realm_id): Path<String>,
    Json(input): Json<GrantQuotaEntitlementRequest>,
) -> Result<Json<GrantQuotaEntitlementResponse>, ApiError> {
    input
        .validate()
        .map_err(|e| ApiError::bad_request(format!("Invalid request: {}", e)))?;

    if input.windows.is_empty() {
        return Err(ApiError::bad_request(
            "At least one quota window is required",
        ));
    }

    ensure_user_and_bucket_in_realm(&state, &realm_id, input.user_id, input.bucket_id).await?;

    let credit_type = parse_credit_type(input.credit_type.as_deref())?;
    let source_type = parse_source_type(input.source_type.as_deref())?;
    let effective_until = parse_effective_until(input.effective_until.as_deref())?;

    let windows = input
        .windows
        .into_iter()
        .map(|w| QuotaWindow {
            key: w.key,
            window_seconds: w.window_seconds,
            limit: w.limit,
        })
        .collect::<Vec<_>>();

    // Per-grant-unique idempotency key. The demo revoke→grant baseline pattern
    // revokes the prior active entitlement (by `source_id`) then re-grants; a
    // fixed key would hit the UNIQUE constraint and return the still-revoked
    // row (see `grant_quota_entitlement_atomic`'s DO-NOTHING replay path).
    // Using a fresh UUID per grant lets each grant insert a new active row while
    // `source_id` remains the revoke locator. Idempotent-replay semantics are
    // not needed for the internal demo seeding path.
    let idempotency_key = format!("internal:{}:{}", input.source_id, Uuid::now_v7());

    let granted = state
        .subscription_service
        .grant_quota_entitlement(
            &realm_id,
            input.user_id,
            input.bucket_id,
            credit_type,
            source_type,
            input.source_id,
            windows,
            now_utc(),
            effective_until,
            idempotency_key,
        )
        .await
        .map_err(ApiError::from)?;

    Ok(Json(GrantQuotaEntitlementResponse {
        entitlement_id: granted.id,
        status: match granted.status {
            QuotaEntitlementStatus::Active => "active",
            QuotaEntitlementStatus::Revoked => "revoked",
            QuotaEntitlementStatus::Expired => "expired",
        }
        .to_string(),
    }))
}

/// Revoke the active quota entitlement identified by `source_id`. Idempotent:
/// a no-match returns `{ revoked: true }` (the post-condition "no active
/// entitlement" holds).
#[utoipa::path(
    post,
    path = "/api/internal/points/{realmId}/quota-entitlement/revoke",
    params(
        ("realmId" = String, Path, description = "Realm ID")
    ),
    request_body = RevokeQuotaEntitlementRequest,
    responses(
        (status = 200, description = "Quota entitlement revoked (or already absent)", body = RevokeQuotaEntitlementResponse),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Unauthorized — missing/invalid X-Internal-API-Key", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "InternalPoints"
)]
#[tracing::instrument(skip_all, fields(db.operation = "internal_revoke_quota_entitlement"))]
pub async fn revoke_quota_entitlement(
    State(state): State<AppState>,
    Path(realm_id): Path<String>,
    Json(input): Json<RevokeQuotaEntitlementRequest>,
) -> Result<Json<RevokeQuotaEntitlementResponse>, ApiError> {
    input
        .validate()
        .map_err(|e| ApiError::bad_request(format!("Invalid request: {}", e)))?;

    let credit_type = parse_credit_type(input.credit_type.as_deref())?;

    ensure_user_and_bucket_in_realm(&state, &realm_id, input.user_id, input.bucket_id).await?;

    state
        .subscription_service
        .revoke_quota_entitlement(
            &realm_id,
            input.user_id,
            input.bucket_id,
            credit_type,
            &input.source_id,
            now_utc(),
        )
        .await
        .map_err(ApiError::from)?;

    Ok(Json(RevokeQuotaEntitlementResponse { revoked: true }))
}

fn parse_credit_type(s: Option<&str>) -> Result<CreditType, ApiError> {
    use std::str::FromStr;
    let raw = s.unwrap_or("subscription_credit");
    CreditType::from_str(raw)
        .map_err(|_| ApiError::bad_request(format!("Invalid credit_type: {}", raw)))
}

fn parse_source_type(s: Option<&str>) -> Result<QuotaSourceType, ApiError> {
    use std::str::FromStr;
    let raw = s.unwrap_or("subscription_initial");
    QuotaSourceType::from_str(raw)
        .map_err(|_| ApiError::bad_request(format!("Invalid source_type: {}", raw)))
}

fn parse_effective_until(s: Option<&str>) -> Result<Option<DateTime<Utc>>, ApiError> {
    match s {
        None => Ok(None),
        Some(raw) => {
            let dt = DateTime::parse_from_rfc3339(raw)
                .map_err(|_| ApiError::bad_request("Invalid effective_until (expected RFC3339)"))?
                .with_timezone(&Utc);
            Ok(Some(dt))
        }
    }
}
