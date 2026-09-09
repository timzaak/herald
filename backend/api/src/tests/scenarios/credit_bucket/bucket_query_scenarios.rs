// =============================================================================
// Scenario Tests: per-bucket query surface
// =============================================================================
//
// Covers design `.ai/design/credit-bucket.md`:
//   - response contracts for the per-bucket query endpoints:
//       * `GET .../points/wallets` → `ListWalletsByBucketResponse`
//         `{ items: WalletByBucket[], crossBucketTotal }` where each
//         `WalletByBucket` carries `bucketId / name / enabled / userId /
//         balancesByType{} / bucketTotal`.
//       * `GET .../points/transactions` → paged response where each row
//         carries `bucketId` and the query supports a `?bucketId=<uuid>`
//         filter.
//       * admin cross-tenant wallets view returns `WalletByBucket[]` with
//         `userId` and rows expanded per `(user, bucket)`.
//   - "查询" testable behaviors.
//   - User Stories US-CB-005 (查看按 Bucket 分组的积分余额) and
//     US-CB-006 (查看 Bucket 维度的交易历史).
//
// KNOWN CONTRACT GAPS (from handoff; flagged in this item's
// `open_questions` and encoded as intended contract — tests do NOT hardcode
// the broken current shapes):
//   (a) Wallet grouping + txn bucket filter are handler-side; txn paged `total`
//       reflects the PRE-filter page (server-side `TransactionFilters.bucket_id`
//       not yet wired). These tests therefore only assert that the RETURNED
//       `items` respect the bucket filter, NOT that `total` reflects the
//       filtered count.
//   (b) `WalletByBucket.name` / `enabled` currently serialize as `null`
//       (bucket directory not yet wired into the grouping). Tests assert the
//       fields are PRESENT (intended contract) but tolerate `null`.
//   (c) There is NO distinct `/users/me/points/wallets` or
//       `/billing/points/wallets` route — a single
//       `GET /api/points/{realmId}/wallets` handler (gated on `points.view`)
//       that groups by `(bucket_id, user_id)`. The user-facing and admin
//       scenarios both exercise the real route; the admin scenario additionally
//       seeds a second user's wallet to prove the `(user, bucket)` per-row
//       expansion + `userId` field.
//
// All scenarios drive the real production HTTP path through the unified test
// router. Direct-DB seed helpers materialize bucket/wallet/transaction rows.
//
// =============================================================================

#![allow(clippy::too_many_arguments)]

use crate::tests::helpers::billing_helpers::setup_billing_admin_session_with_user;
use crate::tests::helpers::credit_bucket_helpers::{
    CreditBucketOpts, admin_grant_to_bucket, auth_user_get_via_api, create_test_credit_bucket,
    grant_points_to_bucket, seed_transaction_on_bucket,
};
use crate::tests::helpers::{create_admin_session_with_user, grant_points_view_role};
use crate::tests::scenarios::points::fixtures::create_test_user;
use crate::tests::schema_test_context::SchemaTestContext as TestContext;
use axum::http::StatusCode;
use serde_json::Value;
use test_context::test_context;
use uuid::Uuid;

// =============================================================================
// Local helpers
// =============================================================================

/// Extract the `items` array from a `ListWalletsByBucketResponse` body.
fn items_array(body: &Option<Value>) -> Option<&Vec<Value>> {
    body.as_ref()
        .and_then(|v| v.get("items"))
        .and_then(|i| i.as_array())
}

/// Extract the `crossBucketTotal` integer field from a
/// `ListWalletsByBucketResponse` body.
fn cross_bucket_total(body: &Option<Value>) -> Option<i64> {
    body.as_ref()
        .and_then(|v| v.get("crossBucketTotal"))
        .and_then(|n| n.as_i64())
}

/// Find a wallet row by its `bucketId` (compared as a UUID string).
fn find_row_by_bucket<'a>(items: &'a [Value], bucket_id: &str) -> Option<&'a Value> {
    items.iter().find(|r| {
        r.get("bucketId")
            .and_then(|v| v.as_str())
            .map(|s| s == bucket_id)
            .unwrap_or(false)
    })
}

// =============================================================================
// Scenario 1: user wallets grouped by bucket with cross-bucket total
// =============================================================================

/// User Story: US-CB-005 (查看按 Bucket 分组的积分余额).
/// Covers:
///   - `GET .../points/wallets` → `WalletByBucket[]` grouped by
///     `(bucket_id, user_id)` with one row per bucket.
///   - Each row exposes `bucketId`, `balancesByType{}`, and `bucketTotal`.
///   - Top-level `crossBucketTotal` equals the sum of every row's `bucketTotal`.
#[test_context(TestContext)]
#[tokio::test]
async fn user_wallets_grouped_by_bucket_with_cross_bucket_total(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let pool = ctx.app_state.pool.clone();

    // Create an admin-privileged user (the points query handlers are gated on
    // `points.view`; the realm-admin role carries that permission).
    let (token, user_id) =
        setup_billing_admin_session_with_user(ctx, "cb_t05_wallets_multi@example.com").await;

    // Two buckets, each granted a distinct amount to a distinct credit type so
    // per-row `bucketTotal` differs and the cross-bucket sum is non-trivial.
    let bucket_a = create_test_credit_bucket(
        &pool,
        &realm_id,
        CreditBucketOpts {
            name: Some("Bucket A".into()),
            bucket_key: Some(format!("bucket-a-{}", Uuid::now_v7())),
            ..Default::default()
        },
    )
    .await;
    let bucket_b = create_test_credit_bucket(
        &pool,
        &realm_id,
        CreditBucketOpts {
            name: Some("Bucket B".into()),
            bucket_key: Some(format!("bucket-b-{}", Uuid::now_v7())),
            ..Default::default()
        },
    )
    .await;

    let amount_a: i64 = 100;
    let amount_b: i64 = 250;
    admin_grant_to_bucket(ctx, &realm_id, user_id, bucket_a, amount_a, None).await;
    admin_grant_to_bucket(ctx, &realm_id, user_id, bucket_b, amount_b, None).await;

    let (status, body) = auth_user_get_via_api(
        ctx,
        &format!("/api/points/{}/wallets", realm_id),
        "",
        &token,
    )
    .await;

    assert_eq!(
        status,
        StatusCode::OK,
        "wallets grouped: {:?} body={:?}",
        status,
        body
    );

    let items = items_array(&body).expect("wallets response must contain items[]");
    assert!(
        !items.is_empty(),
        "seeded two buckets — items[] must not be empty: {:?}",
        body
    );

    let row_a = find_row_by_bucket(items, &bucket_a.to_string())
        .expect("wallets items[] must contain bucket A row");
    let row_b = find_row_by_bucket(items, &bucket_b.to_string())
        .expect("wallets items[] must contain bucket B row");

    // Intended contract: each row exposes bucketId, balancesByType,
    // and bucketTotal. `name`/`enabled` are part of the intended contract but
    // may currently be null (gap b) — assert presence only.
    for row in [row_a, row_b] {
        assert!(
            row.get("bucketId").is_some(),
            "row missing bucketId: {:?}",
            row
        );
        assert!(
            row.get("balancesByType").is_some(),
            "row missing balancesByType: {:?}",
            row
        );
        assert!(
            row.get("bucketTotal").is_some(),
            "row missing bucketTotal: {:?}",
            row
        );
        assert!(
            row.get("userId").is_some(),
            "row missing userId (WalletByBucket): {:?}",
            row
        );
        // name / enabled are intended-contract fields; tolerate null per gap (b).
        assert!(
            row.get("name").is_some(),
            "row missing `name` field (intended contract): {:?}",
            row
        );
        assert!(
            row.get("enabled").is_some(),
            "row missing `enabled` field (intended contract): {:?}",
            row
        );
    }

    let total_a = row_a
        .get("bucketTotal")
        .and_then(|v| v.as_i64())
        .expect("bucket A bucketTotal must be an integer");
    let total_b = row_b
        .get("bucketTotal")
        .and_then(|v| v.as_i64())
        .expect("bucket B bucketTotal must be an integer");
    assert_eq!(
        total_a, amount_a,
        "bucket A bucketTotal must equal its grant"
    );
    assert_eq!(
        total_b, amount_b,
        "bucket B bucketTotal must equal its grant"
    );

    let cross_total = cross_bucket_total(&body).expect("response must carry crossBucketTotal");
    assert_eq!(
        cross_total,
        amount_a + amount_b,
        "crossBucketTotal must equal the sum of per-bucket totals: body={:?}",
        body
    );
}

// =============================================================================
// Scenario 2: single bucket — crossBucketTotal still present, equals its total
// =============================================================================

/// User Story: US-CB-005 (查看按 Bucket 分组的积分余额 — single-bucket case).
/// Covers:
///   - `ListWalletsByBucketResponse.crossBucketTotal` is ALWAYS
///     present. With a single bucket, it equals that bucket's `bucketTotal`
///     (no degradation of the response shape).
#[test_context(TestContext)]
#[tokio::test]
async fn user_wallets_single_bucket_no_cross_bucket_total_degradation(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let pool = ctx.app_state.pool.clone();
    let (token, user_id) =
        setup_billing_admin_session_with_user(ctx, "cb_t05_wallets_single@example.com").await;

    let bucket = create_test_credit_bucket(
        &pool,
        &realm_id,
        CreditBucketOpts {
            name: Some("Only Bucket".into()),
            bucket_key: Some(format!("only-bucket-{}", Uuid::now_v7())),
            ..Default::default()
        },
    )
    .await;

    let amount: i64 = 80;
    admin_grant_to_bucket(ctx, &realm_id, user_id, bucket, amount, None).await;

    let (status, body) = auth_user_get_via_api(
        ctx,
        &format!("/api/points/{}/wallets", realm_id),
        "",
        &token,
    )
    .await;

    assert_eq!(
        status,
        StatusCode::OK,
        "wallets single: {:?} body={:?}",
        status,
        body
    );

    // The crossBucketTotal field MUST be present even with a single bucket
    // (the response contract is unchanged).
    let cross_total = cross_bucket_total(&body).expect("crossBucketTotal must always be present");
    let items = items_array(&body).expect("wallets response must contain items[]");

    let row = find_row_by_bucket(items, &bucket.to_string())
        .expect("single bucket must appear in items[]");
    let bucket_total = row
        .get("bucketTotal")
        .and_then(|v| v.as_i64())
        .expect("bucketTotal must be an integer");
    assert_eq!(
        bucket_total, amount,
        "single bucket total must equal its grant"
    );
    assert_eq!(
        cross_total, bucket_total,
        "single-bucket crossBucketTotal must equal that bucket's total: body={:?}",
        body
    );
}

// =============================================================================
// Scenario 3: no buckets — empty array + crossBucketTotal 0
// =============================================================================

/// User Story: US-CB-005 (查看按 Bucket 分组的积分余额 — empty case).
/// Covers:
///   - empty case: a user holding no pool returns an empty
///     `items[]` array and `crossBucketTotal = 0` (never null, never 404).
#[test_context(TestContext)]
#[tokio::test]
async fn user_wallets_empty_when_no_buckets(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let (token, _user_id) =
        setup_billing_admin_session_with_user(ctx, "cb_t05_wallets_empty@example.com").await;

    let (status, body) = auth_user_get_via_api(
        ctx,
        &format!("/api/points/{}/wallets", realm_id),
        "",
        &token,
    )
    .await;

    assert_eq!(
        status,
        StatusCode::OK,
        "wallets empty: {:?} body={:?}",
        status,
        body
    );

    let items = items_array(&body).expect("wallets response must contain items[] (possibly empty)");
    assert!(
        items.is_empty(),
        "user with no buckets must get an empty items[]: {:?}",
        body
    );
    let cross_total = cross_bucket_total(&body).expect("crossBucketTotal must always be present");
    assert_eq!(
        cross_total, 0,
        "empty wallets crossBucketTotal must be 0: body={:?}",
        body
    );
}

// =============================================================================
// Scenario 4: transactions carry bucketId matching their pool
// =============================================================================

/// User Story: US-CB-006 (查看 Bucket 维度的交易历史 — bucketId field).
/// Covers:
///   - each transaction row carries `bucketId` matching the pool
///     the transaction landed in.
#[test_context(TestContext)]
#[tokio::test]
async fn user_transactions_include_bucket_id_field(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let pool = ctx.app_state.pool.clone();
    let (token, user_id) =
        setup_billing_admin_session_with_user(ctx, "cb_t05_txn_field@example.com").await;

    let bucket = create_test_credit_bucket(
        &pool,
        &realm_id,
        CreditBucketOpts {
            name: Some("Txn Bucket".into()),
            bucket_key: Some(format!("txn-bucket-{}", Uuid::now_v7())),
            ..Default::default()
        },
    )
    .await;

    // Seed a deterministic grant transaction in this bucket via the real grant
    // path so the row carries the correct bucket_id.
    grant_points_to_bucket(
        ctx,
        &realm_id,
        user_id,
        bucket,
        herald_core::domain::points::entities::CreditType::GrantedCredit,
        herald_core::domain::points::entities::CreditSourceType::SystemGrant,
        50,
        None,
        Some(format!("txn-field-{}", Uuid::now_v7())),
        Some("txn field seed".into()),
        None,
    )
    .await
    .expect("grant_points_to_bucket for txn field scenario");

    let (status, body) = auth_user_get_via_api(
        ctx,
        &format!("/api/points/{}/transactions", realm_id),
        &format!("userId={}", user_id),
        &token,
    )
    .await;

    assert_eq!(
        status,
        StatusCode::OK,
        "txn list: {:?} body={:?}",
        status,
        body
    );

    let items = body
        .as_ref()
        .and_then(|v| v.get("items"))
        .and_then(|i| i.as_array())
        .expect("transactions response must contain items[]");
    assert!(
        !items.is_empty(),
        "seeded one transaction — items[] must not be empty: {:?}",
        body
    );

    // Each row MUST carry a bucketId field; the seeded transaction's row MUST
    // report the bucket it landed in.
    for row in items {
        assert!(
            row.get("bucketId").is_some(),
            "transaction row missing bucketId: {:?}",
            row
        );
    }
    let ours = items
        .iter()
        .find(|r| {
            r.get("bucketId")
                .and_then(|v| v.as_str())
                .map(|s| s == bucket.to_string())
                .unwrap_or(false)
        })
        .expect("seeded transaction must surface with its bucket's bucketId");
    assert_eq!(
        ours.get("bucketId").and_then(|v| v.as_str()),
        Some(bucket.to_string()).as_deref(),
        "seeded txn bucketId must match its pool: {:?}",
        ours
    );
}

// =============================================================================
// Scenario 5: transactions filtered by ?bucketId=<uuid>
// =============================================================================

/// User Story: US-CB-006 (查看 Bucket 维度的交易历史 — bucket filter).
/// Covers:
///   - `?bucketId=<uuid>` filter returns only that bucket's
///     transactions.
///
/// Note (gap a): the bucket filter is applied at the handler on the
/// returned page, so the paged `total` may reflect the PRE-filter page. This
/// test only asserts that every returned `items[]` row belongs to the filtered
/// bucket — it does NOT assert `total` reflects the filtered count.
#[test_context(TestContext)]
#[tokio::test]
async fn user_transactions_filtered_by_bucket_id(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let pool = ctx.app_state.pool.clone();
    let (token, user_id) =
        setup_billing_admin_session_with_user(ctx, "cb_t05_txn_filter@example.com").await;

    let bucket_keep = create_test_credit_bucket(
        &pool,
        &realm_id,
        CreditBucketOpts {
            name: Some("Keep Bucket".into()),
            bucket_key: Some(format!("keep-bucket-{}", Uuid::now_v7())),
            ..Default::default()
        },
    )
    .await;
    let bucket_drop = create_test_credit_bucket(
        &pool,
        &realm_id,
        CreditBucketOpts {
            name: Some("Drop Bucket".into()),
            bucket_key: Some(format!("drop-bucket-{}", Uuid::now_v7())),
            ..Default::default()
        },
    )
    .await;

    // Seed one transaction in each bucket directly so the filter has
    // deterministic rows that differ ONLY by bucket_id.
    seed_transaction_on_bucket(
        &pool,
        &realm_id,
        user_id,
        bucket_keep,
        "grant",
        30,
        chrono::Utc::now(),
    )
    .await;
    seed_transaction_on_bucket(
        &pool,
        &realm_id,
        user_id,
        bucket_drop,
        "grant",
        40,
        chrono::Utc::now(),
    )
    .await;

    let query = format!("userId={}&bucketId={}", user_id, bucket_keep);
    let (status, body) = auth_user_get_via_api(
        ctx,
        &format!("/api/points/{}/transactions", realm_id),
        &query,
        &token,
    )
    .await;

    assert_eq!(
        status,
        StatusCode::OK,
        "txn filtered: {:?} body={:?}",
        status,
        body
    );

    let items = body
        .as_ref()
        .and_then(|v| v.get("items"))
        .and_then(|i| i.as_array())
        .expect("filtered transactions response must contain items[]");

    // Every returned row MUST belong to bucket_keep.
    for row in items {
        let row_bucket = row
            .get("bucketId")
            .and_then(|v| v.as_str())
            .unwrap_or("<missing>");
        assert_eq!(
            row_bucket,
            bucket_keep.to_string(),
            "filtered txn row must belong to bucket_keep, got {}: {:?}",
            row_bucket,
            row
        );
    }

    // The keep-bucket row MUST be present (filter did not over-exclude).
    assert!(
        items
            .iter()
            .any(|r| r.get("bucketId").and_then(|v| v.as_str())
                == Some(bucket_keep.to_string()).as_deref()),
        "filtered items[] must contain the keep-bucket transaction: {:?}",
        body
    );
    // The drop-bucket row MUST NOT appear.
    assert!(
        !items
            .iter()
            .any(|r| r.get("bucketId").and_then(|v| v.as_str())
                == Some(bucket_drop.to_string()).as_deref()),
        "filtered items[] must NOT contain the drop-bucket transaction: {:?}",
        body
    );
}

// =============================================================================
// Scenario 6: admin cross-tenant wallets — per (user, bucket) rows + userId
// =============================================================================

/// User Story: US-CB-005 (管理员视角 — 跨租户 WalletByBucket with userId).
/// Covers:
///   - admin wallets view returns `WalletByBucket[]` with
///     `userId`, rows expanded per `(user, bucket)` — i.e. two users in the
///     same bucket produce TWO rows, each tagged with its `userId`.
#[test_context(TestContext)]
#[tokio::test]
async fn admin_wallets_cross_tenant_with_user_id(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let pool = ctx.app_state.pool.clone();

    // Admin viewer.
    let (admin_token, admin_id) =
        setup_billing_admin_session_with_user(ctx, "cb_t05_admin_view@example.com").await;

    // Two non-admin users that own wallets in the SAME bucket.
    let user_a = create_test_user(&pool, &realm_id, "cb_t05_user_a@example.com").await;
    let user_b = create_test_user(&pool, &realm_id, "cb_t05_user_b@example.com").await;
    // Grant admin viewer the points.view permission is already covered by the
    // realm-admin role (setup_billing_admin_session_with_user grants it).

    let shared_bucket = create_test_credit_bucket(
        &pool,
        &realm_id,
        CreditBucketOpts {
            name: Some("Shared Bucket".into()),
            bucket_key: Some(format!("shared-bucket-{}", Uuid::now_v7())),
            ..Default::default()
        },
    )
    .await;

    // Grant each user a distinct amount in the same bucket via the real grant
    // path. (admin_grant_to_bucket requires a TestContext and routes through
    // PointsService; the user ids are real account rows so grants succeed.)
    admin_grant_to_bucket(ctx, &realm_id, user_a, shared_bucket, 60, None).await;
    admin_grant_to_bucket(ctx, &realm_id, user_b, shared_bucket, 90, None).await;

    let (status, body) = auth_user_get_via_api(
        ctx,
        &format!("/api/points/{}/wallets", realm_id),
        "",
        &admin_token,
    )
    .await;

    assert_eq!(
        status,
        StatusCode::OK,
        "admin wallets: {:?} body={:?}",
        status,
        body
    );

    let items = items_array(&body).expect("admin wallets response must contain items[]");

    // Intended contract: admin view expands per (user, bucket). With two users
    // in one bucket there must be two rows tagged with distinct userIds.
    let bucket_str = shared_bucket.to_string();
    let bucket_rows: Vec<&Value> = items
        .iter()
        .filter(|r| r.get("bucketId").and_then(|v| v.as_str()) == Some(bucket_str.as_str()))
        .collect();

    assert!(
        bucket_rows.len() >= 2,
        "admin view must expand per (user, bucket) — expected ≥2 rows for shared bucket, got {}: {:?}",
        bucket_rows.len(),
        body
    );

    // Each row MUST carry userId; the two seeded users MUST each appear.
    let user_ids: Vec<String> = bucket_rows
        .iter()
        .filter_map(|r| r.get("userId").and_then(|v| v.as_str()).map(String::from))
        .collect();
    assert!(
        user_ids.contains(&user_a.to_string()),
        "admin view must include user_a's row for shared bucket: {:?}",
        body
    );
    assert!(
        user_ids.contains(&user_b.to_string()),
        "admin view must include user_b's row for shared bucket: {:?}",
        body
    );

    // The admin viewer's own grant-less wallet must NOT pollute the count for
    // the shared bucket (no admin_id row expected — admin never granted into
    // the shared bucket here).
    assert!(
        !user_ids.contains(&admin_id.to_string()),
        "admin viewer without a shared-bucket wallet must not appear: {:?}",
        body
    );

    // Per-row bucketTotal must match each user's grant (not be coalesced).
    let row_a = bucket_rows
        .iter()
        .find(|r| r.get("userId").and_then(|v| v.as_str()) == Some(user_a.to_string()).as_deref())
        .expect("user_a row must be present");
    let row_b = bucket_rows
        .iter()
        .find(|r| r.get("userId").and_then(|v| v.as_str()) == Some(user_b.to_string()).as_deref())
        .expect("user_b row must be present");
    assert_eq!(
        row_a.get("bucketTotal").and_then(|v| v.as_i64()),
        Some(60),
        "user_a bucketTotal must equal her grant: {:?}",
        row_a
    );
    assert_eq!(
        row_b.get("bucketTotal").and_then(|v| v.as_i64()),
        Some(90),
        "user_b bucketTotal must equal her grant: {:?}",
        row_b
    );
}

// =============================================================================
// Scenario 5 (Gap #2 regression): points.view-only user sees ONLY own wallets
// =============================================================================
//
// Existing user_wallets_* scenarios all authenticate via realm-admin (which
// carries points.manage), so they never exercised the points.view-ONLY path —
// which is exactly why the original Gap #2 (service gated on can_manage_points)
// 403'd real end-users undetected. This scenario closes that blind spot by
// exercising the self-service `GET /api/user/wallets` endpoint (the admin
// `GET /api/points/{realmId}/wallets` route is manage-gated):
//   - A points.view-only (non-admin) caller MUST get 200 (view-gated, not
//     manage-gated) and MUST see only its own wallet rows.
//   - `?search=<other-user>` MUST NOT leak the other user's rows
//     (`UserWalletsQuery` has no `search` field, so the parameter is ignored
//     and `user_id` is hard-scoped to the caller by the handler).
//   - `crossBucketTotal` is the caller's OWN sum, not the realm-wide total.
// Contrast with `admin_wallets_cross_tenant_with_user_id` (points.manage sees
// every user's rows).
//
/// User Story: US-CB-005 — a normal end-user viewing their own bucketed balance.
#[test_context(TestContext)]
#[tokio::test]
async fn points_view_only_user_sees_only_own_wallets(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let pool = ctx.app_state.pool.clone();

    // A second user whose wallet MUST NOT leak to the viewer.
    let other_user = create_test_user(&pool, &realm_id, "cb_t05_view_only_other@example.com").await;

    // The viewer: clean user + session, granted ONLY points.view (no manage).
    let (viewer_token, viewer_id_str) =
        create_admin_session_with_user(ctx, "cb_t05_view_only@example.com", 1800).await;
    let viewer_id = Uuid::parse_str(&viewer_id_str).expect("viewer user_id must parse as UUID");
    grant_points_view_role(ctx, &viewer_id_str).await;

    // One shared bucket; both users hold a wallet in it with distinct amounts.
    let shared_bucket = create_test_credit_bucket(
        &pool,
        &realm_id,
        CreditBucketOpts {
            name: Some("Shared Bucket (view-only)".into()),
            bucket_key: Some(format!("shared-vo-{}", Uuid::now_v7())),
            ..Default::default()
        },
    )
    .await;

    let viewer_amount: i64 = 50;
    let other_amount: i64 = 777; // sentinel: must NEVER appear in the viewer's response
    admin_grant_to_bucket(
        ctx,
        &realm_id,
        viewer_id,
        shared_bucket,
        viewer_amount,
        None,
    )
    .await;
    admin_grant_to_bucket(
        ctx,
        &realm_id,
        other_user,
        shared_bucket,
        other_amount,
        None,
    )
    .await;

    // --- (1) Baseline: viewer lists wallets → 200 + own-only rows. ----------
    let (status, body) = auth_user_get_via_api(ctx, "/api/user/wallets", "", &viewer_token).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "points.view-only user must list wallets (Gap #2): status={:?} body={:?}",
        status,
        body
    );

    let items = items_array(&body).expect("viewer wallets response must contain items[]");

    // DATA ISOLATION (the core assertion): every row belongs to the viewer,
    // never the other user.
    for row in items {
        let row_user = row
            .get("userId")
            .and_then(|v| v.as_str())
            .expect("row must carry userId");
        assert_eq!(
            row_user,
            viewer_id_str.as_str(),
            "DATA ISOLATION (Gap #2): points.view-only user must never see another \
             user's wallet row: row={:?}",
            row
        );
    }

    // The viewer's own bucket total is intact; crossBucketTotal is the viewer's
    // OWN sum (self-scoped), NOT the realm-wide total (which would include other).
    let viewer_row = find_row_by_bucket(items, &shared_bucket.to_string())
        .expect("viewer must see its own shared-bucket row");
    assert_eq!(
        viewer_row.get("bucketTotal").and_then(|v| v.as_i64()),
        Some(viewer_amount),
        "viewer's own bucketTotal must equal its grant: row={:?}",
        viewer_row
    );
    assert_eq!(
        cross_bucket_total(&body),
        Some(viewer_amount),
        "self-scoped crossBucketTotal must be the viewer's own sum ({}), not the \
         realm-wide sum ({}): body={:?}",
        viewer_amount,
        viewer_amount + other_amount,
        body
    );

    // --- (2) Negative: ?search=<other-user-uuid> must NOT leak other_user. --
    // `UserWalletsQuery` has no `search` field, so the unknown parameter is
    // ignored and this call is identical to (1): own-only rows, viewer's own total.
    let (status2, body2) = auth_user_get_via_api(
        ctx,
        "/api/user/wallets",
        &format!("search={}", other_user),
        &viewer_token,
    )
    .await;
    assert_eq!(
        status2,
        StatusCode::OK,
        "search-of-other-user must still 200 (search stripped for non-managers): body={:?}",
        body2
    );
    let items2 = items_array(&body2).expect("search response must contain items[]");
    for row in items2 {
        let row_user = row
            .get("userId")
            .and_then(|v| v.as_str())
            .expect("row must carry userId");
        assert_eq!(
            row_user,
            viewer_id_str.as_str(),
            "DATA ISOLATION: search=<other-user> must not leak other user's row: row={:?}",
            row
        );
    }
    assert_eq!(
        cross_bucket_total(&body2),
        Some(viewer_amount),
        "search=<other-user> must yield the same self-scoped total (search ignored): body={:?}",
        body2
    );
}

/// User Story: US-MWGR-004
/// Source: `.ai/user-stories/billing/multi-wallet-grant-rules.md`
/// Covers: Mapping/Realm rule references, disabled and empty reference states,
/// and non-leakage from both the user response and its OpenAPI schema.
#[test_context(TestContext)]
#[tokio::test]
async fn test_multi_wallet_grant_rule_bucket_references_and_user_non_leak(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let bucket = crate::tests::scenarios::points::multi_wallet_grant_rule_scenarios::seed_bucket(
        ctx, &realm_id, true,
    )
    .await;
    let mapping = crate::tests::scenarios::points::multi_wallet_grant_rule_scenarios::seed_mapping(
        ctx, &realm_id, "one_time",
    )
    .await;
    let mapping_rule =
        crate::tests::scenarios::points::multi_wallet_grant_rule_scenarios::seed_rule(
            ctx,
            &realm_id,
            Some(mapping),
            bucket,
            &["topup"],
            "fixed",
            Some(10),
            false,
            0,
        )
        .await;
    let registration_rule =
        crate::tests::scenarios::points::multi_wallet_grant_rule_scenarios::seed_rule(
            ctx,
            &realm_id,
            None,
            bucket,
            &["registration"],
            "fixed",
            Some(5),
            true,
            1,
        )
        .await;

    let token = crate::tests::helpers::billing_helpers::setup_billing_admin_session(
        ctx,
        "multi-wallet-bucket-reference@test.com",
    )
    .await;
    let (status, body) = crate::tests::helpers::credit_bucket_helpers::auth_admin_request_via_api(
        ctx,
        "GET",
        &format!("/api/realms/{realm_id}/billing/credit-buckets/{bucket}"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let refs = body
        .as_ref()
        .and_then(|b| b.get("ruleReferences"))
        .and_then(Value::as_array)
        .expect("management detail carries ruleReferences");
    assert_eq!(refs.len(), 2);
    assert!(refs.iter().any(|r| {
        r["ruleId"] == mapping_rule.to_string()
            && r["ownerType"] == "entitlement_mapping"
            && r["enabled"] == false
    }));
    assert!(refs.iter().any(|r| {
        r["ruleId"] == registration_rule.to_string()
            && r["ownerType"] == "realm_registration"
            && r["enabled"] == true
    }));

    let empty_bucket =
        crate::tests::scenarios::points::multi_wallet_grant_rule_scenarios::seed_bucket(
            ctx, &realm_id, true,
        )
        .await;
    let (status, empty) = crate::tests::helpers::credit_bucket_helpers::auth_admin_request_via_api(
        ctx,
        "GET",
        &format!("/api/realms/{realm_id}/billing/credit-buckets/{empty_bucket}"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        empty.as_ref().unwrap()["ruleReferences"]
            .as_array()
            .unwrap()
            .len(),
        0
    );

    let (user_token, user_id) =
        create_admin_session_with_user(ctx, "multi-wallet-reference-user@test.com", 1800).await;
    grant_points_view_role(ctx, &user_id).await;
    admin_grant_to_bucket(
        ctx,
        &realm_id,
        Uuid::parse_str(&user_id).expect("user id"),
        bucket,
        7,
        None,
    )
    .await;
    let (status, user_wallets) =
        auth_user_get_via_api(ctx, "/api/user/wallets", "", &user_token).await;
    assert_eq!(status, StatusCode::OK);
    let user_items = items_array(&user_wallets).expect("user wallet response carries items");
    let user_bucket = find_row_by_bucket(user_items, &bucket.to_string())
        .expect("user wallet response contains granted bucket");
    assert!(
        user_bucket.get("ruleReferences").is_none(),
        "ordinary user response must not leak ruleReferences"
    );

    let spec: Value =
        serde_json::to_value(crate::application::http::server::build_openapi_spec()).unwrap();
    let wallet_schema = &spec["components"]["schemas"]["WalletByBucket"]["properties"];
    assert!(
        wallet_schema.get("ruleReferences").is_none(),
        "ordinary wallet schema must not leak ruleReferences"
    );
}
