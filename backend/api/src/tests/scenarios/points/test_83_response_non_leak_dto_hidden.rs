// =============================================================================
// response/wallet-list non-leak + DTO effective_at hiding
// =============================================================================
//
// Encodes the point-time non-leak invariants:
//   * "管理员钱包列表不泄漏未来期积分" — `list_wallets` cross-user
//     batched derived assembly must not leak future-effective rows.
//   * `PointsTransactionResponse.effective_at` is admin/audit-only. A
//     `points.view` (regular user) response MUST have the
//     `effectiveAt` JSON key ABSENT (via `skip_serializing_if`); a
//     `points.manage` (admin) response MUST include the key with the real
//     ledger value when the source row carries one.
//   * wallet Stored 列读点遗漏 — no read path may consult the removed
//     `points_wallets` Stored columns.
//
// All derived-balance cross-checks use the helpers
// (`assert_derived_balance`, `count_future_effective_active_rows`); they mirror
// production `compute_available_balance` verbatim and never read
// `points_wallets` Stored columns (those were removed — derived SUM is the
// sole available-balance authority under point-time).
//
// HTTP response JSON is inspected via `serde_json::Value` so we can assert
// KEY presence/absence (the `skip_serializing_if` contract), not just value.
//
// =============================================================================

use crate::tests::helpers::auth_helpers::{create_admin_session_with_user, grant_realm_admin_role};
use crate::tests::helpers::points_helpers::{
    assert_derived_balance, count_future_effective_active_rows,
    create_credit_ledger_entry_with_effective_at,
};
use crate::tests::helpers::test_setup_helpers::record_test_user_consent;
use crate::tests::scenarios::points::fixtures::*;
use crate::tests::schema_test_context::SchemaTestContext as TestContext;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use chrono::{Duration, Utc};
use herald_core::domain::points::entities::{CreditSourceType, CreditType};
use serde_json::json;
use sqlx::Row;
use test_context::test_context;
use tower::ServiceExt;
use uuid::Uuid;

// =============================================================================
// Shared local helpers
// =============================================================================

/// Create a `points_wallets` row with the post-migration schema (no Stored
/// balance columns — only analytics + status). We can NOT use the legacy
/// `create_test_points_wallet` fixture because it still writes the now-deleted
/// `subscription_balance`/`topup_balance` columns (BE-TR is migrating those
/// helpers; see the spec note "AVOID broken old helpers").
async fn create_wallet_row_post_be_d11(
    ctx: &mut TestContext,
    user_id: Uuid,
    realm_id: &str,
) -> Uuid {
    let bucket_id = crate::tests::helpers::points_helpers::ensure_test_bucket_for_realm(
        &ctx._app_state.pool,
        realm_id,
    )
    .await;
    let wallet_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO points_wallets
            (id, user_id, realm_id, bucket_id, total_recharged, total_consumed,
             total_topup_granted, total_subscription_granted, status)
         VALUES ($1, $2, $3, $4, 0, 0, 0, 0, 'active')
         ON CONFLICT (realm_id, user_id, bucket_id) DO NOTHING",
    )
    .bind(wallet_id)
    .bind(user_id)
    .bind(realm_id)
    .bind(bucket_id)
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to create points_wallets row (post-refactor schema)");

    // Re-read in case a wallet already existed for this (user, bucket).
    sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM points_wallets WHERE realm_id = $1 AND user_id = $2 AND bucket_id = $3",
    )
    .bind(realm_id)
    .bind(user_id)
    .bind(bucket_id)
    .fetch_one(&ctx._app_state.pool)
    .await
    .expect("Failed to fetch wallet after ensure")
}

/// Seed a `points_transactions` recharge row whose `external_ref_id` matches
/// the production `find_transactions` LEFT JOIN pattern
/// (`t.external_ref_id LIKE (l.source_id || ':%')`) so the row resolves
/// `effective_at` from the linked ledger. Returns the txn id.
///
/// `wallet_id`/`bucket_id` MUST pre-exist (FK). `ledger_source_id` is the
/// linked ledger row's `source_id` — production topup/recharge writes the
/// transaction `external_ref_id` as `"<source_id>:<txn_id>"` (see
/// `find_transactions` doc comment).
async fn seed_topup_transaction_linked_to_ledger(
    ctx: &mut TestContext,
    user_id: Uuid,
    realm_id: &str,
    wallet_id: Uuid,
    ledger_source_id: &str,
    txn_type: &str,
    amount: i64,
) -> Uuid {
    let row = sqlx::query("SELECT bucket_id FROM points_wallets WHERE id = $1")
        .bind(wallet_id)
        .fetch_one(&ctx._app_state.pool)
        .await
        .expect("Failed to fetch wallet bucket_id");
    let bucket_id: Uuid = row.get("bucket_id");

    let txn_id = Uuid::now_v7();
    // Production write convention: external_ref_id = "<source_id>:<txn_id>".
    // This is exactly the prefix-then-colon form the LEFT JOIN matches on.
    let external_ref_id = format!("{}:{}", ledger_source_id, txn_id);

    sqlx::query(
        "INSERT INTO points_transactions
            (id, wallet_id, user_id, realm_id, bucket_id, type, amount,
             balance_after, credit_type, external_ref_id, created_at, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $7, $8, $9, NOW(), NOW())",
    )
    .bind(txn_id)
    .bind(wallet_id)
    .bind(user_id)
    .bind(realm_id)
    .bind(bucket_id)
    .bind(txn_type)
    .bind(amount)
    .bind(CreditType::TopupCredit.to_string())
    .bind(&external_ref_id)
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to seed topup transaction linked to ledger");

    txn_id
}

/// Login helper — returns the session token. Mirrors `unified_filter_tests`.
async fn login(ctx: &mut TestContext, app_url: &str, email: &str, password: &str) -> String {
    let login_payload = json!({
        "clientId": ctx._client_id,
        "email": email,
        "password": password,
        "turnstileToken": "dummy"
    });
    let login_request = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{}/login", ctx._realm_id))
        .header("content-type", "application/json")
        .header("x-forwarded-for", app_url)
        .body(Body::from(login_payload.to_string()))
        .unwrap();
    let login_response = ctx
        .create_unified_test_router()
        .oneshot(login_request)
        .await
        .unwrap();
    assert_eq!(login_response.status(), StatusCode::OK);
    let (_response, token) = crate::tests::extract_bearer_token(login_response).await;
    token.expect("Login should return accessToken")
}

// =============================================================================
// Test 1: points.view response hides effectiveAt (key ABSENT)
// =============================================================================

// User Story: US-PU-002 / US-PU-003 (regular users must not see pre-grant
// timing metadata; effective_at is admin/audit-only).
//
// Covers P1-2 + P1 "PointsTransactionResponse 在 points.view
// 下不含 effective_at".
//
// Why this test exists: the `effective_at` field on
// `PointsTransactionResponse` is annotated with
// `#[serde(skip_serializing_if = "Option::is_none")]` AND the
// `list_transactions` handler forces `effective_at = None` on the `points.view`
// path (non-erroring `points.manage` probe resolves to false). Together these
// two mechanisms must guarantee the `effectiveAt` key is ABSENT (not `null`)
// from regular-user JSON. Asserting KEY ABSENCE — not just value — is the
// load-bearing check: if someone removed the `skip_serializing_if` attribute
// the field would leak as `"effectiveAt": null` to regular users (a metadata
// disclosure even when the value is unknown).
#[test_context(TestContext)]
#[tokio::test]
async fn test_user_transaction_response_hides_effective_at_for_view(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let app = ctx.create_unified_test_router();

    // Given: a regular user (`points.view` only) with a future-effective
    // topup ledger row + a linked recharge transaction row whose
    // `external_ref_id` matches the production LEFT JOIN pattern.
    let email = "be-t09-view@example.com";
    let password = "password123";
    let user_id =
        create_test_user_with_auth(&ctx._app_state.pool, &realm_id, email, password).await;
    record_test_user_consent(&ctx._app_state.pool, user_id, &realm_id).await;

    let wallet_id = create_wallet_row_post_be_d11(ctx, user_id, &realm_id).await;

    let future_effective_at = Utc::now() + Duration::days(1);
    let ledger_source_id = format!("be-t09-view-ledger-{}", Uuid::now_v7());
    let ledger_id = create_credit_ledger_entry_with_effective_at(
        ctx,
        user_id,
        &realm_id,
        CreditType::TopupCredit,
        CreditSourceType::Topup,
        ledger_source_id.clone(),
        5_000,
        None,
        Some(future_effective_at),
    )
    .await;

    let txn_id = seed_topup_transaction_linked_to_ledger(
        ctx,
        user_id,
        &realm_id,
        wallet_id,
        &ledger_source_id,
        "recharge",
        5_000,
    )
    .await;

    // Sanity: the ledger row IS future-effective (so non-leak is meaningful).
    assert_eq!(
        count_future_effective_active_rows(ctx, user_id, &realm_id).await,
        1,
        "precondition: the future-effective row exists"
    );
    // Sanity: the transaction→ledger LEFT JOIN would resolve effective_at for a
    // points.manage caller (asserted in the companion test). For the view
    // path, the handler forces it to None regardless.
    let _ = (ledger_id, txn_id);

    // When: the regular user lists their transactions.
    let token = login(ctx, "3.3.4.1", email, password).await;
    let request = Request::builder()
        .method("GET")
        .uri("/api/user/transactions")
        .header("authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("Failed to read response body");
    let body: serde_json::Value =
        serde_json::from_slice(&body_bytes).expect("Failed to parse JSON");

    // Then: the matching transaction row is present in the response...
    let items = body["items"]
        .as_array()
        .expect("response should contain items[]");
    let matching = items
        .iter()
        .find(|item| item["transactionType"].as_str() == Some("recharge"))
        .expect("the seeded recharge transaction should be in the response");

    // ...and its `effectiveAt` key is ABSENT — NOT just null. The
    // `skip_serializing_if` attribute must drop the key entirely on the
    // `points.view` path (P1-2). This is the load-bearing check.
    assert!(
        matching.get("effectiveAt").is_none(),
        "points.view response must OMIT the `effectiveAt` key entirely (skip_serializing_if); \
         got: {:?}. If the key appears as `null`, someone removed the \
         skip_serializing_if attribute and regular users would see a \
         metadata field that should not exist on their view.",
        matching.get("effectiveAt")
    );
    assert!(
        !matching
            .as_object()
            .map(|o| o.contains_key("effectiveAt"))
            .unwrap_or(false),
        "points.view response must NOT contain `effectiveAt` key in any form (null or value)"
    );
}

// =============================================================================
// Test 2: points.manage response includes effectiveAt with the real value
// =============================================================================

// User Story: admin/audit reconciliation of pre-generated vs already-effective
// rows (P1-2 motivation).
//
// Covers P1-2 + P1 "PointsTransactionResponse 在
// points.manage 下含 effective_at".
//
// Why this test exists: the same `effective_at` column must surface to
// admins/auditors with the real ledger value so they can distinguish
// pre-generated future periods from already-effective ones. The handler
// populates `effective_at` only when the `points.manage` probe succeeds.
#[test_context(TestContext)]
#[tokio::test]
async fn test_admin_transaction_response_includes_effective_at_for_manage(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let app = ctx.create_unified_test_router();

    // Given: a regular user owns the future-effective ledger + linked recharge
    // transaction (the data subject). An admin (`points.manage`) then queries
    // the realm-wide transactions list.
    let user_email = "be-t09-manage-user@example.com";
    let user_password = "password123";
    let user_id =
        create_test_user_with_auth(&ctx._app_state.pool, &realm_id, user_email, user_password)
            .await;
    record_test_user_consent(&ctx._app_state.pool, user_id, &realm_id).await;

    let wallet_id = create_wallet_row_post_be_d11(ctx, user_id, &realm_id).await;

    let future_effective_at = Utc::now() + Duration::days(1);
    let ledger_source_id = format!("be-t09-manage-ledger-{}", Uuid::now_v7());
    create_credit_ledger_entry_with_effective_at(
        ctx,
        user_id,
        &realm_id,
        CreditType::TopupCredit,
        CreditSourceType::Topup,
        ledger_source_id.clone(),
        7_000,
        None,
        Some(future_effective_at),
    )
    .await;
    seed_topup_transaction_linked_to_ledger(
        ctx,
        user_id,
        &realm_id,
        wallet_id,
        &ledger_source_id,
        "recharge",
        7_000,
    )
    .await;

    // Admin setup: mint an admin-console (FirstParty) session with
    // `realm-admin` (carries `points.manage`). A plain HTTP `login` token is
    // CredentialClass::CustomUserUi and is rejected (403) by the
    // admin-console gate mounted on `/api/points/*`.
    let (token, _admin_user_id) =
        create_admin_session_with_user(ctx, "be-t09-admin@example.com", 1800).await;
    grant_realm_admin_role(ctx, &_admin_user_id).await;

    // When: the admin lists realm transactions (userId filter ⟹ the data
    // subject's cross-user view).
    let request = Request::builder()
        .method("GET")
        .uri(format!(
            "/api/points/{}/transactions?userId={}",
            realm_id, user_id
        ))
        .header("authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("Failed to read response body");
    let body: serde_json::Value =
        serde_json::from_slice(&body_bytes).expect("Failed to parse JSON");

    // Then: the matching transaction's `effectiveAt` key IS present and holds
    // the future timestamp (P1-2 manage path).
    let items = body["items"]
        .as_array()
        .expect("response should contain items[]");
    let matching = items
        .iter()
        .find(|item| item["transactionType"].as_str() == Some("recharge"))
        .expect("the seeded recharge transaction should be in the response");

    assert!(
        matching.get("effectiveAt").is_some(),
        "points.manage response MUST include the `effectiveAt` key (P1-2 admin/audit \
         path); got object keys: {:?}",
        matching.as_object().map(|o| o.keys().collect::<Vec<_>>())
    );
    // Value matches the ledger row's future effective_at (allow small skew for
    // serialization rounding — the key presence above is the load-bearing
    // assertion; value parity confirms the LEFT JOIN sourced the right row).
    let resp_effective = matching["effectiveAt"]
        .as_str()
        .expect("effectiveAt should serialize as an RFC3339 string");
    let parsed = chrono::DateTime::parse_from_rfc3339(resp_effective)
        .expect("effectiveAt should be a valid RFC3339 timestamp")
        .with_timezone(&Utc);
    let skew = (parsed - future_effective_at).num_seconds().abs();
    assert!(
        skew < 2,
        "points.manage effectiveAt ({}) should match the ledger future-effective timestamp ({}); \
         skew={}s",
        resp_effective,
        future_effective_at,
        skew
    );
}

// =============================================================================
// Test 3: admin list_wallets cross-user batched derived does not leak
// =============================================================================

// User Story: US-PO-02 (admin views all user wallets) — must not leak
// future-effective pre-grant rows into any admin-visible "available/remaining"
// figure.
//
// Covers P1 "管理员钱包列表不泄漏未来期积分" + risk "wallet
// Stored 列读点遗漏：list_wallets ... 若继续读 points_wallets.total_balance 会
// 泄漏未来期积分" (P1).
//
// Why this test exists: `list_wallets` (`/api/points/{realm}/wallets`) is the
// admin cross-user view. Its `group_wallets_by_bucket` assembly MUST source
// typed balances and `bucket_total` from the batched derived SUM (same
// `effective_at <= NOW()` predicate as consumption), NOT from any
// `points_wallets` Stored column. A future-effective-only user must show as
// zero available balance in the list; analytics (lifetime totals) remain on
// Stored columns and may be non-zero independently.
#[test_context(TestContext)]
#[tokio::test]
async fn test_admin_list_wallets_excludes_future_effective(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let app = ctx.create_unified_test_router();

    // Given: two regular users in the same realm/bucket.
    //   * user_future  — only future-effective topup_credit rows.
    //   * user_now     — only immediately-available topup_credit rows.
    // Both wallet rows are pre-created (post-migration schema: analytics-only).
    let user_future_email = "be-t09-lw-future@example.com";
    let user_now_email = "be-t09-lw-now@example.com";
    let user_pwd = "password123";
    let user_future =
        create_test_user_with_auth(&ctx._app_state.pool, &realm_id, user_future_email, user_pwd)
            .await;
    record_test_user_consent(&ctx._app_state.pool, user_future, &realm_id).await;
    let user_now =
        create_test_user_with_auth(&ctx._app_state.pool, &realm_id, user_now_email, user_pwd).await;
    record_test_user_consent(&ctx._app_state.pool, user_now, &realm_id).await;

    let wallet_future = create_wallet_row_post_be_d11(ctx, user_future, &realm_id).await;
    let wallet_now = create_wallet_row_post_be_d11(ctx, user_now, &realm_id).await;

    let future_at = Utc::now() + Duration::days(2);
    // user_future: two future-effective rows, total 4_000 — must NOT count.
    create_credit_ledger_entry_with_effective_at(
        ctx,
        user_future,
        &realm_id,
        CreditType::TopupCredit,
        CreditSourceType::Topup,
        format!("be-t09-lw-fut-a-{}", Uuid::now_v7()),
        2_500,
        None,
        Some(future_at),
    )
    .await;
    create_credit_ledger_entry_with_effective_at(
        ctx,
        user_future,
        &realm_id,
        CreditType::TopupCredit,
        CreditSourceType::Topup,
        format!("be-t09-lw-fut-b-{}", Uuid::now_v7()),
        1_500,
        None,
        Some(future_at),
    )
    .await;
    // user_now: one immediately-available row of 3_000 — counts in full.
    create_credit_ledger_entry_with_effective_at(
        ctx,
        user_now,
        &realm_id,
        CreditType::TopupCredit,
        CreditSourceType::Topup,
        format!("be-t09-lw-now-{}", Uuid::now_v7()),
        3_000,
        None,
        None, // effective_at=NULL ⟺ immediately available
    )
    .await;

    // Stamp wallet analytics for user_future (lifetime recharged non-zero) so
    // we can assert analytics stay Stored-side and do NOT masquerade as
    // available balance in the admin view.
    sqlx::query("UPDATE points_wallets SET total_recharged = 9999 WHERE id = $1")
        .bind(wallet_future)
        .execute(&ctx._app_state.pool)
        .await
        .expect("Failed to stamp analytics on user_future wallet");
    sqlx::query("UPDATE points_wallets SET total_recharged = 9999 WHERE id = $1")
        .bind(wallet_now)
        .execute(&ctx._app_state.pool)
        .await
        .expect("Failed to stamp analytics on user_now wallet");

    // Cross-check (a): the derived predicate excludes the future-only user.
    assert_derived_balance(ctx, user_future, &realm_id, CreditType::TopupCredit, 0).await;
    assert_derived_balance(ctx, user_now, &realm_id, CreditType::TopupCredit, 3_000).await;
    assert_eq!(
        count_future_effective_active_rows(ctx, user_future, &realm_id).await,
        2,
        "user_future has two future-effective rows that must not leak"
    );

    // Admin login (points.manage): mint an admin-console (FirstParty) session —
    // plain HTTP login tokens are rejected by the admin-console gate.
    let (token, _admin_user_id) =
        create_admin_session_with_user(ctx, "be-t09-lw-admin@example.com", 1800).await;
    grant_realm_admin_role(ctx, &_admin_user_id).await;

    // When: the admin calls list_wallets (cross-user realm-wide view).
    let request = Request::builder()
        .method("GET")
        .uri(format!("/api/points/{}/wallets", realm_id))
        .header("authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "admin list_wallets should return 200"
    );

    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("Failed to read response body");
    let body: serde_json::Value =
        serde_json::from_slice(&body_bytes).expect("Failed to parse JSON");

    let items = body["items"]
        .as_array()
        .expect("list_wallets response should contain items[]");

    // Then: the future-only user's row shows bucket_total == 0 (future rows
    // do not leak into available balance), while analytics remain non-zero
    // (Stored口径, unaffected by effective_at — derived predicate).
    let row_future = items
        .iter()
        .find(|item| item["userId"].as_str() == Some(user_future.to_string().as_str()))
        .unwrap_or_else(|| {
            panic!(
                "user_future should appear in list_wallets; items = {:?}",
                items
            )
        });
    assert_eq!(
        row_future["bucketTotal"].as_i64(),
        Some(0),
        "user_future (future-effective only) must show bucketTotal=0 in list_wallets — \
         future-effective rows must NOT leak into the admin available-balance view"
    );
    assert_eq!(
        row_future["balancesByType"]["topup"].as_i64(),
        Some(0),
        "user_future typed topup balance must be 0 (future-effective excluded)"
    );

    // Sanity: user_now shows the immediately-available 3_000 in both total and
    // typed breakdown.
    let row_now = items
        .iter()
        .find(|item| item["userId"].as_str() == Some(user_now.to_string().as_str()))
        .unwrap_or_else(|| {
            panic!(
                "user_now should appear in list_wallets; items = {:?}",
                items
            )
        });
    assert_eq!(
        row_now["bucketTotal"].as_i64(),
        Some(3_000),
        "user_now (immediately-available) bucketTotal should equal the active ledger sum"
    );
    assert_eq!(
        row_now["balancesByType"]["topup"].as_i64(),
        Some(3_000),
        "user_now typed topup balance should equal the immediately-available row"
    );

    // spendableFromPool regression guard: a pool-only bucket (topup_credit, no
    // quota entitlement) MUST surface its real pool balance, not null. The
    // gating predicate is "non-zero OR has window view"; a zero-pool bucket
    // with no windows (user_future below) stays null. Without this guard,
    // user_now would render "充值余额 0" in the UI despite holding 3_000 topup.
    assert_eq!(
        row_now["spendableFromPool"].as_i64(),
        Some(3_000),
        "user_now (pool-only bucket, active topup) must surface spendableFromPool=3000, \
         not null — pool-only buckets report their real pool balance"
    );
    assert!(
        row_future["spendableFromPool"].is_null(),
        "user_future (zero active pool balance, no quota windows) must keep \
         spendableFromPool null — both sides zero omits the field"
    );

    println!(
        "\n✅ Scenario #3 完成：管理员 list_wallets 跨用户批量派生不泄漏 future-effective \
         (user_future bucketTotal=0, user_now bucketTotal=3000)"
    );
}
