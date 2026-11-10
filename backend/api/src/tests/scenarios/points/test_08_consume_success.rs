// Points System Scenario Test 8: Consume Success
//
// **User Story**: US-PU-01 (balance update), SDK consumption
// **Priority**: P0
//
// **Scenario**: Third Party Consumes Points Successfully
//
// **Given**:
// - A valid API Key for a client app
// - A user with points account balance 5000
// - The API Key belongs to realm matching user's realm
//
// **When**:
// - The third party calls `POST /api/ext/points/{realmId}/consume` with:
//   - userId: valid user UUID
//   - clientAppId: valid client app UUID
//   - amount: 100
//   - description: "AI API call"
//
// **Then**:
// - Response contains transactionId
// - Response contains amount: -100
// - Response contains balanceAfter: 4900
// - HTTP status is 200 OK
// - Account balance is updated to 4900
// - Total consumed is increased by 100
// - A transaction record is created
//

use crate::tests::helpers::points_helpers::{
    assert_derived_balance, create_credit_ledger_entry_with_effective_at,
};
use crate::tests::scenarios::points::fixtures::*;
use crate::tests::schema_test_context::SchemaTestContext as TestContext;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use herald_core::domain::points::entities::{CreditSourceType, CreditType};
use serde_json::json;
use test_context::test_context;
use tower::ServiceExt;
use uuid::Uuid;

/// Scenario 2.1: Third Party Consumes Points Successfully
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_consume_points_success(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    // Given: A valid API Key, user with points account
    println!("[Step 1] Create test user and API Key");

    let user_id = create_test_user(&ctx._app_state.pool, &ctx._realm_id, "user8@example.com").await;
    let initial_balance = 5000;

    let wallet_id = create_test_points_wallet(&ctx._app_state.pool, user_id, initial_balance).await;

    let client_app_id = create_test_client_app(&ctx._app_state.pool, &ctx._realm_id).await;
    let api_key = create_test_api_key(&ctx._app_state.pool, &ctx._realm_id, client_app_id).await;

    let consume_amount = 100;
    let description = "AI API call";

    println!(
        "[Step 1] ✓ Test data created: user={}, account={}, client_app={}, api_key={}",
        user_id, wallet_id, client_app_id, api_key
    );

    // When: Third party consumes points
    println!("[Step 2] Third party consumes points");

    let consume_payload = json!({
        "userId": user_id.to_string(),
        "clientAppId": client_app_id.to_string(),
        "amount": consume_amount,
        "description": description.to_string()
    });

    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/ext/points/{}/consume", ctx._realm_id))
        .header("content-type", "application/json")
        .header("X-API-Key", api_key)
        .body(Body::from(consume_payload.to_string()))
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();

    // Then: Verify consumption response and database state
    println!("[Step 3] Verify consumption response");

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "Consumption should succeed with 200 OK"
    );

    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("Failed to read response body");
    let body: serde_json::Value =
        serde_json::from_slice(&body_bytes).expect("Failed to parse JSON");

    assert!(
        body["transactions"].is_array(),
        "Response should contain a transactions array (multi-bucket consume)"
    );
    let transactions = body["transactions"].as_array().unwrap();
    assert_eq!(
        transactions.len(),
        1,
        "Single-pool consume produces exactly one per-bucket transaction"
    );
    let txn = &transactions[0];
    assert!(
        txn["transactionId"].is_string(),
        "Per-bucket transaction should contain transactionId"
    );
    assert_eq!(
        body["amount"].as_i64(),
        Some(consume_amount),
        "Response amount should be the total consumed (100)"
    );
    assert_eq!(
        txn["amount"].as_i64(),
        Some(consume_amount),
        "Per-bucket transaction amount should be 100 (deduction magnitude)"
    );
    assert_eq!(
        txn["balanceAfter"].as_i64(),
        Some(initial_balance - consume_amount),
        "Per-bucket transaction balanceAfter should be 4900"
    );

    let expected_balance_after = initial_balance - consume_amount;
    println!(
        "[Step 3] ✓ Response verified: transactionId={}, amount={}, balanceAfter={}",
        txn["transactionId"], txn["amount"], txn["balanceAfter"]
    );

    // Verify database state
    println!("[Step 4] Verify database state");

    // point-time: `points_wallets.total_balance` was dropped; the
    // available balance is derived from `points_credit_ledger` using the same
    // predicate as consumption.
    let (new_balance, total_consumed): (i64, i64) = sqlx::query_as(
        "SELECT COALESCE(SUM(l.remaining_amount) FILTER (
                    WHERE l.status = 'active' AND l.remaining_amount > 0
                      AND (l.effective_at IS NULL OR l.effective_at <= NOW())
                      AND (l.expires_at  IS NULL OR l.expires_at  >  NOW())
                ), 0)::BIGINT AS new_balance,
                w.total_consumed AS total_consumed
         FROM points_wallets w
         LEFT JOIN points_credit_ledger l
           ON l.realm_id = w.realm_id AND l.user_id = w.user_id AND l.bucket_id = w.bucket_id
         WHERE w.id = $1
         GROUP BY w.id, w.total_consumed",
    )
    .bind(wallet_id)
    .fetch_one(&ctx._app_state.pool)
    .await
    .expect("Failed to fetch account");

    assert_eq!(
        new_balance, expected_balance_after,
        "Account balance should be updated to 4900"
    );
    assert_eq!(
        total_consumed, consume_amount,
        "Total consumed should be increased by 100"
    );

    println!(
        "[Step 4] ✓ Database verified: balance={}, total_consumed={}",
        new_balance, total_consumed
    );

    // Verify transaction was created
    println!("[Step 5] Verify transaction record");

    let (txn_type, txn_amount, txn_balance_after, txn_description): (
        String,
        i64,
        i64,
        Option<String>,
    ) = sqlx::query_as(
        "SELECT type, amount, balance_after, description
             FROM points_transactions
             WHERE user_id = $1 AND type = 'consume'
             ORDER BY created_at DESC
             LIMIT 1",
    )
    .bind(user_id)
    .fetch_one(&ctx._app_state.pool)
    .await
    .expect("Failed to fetch transaction");

    assert_eq!(txn_type, "consume", "Transaction type should be consume");
    assert_eq!(
        txn_amount, -consume_amount,
        "Transaction amount should be -100"
    );
    assert_eq!(
        txn_balance_after, expected_balance_after,
        "Transaction balance_after should be 4900"
    );
    // Verify transaction description
    assert!(
        txn_description.is_some(),
        "Transaction description should exist"
    );
    assert_eq!(
        txn_description.unwrap(),
        description,
        "Transaction description should match"
    );

    println!(
        "[Step 5] ✓ Transaction verified: type={}, amount={}",
        txn_type, txn_amount
    );

    // Step 6 — point-time zero-regression assertion (P0 / P0).
    //
    // The seed (`create_test_points_wallet`) wrote a `subscription_credit`
    // ledger row with `granted_amount = initial_balance = 5000` and
    // `effective_at = NULL` (immediately available). The consume above
    // decremented 100. Under point-time the available balance is a derived
    // SUM over ledger rows gated by
    //   status='active' AND remaining_amount>0
    //     AND (effective_at IS NULL OR effective_at <= NOW())
    //     AND (expires_at  IS NULL OR expires_at  >  NOW())
    // — for the all-NULL/active case this MUST equal the value the old
    // Stored-balance口径 would have held (5000 − 100 = 4900). If the derived
    // predicate diverged from the legacy Stored口径 for effective_at=NULL
    // rows, this assertion fails — locking out the P0 regression risk.
    // (We MUST NOT `SELECT subscription_balance FROM points_wallets`:
    // That column was physically removed — see
    // `test_wallet_balance_columns_dropped` below.)
    println!("[Step 6] point-time zero-regression: assert derived balance == legacy seed baseline");

    assert_derived_balance(
        ctx,
        user_id,
        &ctx._realm_id,
        CreditType::SubscriptionCredit,
        initial_balance - consume_amount,
    )
    .await;

    println!("[Step 6] ✓ Derived balance = 4900 (matches legacy Stored口径 for effective_at=NULL)");

    println!("\n✅ Scenario 2.1 完成：第三方成功消耗积分");
}

// point-time: physical removal of wallet balance columns (P0)
//
// User Story: — (developer / contract invariants; not user-facing).
//
// Covers P0 "Stored balance 列物理删除 + balance_after 真实快照"
//   + risk "`points_wallets` Stored balance 列物理删除：必须同
// 一编译单元原子完成".
//
// Why this test exists: the migration dropped 6 columns (`topup_balance`,
// `subscription_balance`, `granted_balance`, `registration_balance`,
// `free_periodic_balance`, `total_balance`) from the base DDL of
// `points_wallets` (project not yet in production → destructive in-place,
// no DROP COLUMN delta). Available balance is exclusively the derived SUM
// over `points_credit_ledger` now. If any of those columns ever re-appears
// (or any of the 4 lifetime-analytics columns is accidentally removed),
// this test fails — locking the schema invariant.
#[test_context(TestContext)]
#[tokio::test]
async fn test_wallet_balance_columns_dropped(ctx: &mut TestContext) {
    // All columns currently present on `points_wallets`.
    let columns: Vec<String> =
        sqlx::query_scalar("SELECT column_name FROM information_schema.columns WHERE table_name = 'points_wallets' ORDER BY column_name")
            .fetch_all(&ctx.app_state.pool)
            .await
            .expect("Failed to introspect points_wallets columns");

    let has = |name: &str| -> bool { columns.iter().any(|c| c == name) };

    // The 5 per-type balance columns + the GENERATED total must be GONE.
    for dropped in [
        "topup_balance",
        "subscription_balance",
        "granted_balance",
        "registration_balance",
        "free_periodic_balance",
        "total_balance",
    ] {
        assert!(
            !has(dropped),
            "invariant: points_wallets.{} should be physically removed, but column still present. columns = {:?}",
            dropped,
            columns
        );
    }

    // The 4 lifetime-analytics Stored columns MUST remain.
    for retained in [
        "total_consumed",
        "total_recharged",
        "total_topup_granted",
        "total_subscription_granted",
    ] {
        assert!(
            has(retained),
            "invariant: points_wallets.{} analytics column must remain, but it is missing. columns = {:?}",
            retained,
            columns
        );
    }
}

// point-time: balance_after is a REAL derived snapshot, not seed-0
//
// User Story: US-PU-001 (balance visibility) / contract (transaction
// history balance_after reflects the real post-mutation derived balance).
//
// Covers P0 "Stored balance 列物理删除 + balance_after 真实快照"
//   + risk "`balance_after` 非 0 真实" + design ref to
// `webhook_common.rs:20-22` seed-0 fix.
//
// Why this test exists: before the migration, several transaction-insert sites
// (notably `webhook_common.rs:20-22`) seeded `balance_after` and the typed
// `*_balance_after` snapshots with `0` as a placeholder, breaking the
// contract that a transaction records the real post-mutation balance. The migration
// fixed those sites to compute the real derived SUM (same predicate as
// `compute_available_balance`) in-transaction. This test grants a ledger
// row then consumes from it via the real service path, and asserts that the
// resulting `points_transactions.balance_after` is NON-ZERO and EQUALS the
// derived SUM for the same (realm, user, bucket) at that moment — proving
// the snapshot is real, not a placeholder. If any future change re-introduces
// a seed-0 placeholder on the consume path, this test fails.
#[test_context(TestContext)]
#[tokio::test]
async fn test_balance_after_is_real_snapshot(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let user_id = create_test_user(&ctx.app_state.pool, &realm_id, "be-t02-ba@exam.com").await;

    // Seed 3000 immediately-available topup_credit (no wallet Stored columns
    // touched — they were removed).
    let _ledger_id = create_credit_ledger_entry_with_effective_at(
        ctx,
        user_id,
        &realm_id,
        CreditType::TopupCredit,
        CreditSourceType::Topup,
        format!("be-t02-ba-{}", Uuid::now_v7()),
        3000,
        None,
        None,
    )
    .await;

    // Derived available balance pre-consume: 3000 topup_credit.
    assert_derived_balance(ctx, user_id, &realm_id, CreditType::TopupCredit, 3000).await;

    // Consume 1000 via the real service path — this is the path the migration
    // touched to write the real derived snapshot into `balance_after`.
    let identity =
        crate::tests::helpers::points_helpers::create_test_third_party_identity(&realm_id);
    let input = herald_core::domain::points::dtos::ConsumePointsInput {
        user_id: user_id.to_string(),
        client_app_id: ctx._client_app_id.clone(),
        amount: 1000,
        description: Some("be-t02 balance_after snapshot".to_string()),
    };
    let saved = ctx
        .app_state
        .points_service
        .consume_points(identity, &realm_id, input)
        .await
        .expect("consume must succeed");
    assert_eq!(saved.len(), 1, "single-bucket consume");
    let txn_id = saved[0].id;

    // Read the consume transaction row back from DB to inspect the snapshot
    // columns exactly as written by the consume atomic flow.
    let row = sqlx::query(
        "SELECT balance_after, topup_balance_after, subscription_balance_after, bucket_id
         FROM points_transactions WHERE id = $1",
    )
    .bind(txn_id)
    .fetch_one(&ctx.app_state.pool)
    .await
    .expect("Failed to fetch consume transaction row");

    use sqlx::Row;
    let balance_after: i64 = row.get("balance_after");
    let topup_after: Option<i64> = row.get("topup_balance_after");
    let subscription_after: Option<i64> = row.get("subscription_balance_after");
    let bucket_id: Uuid = row.get("bucket_id");

    // Contract: balance_after is NON-ZERO (would be 0 if seed-0 placeholder
    // were still in place — only 0 accidentally here means a regression).
    assert_ne!(
        balance_after, 0,
        "balance_after must be a real derived snapshot, not the legacy seed-0 placeholder"
    );

    // balance_after must equal the derived available balance SUM at the
    // post-consume moment for the same (realm, user, bucket). We recompute
    // the derived SUM scoped to the same single bucket so the assertion is
    // exact (not affected by other buckets the user might own).
    let expected_derived: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(remaining_amount), 0)::BIGINT FROM points_credit_ledger
         WHERE realm_id = $1 AND user_id = $2 AND bucket_id = $3
           AND status = 'active' AND remaining_amount > 0
           AND (effective_at IS NULL OR effective_at <= NOW())
           AND (expires_at  IS NULL OR expires_at  >  NOW())",
    )
    .bind(&realm_id)
    .bind(user_id)
    .bind(bucket_id)
    .fetch_one(&ctx.app_state.pool)
    .await
    .expect("Failed to compute expected derived balance for snapshot assertion");

    assert_eq!(
        balance_after, expected_derived,
        "balance_after ({}) must equal post-consume derived SUM ({}) for the same bucket — derived-balance contract",
        balance_after, expected_derived
    );

    // The per-type snapshots also come from the derived SUM split by
    // credit_type. After consuming 1000 of 3000 topup_credit,
    // 2000 topup_credit remains available → topup_balance_after = 2000.
    // subscription_credit has 0 available → subscription_balance_after = 0.
    assert_eq!(
        topup_after,
        Some(2000),
        "topup_balance_after must be the real derived topup available (2000), not a placeholder"
    );
    assert_eq!(
        subscription_after,
        Some(0),
        "subscription_balance_after must be the real derived subscription available (0)"
    );
}
