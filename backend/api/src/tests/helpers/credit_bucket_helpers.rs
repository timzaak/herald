// =============================================================================
// Credit Bucket Test Helpers
// =============================================================================
//
// Test-owned helpers for credit-bucket scenario tests. These directly drive the
// real domain write paths (`PointsService::grant_points_internal` →
// `PostgresPointsRepository::grant_points_atomic`) so tests exercise the
// intended multi-bucket contracts instead of any single-wallet
// legacy helpers.
//
// Owned by the backend-test slot. Extended in place by later items.
//
// =============================================================================

#![allow(dead_code)]

use crate::tests::schema_test_context::SchemaTestContext as TestContext;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use herald_core::domain::common::entities::app_errors::CoreError;
use herald_core::domain::points::entities::{CreditSourceType, CreditType};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

/// Options for [`create_test_credit_bucket`].
#[derive(Default, Clone)]
pub struct CreditBucketOpts {
    /// Human-readable name. Defaults to "Test Bucket <suffix>".
    pub name: Option<String>,
    /// Machine key. Must match `^[a-z0-9-]{1,64}$`. Defaults to a unique slug.
    pub bucket_key: Option<String>,
    /// `enabled` flag on the bucket. Defaults to `true`.
    pub enabled: Option<bool>,
    /// Display order. Defaults to `0`.
    pub display_order: Option<i32>,
}

/// Insert a `credit_buckets` row and return its id.
///
/// The bucket starts with no `credit_bucket_client_apps` rows; attach coverage
/// via [`attach_bucket_client_app`].
pub async fn create_test_credit_bucket(
    pool: &PgPool,
    realm_id: &str,
    opts: CreditBucketOpts,
) -> Uuid {
    let bucket_id = Uuid::now_v7();
    let bucket_key = opts
        .bucket_key
        .unwrap_or_else(|| format!("bucket-{}", bucket_id));
    let name = opts
        .name
        .unwrap_or_else(|| format!("Test Bucket {}", bucket_id));
    let enabled = opts.enabled.unwrap_or(true);
    let display_order = opts.display_order.unwrap_or(0);

    sqlx::query(
        r#"INSERT INTO credit_buckets
             (id, realm_id, bucket_key, name, display_order, enabled,
              created_at, updated_at)
           VALUES ($1, $2, $3, $4, $5, $6, NOW(), NOW())"#,
    )
    .bind(bucket_id)
    .bind(realm_id)
    .bind(&bucket_key)
    .bind(&name)
    .bind(display_order)
    .bind(enabled)
    .execute(pool)
    .await
    .expect("Failed to insert credit_buckets row");

    bucket_id
}

/// Attach a Client App to a Bucket (writes a `credit_bucket_client_apps` row).
///
/// Coverage sets are resolved from these explicit rows at consume time; no
/// implicit / default bucket is merged in.
pub async fn attach_bucket_client_app(
    pool: &PgPool,
    realm_id: &str,
    bucket_id: Uuid,
    client_app_id: Uuid,
) {
    sqlx::query(
        r#"INSERT INTO credit_bucket_client_apps
             (bucket_id, client_app_id, realm_id, created_at)
           VALUES ($1, $2, $3, NOW())
           ON CONFLICT (bucket_id, client_app_id) DO NOTHING"#,
    )
    .bind(bucket_id)
    .bind(client_app_id)
    .bind(realm_id)
    .execute(pool)
    .await
    .expect("Failed to insert credit_bucket_client_apps row");
}

/// Grant points to a specific `(user, bucket)` pool via the real domain write
/// path (`PointsService::grant_points_internal` → `grant_points_atomic`).
///
/// This routes through the same `bucket_id` parameter the production grant path
/// uses so tests assert the intended multi-bucket behavior.
/// Returns the new ledger id (i.e. the "transaction" id surfaced by grants).
pub async fn grant_points_to_bucket(
    ctx: &TestContext,
    realm_id: &str,
    user_id: Uuid,
    bucket_id: Uuid,
    credit_type: CreditType,
    source_type: CreditSourceType,
    amount: i64,
    expires_at: Option<chrono::DateTime<chrono::Utc>>,
    source_id: Option<String>,
    description: Option<String>,
    idempotency_key: Option<String>,
) -> Result<Uuid, CoreError> {
    ctx.app_state
        .points_service
        .grant_points_internal(
            realm_id,
            user_id,
            bucket_id,
            credit_type,
            source_type,
            amount,
            expires_at,
            // effective_at: None ⟺ immediately available.
            None,
            source_id,
            description,
            idempotency_key,
        )
        .await
}

/// Convenience: grant points to a bucket with a simple admin-grant source.
pub async fn admin_grant_to_bucket(
    ctx: &TestContext,
    realm_id: &str,
    user_id: Uuid,
    bucket_id: Uuid,
    amount: i64,
    expires_at: Option<chrono::DateTime<chrono::Utc>>,
) -> Uuid {
    grant_points_to_bucket(
        ctx,
        realm_id,
        user_id,
        bucket_id,
        CreditType::GrantedCredit,
        CreditSourceType::AdminGrant,
        amount,
        expires_at,
        Some(format!("admin-grant-{}", Uuid::now_v7())),
        Some("Test admin grant".to_string()),
        None,
    )
    .await
    .expect("admin_grant_to_bucket failed")
}

// =============================================================================
// HTTP request builders — consume endpoint
// =============================================================================

/// Build a `POST /api/ext/points/{realmId}/consume` request authenticated via
/// API Key. The returned request is ready for `oneshot`.
pub fn consume_points_ext_request(
    realm_id: &str,
    user_id: Uuid,
    client_app_id: Uuid,
    amount: i64,
    description: Option<&str>,
    idempotency_key: Option<&str>,
    api_key: &str,
) -> Request<Body> {
    let mut body = serde_json::json!({
        "userId": user_id.to_string(),
        "clientAppId": client_app_id.to_string(),
        "amount": amount,
    });
    if let Some(desc) = description {
        body["description"] = serde_json::json!(desc);
    }
    if let Some(key) = idempotency_key {
        body["idempotencyKey"] = serde_json::json!(key);
    }

    Request::builder()
        .method("POST")
        .uri(format!("/api/ext/points/{}/consume", realm_id))
        .header("content-type", "application/json")
        .header("X-API-Key", api_key)
        .body(Body::from(body.to_string()))
        .unwrap()
}

/// Call the ext consume endpoint via the unified test router and return the
/// status code with the parsed JSON body (if any).
pub async fn consume_points_ext_via_api(
    ctx: &TestContext,
    realm_id: &str,
    user_id: Uuid,
    client_app_id: Uuid,
    amount: i64,
    description: Option<&str>,
    idempotency_key: Option<&str>,
    api_key: &str,
) -> (StatusCode, Option<serde_json::Value>) {
    let app = ctx.create_unified_test_router();
    let request = consume_points_ext_request(
        realm_id,
        user_id,
        client_app_id,
        amount,
        description,
        idempotency_key,
        api_key,
    );
    let response = app.oneshot(request).await.unwrap();

    let status = response.status();
    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body: Option<serde_json::Value> = if body_bytes.is_empty() {
        None
    } else {
        Some(serde_json::from_slice(&body_bytes).unwrap())
    };
    (status, body)
}

// =============================================================================
// Fulfillment + subscription lifecycle bucket routing helpers
// =============================================================================
//
// Direct-DB and direct-service helpers for grant/fulfillment bucket routing
// + subscription lifecycle reclamation (in-flight attempt not rerouted on
// mapping change; routing source = `payment_attempt.bucket_id` snapshot;
// first fulfillment freezes `subscription.bucket_id`).
//
// These intentionally do NOT replicate webhook signature logic — the existing
// `billing_helpers` / `webhook_helpers` cover the HTTP path. These helpers
// drive the real domain services (`fulfillment_service`, `subscription_service`)
// and read the bucket-dimension columns directly for assertions.
//
// =============================================================================

/// Attach (or re-attach) a `provider_entitlement_mappings` row to a Bucket by
/// writing `provider_entitlement_mappings.bucket_id`.
///
/// Used by the regression scenario to re-point the mapping AFTER the
/// purchase attempt is in flight, proving the in-flight attempt still fulfills
/// to the original snapshot Bucket.
pub async fn attach_mapping_to_bucket(pool: &PgPool, mapping_id: Uuid, bucket_id: Uuid) {
    sqlx::query(
        "UPDATE provider_entitlement_mappings SET bucket_id = $1, updated_at = NOW() WHERE id = $2",
    )
    .bind(bucket_id)
    .bind(mapping_id)
    .execute(pool)
    .await
    .expect("Failed to attach mapping to bucket");
}

/// Detach a `provider_entitlement_mappings` row from any Bucket by setting
/// `bucket_id = NULL` (simulates a never-attached mapping, US-CB-003 scenario 2).
pub async fn detach_mapping_from_bucket(pool: &PgPool, mapping_id: Uuid) {
    sqlx::query(
        "UPDATE provider_entitlement_mappings SET bucket_id = NULL, updated_at = NOW() WHERE id = $2",
    )
    .bind(mapping_id)
    .execute(pool)
    .await
    .expect("Failed to detach mapping from bucket");
}

/// Read `subscription.bucket_id` for a subscription. The column is NOT NULL
/// since the eager-binding migration (`bucket_id` is frozen
/// at subscription creation from the entitlement mapping), so this returns a
/// flat `Uuid`. Panics if the row does not exist (a missing subscription row
/// in these scenarios is always a test-setup bug, not a runtime state).
///
/// Used to assert the first-fulfillment snapshot freeze and that
/// lifecycle events stay bound to the original Bucket.
pub async fn read_subscription_bucket(pool: &PgPool, subscription_id: Uuid) -> Uuid {
    sqlx::query_scalar::<_, Uuid>("SELECT bucket_id FROM subscription WHERE id = $1")
        .bind(subscription_id)
        .fetch_optional(pool)
        .await
        .expect("Failed to read subscription.bucket_id")
        .expect("subscription row disappeared — test-setup bug")
}

/// Force-set `subscription.bucket_id` to a specific Bucket. The column is NOT
/// NULL (eager binding), so callers must pass a real
/// `bucket_id`. Kept for scenarios that need to re-bind a subscription to a
/// different Bucket after creation.
pub async fn set_subscription_bucket(pool: &PgPool, subscription_id: Uuid, bucket_id: Uuid) {
    sqlx::query("UPDATE subscription SET bucket_id = $1, updated_at = NOW() WHERE id = $2")
        .bind(bucket_id)
        .bind(subscription_id)
        .execute(pool)
        .await
        .expect("Failed to set subscription.bucket_id");
}

/// Read `payment_attempts.bucket_id` snapshot for an attempt; returns None if
/// the row does not exist or the column is NULL.
pub async fn read_attempt_bucket(pool: &PgPool, attempt_id: Uuid) -> Option<Uuid> {
    sqlx::query_scalar::<_, Option<Uuid>>("SELECT bucket_id FROM payment_attempts WHERE id = $1")
        .bind(attempt_id)
        .fetch_optional(pool)
        .await
        .expect("Failed to read payment_attempts.bucket_id")
        .flatten()
}

/// Read the derived available balance for a `(user, bucket)` wallet; returns 0
/// if no wallet row exists yet (the wallet is created lazily on first grant).
/// Under point-time `points_wallets` has no Stored balance columns;
/// the available balance is derived from `points_credit_ledger` using the same
/// predicate as consumption.
pub async fn read_wallet_total_balance(
    pool: &PgPool,
    realm_id: &str,
    user_id: Uuid,
    bucket_id: Uuid,
) -> i64 {
    let row: Option<i64> = sqlx::query_scalar(
        "SELECT COALESCE(SUM(l.remaining_amount) FILTER (
                    WHERE l.status = 'active' AND l.remaining_amount > 0
                      AND (l.effective_at IS NULL OR l.effective_at <= NOW())
                      AND (l.expires_at  IS NULL OR l.expires_at  >  NOW())
                ), 0)::BIGINT
         FROM points_wallets w
         LEFT JOIN points_credit_ledger l
           ON l.realm_id = w.realm_id AND l.user_id = w.user_id AND l.bucket_id = w.bucket_id
         WHERE w.realm_id = $1 AND w.user_id = $2 AND w.bucket_id = $3
         GROUP BY w.id",
    )
    .bind(realm_id)
    .bind(user_id)
    .bind(bucket_id)
    .fetch_optional(pool)
    .await
    .expect("Failed to read wallet total_balance");
    row.unwrap_or(0)
}

/// Sum the granted (positive) ledger amounts for a user in a specific Bucket
/// filtered by credit type; returns 0 if no ledger rows match.
pub async fn sum_ledger_granted_in_bucket(
    pool: &PgPool,
    user_id: Uuid,
    bucket_id: Uuid,
    credit_type: &str,
) -> i64 {
    let row: Option<i64> = sqlx::query_scalar(
        "SELECT COALESCE(SUM(granted_amount - revoked_amount), 0)::BIGINT
           FROM points_credit_ledger
          WHERE user_id = $1 AND bucket_id = $2 AND credit_type = $3",
    )
    .bind(user_id)
    .bind(bucket_id)
    .bind(credit_type)
    .fetch_optional(pool)
    .await
    .expect("Failed to sum ledger amounts in bucket")
    .or(Some(0));
    row.unwrap_or(0)
}

/// Count ledger rows for a user in a specific Bucket with a given credit type.
pub async fn count_ledger_in_bucket(
    pool: &PgPool,
    user_id: Uuid,
    bucket_id: Uuid,
    credit_type: &str,
) -> i64 {
    sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*)::BIGINT FROM points_credit_ledger
          WHERE user_id = $1 AND bucket_id = $2 AND credit_type = $3",
    )
    .bind(user_id)
    .bind(bucket_id)
    .bind(credit_type)
    .fetch_one(pool)
    .await
    .expect("Failed to count ledger rows in bucket")
}

/// Count ledger rows for a user in ANY Bucket OTHER than the one passed
/// (cross-pool leak detector).
pub async fn count_ledger_outside_bucket(
    pool: &PgPool,
    user_id: Uuid,
    excluded_bucket_id: Uuid,
) -> i64 {
    sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*)::BIGINT FROM points_credit_ledger
          WHERE user_id = $1 AND bucket_id IS DISTINCT FROM $2",
    )
    .bind(user_id)
    .bind(excluded_bucket_id)
    .fetch_one(pool)
    .await
    .expect("Failed to count ledger rows outside bucket")
}

// =============================================================================
// HTTP request builders — explicit-bucketId grant endpoints (admin + ext)
// =============================================================================
//
// Mirror `points_grant_helpers::grant_points_admin_request` /
// `grant_points_ext_request` but accept an OPTIONAL `bucket_id`. Passing `None`
// exercises the "missing bucketId → 400 grant_bucket_required" rejection;
// passing `Some(uuid)` exercises the intended multi-bucket routing.
//
// =============================================================================

/// Build a `POST /api/points/{realmId}/grant` (admin) request. When
/// `bucket_id` is `None` the field is omitted entirely so the handler exercises
/// the missing-bucketId rejection path.
pub fn grant_points_admin_request_with_bucket(
    realm_id: &str,
    user_id: Uuid,
    amount: i64,
    reason: &str,
    validity_days: Option<i64>,
    bucket_id: Option<Uuid>,
    session_token: &str,
) -> Request<Body> {
    let mut body = serde_json::json!({
        "userId": user_id.to_string(),
        "amount": amount,
        "reason": reason,
    });
    if let Some(days) = validity_days {
        body["validityDays"] = serde_json::json!(days);
    }
    if let Some(b) = bucket_id {
        body["bucketId"] = serde_json::json!(b.to_string());
    }

    Request::builder()
        .method("POST")
        .uri(format!("/api/points/{}/grant", realm_id))
        .header("content-type", "application/json")
        .header(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {}", session_token),
        )
        .body(Body::from(body.to_string()))
        .unwrap()
}

/// Call the admin grant endpoint with an optional explicit `bucket_id` and
/// return the status code with the parsed JSON body (if any).
pub async fn grant_points_admin_with_bucket_via_api(
    ctx: &TestContext,
    realm_id: &str,
    user_id: Uuid,
    amount: i64,
    reason: &str,
    validity_days: Option<i64>,
    bucket_id: Option<Uuid>,
    session_token: &str,
) -> (StatusCode, Option<serde_json::Value>) {
    let app = ctx.create_unified_test_router();
    let request = grant_points_admin_request_with_bucket(
        realm_id,
        user_id,
        amount,
        reason,
        validity_days,
        bucket_id,
        session_token,
    );
    let response = app.oneshot(request).await.unwrap();

    let status = response.status();
    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body: Option<serde_json::Value> = if body_bytes.is_empty() {
        None
    } else {
        Some(serde_json::from_slice(&body_bytes).unwrap())
    };
    (status, body)
}

/// Build a `POST /api/ext/points/{realmId}/grant` (SDK) request. When
/// `bucket_id` is `None` the field is omitted entirely so the handler exercises
/// the missing-bucketId rejection path.
pub fn grant_points_ext_request_with_bucket(
    realm_id: &str,
    user_id: Uuid,
    amount: i64,
    reason: &str,
    validity_days: Option<i64>,
    bucket_id: Option<Uuid>,
    api_key: &str,
) -> Request<Body> {
    let mut body = serde_json::json!({
        "userId": user_id.to_string(),
        "amount": amount,
        "reason": reason,
    });
    if let Some(days) = validity_days {
        body["validityDays"] = serde_json::json!(days);
    }
    if let Some(b) = bucket_id {
        body["bucketId"] = serde_json::json!(b.to_string());
    }

    Request::builder()
        .method("POST")
        .uri(format!("/api/ext/points/{}/grant", realm_id))
        .header("content-type", "application/json")
        .header("X-API-Key", api_key)
        .body(Body::from(body.to_string()))
        .unwrap()
}

/// Call the ext (SDK) grant endpoint with an optional explicit `bucket_id` and
/// return the status code with the parsed JSON body (if any).
pub async fn grant_points_ext_with_bucket_via_api(
    ctx: &TestContext,
    realm_id: &str,
    user_id: Uuid,
    amount: i64,
    reason: &str,
    validity_days: Option<i64>,
    bucket_id: Option<Uuid>,
    api_key: &str,
) -> (StatusCode, Option<serde_json::Value>) {
    let app = ctx.create_unified_test_router();
    let request = grant_points_ext_request_with_bucket(
        realm_id,
        user_id,
        amount,
        reason,
        validity_days,
        bucket_id,
        api_key,
    );
    let response = app.oneshot(request).await.unwrap();

    let status = response.status();
    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body: Option<serde_json::Value> = if body_bytes.is_empty() {
        None
    } else {
        Some(serde_json::from_slice(&body_bytes).unwrap())
    };
    (status, body)
}

// =============================================================================
// Credit Bucket directory CRUD + overview + delete intercept helpers
// =============================================================================
//
// Direct-DB seed helpers + HTTP request builders for the Bucket directory CRUD
// + overview + error contract + "directory CRUD" testable behavior
// (NO `is_default` field anywhere).
//
// `delete_credit_bucket` (infra `postgres_repository.rs`) rejects when:
//   - `subscription.bucket_id = $bucket_id` AND status IN
//     ('active','trialing','past_due','scheduled_cancel','dispute'); OR
//   - `points_wallets.bucket_id = $bucket_id` AND `total_balance > 0`.
// The seed helpers below materialize exactly those rows so delete-intercept
// scenarios assert the 409 `bucket_in_use` body (`activeSubscriptions` /
// `holdersWithBalance`).
//
// =============================================================================

/// Seed an in-progress `subscription` row.
///
/// Bind an `active` subscription to `bucket_id` so the
/// `delete_credit_bucket` "active subscriptions" intercept fires. After the
/// distribution-rules refactor `subscription.bucket_id` no longer exists; the
/// subscription references the bucket via its `subscription_credit` grant
/// results (`points_quota_entitlements.source_id = subscription_id`,
/// `bucket_id`). This helper mints the subscription AND an active
/// `subscription_credit` quota entitlement in `bucket_id` keyed by the
/// subscription, mirroring what production's renewal/initial grant writes, so
/// the delete guard (which joins subscription → grant results by `source_id`)
/// detects the in-use bucket.
///
/// `subscription.client_app_id` carries a UNIQUE constraint, so each call mints
/// a fresh client app row. The subscription uses `status = 'active'`.
pub async fn seed_active_subscription_on_bucket(
    pool: &PgPool,
    realm_id: &str,
    bucket_id: Uuid,
) -> Uuid {
    use sqlx::Row;

    // Mint a unique client_app to satisfy `uq_subscription_client_app`.
    let client_app_id = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO client_app (id, realm_id, client_id, name, enabled)
           VALUES ($1, $2, $3, 'sub-seed-app', true)"#,
    )
    .bind(client_app_id)
    .bind(realm_id)
    .bind(format!("sub-seed-{}", client_app_id))
    .execute(pool)
    .await
    .expect("insert client_app for subscription seed");

    let user_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO account (id, realm_id, email, password, status)
         VALUES ($1, $2, $3, '$2a$12$dummy_password_hash', 1)",
    )
    .bind(user_id)
    .bind(realm_id)
    .bind(format!("bucket-sub-owner-{}@test.com", user_id))
    .execute(pool)
    .await
    .expect("insert account for subscription seed");

    let subscription_id = Uuid::now_v7();
    let row = sqlx::query(
        r#"INSERT INTO subscription
             (id, realm_id, user_id, external_subscription_id, external_product_id,
              payment_provider, status, entitlement_key, client_app_id,
              created_at, updated_at, billing_type)
           VALUES ($1, $2, $3, $4, 'prod-seed', 'creem', 'active', '', $5, NOW(), NOW(), 'recurring')
           RETURNING id"#,
    )
    .bind(subscription_id)
    .bind(realm_id)
    .bind(user_id)
    .bind(format!("ext-sub-{}", subscription_id))
    .bind(client_app_id)
    .fetch_one(pool)
    .await
    .expect("insert subscription row");
    let id: Uuid = row.get("id");

    // Ensure a wallet exists for (user, bucket), then write an active
    // subscription_credit quota entitlement keyed by the subscription so the
    // bucket is recognized as in-use by an active subscription.
    sqlx::query(
        r#"INSERT INTO points_wallets (id, user_id, realm_id, bucket_id, created_at, updated_at)
           VALUES ($1, $2, $3, $4, NOW(), NOW())
           ON CONFLICT DO NOTHING"#,
    )
    .bind(Uuid::now_v7())
    .bind(user_id)
    .bind(realm_id)
    .bind(bucket_id)
    .execute(pool)
    .await
    .expect("insert wallet for subscription-bucket seed");

    sqlx::query(
        r#"INSERT INTO points_quota_entitlements
             (id, user_id, realm_id, bucket_id, credit_type, source_type, source_id,
              quota_windows, effective_from, effective_until, status, idempotency_key,
              distribution_event_id, distribution_rule_id, created_at, updated_at)
           VALUES ($1, $2, $3, $4, 'subscription_credit', 'subscription_initial', $5,
                   $6::jsonb, NOW(), NOW() + INTERVAL '30 days', 'active', $7,
                   NULL, NULL, NOW(), NOW())"#,
    )
    .bind(Uuid::now_v7())
    .bind(user_id)
    .bind(realm_id)
    .bind(bucket_id)
    .bind(subscription_id.to_string())
    .bind(serde_json::json!([{"windowSeconds": 2592000, "limit": 1000, "key": "period"}]))
    .bind(format!("seed-sub-{}", subscription_id))
    .execute(pool)
    .await
    .expect("insert subscription_credit quota entitlement for bucket seed");

    id
}

/// Seed a `points_wallets` row for `(realm, user, bucket)` plus a
/// `points_credit_ledger` row of the given `balance` so the
/// `delete_credit_bucket` "holders with balance" intercept (which reads the
/// DERIVED available balance post-point-time) fires. Under point-time
/// the wallet has no Stored balance columns; the ledger row is what makes the
/// derived balance non-zero.
/// Seed an available `granted_credit` ledger row on a bucket so the derived
/// Credit Bucket overview reports a real balance. Under point-time
/// `grandTotal` / `byCreditType` are a derived SUM over `points_credit_ledger`
/// (the wallet Stored balance columns were dropped), so a bucket
/// with only a `points_wallets` analytics row shows zero — tests asserting a
/// non-zero overview total must seed a real ledger row.
pub async fn seed_granted_credit_ledger_on_bucket(
    pool: &PgPool,
    realm_id: &str,
    user_id: Uuid,
    bucket_id: Uuid,
    amount: i64,
) -> Uuid {
    let ledger_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO points_credit_ledger
            (id, user_id, realm_id, bucket_id, credit_type, source_type, source_id,
             granted_amount, used_amount, revoked_amount,
             expires_at, effective_at, status, created_at, updated_at)
         VALUES ($1, $2, $3, $4, 'granted_credit', 'admin_grant', $5, $6, 0, 0,
                 NULL, NULL, 'active', NOW(), NOW())",
    )
    .bind(ledger_id)
    .bind(user_id)
    .bind(realm_id)
    .bind(bucket_id)
    .bind(format!("granted-overview-{}", Uuid::now_v7()))
    .bind(amount)
    .execute(pool)
    .await
    .expect("seed granted_credit ledger row on bucket");
    ledger_id
}

pub async fn seed_wallet_with_balance_on_bucket(
    pool: &PgPool,
    realm_id: &str,
    user_id: Uuid,
    bucket_id: Uuid,
    balance: i64,
) -> Uuid {
    use sqlx::Row;

    let wallet_id = Uuid::now_v7();
    let row = sqlx::query(
        r#"INSERT INTO points_wallets
             (id, realm_id, user_id, bucket_id, total_topup_granted,
              total_recharged, total_consumed, total_subscription_granted,
              status, created_at, updated_at)
           VALUES ($1, $2, $3, $4, $5, $5, 0, 0, 'active', NOW(), NOW())
           ON CONFLICT (realm_id, user_id, bucket_id) DO NOTHING
           RETURNING id"#,
    )
    .bind(wallet_id)
    .bind(realm_id)
    .bind(user_id)
    .bind(bucket_id)
    .bind(balance)
    .fetch_optional(pool)
    .await
    .expect("insert points_wallets row with balance");

    let id: Uuid = match row {
        Some(row) => row.get("id"),
        None => {
            // Row pre-existed; fetch the canonical id.
            sqlx::query_scalar::<_, Uuid>(
                "SELECT id FROM points_wallets
                  WHERE realm_id = $1 AND user_id = $2 AND bucket_id = $3",
            )
            .bind(realm_id)
            .bind(user_id)
            .bind(bucket_id)
            .fetch_one(pool)
            .await
            .expect("fetch pre-existing wallet id in seed_wallet_with_balance_on_bucket")
        }
    };

    if balance > 0 {
        sqlx::query(
            r#"INSERT INTO points_credit_ledger
                 (id, realm_id, user_id, bucket_id, credit_type, source_type,
                  source_id, granted_amount, used_amount, revoked_amount,
                  status, created_at, updated_at)
               VALUES ($1, $2, $3, $4, 'topup_credit', 'system_grant', $5,
                       $6, 0, 0, 'active', NOW(), NOW())"#,
        )
        .bind(Uuid::now_v7())
        .bind(realm_id)
        .bind(user_id)
        .bind(bucket_id)
        .bind(format!("seed-bucket-{}", id))
        .bind(balance)
        .execute(pool)
        .await
        .expect("insert ledger row for seed_wallet_with_balance_on_bucket");
    }

    id
}

/// Build a Bucket directory API request authenticated via admin session cookie
/// (`Bearer <token>`). `body` is sent as JSON; pass `None` for bodyless verbs
/// (GET/DELETE).
pub fn auth_admin_request(
    method: &str,
    uri: &str,
    token: &str,
    body: Option<&serde_json::Value>,
) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .header(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {}", token),
        )
        .body(if let Some(b) = body {
            Body::from(b.to_string())
        } else {
            Body::empty()
        })
        .unwrap()
}

/// Drive the Bucket directory API through the unified test router and return
/// the status code with the parsed JSON body (if any).
pub async fn auth_admin_request_via_api(
    ctx: &TestContext,
    method: &str,
    uri: &str,
    token: &str,
    body: Option<&serde_json::Value>,
) -> (StatusCode, Option<serde_json::Value>) {
    let app = ctx.create_unified_test_router();
    let request = auth_admin_request(method, uri, token, body);
    let response = app.oneshot(request).await.unwrap();

    let status = response.status();
    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let parsed: Option<serde_json::Value> = if body_bytes.is_empty() {
        None
    } else {
        Some(serde_json::from_slice(&body_bytes).unwrap_or(serde_json::Value::Null))
    };
    (status, parsed)
}

// =============================================================================
// Per-bucket query surface helpers (wallets / transactions / admin)
// =============================================================================
//
// HTTP request builders for the query contracts. The user-facing and
// admin-facing query endpoints are served by a single handler at
// `GET /api/points/{realmId}/wallets` (and `.../transactions`) gated on
// `points.view`; both group rows by `(bucket_id, user_id)` (the admin view
// `/users/me/points/wallets` / `/billing/points/wallets` distinct routes are a
// KNOWN contract gap (recorded in `open_questions`); tests exercise the
// real route that implements the response contract.
//
// `seed_transaction_on_bucket` writes a `points_transactions` row directly with
// a known bucket_id so the bucketId filter scenario has deterministic rows
// without driving the full consume path.
//
// =============================================================================

/// Build a GET request to the points query endpoints authenticated via the
/// session cookie (`Bearer <token>`). `query` is appended as the raw query
/// string (may be empty).
pub fn auth_user_get_request(uri: &str, query: &str, token: &str) -> Request<Body> {
    let full_uri = if query.is_empty() {
        uri.to_string()
    } else {
        format!("{}?{}", uri, query)
    };
    Request::builder()
        .method("GET")
        .uri(full_uri)
        .header(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {}", token),
        )
        .body(Body::empty())
        .unwrap()
}

/// Drive a GET points query through the unified test router with session cookie
/// auth and return the status code with the parsed JSON body (if any).
pub async fn auth_user_get_via_api(
    ctx: &TestContext,
    uri: &str,
    query: &str,
    token: &str,
) -> (StatusCode, Option<serde_json::Value>) {
    let app = ctx.create_unified_test_router();
    let request = auth_user_get_request(uri, query, token);
    let response = app.oneshot(request).await.unwrap();

    let status = response.status();
    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let parsed: Option<serde_json::Value> = if body_bytes.is_empty() {
        None
    } else {
        Some(serde_json::from_slice(&body_bytes).unwrap_or(serde_json::Value::Null))
    };
    (status, parsed)
}

/// Seed a `points_transactions` row in a specific `(user, bucket)` pool directly
/// at the DB layer with an explicit `created_at` (window-boundary scenarios
/// backdate rows; pass `Utc::now()` for current-day rows). Returns the new
/// transaction id.
///
/// Used by the bucketId filter scenario to deterministically place transactions
/// in known buckets without driving the full grant/consume paths. The row
/// carries a real `wallet_id` (resolved or created for the pool) so the
/// `wallet_id NOT NULL` constraint holds, and a real `bucket_id` so the handler
/// `bucketId` filter returns it.
pub async fn seed_transaction_on_bucket(
    pool: &PgPool,
    realm_id: &str,
    user_id: Uuid,
    bucket_id: Uuid,
    txn_type: &str,
    amount: i64,
    created_at: chrono::DateTime<chrono::Utc>,
) -> Uuid {
    use sqlx::Row;

    // Ensure a wallet row exists for the (realm, user, bucket) pool so the
    // transaction's wallet_id FK is satisfied.
    let wallet_id: Uuid = sqlx::query_scalar::<_, Option<Uuid>>(
        "SELECT id FROM points_wallets
              WHERE realm_id = $1 AND user_id = $2 AND bucket_id = $3",
    )
    .bind(realm_id)
    .bind(user_id)
    .bind(bucket_id)
    .fetch_optional(pool)
    .await
    .expect("lookup wallet for seed_transaction_on_bucket")
    .flatten()
    .unwrap_or_else(Uuid::now_v7);

    // Insert the wallet row if it did not exist (cheap no-op when already
    // present via ON CONFLICT). Under point-time `points_wallets`
    // has no Stored balance columns; available balance is derived.
    sqlx::query(
        r#"INSERT INTO points_wallets
             (id, realm_id, user_id, bucket_id, status,
              created_at, updated_at)
           VALUES ($1, $2, $3, $4, 'active', NOW(), NOW())
           ON CONFLICT (realm_id, user_id, bucket_id) DO NOTHING"#,
    )
    .bind(wallet_id)
    .bind(realm_id)
    .bind(user_id)
    .bind(bucket_id)
    .execute(pool)
    .await
    .expect("ensure wallet for seed_transaction_on_bucket");

    // Re-read in case the row pre-existed and our minted id collided with the
    // unique (realm, user, bucket) row.
    let wallet_id: Uuid = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM points_wallets
          WHERE realm_id = $1 AND user_id = $2 AND bucket_id = $3",
    )
    .bind(realm_id)
    .bind(user_id)
    .bind(bucket_id)
    .fetch_one(pool)
    .await
    .expect("fetch wallet id for seed_transaction_on_bucket");

    let txn_id = Uuid::now_v7();
    let row = sqlx::query(
        r#"INSERT INTO points_transactions
             (id, realm_id, user_id, wallet_id, bucket_id, type, amount,
              balance_after, description, created_at)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 'seed txn', $9)
           RETURNING id"#,
    )
    .bind(txn_id)
    .bind(realm_id)
    .bind(user_id)
    .bind(wallet_id)
    .bind(bucket_id)
    .bind(txn_type)
    .bind(amount)
    .bind(amount.max(0))
    .bind(created_at)
    .fetch_one(pool)
    .await
    .expect("insert seed points_transactions row");
    let id: Uuid = row.get("id");
    id
}
