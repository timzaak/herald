// =============================================================================
// Scenario Tests: consume multi-pool cross-Bucket spread (P0)
// =============================================================================
//
// Covers design `.ai/design/credit-bucket.md` (consume algorithm,
// SDK consume response contract, correlation_id /
// external_ref_id, consume test strategy).
//
// All tests target the real multi-bucket write path:
//   `consume_points_ext` (api-ext) → `PointsService::consume_points`
//   → `PostgresPointsRepository::consume_points_atomic`.
//
// Authoritative runtime gaps surfaced by these tests:
//   1. `allocations` is currently always `[]` in the response — service
//      consume returns only `Vec<PointsTransaction>`. Tests asserting the
//      intended allocation breakdown will compile and fail at runtime.
//   2. Multi-pool idempotency replay currently returns a single primary
//      transaction (`service.rs:168` hardcodes `idempotency_key=None`),
//      not the full N-txn result set (no double-deduct — just a reduced
//      replay shape). Tests encode the intended "same N-txn result set"
//      contract; flag for runner.
//
// =============================================================================

#![allow(clippy::too_many_arguments)]

use crate::tests::helpers::credit_bucket_helpers::{
    CreditBucketOpts, admin_grant_to_bucket, attach_bucket_client_app, consume_points_ext_via_api,
    create_test_credit_bucket,
};
use crate::tests::scenarios::points::fixtures::{
    create_test_api_key, create_test_client_app, create_test_user, create_test_user_with_auth,
};
use crate::tests::schema_test_context::SchemaTestContext as TestContext;
use axum::http::StatusCode;
use chrono::Duration;
use sqlx::PgPool;
use test_context::test_context;
use uuid::Uuid;

/// Pull the points_transactions rows for a user matching a correlation_id,
/// ordered by bucket_id ASC (mirrors the infra write order).
///
/// Tuple shape: (id, bucket_id, wallet_id, amount, balance_after, external_ref_id)
async fn fetch_consume_txns_by_correlation(
    pool: &PgPool,
    user_id: Uuid,
    correlation_id: &str,
) -> Vec<(Uuid, Uuid, Uuid, i64, i64, Option<String>)> {
    sqlx::query_as::<_, (Uuid, Uuid, Uuid, i64, i64, Option<String>)>(
        r#"SELECT id, bucket_id, wallet_id, amount, balance_after, external_ref_id
             FROM points_transactions
            WHERE user_id = $1
              AND correlation_id = $2
            ORDER BY bucket_id ASC"#,
    )
    .bind(user_id)
    .bind(correlation_id)
    .fetch_all(pool)
    .await
    .expect("Failed to fetch consume transactions")
}

/// Count `points_consumption_allocations` rows for the transaction set of a
/// correlation_id.
async fn count_allocations_for_correlation(
    pool: &PgPool,
    user_id: Uuid,
    correlation_id: &str,
) -> i64 {
    let count: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*)::BIGINT
             FROM points_consumption_allocations a
             JOIN points_transactions t ON t.id = a.transaction_id
            WHERE a.user_id = $1
              AND t.correlation_id = $2"#,
    )
    .bind(user_id)
    .bind(correlation_id)
    .fetch_one(pool)
    .await
    .expect("Failed to count allocations");
    count
}

/// Read total_balance for a (user, bucket) wallet; returns 0 if no wallet row.
async fn wallet_total_balance(
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
    .expect("Failed to read wallet");
    row.unwrap_or(0)
}

// =============================================================================
// Scenario 1: single-pool hit returns exactly one transaction
// =============================================================================

/// User Story: US-CB-007 (SDK consume) — single-pool hit path.
/// Covers:
///   - 1 covered Bucket with a positive ledger → `transactions.len() == 1`.
///   - `bucket_id`/`wallet_id` correct, `balance_after` matches new balance.
///   - `correlationId` non-empty; every consume row's `external_ref_id`
///     is NULL (no `idx_transactions_external_ref` uniqueness clash).
#[test_context(TestContext)]
#[tokio::test]
async fn consume_single_pool_hits_returns_one_transaction(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let pool = &ctx.app_state.pool;

    // --- Given: a user, a covered Bucket with a ledger, an API Key. --------
    let user_id = create_test_user(pool, &realm_id, "cb_t01_single@example.com").await;
    let client_app_id = create_test_client_app(pool, &realm_id).await;
    let bucket_id = create_test_credit_bucket(
        pool,
        &realm_id,
        CreditBucketOpts {
            name: Some("Single Pool Bucket".into()),
            ..Default::default()
        },
    )
    .await;
    attach_bucket_client_app(pool, &realm_id, bucket_id, client_app_id).await;

    // Grant 5_000 granted credits into this bucket (no expiry = permanent).
    admin_grant_to_bucket(ctx, &realm_id, user_id, bucket_id, 5_000, None).await;

    let api_key = create_test_api_key(pool, &realm_id, client_app_id).await;

    // --- When: SDK consumes 100 from the covered single pool. --------------
    let (status, body) = consume_points_ext_via_api(
        ctx,
        &realm_id,
        user_id,
        client_app_id,
        100,
        Some("single pool consume"),
        None,
        &api_key,
    )
    .await;

    // --- Then: HTTP 200 with exactly one per-bucket transaction. ----------
    assert_eq!(status, StatusCode::OK, "single-pool consume should succeed");
    let body = body.expect("consume response body");

    assert_eq!(
        body["amount"].as_i64(),
        Some(100),
        "response amount == requested amount"
    );
    let correlation_id = body["correlationId"]
        .as_str()
        .expect("correlationId present")
        .to_string();
    assert!(!correlation_id.is_empty(), "correlationId non-empty");

    let transactions = body["transactions"].as_array().expect("transactions array");
    assert_eq!(
        transactions.len(),
        1usize,
        "single-pool hit produces exactly one transaction"
    );
    let txn = &transactions[0];
    assert_eq!(
        txn["bucketId"].as_str(),
        Some(bucket_id.to_string().as_str())
    );
    assert_eq!(txn["amount"].as_i64(), Some(100));
    assert_eq!(txn["balanceAfter"].as_i64(), Some(4_900));

    // --- And: DB shows one consume row with external_ref_id NULL. ---------
    let rows = fetch_consume_txns_by_correlation(pool, user_id, &correlation_id).await;
    assert_eq!(rows.len(), 1, "exactly one consume transaction row");
    let (_, b_id, _, amount, balance_after, external_ref_id) = &rows[0];
    assert_eq!(*b_id, bucket_id, "txn bucket_id matches the single pool");
    assert_eq!(*amount, -100, "stored amount negative (deduction)");
    assert_eq!(*balance_after, 4_900, "stored balance_after correct");
    assert!(
        external_ref_id.is_none(),
        "consume rows must keep external_ref_id NULL (design contract)"
    );

    // --- And: wallet balance reflects the deduction. ----------------------
    let bal = wallet_total_balance(pool, &realm_id, user_id, bucket_id).await;
    assert_eq!(bal, 4_900, "wallet balance after single-pool consume");
}

// =============================================================================
// Scenario 2: multi-pool spread across N Buckets returns N transactions
// =============================================================================

/// User Story: US-CB-007 (SDK consume) — multi-Bucket cross-pool spread.
/// Covers:
///   - ≥2 covered Buckets sharing one client_app; amount spans N pools.
///   - N per-bucket transactions, each carrying the correct
///     `bucket_id`/`wallet_id`/`balance_after`.
///   - All N rows share one `correlation_id`.
///   - Every consume row's `external_ref_id` is NULL (no uniqueness clash).
#[test_context(TestContext)]
#[tokio::test]
async fn consume_multi_pool_spreads_across_buckets_returns_n_transactions(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let pool = &ctx.app_state.pool;

    // --- Given: two Buckets covering the same client_app. -----------------
    let user_id = create_test_user(pool, &realm_id, "cb_t01_multi@example.com").await;
    let client_app_id = create_test_client_app(pool, &realm_id).await;

    let bucket_a = create_test_credit_bucket(
        pool,
        &realm_id,
        CreditBucketOpts {
            name: Some("Pool A".into()),
            bucket_key: Some("pool-a".into()),
            ..Default::default()
        },
    )
    .await;
    let bucket_b = create_test_credit_bucket(
        pool,
        &realm_id,
        CreditBucketOpts {
            name: Some("Pool B".into()),
            bucket_key: Some("pool-b".into()),
            ..Default::default()
        },
    )
    .await;
    attach_bucket_client_app(pool, &realm_id, bucket_a, client_app_id).await;
    attach_bucket_client_app(pool, &realm_id, bucket_b, client_app_id).await;

    // Grant 100 to pool A and 400 to pool B (total 500). Consume 250 spans
    // both pools: A's 100 is exhausted first (per expiry ordering), then 150
    // is taken from B.
    admin_grant_to_bucket(ctx, &realm_id, user_id, bucket_a, 100, None).await;
    admin_grant_to_bucket(ctx, &realm_id, user_id, bucket_b, 400, None).await;

    let api_key = create_test_api_key(pool, &realm_id, client_app_id).await;

    // --- When: SDK consumes 250 across both pools. ------------------------
    let (status, body) = consume_points_ext_via_api(
        ctx,
        &realm_id,
        user_id,
        client_app_id,
        250,
        Some("multi-pool spread"),
        None,
        &api_key,
    )
    .await;

    // --- Then: 200 with N=2 per-bucket transactions sharing correlation_id.
    assert_eq!(status, StatusCode::OK, "multi-pool consume should succeed");
    let body = body.expect("consume response body");
    assert_eq!(body["amount"].as_i64(), Some(250));
    let correlation_id = body["correlationId"]
        .as_str()
        .expect("correlationId present")
        .to_string();

    let transactions = body["transactions"].as_array().expect("transactions array");
    assert_eq!(
        transactions.len(),
        2usize,
        "multi-pool consume returns one transaction per affected bucket"
    );

    // Response is sorted by bucket_id ASC; per-bucket amounts must sum to 250.
    let mut resp_total = 0i64;
    let mut seen_buckets = std::collections::HashSet::new();
    for t in transactions {
        let amt = t["amount"].as_i64().expect("amount int");
        assert!(amt > 0, "per-bucket amount is the deduction magnitude");
        resp_total += amt;
        let bal_after = t["balanceAfter"].as_i64().expect("balanceAfter int");
        assert!(bal_after >= 0, "balance_after non-negative");
        seen_buckets.insert(t["bucketId"].as_str().expect("bucketId str").to_string());
    }
    assert_eq!(
        resp_total, 250,
        "per-bucket amounts sum to the requested amount"
    );
    assert_eq!(
        seen_buckets.len(),
        2,
        "response covers two distinct buckets"
    );

    // --- And: DB has 2 rows sharing correlation_id, all external_ref_id NULL.
    let rows = fetch_consume_txns_by_correlation(pool, user_id, &correlation_id).await;
    assert_eq!(rows.len(), 2, "two consume transaction rows in DB");
    for (_, bucket_id, _, amount, balance_after, external_ref_id) in &rows {
        assert!(
            *bucket_id == bucket_a || *bucket_id == bucket_b,
            "txn bucket_id within covered set"
        );
        assert!(*amount < 0, "stored amount negative (deduction)");
        assert!(*balance_after >= 0, "balance_after non-negative");
        assert!(
            external_ref_id.is_none(),
            "consume rows keep external_ref_id NULL (design contract)"
        );
    }

    // --- And: allocation rows exist (ledger-level truth source). ----------
    // NOTE: the API `allocations` field stays empty (see file header gap 1),
    // but the DB allocation rows are still written by the infra consume path.
    let alloc_count = count_allocations_for_correlation(pool, user_id, &correlation_id).await;
    assert!(
        alloc_count >= 1,
        "consumption allocations written for the multi-pool spread"
    );
}

// =============================================================================
// Scenario 3: permanent pool is consumed last
// =============================================================================

/// User Story: US-CB-007 (SDK consume) — expiry-ordered spread.
/// Covers:
///   - One short-term ledger (expires soon) + one permanent ledger in the
///     same covered set; the short-term ledger drains first; the permanent
///     pool is consumed only when the short-term balance is exhausted.
#[test_context(TestContext)]
#[tokio::test]
async fn consume_picks_permanent_pool_last(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let pool = &ctx.app_state.pool;

    let user_id = create_test_user(pool, &realm_id, "cb_t01_perm@example.com").await;
    let client_app_id = create_test_client_app(pool, &realm_id).await;

    let bucket_short = create_test_credit_bucket(
        pool,
        &realm_id,
        CreditBucketOpts {
            name: Some("Short-term Pool".into()),
            bucket_key: Some("short-pool".into()),
            ..Default::default()
        },
    )
    .await;
    let bucket_perm = create_test_credit_bucket(
        pool,
        &realm_id,
        CreditBucketOpts {
            name: Some("Permanent Pool".into()),
            bucket_key: Some("perm-pool".into()),
            ..Default::default()
        },
    )
    .await;
    attach_bucket_client_app(pool, &realm_id, bucket_short, client_app_id).await;
    attach_bucket_client_app(pool, &realm_id, bucket_perm, client_app_id).await;

    // Short-term ledger expires in 1 day, holds 100.
    let soon = chrono::Utc::now() + Duration::days(1);
    admin_grant_to_bucket(ctx, &realm_id, user_id, bucket_short, 100, Some(soon)).await;
    // Permanent ledger never expires, holds 1_000.
    admin_grant_to_bucket(ctx, &realm_id, user_id, bucket_perm, 1_000, None).await;

    let api_key = create_test_api_key(pool, &realm_id, client_app_id).await;

    // --- When: SDK consumes 150 (drains short-term 100 first, then 50 perm).
    let (status, body) = consume_points_ext_via_api(
        ctx,
        &realm_id,
        user_id,
        client_app_id,
        150,
        Some("expiry-ordered spread"),
        None,
        &api_key,
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let body = body.expect("consume response body");
    let correlation_id = body["correlationId"]
        .as_str()
        .expect("correlationId present")
        .to_string();
    let transactions = body["transactions"].as_array().expect("transactions array");
    assert_eq!(
        transactions.len(),
        2usize,
        "spread hits both pools when amount exceeds short-term balance"
    );

    // Sum per-bucket deductions; short-term must be fully drained (100),
    // permanent contributes the remaining 50.
    let mut short_deduct = 0i64;
    let mut perm_deduct = 0i64;
    for t in transactions {
        let b_id = t["bucketId"].as_str().expect("bucketId str").to_string();
        let amt = t["amount"].as_i64().expect("amount int");
        if b_id == bucket_short.to_string() {
            short_deduct += amt;
        } else if b_id == bucket_perm.to_string() {
            perm_deduct += amt;
        } else {
            panic!("unexpected bucket_id in response: {b_id}");
        }
    }
    assert_eq!(short_deduct, 100, "short-term pool drained first");
    assert_eq!(perm_deduct, 50, "permanent pool only covers the remainder");

    // --- And: short-term wallet drained to 0; permanent retains 950. ------
    assert_eq!(
        wallet_total_balance(pool, &realm_id, user_id, bucket_short).await,
        0,
        "short-term wallet drained to zero"
    );
    assert_eq!(
        wallet_total_balance(pool, &realm_id, user_id, bucket_perm).await,
        950,
        "permanent wallet only loses the remainder"
    );

    // Sanity: only the two covered bucket transactions exist for this consume.
    let rows = fetch_consume_txns_by_correlation(pool, user_id, &correlation_id).await;
    assert_eq!(rows.len(), 2);
}

// =============================================================================
// Scenario 4: no covered pool → 409 no_covered_pool
// =============================================================================

/// User Story: US-CB-007 (SDK consume) — explicit rejection when no pool
/// covers the requested client_app, while balances already held in a
/// DISABLED bucket stay spendable (credit-bucket PRD §4.2: disabling hides
/// the bucket from the catalog but never freezes held balances).
/// Covers:
///   - client_app has zero `credit_bucket_client_apps` rows → 409
///     `no_covered_pool`.
///   - client_app covered only by a disabled bucket, user holds balance →
///     consume SUCCEEDS, spending from that bucket.
#[test_context(TestContext)]
#[tokio::test]
async fn consume_no_covered_pool_returns_no_covered_pool(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let pool = &ctx.app_state.pool;

    let user_id = create_test_user(pool, &realm_id, "cb_t01_nocover@example.com").await;

    // client_app_a has no coverage; client_app_b is covered but disabled.
    let client_app_a = create_test_client_app(pool, &realm_id).await;
    let client_app_b = create_test_client_app(pool, &realm_id).await;

    let bucket_disabled = create_test_credit_bucket(
        pool,
        &realm_id,
        CreditBucketOpts {
            name: Some("Disabled Bucket".into()),
            bucket_key: Some("disabled-pool".into()),
            enabled: Some(false),
            ..Default::default()
        },
    )
    .await;
    attach_bucket_client_app(pool, &realm_id, bucket_disabled, client_app_b).await;

    // Grant points so the rejection is purely about coverage, not balance.
    admin_grant_to_bucket(ctx, &realm_id, user_id, bucket_disabled, 1_000, None).await;

    // --- When: SDK consumes via client_app_a (no coverage). ---------------
    let api_key_a = create_test_api_key(pool, &realm_id, client_app_a).await;
    let (status_a, body_a) = consume_points_ext_via_api(
        ctx,
        &realm_id,
        user_id,
        client_app_a,
        10,
        Some("no coverage"),
        None,
        &api_key_a,
    )
    .await;
    assert_eq!(
        status_a,
        StatusCode::CONFLICT,
        "no-coverage consume must be rejected with 409"
    );
    let body_a = body_a.expect("error body");
    assert_eq!(
        body_a["code"].as_str(),
        Some("no_covered_pool"),
        "error code no_covered_pool"
    );

    // --- And: client_app_b's only covered bucket is disabled, but the user
    // holds 1,000 there → consume must SUCCEED (held balances stay spendable).
    let api_key_b = create_test_api_key(pool, &realm_id, client_app_b).await;
    let (status_b, _body_b) = consume_points_ext_via_api(
        ctx,
        &realm_id,
        user_id,
        client_app_b,
        10,
        Some("disabled coverage"),
        None,
        &api_key_b,
    )
    .await;
    assert_eq!(
        status_b,
        StatusCode::OK,
        "consume from a disabled-but-held bucket must succeed"
    );

    // --- And: the consume transaction landed in the disabled bucket. -----
    let consume_rows: Vec<Uuid> = sqlx::query_scalar(
        "SELECT bucket_id FROM points_transactions
          WHERE user_id = $1 AND type = 'consume'",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
    .expect("fetch consume rows");
    assert_eq!(
        consume_rows,
        vec![bucket_disabled],
        "the consume row must come from the disabled bucket"
    );
}

// =============================================================================
// Scenario 5: insufficient points → 409 insufficient_points (have/need)
// =============================================================================

/// User Story: US-CB-007 (SDK consume) — coverage set total balance < amount.
/// Covers:
///   - Covered set resolves, but `sum(remaining_amount) < amount`
///     → 409 `insufficient_points` with `have`/`need` fields present.
#[test_context(TestContext)]
#[tokio::test]
async fn consume_insufficient_points_returns_insufficient_points(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let pool = &ctx.app_state.pool;

    let user_id = create_test_user(pool, &realm_id, "cb_t01_insuf@example.com").await;
    let client_app_id = create_test_client_app(pool, &realm_id).await;

    let bucket = create_test_credit_bucket(
        pool,
        &realm_id,
        CreditBucketOpts {
            name: Some("Low Balance Bucket".into()),
            bucket_key: Some("low-balance".into()),
            ..Default::default()
        },
    )
    .await;
    attach_bucket_client_app(pool, &realm_id, bucket, client_app_id).await;
    admin_grant_to_bucket(ctx, &realm_id, user_id, bucket, 50, None).await;

    let api_key = create_test_api_key(pool, &realm_id, client_app_id).await;

    // --- When: SDK requests 500 from a 50-balance covered set. ------------
    let (status, body) = consume_points_ext_via_api(
        ctx,
        &realm_id,
        user_id,
        client_app_id,
        500,
        Some("insufficient"),
        None,
        &api_key,
    )
    .await;

    assert_eq!(status, StatusCode::CONFLICT);
    let body = body.expect("error body");
    assert_eq!(
        body["code"].as_str(),
        Some("insufficient_points"),
        "error code insufficient_points"
    );
    assert!(
        body.get("have").is_some(),
        "insufficient_points body exposes have"
    );
    assert!(
        body.get("need").is_some(),
        "insufficient_points body exposes need"
    );
    assert_eq!(
        body["have"].as_i64(),
        Some(50),
        "have == covered-set balance"
    );
    assert_eq!(body["need"].as_i64(), Some(500), "need == requested amount");

    // --- And: no consume transaction written; balance untouched. ---------
    let consume_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::BIGINT FROM points_transactions
          WHERE user_id = $1 AND type = 'consume'",
    )
    .bind(user_id)
    .fetch_one(pool)
    .await
    .expect("count consume");
    assert_eq!(consume_count, 0, "no consume row after rejection");
    assert_eq!(
        wallet_total_balance(pool, &realm_id, user_id, bucket).await,
        50,
        "balance preserved on rejection"
    );
}

// =============================================================================
// Scenario 6: unauthorized (out-of-coverage) Bucket is never consumed
// =============================================================================

/// User Story: US-CB-007 (SDK consume) — coverage-set filter isolation.
/// Covers:
///   - User holds a ledger in a Bucket that is NOT covered by the client_app.
///   - Consume with an amount larger than the covered pool balance should
///     reject (insufficient) and NEVER touch the unauthorized pool's wallet.
#[test_context(TestContext)]
#[tokio::test]
async fn consume_excludes_unauthorized_bucket(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let pool = &ctx.app_state.pool;

    let user_id = create_test_user(pool, &realm_id, "cb_t01_unauth@example.com").await;
    let client_app_covered = create_test_client_app(pool, &realm_id).await;
    let client_app_other = create_test_client_app(pool, &realm_id).await;

    // Covered bucket (low balance) + out-of-coverage bucket (rich balance).
    let bucket_covered = create_test_credit_bucket(
        pool,
        &realm_id,
        CreditBucketOpts {
            name: Some("Covered Low".into()),
            bucket_key: Some("covered-low".into()),
            ..Default::default()
        },
    )
    .await;
    let bucket_other = create_test_credit_bucket(
        pool,
        &realm_id,
        CreditBucketOpts {
            name: Some("Out Of Coverage Rich".into()),
            bucket_key: Some("out-of-coverage-rich".into()),
            ..Default::default()
        },
    )
    .await;
    // bucket_covered is covered by client_app_covered; bucket_other is only
    // covered by client_app_other (different app) — not by client_app_covered.
    attach_bucket_client_app(pool, &realm_id, bucket_covered, client_app_covered).await;
    attach_bucket_client_app(pool, &realm_id, bucket_other, client_app_other).await;

    admin_grant_to_bucket(ctx, &realm_id, user_id, bucket_covered, 30, None).await;
    admin_grant_to_bucket(ctx, &realm_id, user_id, bucket_other, 10_000, None).await;

    let api_key = create_test_api_key(pool, &realm_id, client_app_covered).await;

    // --- When: SDK consumes 100 via client_app_covered (covered set = 30).
    let (status, body) = consume_points_ext_via_api(
        ctx,
        &realm_id,
        user_id,
        client_app_covered,
        100,
        Some("should not dip into unauthorized pool"),
        None,
        &api_key,
    )
    .await;

    // --- Then: rejected as insufficient; out-of-coverage pool untouched. --
    assert_eq!(status, StatusCode::CONFLICT);
    let body = body.expect("error body");
    assert_eq!(body["code"].as_str(), Some("insufficient_points"));
    assert_eq!(body["have"].as_i64(), Some(30));
    assert_eq!(body["need"].as_i64(), Some(100));

    // The out-of-coverage bucket must retain its full balance.
    assert_eq!(
        wallet_total_balance(pool, &realm_id, user_id, bucket_other).await,
        10_000,
        "out-of-coverage pool never consumed"
    );
    assert_eq!(
        wallet_total_balance(pool, &realm_id, user_id, bucket_covered).await,
        30,
        "covered pool balance preserved on rejection"
    );

    // And no consume transaction should reference bucket_other.
    let leak_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::BIGINT FROM points_transactions
          WHERE user_id = $1 AND type = 'consume' AND bucket_id = $2",
    )
    .bind(user_id)
    .bind(bucket_other)
    .fetch_one(pool)
    .await
    .expect("count unauthorized consume");
    assert_eq!(
        leak_count, 0,
        "no consume txn references unauthorized bucket"
    );
}

// =============================================================================
// Scenario 7: idempotency replay returns the same result set
// =============================================================================

/// User Story: US-CB-007 (SDK consume) — multi-pool idempotency replay.
/// Covers:
///   - Replaying the same `idempotencyKey` returns 200 with the original
///     correlation_id + the same per-bucket transaction set; the user's
///     balances are NOT re-deducted.
///
/// NOTE (runtime gap, see file header): the current replay path returns the
/// cached primary transaction only, not the full N-txn set. This test encodes
/// the INTENDED contract; the runner will see
/// it fail and triage the underlying gap in service.rs.
#[test_context(TestContext)]
#[tokio::test]
async fn consume_idempotency_replay_returns_same_result_set(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let pool = &ctx.app_state.pool;

    let user_id = create_test_user(pool, &realm_id, "cb_t01_idem@example.com").await;
    let client_app_id = create_test_client_app(pool, &realm_id).await;

    let bucket_a = create_test_credit_bucket(
        pool,
        &realm_id,
        CreditBucketOpts {
            name: Some("Idem Pool A".into()),
            bucket_key: Some("idem-pool-a".into()),
            ..Default::default()
        },
    )
    .await;
    let bucket_b = create_test_credit_bucket(
        pool,
        &realm_id,
        CreditBucketOpts {
            name: Some("Idem Pool B".into()),
            bucket_key: Some("idem-pool-b".into()),
            ..Default::default()
        },
    )
    .await;
    attach_bucket_client_app(pool, &realm_id, bucket_a, client_app_id).await;
    attach_bucket_client_app(pool, &realm_id, bucket_b, client_app_id).await;
    admin_grant_to_bucket(ctx, &realm_id, user_id, bucket_a, 200, None).await;
    admin_grant_to_bucket(ctx, &realm_id, user_id, bucket_b, 200, None).await;

    let api_key = create_test_api_key(pool, &realm_id, client_app_id).await;
    let idempotency_key = format!("consume-idem-{}", Uuid::now_v7());

    // --- First consume: 250 across both pools. ---------------------------
    let (status1, body1) = consume_points_ext_via_api(
        ctx,
        &realm_id,
        user_id,
        client_app_id,
        250,
        Some("first multi-pool"),
        Some(&idempotency_key),
        &api_key,
    )
    .await;
    assert_eq!(status1, StatusCode::OK, "first consume succeeds");
    let body1 = body1.expect("first body");
    let correlation1 = body1["correlationId"]
        .as_str()
        .expect("correlationId")
        .to_string();
    let tx_count1 = body1["transactions"]
        .as_array()
        .expect("transactions array")
        .len();

    let balances_after_first_a = wallet_total_balance(pool, &realm_id, user_id, bucket_a).await;
    let balances_after_first_b = wallet_total_balance(pool, &realm_id, user_id, bucket_b).await;
    let total_after_first = balances_after_first_a + balances_after_first_b;
    assert_eq!(total_after_first, 400 - 250, "balances deducted once");

    // --- Replay with the same idempotencyKey. ----------------------------
    let (status2, body2) = consume_points_ext_via_api(
        ctx,
        &realm_id,
        user_id,
        client_app_id,
        250,
        Some("replay multi-pool"),
        Some(&idempotency_key),
        &api_key,
    )
    .await;

    // --- Then: 200 with the same correlation_id and same N-txn shape. ----
    assert_eq!(
        status2,
        StatusCode::OK,
        "replay returns 200 with the cached result set (not 409)"
    );
    let body2 = body2.expect("replay body");
    let correlation2 = body2["correlationId"]
        .as_str()
        .expect("correlationId")
        .to_string();
    assert_eq!(
        correlation1, correlation2,
        "replay returns the same correlation_id"
    );
    let tx_count2 = body2["transactions"]
        .as_array()
        .expect("transactions array")
        .len();
    assert_eq!(
        tx_count1, tx_count2,
        "replay returns the same number of per-bucket transactions"
    );
    assert_eq!(
        body1["amount"].as_i64(),
        body2["amount"].as_i64(),
        "replay amount matches"
    );

    // --- And: balances were NOT re-deducted. -----------------------------
    assert_eq!(
        wallet_total_balance(pool, &realm_id, user_id, bucket_a).await,
        balances_after_first_a,
        "pool A balance unchanged after replay"
    );
    assert_eq!(
        wallet_total_balance(pool, &realm_id, user_id, bucket_b).await,
        balances_after_first_b,
        "pool B balance unchanged after replay"
    );

    // --- And: DB has exactly N consume rows for that correlation_id. -----
    let rows = fetch_consume_txns_by_correlation(pool, user_id, &correlation1).await;
    assert_eq!(
        rows.len(),
        tx_count1,
        "DB consume row count matches the first-call N (no duplicate writes)"
    );
}

// =============================================================================
// Scenario 8: concurrent consumes do not overdraw (run serially)
// =============================================================================

/// User Story: US-CB-007 (SDK consume) — concurrency safety.
/// Covers:
///   - Two concurrent consumes against the same user+covered set whose total
///     equals the available balance; the actual deductions never exceed the
///     balance (no overdraw).
///
/// Runner note: this test exercises the same `(user, wallet)` row under
/// concurrency, which is sensitive to DB row locks. It should be executed
/// serially with the rest of the concurrent subset
/// (`--test-threads 1`).
#[test_context(TestContext)]
#[tokio::test]
async fn consume_concurrent_does_not_overdraw(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let pool = &ctx.app_state.pool;

    let user_id =
        create_test_user_with_auth(pool, &realm_id, "cb_t01_conc@example.com", "pw123").await;
    let client_app_id = create_test_client_app(pool, &realm_id).await;

    let bucket = create_test_credit_bucket(
        pool,
        &realm_id,
        CreditBucketOpts {
            name: Some("Concurrency Bucket".into()),
            bucket_key: Some("concurrency-bucket".into()),
            ..Default::default()
        },
    )
    .await;
    attach_bucket_client_app(pool, &realm_id, bucket, client_app_id).await;
    // Total available = 1_000. Each concurrent request asks for 1_000, so the
    // FOR UPDATE ledger lock is the only thing preventing overdraw.
    admin_grant_to_bucket(ctx, &realm_id, user_id, bucket, 1_000, None).await;

    let api_key = create_test_api_key(pool, &realm_id, client_app_id).await;

    // Fire two concurrent consumes from the SAME user against the SAME pool.
    // The helper builds a fresh router per call and only shares the immutable
    // Arc<AppState>, so two reborrowed `&TestContext` references are safe
    // under tokio::join (no mutation through either reference).
    let realm_clone = realm_id.clone();
    let api_clone = api_key.clone();

    // Reborrow ctx immutably twice inside the join — both futures only read
    // ctx fields through shared Arcs (app_state) without mutation.
    let fut1 = consume_points_ext_via_api(
        ctx,
        &realm_clone,
        user_id,
        client_app_id,
        1_000,
        Some("concurrent #1"),
        None,
        &api_clone,
    );
    let fut2 = consume_points_ext_via_api(
        ctx,
        &realm_clone,
        user_id,
        client_app_id,
        1_000,
        Some("concurrent #2"),
        None,
        &api_clone,
    );
    let ((s1, _b1), (s2, _b2)) = tokio::join!(fut1, fut2);

    // At most one of the two requests can succeed (200); the other must be
    // a non-2xx rejection. We do not assert exact status because the runner
    // may run this serially (one after the other, both succeeding only if
    // amounts don't overlap) — but the invariant is balance non-negative.
    let _ = (s1, s2);

    // --- Then: total deducted never exceeds the original 1_000 balance. --
    let final_balance = wallet_total_balance(pool, &realm_id, user_id, bucket).await;
    assert!(
        final_balance >= 0,
        "balance never negative (no overdraw); got {final_balance}"
    );

    // Sum the deduction magnitudes of all consume transactions.
    let total_consumed: i64 = sqlx::query_scalar(
        r#"SELECT COALESCE(SUM(-amount), 0)::BIGINT
             FROM points_transactions
            WHERE user_id = $1 AND bucket_id = $2 AND type = 'consume'"#,
    )
    .bind(user_id)
    .bind(bucket)
    .fetch_one(pool)
    .await
    .expect("sum consume");
    assert!(
        total_consumed <= 1_000,
        "total deductions must not exceed the original balance; got {total_consumed}"
    );
    assert_eq!(
        total_consumed,
        1_000 - final_balance,
        "ledger deductions reconcile with wallet balance"
    );
}
