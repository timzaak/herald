// =============================================================================
// Scenario Tests: Bucket overview/delete derived availability predicate
// =============================================================================
//
// Covers the derived availability predicate behind the bucket overview read
// path and PRD `docs/prd/billing/credit-bucket.md:112` (delete guard). The
// overview display and the delete guard's holdersWithBalance arm derive from
// the SAME predicate — a future-effective row is never spendable balance —
// while the DELETE path answers to the PRD alone: 存在钱包/交易引用时
// `DELETE` 返回 409 bucket_in_use,改用禁用以保留历史归属 — a
// not-yet-effective pre-grant row is such a reference.
//
// WHY these tests exist (encoded as assertions, per Rule 9):
//
// (a) bucket overview's available balance EXCLUDES future-effective rows. A
//     bucket whose ONLY ledger row is future-effective (effective_at > NOW())
//     must show `bucketTotal == 0` in overview — the derived SUM predicate
//     `(effective_at IS NULL OR effective_at <= NOW())` filters it out. If the
//     overview ever regressed to reading the (now-dropped) Stored
//     `points_wallets.total_balance` column, or if the predicate were dropped,
//     the future-effective row would leak into the displayed balance.
//
// (b) a bucket with ONLY future-effective rows and NO active subscription is
//     NOT deletable (PRD credit-bucket.md:112). A pre-grant ledger row is a
//     交易/钱包 history reference even though it is not yet spendable, so the
//     delete answers 409 bucket_in_use with historyReferences >= 1 while
//     holdersWithBalance stays 0 — the derived availability predicate remains
//     the authority for the balance arm and a future-effective row must never
//     be misreported as spendable balance. Sweeping such rows on delete
//     would silently destroy a grant that is due to
//     materialize at effective_at.
//
// (c) a bucket with at least one CURRENTLY-available ledger row (effective_at
//     IS NULL or <= NOW(), expires_at IS NULL or > NOW(), remaining_amount > 0,
//     status = active) is NOT deletable — the derived guard blocks it with 409
//     bucket_in_use + holdersWithBalance >= 1.
//
// (d) a bucket bound to an active subscription is NOT deletable regardless of
//     balance (409 bucket_in_use + activeSubscriptions >= 1), even if its only
//     ledger rows are future-effective.
//
// All scenarios exercise the real production HTTP path through the unified test
// router (`/api/realms/{realmId}/billing/credit-buckets...`) gated on Realm
// Admin `points.manage`. Derived balance is asserted via direct SQL mirroring
// the production `compute_bucket_available_balances` predicate (the
// helper), NOT by reading any Stored wallet column.
//
// Per authoring rules: tests target the intended design contract. Runtime gaps
// (if any) are recorded inline; the runner (BE-TR) triages runtime failures.
//
// =============================================================================

#![allow(clippy::too_many_arguments)]

use crate::tests::helpers::billing_helpers::setup_billing_admin_session;
use crate::tests::helpers::credit_bucket_helpers::{
    CreditBucketOpts, attach_bucket_client_app, auth_admin_request_via_api,
    create_test_credit_bucket, seed_active_subscription_on_bucket,
};
use crate::tests::scenarios::points::fixtures::{create_test_client_app, create_test_user};
use crate::tests::schema_test_context::SchemaTestContext as TestContext;
use axum::http::StatusCode;
use chrono::{Duration, Utc};
use serde_json::Value;
use test_context::test_context;
use uuid::Uuid;

// =============================================================================
// Local helpers
// =============================================================================

/// Availability predicate mirroring production `compute_bucket_available_
/// balancelist_bucket_overview` and the `delete_credit_bucket` holders guard
/// Kept inline so the full SQL is readable at the call
/// site. If this drifts from the production text in
/// `backend/infra/src/billing/postgres_repository.rs`, step 2 will catch it;
/// this local copy is for SEED-TIME filtering only.
const DERIVED_AVAILABILITY_PREDICATE: &str = concat!(
    "status = 'active'",
    " AND remaining_amount > 0",
    " AND (effective_at IS NULL OR effective_at <= NOW())",
    " AND (expires_at IS NULL OR expires_at > NOW())"
);

/// Seed a `points_credit_ledger` row bound to a SPECIFIC `bucket_id` with an
/// explicit `effective_at`.
///
/// This is a bucket-directory-specific variant of the
/// `create_credit_ledger_entry_with_effective_at` helper: that helper always
/// binds the row to the realm's legacy test bucket (via
/// `ensure_test_bucket_for_realm`), which is wrong for bucket-directory
/// scenarios that need the row tied to a freshly created, identifiable bucket
/// so the overview/delete endpoints read it under that bucket.
///
/// Mirrors the column list of the helper verbatim (including
/// `effective_at`, added by migration `20260621_points_effective_at.sql`)
/// and does NOT touch any `points_wallets` Stored column (the balance columns
/// were physically dropped; available balance is
/// exclusively a derived SUM over this table).
///
/// `effective_at`:
///   - `None`           ⟺ immediately available (control / currently-effective).
///   - `Some(t)`        ⟺ enters the available set only at/after `t`.
///
/// `expires_at`:
///   - `None`           ⟺ never expires.
///   - `Some(t)`        ⟺ excluded from availability after `t`.
///
/// The DB CHECK `points_credit_ledger_effective_before_expires` rejects an
/// inverted window (`effective_at > expires_at`); callers must keep
/// `effective_at <= expires_at` (we set a wide expiry by default below).
async fn seed_ledger_on_bucket_with_effective_at(
    pool: &sqlx::PgPool,
    realm_id: &str,
    user_id: Uuid,
    bucket_id: Uuid,
    credit_type: &str,
    source_type: &str,
    source_id: &str,
    amount: i64,
    expires_at: Option<chrono::DateTime<chrono::Utc>>,
    effective_at: Option<chrono::DateTime<chrono::Utc>>,
) -> Uuid {
    let ledger_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO points_credit_ledger
            (id, user_id, realm_id, bucket_id, credit_type, source_type, source_id,
             granted_amount, used_amount, revoked_amount,
             expires_at, effective_at, status, created_at, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 0, 0, $9, $10, 'active', NOW(), NOW())",
    )
    .bind(ledger_id)
    .bind(user_id)
    .bind(realm_id)
    .bind(bucket_id)
    .bind(credit_type)
    .bind(source_type)
    .bind(source_id)
    .bind(amount)
    .bind(expires_at)
    .bind(effective_at)
    .execute(pool)
    .await
    .expect("Failed to seed points_credit_ledger row on specific bucket");
    ledger_id
}

/// Count ledger rows on a specific bucket matching the derived availability
/// predicate. Mirrors the production `delete_credit_bucket`
/// holders_with_balance count. Used as a sanity check to prove which guard
/// arm (balance vs history vs subscription) is the one firing.
async fn count_available_ledger_rows_on_bucket(pool: &sqlx::PgPool, bucket_id: Uuid) -> i64 {
    let sql = format!(
        "SELECT COUNT(*)::BIGINT FROM points_credit_ledger
          WHERE bucket_id = $1 AND ({pred})",
        pred = DERIVED_AVAILABILITY_PREDICATE
    );
    sqlx::query_scalar(&sql)
        .bind(bucket_id)
        .fetch_one(pool)
        .await
        .expect("Failed to count available ledger rows on bucket")
}

/// Count ALL ledger rows on a specific bucket (any status / any window). Used
/// as a sanity check that a seeded future-effective row physically exists —
/// the availability predicate excludes it from the available count.
async fn count_all_ledger_rows_on_bucket(pool: &sqlx::PgPool, bucket_id: Uuid) -> i64 {
    sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*)::BIGINT FROM points_credit_ledger WHERE bucket_id = $1",
    )
    .bind(bucket_id)
    .fetch_one(pool)
    .await
    .expect("Failed to count all ledger rows on bucket")
}

/// Extract the `code` field from an error JSON body, or None if absent.
fn error_code(body: &Option<Value>) -> Option<&str> {
    body.as_ref()
        .and_then(|v| v.get("code"))
        .and_then(|c| c.as_str())
}

/// Extract an integer field from a JSON body.
fn int_field(body: &Option<Value>, field: &str) -> Option<i64> {
    body.as_ref()
        .and_then(|v| v.get(field))
        .and_then(|n| n.as_i64())
}

/// Find a specific bucket row in the overview `rows[]` array by id.
fn find_overview_row(body: &Value, bucket_id: Uuid) -> Option<&serde_json::Map<String, Value>> {
    body.get("rows").and_then(|v| v.as_array()).and_then(|arr| {
        arr.iter().find_map(|r| {
            r.as_object().filter(|m| {
                m.get("bucketId").and_then(|v| v.as_str()) == Some(&bucket_id.to_string())
            })
        })
    })
}

// =============================================================================
// Scenario 1: overview EXCLUDES future-effective rows from available balance
// =============================================================================

/// User Story: US-CB-001 (admin views the bucket × credit-type overview matrix;
/// the displayed available balance must reflect only spendable credits).
/// Covers (risk P1):
///   - `list_bucket_overview` builds each bucket's `byCreditType` /
///     `bucketTotal` from a DERIVED SUM over `points_credit_ledger` with the
///     availability predicate `status='active' AND remaining_amount>0 AND
///     (effective_at IS NULL OR effective_at<=NOW()) AND (expires_at IS NULL
///     OR expires_at>NOW())`. Future-effective pre-grant rows are excluded —
///     they are not yet spendable.
///   - A bucket whose ONLY ledger row is future-effective must appear in
///     overview (zero-balance buckets still list) with `bucketTotal == 0` and
///     zero across every `byCreditType` field. If overview regressed to
///     reading the now-dropped `points_wallets.total_balance` Stored column
///     (or dropped the effective_at predicate), the future-effective row would
///     leak into the displayed balance — this test fails loud in that case.
#[test_context(TestContext)]
#[tokio::test]
async fn test_bucket_overview_excludes_future_effective(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let token = setup_billing_admin_session(ctx, "cb_t10_ov_future@example.com").await;
    let pool = &ctx.app_state.pool;

    let client_app = create_test_client_app(pool, &realm_id).await;
    let bucket = create_test_credit_bucket(
        pool,
        &realm_id,
        CreditBucketOpts {
            name: Some("Future-Effective Only".into()),
            bucket_key: Some(format!("future-only-{}", Uuid::now_v7())),
            ..Default::default()
        },
    )
    .await;
    attach_bucket_client_app(pool, &realm_id, bucket, client_app).await;

    // Holder owns a ledger row in this bucket, but it is future-effective
    // (effective_at = NOW + 1 day) so it is NOT spendable yet. expires_at is
    // set wide (NOW + 30 days) so the row is NOT expired — the ONLY reason it
    // is excluded is the effective_at predicate, which is exactly the
    // regression we are pinning.
    let holder = create_test_user(pool, &realm_id, "cb_t10_ov_future_holder@example.com").await;
    let future_effective = Some(Utc::now() + Duration::days(1));
    let wide_expiry = Some(Utc::now() + Duration::days(30));
    seed_ledger_on_bucket_with_effective_at(
        pool,
        &realm_id,
        holder,
        bucket,
        "granted_credit",
        "admin_grant",
        &format!("t10-ov-future-{}", Uuid::now_v7()),
        400,
        wide_expiry,
        future_effective,
    )
    .await;

    // Sanity: the row exists and matches the predicate EXCLUSION (available
    // count is 0, total count is 1).
    assert_eq!(
        count_available_ledger_rows_on_bucket(pool, bucket).await,
        0,
        "future-effective row must NOT match the availability predicate"
    );
    assert_eq!(
        count_all_ledger_rows_on_bucket(pool, bucket).await,
        1,
        "the seeded future-effective row must still exist on the bucket"
    );

    let (status, body) = auth_admin_request_via_api(
        ctx,
        "GET",
        &format!("/api/realms/{}/billing/credit-buckets/overview", realm_id),
        &token,
        None,
    )
    .await;

    assert_eq!(
        status,
        StatusCode::OK,
        "overview: {:?} body={:?}",
        status,
        body
    );
    let body = body.expect("overview body must be present");

    // The bucket must still appear in rows[] (zero-balance buckets are kept).
    let row = find_overview_row(&body, bucket).unwrap_or_else(|| {
        panic!(
            "future-effective-only bucket must still appear in overview rows; body: {}",
            body
        )
    });

    // The core regression assertion: bucketTotal == 0 (future-effective row
    // excluded from the derived SUM).
    let bucket_total = row
        .get("bucketTotal")
        .and_then(|v| v.as_i64())
        .unwrap_or(-1);
    assert_eq!(
        bucket_total, 0,
        "bucketTotal MUST be 0 for a future-effective-only bucket (derived \
         predicate excludes effective_at > NOW()); got {} — overview is leaking \
         future-effective rows into the displayed available balance; body: {}",
        bucket_total, body
    );

    // Every per-credit-type field must also be 0 (no leakage into a specific
    // bucket either). `byCreditType` is an object keyed by credit-type field
    // name; assert none of the known typed fields are positive.
    if let Some(by_ct) = row.get("byCreditType").and_then(|v| v.as_object()) {
        for typed_field in [
            "topup",
            "subscription",
            "granted",
            "registration",
            "freePeriodic",
        ] {
            if let Some(val) = by_ct.get(typed_field).and_then(|v| v.as_i64()) {
                assert_eq!(
                    val, 0,
                    "byCreditType.{} MUST be 0 for a future-effective-only bucket; \
                     got {} — body: {}",
                    typed_field, val, body
                );
            }
        }
    }
}

// =============================================================================
// Scenario 2: delete REJECTED when only future-effective rows exist (no active
// subscription) — history reference blocks; balance arm stays 0
// =============================================================================

/// User Story: US-CB-001 (admin must disable — not delete — a bucket whose
/// only credits are not yet spendable).
/// Covers (risk P1):
///   - PRD credit-bucket.md:112: any 钱包/交易 reference blocks the delete.
///     A future-effective pre-grant row is such a reference — it is history
///     that will materialize at effective_at — so `delete_credit_bucket`
///     refuses with 409 `bucket_in_use` + `historyReferences >= 1`.
///   - `holdersWithBalance` MUST stay 0: the derived availability predicate
///     (not the raw row count) remains the authority for the balance arm, so
///     a not-yet-spendable row is never misreported as spendable balance.
///   - The bucket row and the ledger row both SURVIVE the refused delete —
///     sweeping history on delete would silently destroy the pending grant.
#[test_context(TestContext)]
#[tokio::test]
async fn test_bucket_delete_rejected_when_only_future_effective(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let token = setup_billing_admin_session(ctx, "cb_t10_del_future@example.com").await;
    let pool = &ctx.app_state.pool;

    let client_app = create_test_client_app(pool, &realm_id).await;
    let bucket = create_test_credit_bucket(
        pool,
        &realm_id,
        CreditBucketOpts {
            name: Some("Del Future-Only".into()),
            bucket_key: Some(format!("del-future-{}", Uuid::now_v7())),
            ..Default::default()
        },
    )
    .await;
    attach_bucket_client_app(pool, &realm_id, bucket, client_app).await;

    // Only a future-effective ledger row on this bucket. No active
    // subscription. The row is NOT spendable yet (excluded by the derived
    // predicate) but IS a history reference that blocks the delete.
    let holder = create_test_user(pool, &realm_id, "cb_t10_del_future_holder@example.com").await;
    let future_effective = Some(Utc::now() + Duration::days(1));
    let wide_expiry = Some(Utc::now() + Duration::days(30));
    let ledger_id = seed_ledger_on_bucket_with_effective_at(
        pool,
        &realm_id,
        holder,
        bucket,
        "granted_credit",
        "admin_grant",
        &format!("t10-del-future-{}", Uuid::now_v7()),
        500,
        wide_expiry,
        future_effective,
    )
    .await;

    // Sanity: the row does NOT count toward the balance arm — the blocker
    // must be the history arm, not a miscounted spendable balance.
    assert_eq!(
        count_available_ledger_rows_on_bucket(pool, bucket).await,
        0,
        "future-effective row must NOT count toward the balance arm of the delete guard"
    );

    let (status, body) = auth_admin_request_via_api(
        ctx,
        "DELETE",
        &format!("/api/realms/{}/billing/credit-buckets/{}", realm_id, bucket),
        &token,
        None,
    )
    .await;

    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "delete of a future-effective-only bucket MUST be 409 bucket_in_use \
         (the pre-grant row is a history reference per PRD credit-bucket.md:112; \
         use disable to retire the bucket); got {}: {:?}",
        status,
        body
    );
    assert_eq!(
        error_code(&body),
        Some("bucket_in_use"),
        "expected bucket_in_use code, got: {:?}",
        body
    );
    assert!(
        int_field(&body, "historyReferences")
            .map(|n| n >= 1)
            .unwrap_or(false),
        "historyReferences MUST be >= 1 (the future-effective ledger row is counted \
         as history); got: {:?}",
        body
    );
    assert_eq!(
        int_field(&body, "holdersWithBalance"),
        Some(0),
        "holdersWithBalance MUST stay 0 — the future-effective row is not spendable \
         and must not leak into the balance arm; got: {:?}",
        body
    );
    assert_eq!(
        int_field(&body, "activeSubscriptions"),
        Some(0),
        "activeSubscriptions MUST be 0 (none seeded); got: {:?}",
        body
    );

    // Post-refusal: the bucket row is still present (delete rolled back).
    let still_exists: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM credit_buckets WHERE id = $1")
            .bind(bucket)
            .fetch_optional(pool)
            .await
            .expect("check bucket present after refused delete");
    assert!(
        still_exists.is_some(),
        "credit_buckets row MUST survive a refused delete"
    );

    // Post-refusal: the future-effective ledger row survives too — no history
    // sweep may run on a refused delete. Destroying it would silently cancel a
    // grant that is due to materialize at effective_at.
    let ledger_still: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM points_credit_ledger WHERE id = $1")
            .bind(ledger_id)
            .fetch_optional(pool)
            .await
            .expect("check ledger row present after refused delete");
    assert!(
        ledger_still.is_some(),
        "future-effective ledger row MUST survive a refused bucket delete — \
         history is preserved for the disable-instead remedy"
    );
}

// =============================================================================
// Scenario 3: delete REJECTED when a currently-effective available row exists
// =============================================================================

/// User Story: US-CB-001 (admin cannot delete a bucket still holding
/// spendable credits).
/// Covers (risk P1):
///   - `delete_credit_bucket` refuses with 409 `bucket_in_use` +
///     `holdersWithBalance >= 1` when at least one currently-effective (i.e.
///     matching the derived availability predicate) active ledger row exists
///     on the bucket.
///   - The seeded row is immediately available (effective_at = NULL,
///     expires_at wide, remaining_amount > 0). It MUST count under the guard.
///   - This pins the "still spendable ⇒ not deletable" half of the
///     delete-protection semantics; paired with scenario 2 (future-
///     effective-only ⇒ 409 via historyReferences while holdersWithBalance
///     stays 0) it locks the derived predicate as the sole authority for the
///     holdersWithBalance count.
#[test_context(TestContext)]
#[tokio::test]
async fn test_bucket_delete_rejected_when_has_effective_balance(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let token = setup_billing_admin_session(ctx, "cb_t10_del_eff_bal@example.com").await;
    let pool = &ctx.app_state.pool;

    let client_app = create_test_client_app(pool, &realm_id).await;
    let bucket = create_test_credit_bucket(
        pool,
        &realm_id,
        CreditBucketOpts {
            name: Some("Del Effective Bal".into()),
            bucket_key: Some(format!("del-eff-bal-{}", Uuid::now_v7())),
            ..Default::default()
        },
    )
    .await;
    attach_bucket_client_app(pool, &realm_id, bucket, client_app).await;

    // A CURRENTLY-AVAILABLE ledger row: effective_at = NULL (immediately
    // spendable), expires_at well in the future, status active,
    // remaining_amount > 0. This row MUST be counted by the guard.
    let holder = create_test_user(pool, &realm_id, "cb_t10_del_eff_bal_holder@example.com").await;
    seed_ledger_on_bucket_with_effective_at(
        pool,
        &realm_id,
        holder,
        bucket,
        "granted_credit",
        "admin_grant",
        &format!("t10-del-eff-bal-{}", Uuid::now_v7()),
        300,
        Some(Utc::now() + Duration::days(30)),
        None, // effective_at = NULL ⟺ immediately available
    )
    .await;

    // Sanity: the row counts under the derived predicate.
    assert_eq!(
        count_available_ledger_rows_on_bucket(pool, bucket).await,
        1,
        "currently-effective row MUST count under the availability predicate"
    );

    let (status, body) = auth_admin_request_via_api(
        ctx,
        "DELETE",
        &format!("/api/realms/{}/billing/credit-buckets/{}", realm_id, bucket),
        &token,
        None,
    )
    .await;

    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "delete with currently-effective available balance MUST be 409 bucket_in_use \
         (derived guard); got {}: {:?}",
        status,
        body
    );
    assert_eq!(
        error_code(&body),
        Some("bucket_in_use"),
        "expected bucket_in_use code, got: {:?}",
        body
    );
    assert!(
        int_field(&body, "holdersWithBalance")
            .map(|n| n >= 1)
            .unwrap_or(false),
        "holdersWithBalance MUST be >= 1 (currently-effective row counted by the \
         derived guard); got: {:?}",
        body
    );

    // Belt-and-suspenders: the bucket row is still present (delete rolled
    // back).
    let still_exists: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM credit_buckets WHERE id = $1")
            .bind(bucket)
            .fetch_optional(pool)
            .await
            .expect("check bucket still present after rejected delete");
    assert!(
        still_exists.is_some(),
        "credit_buckets row MUST still exist after a rejected delete (rollback)"
    );
}

// =============================================================================
// Scenario 4: delete REJECTED when an active subscription is bound (regardless
// of ledger effective state)
// =============================================================================

/// User Story: US-CB-001 (admin cannot delete a bucket still bound to an
/// in-flight subscription).
/// Covers:
///   - `delete_credit_bucket` refuses with 409 `bucket_in_use` +
///     `activeSubscriptions >= 1` when an in-flight subscription is bound to
///     the bucket, REGARDLESS of ledger balance state. "bucket
///     删除前仍必须先确认无 active subscription".
///   - This scenario pairs an active subscription with a future-effective-
///     only ledger state (derived balance == 0). The delete MUST still be
///     refused on the subscription arm — proving the subscription check is
///     independent of the derived-balance check and is not short-circuited
///     by "balance == 0".
#[test_context(TestContext)]
#[tokio::test]
async fn test_bucket_delete_rejected_when_has_active_subscription(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let token = setup_billing_admin_session(ctx, "cb_t10_del_sub@example.com").await;
    let pool = &ctx.app_state.pool;

    let client_app = create_test_client_app(pool, &realm_id).await;
    let bucket = create_test_credit_bucket(
        pool,
        &realm_id,
        CreditBucketOpts {
            name: Some("Del Active Sub".into()),
            bucket_key: Some(format!("del-active-sub-{}", Uuid::now_v7())),
            ..Default::default()
        },
    )
    .await;
    attach_bucket_client_app(pool, &realm_id, bucket, client_app).await;

    // Bind an active subscription to this bucket (in-flight status counted by
    // the delete guard).
    seed_active_subscription_on_bucket(pool, &realm_id, bucket).await;

    // Also seed a future-effective ledger row so the derived balance is 0.
    // This proves the subscription arm fires INDEPENDENTLY of balance — a
    // regression that skipped the subscription check when balance == 0 would
    // wrongly allow the delete here.
    let holder = create_test_user(pool, &realm_id, "cb_t10_del_sub_holder@example.com").await;
    seed_ledger_on_bucket_with_effective_at(
        pool,
        &realm_id,
        holder,
        bucket,
        "subscription_credit",
        "subscription_initial",
        &format!("t10-del-sub-{}", Uuid::now_v7()),
        1000,
        Some(Utc::now() + Duration::days(30)),
        Some(Utc::now() + Duration::days(1)), // future-effective ⟹ not counted
    )
    .await;

    // Sanity: derived balance is 0 (future-effective excluded); the ONLY
    // blocker is the subscription.
    assert_eq!(
        count_available_ledger_rows_on_bucket(pool, bucket).await,
        0,
        "future-effective row must NOT count; the only blocker must be the subscription"
    );

    let (status, body) = auth_admin_request_via_api(
        ctx,
        "DELETE",
        &format!("/api/realms/{}/billing/credit-buckets/{}", realm_id, bucket),
        &token,
        None,
    )
    .await;

    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "delete with an active subscription MUST be 409 bucket_in_use, even when \
         derived balance is 0 (subscription check is independent); \
         got {}: {:?}",
        status,
        body
    );
    assert_eq!(
        error_code(&body),
        Some("bucket_in_use"),
        "expected bucket_in_use code, got: {:?}",
        body
    );
    assert!(
        int_field(&body, "activeSubscriptions")
            .map(|n| n >= 1)
            .unwrap_or(false),
        "activeSubscriptions MUST be >= 1; got: {:?}",
        body
    );

    // Belt-and-suspenders: the bucket row is still present (delete rolled
    // back).
    let still_exists: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM credit_buckets WHERE id = $1")
            .bind(bucket)
            .fetch_optional(pool)
            .await
            .expect("check bucket still present after rejected delete");
    assert!(
        still_exists.is_some(),
        "credit_buckets row MUST still exist after a rejected delete (rollback)"
    );
}
