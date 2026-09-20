// =============================================================================
// Points System Scenario Test 15: Consume Idempotency
// =============================================================================
//
// User Story: SDK points consumption with idempotency_key
// Covers: Idempotent consumption -- same key returns cached result without
//         duplicate deduction; different keys execute independently; absent
//         key behaves as non-idempotent consumption.
//
// =============================================================================

use crate::tests::scenarios::points::fixtures::*;
use crate::tests::schema_test_context::SchemaTestContext as TestContext;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use serde_json::json;
use test_context::test_context;
use tower::ServiceExt;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helper: build a consume request with optional idempotency_key
// ---------------------------------------------------------------------------
fn build_consume_request(
    realm_id: &str,
    api_key: &str,
    user_id: &str,
    client_app_id: &str,
    amount: i64,
    description: &str,
    idempotency_key: Option<&str>,
) -> Request<Body> {
    let mut payload = json!({
        "userId": user_id,
        "clientAppId": client_app_id,
        "amount": amount,
        "description": description,
    });

    if let Some(key) = idempotency_key {
        payload["idempotencyKey"] = json!(key);
    }

    Request::builder()
        .method("POST")
        .uri(format!("/api/ext/points/{}/consume", realm_id))
        .header("content-type", "application/json")
        .header("X-API-Key", api_key)
        .body(Body::from(payload.to_string()))
        .unwrap()
}

/// Parse the JSON body from a response into a serde_json::Value.
async fn parse_response_body(response: axum::response::Response) -> serde_json::Value {
    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("Failed to read response body");
    serde_json::from_slice(&body_bytes).expect("Failed to parse JSON")
}

/// Return the single primary per-bucket transaction from a multi-bucket
/// consume response. Single-pool consumes have exactly one.
fn primary_transaction(body: &serde_json::Value) -> &serde_json::Value {
    let txns = body["transactions"]
        .as_array()
        .expect("consume response should contain a transactions array");
    assert_eq!(txns.len(), 1, "single-pool consume → 1 transaction");
    &txns[0]
}

// ============================================================================
// Scenario 1: Same idempotency_key returns cached result (no double deduction)
// ============================================================================

// User Story: SDK points consumption idempotency
// Covers: Same idempotency_key on repeated calls must return the first
//         cached result without creating additional transactions or changing
//         the balance.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_consume_idempotency_same_key_returns_cached(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    // Given: user with balance 5000
    let user_id =
        create_test_user(&ctx._app_state.pool, &ctx._realm_id, "user15a@example.com").await;
    let initial_balance: i64 = 5000;
    let _wallet_id =
        create_test_points_wallet(&ctx._app_state.pool, user_id, initial_balance).await;

    let client_app_id = create_test_client_app(&ctx._app_state.pool, &ctx._realm_id).await;
    let api_key = create_test_api_key(&ctx._app_state.pool, &ctx._realm_id, client_app_id).await;

    let idempotency_key = format!("req-15a-{}", Uuid::now_v7());

    // When: consume 100 with idempotency_key
    let request1 = build_consume_request(
        &ctx._realm_id,
        &api_key,
        &user_id.to_string(),
        &client_app_id.to_string(),
        100,
        "AI API call",
        Some(&idempotency_key),
    );

    let response1 = app.clone().oneshot(request1).await.unwrap();
    assert_eq!(
        response1.status(),
        StatusCode::OK,
        "First consume should succeed"
    );
    let body1 = parse_response_body(response1).await;

    let txn1 = primary_transaction(&body1);
    let txn_id_1 = txn1["transactionId"]
        .as_str()
        .expect("First response must have transactionId");
    assert_eq!(
        body1["amount"].as_i64(),
        Some(100),
        "First response amount should be the total consumed (100)"
    );
    assert_eq!(
        txn1["balanceAfter"].as_i64(),
        Some(4900),
        "First response balanceAfter should be 4900"
    );

    // When: consume 100 again with the SAME idempotency_key
    let request2 = build_consume_request(
        &ctx._realm_id,
        &api_key,
        &user_id.to_string(),
        &client_app_id.to_string(),
        100,
        "AI API call",
        Some(&idempotency_key),
    );

    let response2 = app.clone().oneshot(request2).await.unwrap();
    assert_eq!(
        response2.status(),
        StatusCode::OK,
        "Second consume with same key should succeed"
    );
    let body2 = parse_response_body(response2).await;
    let txn2 = primary_transaction(&body2);

    // Then: second response returns cached result (same transaction)
    assert_eq!(
        txn2["transactionId"].as_str(),
        Some(txn_id_1),
        "Second response must return the same transactionId"
    );
    assert_eq!(
        body2["amount"].as_i64(),
        Some(100),
        "Second response amount should be the total consumed (100)"
    );
    assert_eq!(
        txn2["balanceAfter"].as_i64(),
        Some(4900),
        "Second response balanceAfter should be 4900"
    );

    // Then: balance is still 4900 and only one consume transaction exists
    let (final_balance,): (i64,) =
        sqlx::query_as("SELECT COALESCE(SUM(l.remaining_amount) FILTER (WHERE l.status = 'active' AND l.remaining_amount > 0 AND (l.effective_at IS NULL OR l.effective_at <= NOW()) AND (l.expires_at IS NULL OR l.expires_at > NOW())), 0)::BIGINT AS total_balance FROM points_wallets w LEFT JOIN points_credit_ledger l ON l.realm_id = w.realm_id AND l.user_id = w.user_id AND l.bucket_id = w.bucket_id WHERE w.user_id = $1 GROUP BY w.id")
            .bind(user_id)
            .fetch_one(&ctx._app_state.pool)
            .await
            .expect("Failed to fetch account");

    assert_eq!(
        final_balance, 4900,
        "Balance should remain 4900 after duplicate idempotent request"
    );

    let (txn_count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM points_transactions WHERE user_id = $1 AND type = 'consume'",
    )
    .bind(user_id)
    .fetch_one(&ctx._app_state.pool)
    .await
    .expect("Failed to count transactions");

    assert_eq!(
        txn_count, 1,
        "Should have exactly one consume transaction, not two"
    );
}

// ============================================================================
// Scenario 2: Different idempotency_keys execute independently
// ============================================================================

// User Story: SDK points consumption idempotency
// Covers: Different idempotency_keys must each execute independently,
//         producing separate transactions and cumulative balance deductions.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_consume_idempotency_different_keys_independent(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    // Given: user with balance 5000
    let user_id =
        create_test_user(&ctx._app_state.pool, &ctx._realm_id, "user15b@example.com").await;
    let initial_balance: i64 = 5000;
    let _wallet_id =
        create_test_points_wallet(&ctx._app_state.pool, user_id, initial_balance).await;

    let client_app_id = create_test_client_app(&ctx._app_state.pool, &ctx._realm_id).await;
    let api_key = create_test_api_key(&ctx._app_state.pool, &ctx._realm_id, client_app_id).await;

    let key_a = format!("req-15b-a-{}", Uuid::now_v7());
    let key_b = format!("req-15b-b-{}", Uuid::now_v7());

    // When: consume 100 with idempotency_key
    let request1 = build_consume_request(
        &ctx._realm_id,
        &api_key,
        &user_id.to_string(),
        &client_app_id.to_string(),
        100,
        "Call A",
        Some(&key_a),
    );

    let response1 = app.clone().oneshot(request1).await.unwrap();
    assert_eq!(
        response1.status(),
        StatusCode::OK,
        "First consume should succeed"
    );
    let body1 = parse_response_body(response1).await;
    assert_eq!(
        primary_transaction(&body1)["balanceAfter"].as_i64(),
        Some(4900)
    );

    // When: consume 200 with a DIFFERENT idempotency_key
    let request2 = build_consume_request(
        &ctx._realm_id,
        &api_key,
        &user_id.to_string(),
        &client_app_id.to_string(),
        200,
        "Call B",
        Some(&key_b),
    );

    let response2 = app.clone().oneshot(request2).await.unwrap();
    assert_eq!(
        response2.status(),
        StatusCode::OK,
        "Second consume with different key should succeed"
    );
    let body2 = parse_response_body(response2).await;
    assert_eq!(
        primary_transaction(&body2)["balanceAfter"].as_i64(),
        Some(4700)
    );

    // Then: balance is 4700 and two consume transactions exist
    let (final_balance,): (i64,) =
        sqlx::query_as("SELECT COALESCE(SUM(l.remaining_amount) FILTER (WHERE l.status = 'active' AND l.remaining_amount > 0 AND (l.effective_at IS NULL OR l.effective_at <= NOW()) AND (l.expires_at IS NULL OR l.expires_at > NOW())), 0)::BIGINT AS total_balance FROM points_wallets w LEFT JOIN points_credit_ledger l ON l.realm_id = w.realm_id AND l.user_id = w.user_id AND l.bucket_id = w.bucket_id WHERE w.user_id = $1 GROUP BY w.id")
            .bind(user_id)
            .fetch_one(&ctx._app_state.pool)
            .await
            .expect("Failed to fetch account");

    assert_eq!(
        final_balance, 4700,
        "Balance should be 4700 after two independent consumptions"
    );

    let (txn_count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM points_transactions WHERE user_id = $1 AND type = 'consume'",
    )
    .bind(user_id)
    .fetch_one(&ctx._app_state.pool)
    .await
    .expect("Failed to count transactions");

    assert_eq!(txn_count, 2, "Should have exactly two consume transactions");
}

// ============================================================================
// Scenario 3: No idempotency_key -- normal consumption without error
// ============================================================================

// User Story: SDK points consumption idempotency
// Covers: When idempotency_key is absent, the endpoint must behave as a
//         normal (non-idempotent) consume operation. Two calls without the
//         key should each succeed and deduct independently.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_consume_idempotency_no_key_normal_consumption(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    // Given: user with balance 5000
    let user_id =
        create_test_user(&ctx._app_state.pool, &ctx._realm_id, "user15c@example.com").await;
    let initial_balance: i64 = 5000;
    let _wallet_id =
        create_test_points_wallet(&ctx._app_state.pool, user_id, initial_balance).await;

    let client_app_id = create_test_client_app(&ctx._app_state.pool, &ctx._realm_id).await;
    let api_key = create_test_api_key(&ctx._app_state.pool, &ctx._realm_id, client_app_id).await;

    // When: consume 100 without idempotency_key
    let request1 = build_consume_request(
        &ctx._realm_id,
        &api_key,
        &user_id.to_string(),
        &client_app_id.to_string(),
        100,
        "Non-idempotent call 1",
        None,
    );

    let response1 = app.clone().oneshot(request1).await.unwrap();
    assert_eq!(
        response1.status(),
        StatusCode::OK,
        "First consume should succeed"
    );
    let body1 = parse_response_body(response1).await;
    assert_eq!(
        primary_transaction(&body1)["balanceAfter"].as_i64(),
        Some(4900)
    );

    // When: consume 100 again without idempotency_key
    let request2 = build_consume_request(
        &ctx._realm_id,
        &api_key,
        &user_id.to_string(),
        &client_app_id.to_string(),
        100,
        "Non-idempotent call 2",
        None,
    );

    let response2 = app.clone().oneshot(request2).await.unwrap();
    assert_eq!(
        response2.status(),
        StatusCode::OK,
        "Second consume without key should also succeed"
    );
    let body2 = parse_response_body(response2).await;
    assert_eq!(
        primary_transaction(&body2)["balanceAfter"].as_i64(),
        Some(4800)
    );

    // Then: balance is 4800 and two consume transactions exist
    let (final_balance,): (i64,) =
        sqlx::query_as("SELECT COALESCE(SUM(l.remaining_amount) FILTER (WHERE l.status = 'active' AND l.remaining_amount > 0 AND (l.effective_at IS NULL OR l.effective_at <= NOW()) AND (l.expires_at IS NULL OR l.expires_at > NOW())), 0)::BIGINT AS total_balance FROM points_wallets w LEFT JOIN points_credit_ledger l ON l.realm_id = w.realm_id AND l.user_id = w.user_id AND l.bucket_id = w.bucket_id WHERE w.user_id = $1 GROUP BY w.id")
            .bind(user_id)
            .fetch_one(&ctx._app_state.pool)
            .await
            .expect("Failed to fetch account");

    assert_eq!(
        final_balance, 4800,
        "Balance should be 4800 after two non-idempotent consumptions"
    );

    let (txn_count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM points_transactions WHERE user_id = $1 AND type = 'consume'",
    )
    .bind(user_id)
    .fetch_one(&ctx._app_state.pool)
    .await
    .expect("Failed to count transactions");

    assert_eq!(txn_count, 2, "Should have exactly two consume transactions");
}

// ============================================================================
// Scenario 4 (audit run-1: api-ext/points.rs/
// consume-idempotency-replay-request-mismatch): same key + different payload
// ============================================================================

// 一个幂等键只应回应"同一请求"的重放。旧代码在 Cached 分支不比对请求指纹：
// 同键不同 amount/user 的请求会拿到第一次消费的流水、却回显第二次请求的
// amount —— 一个从未发生的扣减记录，破坏第三方侧的财务对账。现在：同键
// 不同负载 → 409 idempotency_conflict；同键同负载 → 照常 200 重放。
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_consume_idempotency_same_key_different_payload_conflicts(
    ctx: &mut TestContext,
) {
    let app = ctx.create_unified_test_router();

    let user_id =
        create_test_user(&ctx._app_state.pool, &ctx._realm_id, "user15d@example.com").await;
    let _wallet_id = create_test_points_wallet(&ctx._app_state.pool, user_id, 5000).await;

    let client_app_id = create_test_client_app(&ctx._app_state.pool, &ctx._realm_id).await;
    let api_key = create_test_api_key(&ctx._app_state.pool, &ctx._realm_id, client_app_id).await;

    let idempotency_key = format!("req-15d-{}", Uuid::now_v7());

    // First consume: 100, records the fingerprint for this key.
    let request1 = build_consume_request(
        &ctx._realm_id,
        &api_key,
        &user_id.to_string(),
        &client_app_id.to_string(),
        100,
        "AI API call",
        Some(&idempotency_key),
    );
    let response1 = app.clone().oneshot(request1).await.unwrap();
    assert_eq!(response1.status(), StatusCode::OK);
    let body1 = parse_response_body(response1).await;
    assert_eq!(body1["amount"].as_i64(), Some(100));

    // Same key, DIFFERENT payload (amount 999) must conflict, not fabricate a
    // replay response (old code: 200 with the first consume's transactions
    // next to amount=999).
    let request2 = build_consume_request(
        &ctx._realm_id,
        &api_key,
        &user_id.to_string(),
        &client_app_id.to_string(),
        999,
        "AI API call — corrected amount",
        Some(&idempotency_key),
    );
    let response2 = app.clone().oneshot(request2).await.unwrap();
    assert_eq!(
        response2.status(),
        StatusCode::CONFLICT,
        "key reuse with a different payload must 409, not replay the original consume"
    );
    let body2 = parse_response_body(response2).await;
    assert!(
        body2["transactions"].is_null(),
        "the conflict response must not carry the original consume's transactions: {body2}"
    );

    // Same key + IDENTICAL payload still replays 200 (the guard is a
    // fingerprint equality check, not a blanket rejection).
    let request3 = build_consume_request(
        &ctx._realm_id,
        &api_key,
        &user_id.to_string(),
        &client_app_id.to_string(),
        100,
        "AI API call",
        Some(&idempotency_key),
    );
    let response3 = app.clone().oneshot(request3).await.unwrap();
    assert_eq!(
        response3.status(),
        StatusCode::OK,
        "identical replay must succeed"
    );
    let body3 = parse_response_body(response3).await;
    assert_eq!(body3["amount"].as_i64(), Some(100));

    // Exactly one consume transaction may exist — the conflicting replay
    // neither deducted nor fabricated a second record.
    let (txn_count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM points_transactions WHERE user_id = $1 AND type = 'consume'",
    )
    .bind(user_id)
    .fetch_one(&ctx._app_state.pool)
    .await
    .expect("Failed to count transactions");
    assert_eq!(txn_count, 1, "exactly one consume may take effect");
}

// ============================================================================
// 审计 run-2 回归：幂等状态丢失的失败关闭与时域违规
// ============================================================================

/// SCAN 出与给定幂等键相关的全部 Redis 键（主记录 / :status / reqfp 指纹）。
/// scope 由 realm 与 API key 身份组成，测试直接按模式扫描避免重建命名。
async fn scan_idempotency_keys(ctx: &TestContext, pattern: &str) -> Vec<String> {
    let mut conn = ctx._app_state.redis_manager.get().await.unwrap();
    let mut cursor: u64 = 0;
    let mut found = Vec::new();
    loop {
        let (next, keys): (u64, Vec<String>) = redis::cmd("SCAN")
            .arg(cursor)
            .arg("MATCH")
            .arg(pattern)
            .arg("COUNT")
            .arg(100)
            .query_async(&mut conn)
            .await
            .unwrap();
        found.extend(keys);
        if next == 0 {
            break;
        }
        cursor = next;
    }
    found
}

/// 回归（审计 run-2：domain/points/idempotency/status-loss-failopen-double-consume）：
/// check_or_create 曾把"主键存在但状态标记非 Processing"当作新请求（fail
/// open）—— 状态键丢失（60s TTL 到期、写入失败、进程崩溃）后，字节相同的
/// 重放会再次扣减。修复后：主键存在 + 状态未知/已完成 → 409 失败关闭；
/// 正常缓存重放（状态键完好）不受影响。
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_consume_idempotency_status_loss_fails_closed(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    let user_id = create_test_user(
        &ctx._app_state.pool,
        &ctx._realm_id,
        "user15-statusloss@example.com",
    )
    .await;
    let _wallet_id = create_test_points_wallet(&ctx._app_state.pool, user_id, 5000).await;
    let client_app_id = create_test_client_app(&ctx._app_state.pool, &ctx._realm_id).await;
    let api_key = create_test_api_key(&ctx._app_state.pool, &ctx._realm_id, client_app_id).await;

    let idempotency_key = format!("req-statusloss-{}", Uuid::now_v7());
    let request = |amount: i64| {
        build_consume_request(
            &ctx._realm_id,
            &api_key,
            &user_id.to_string(),
            &client_app_id.to_string(),
            amount,
            "AI API call",
            Some(&idempotency_key),
        )
    };

    // 首次消费成功。
    let response = app.clone().oneshot(request(100)).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // 正向对照：状态键完好时，同键重放返回缓存结果。
    let replay = app.clone().oneshot(request(100)).await.unwrap();
    assert_eq!(
        replay.status(),
        StatusCode::OK,
        "cached replay must keep working"
    );

    // 模拟完成标记丢失（60s TTL 到期 / 写入失败 / save_result 前崩溃）：
    // 主键回退为请求 JSON（完成结果从未写入），并删除 :status 键 ——
    // get_from_cache 因此不可解析，判定落到 lock/status 分类上。
    let main_keys = scan_idempotency_keys(ctx, &format!("idempotency:*:{}", idempotency_key)).await;
    let main_key = main_keys
        .iter()
        .find(|k| !k.ends_with(":status") && !k.contains(":reqfp:"))
        .expect("the main idempotency record should exist");
    let status_keys =
        scan_idempotency_keys(ctx, &format!("idempotency:*:{}:status", idempotency_key)).await;
    assert!(
        !status_keys.is_empty(),
        "the :status sibling key should exist"
    );
    let mut conn = ctx._app_state.redis_manager.get().await.unwrap();
    use redis::AsyncCommands;
    let main_ttl: i64 = conn.ttl(main_key).await.unwrap();
    let _: () = conn
        .set_ex(
            main_key.as_str(),
            r#"{"userId":"mid-flight","amount":100}"#,
            main_ttl.max(1) as u64,
        )
        .await
        .unwrap();
    let _: () = conn.del(&status_keys).await.unwrap();

    // 字节相同的重放必须失败关闭（旧代码：重新执行 → 第二次扣减）。
    let after_loss = app.clone().oneshot(request(100)).await.unwrap();
    assert_eq!(
        after_loss.status(),
        StatusCode::CONFLICT,
        "a lost state marker must fail closed instead of re-executing"
    );

    // 账本仍只有一笔 consume。
    let (txn_count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM points_transactions WHERE user_id = $1 AND type = 'consume'",
    )
    .bind(user_id)
    .fetch_one(&ctx._app_state.pool)
    .await
    .unwrap();
    assert_eq!(txn_count, 1, "no second deduction may land");
}

/// 回归（审计 run-2：consume-idempotency-fingerprint-horizon-gap）：
/// 指纹 TTL（请求起点 24h+1h）与缓存记录 TTL（完成时点 24h）的覆盖前提
/// 曾无强制 —— 完成滞后超过 1h 时指纹先亡，异载重放会以新 amount 回放旧
/// 交易（捏造金额）。修复后：ext 路由有 60s 请求上限（前提强制），且指纹
/// 层防御性地把"缓存记录比指纹活得久"判为时域违规 → 409。
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_consume_idempotency_fingerprint_horizon_violation_conflicts(
    ctx: &mut TestContext,
) {
    let app = ctx.create_unified_test_router();

    let user_id = create_test_user(
        &ctx._app_state.pool,
        &ctx._realm_id,
        "user15-horizon@example.com",
    )
    .await;
    let _wallet_id = create_test_points_wallet(&ctx._app_state.pool, user_id, 5000).await;
    let client_app_id = create_test_client_app(&ctx._app_state.pool, &ctx._realm_id).await;
    let api_key = create_test_api_key(&ctx._app_state.pool, &ctx._realm_id, client_app_id).await;

    let idempotency_key = format!("req-horizon-{}", Uuid::now_v7());
    let request = |amount: i64| {
        build_consume_request(
            &ctx._realm_id,
            &api_key,
            &user_id.to_string(),
            &client_app_id.to_string(),
            amount,
            "AI API call",
            Some(&idempotency_key),
        )
    };

    let response = app.clone().oneshot(request(100)).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // 模拟指纹先于缓存记录过期：删除 reqfp 键，主键（已完成事务）存活。
    let fp_keys =
        scan_idempotency_keys(ctx, &format!("idempotency:reqfp:*:{}", idempotency_key)).await;
    assert!(!fp_keys.is_empty(), "the fingerprint key should exist");
    let mut conn = ctx._app_state.redis_manager.get().await.unwrap();
    use redis::AsyncCommands;
    let _: () = conn.del(&fp_keys).await.unwrap();

    // 异载重放：不得以 200 + 新 amount 回放旧交易，必须 409。
    let replay = app.oneshot(request(200)).await.unwrap();
    assert_eq!(
        replay.status(),
        StatusCode::CONFLICT,
        "a replay whose fingerprint horizon was violated must conflict, not fabricate an amount"
    );

    let (txn_count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM points_transactions WHERE user_id = $1 AND type = 'consume'",
    )
    .bind(user_id)
    .fetch_one(&ctx._app_state.pool)
    .await
    .unwrap();
    assert_eq!(txn_count, 1, "no second deduction may land");
}

/// 回归（审计 run-2：consume-idempotency-key-uncapped-redis-retention）：
/// 调用方选定的 idempotency_key 曾以任意长度进入三个 Redis 键名与存储值
/// （24h+ TTL、失败路径不清理），一个已授权 API key 即可在共享 Redis 上
/// 留下无上限的持久字节。修复后：超 255 字节的键 400，且不留任何幂等键。
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_consume_idempotency_oversized_key_rejected(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    let user_id = create_test_user(
        &ctx._app_state.pool,
        &ctx._realm_id,
        "user15-oversize@example.com",
    )
    .await;
    let _wallet_id = create_test_points_wallet(&ctx._app_state.pool, user_id, 5000).await;
    let client_app_id = create_test_client_app(&ctx._app_state.pool, &ctx._realm_id).await;
    let api_key = create_test_api_key(&ctx._app_state.pool, &ctx._realm_id, client_app_id).await;

    let oversized_key = format!("k-{}", "x".repeat(64 * 1024));
    let request = build_consume_request(
        &ctx._realm_id,
        &api_key,
        &user_id.to_string(),
        &client_app_id.to_string(),
        100,
        "AI API call",
        Some(&oversized_key),
    );
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "an oversized idempotency key must be rejected before any Redis write"
    );

    // 不得留下任何以该键为后缀的 Redis 键。
    let leftover = scan_idempotency_keys(ctx, "*x{64}*").await;
    let leftover = leftover
        .into_iter()
        .filter(|k| k.contains(&oversized_key[..64]))
        .count();
    assert_eq!(leftover, 0, "no idempotency Redis keys may be written");

    // 账本零扣减。
    let (txn_count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM points_transactions WHERE user_id = $1 AND type = 'consume'",
    )
    .bind(user_id)
    .fetch_one(&ctx._app_state.pool)
    .await
    .unwrap();
    assert_eq!(txn_count, 0);
}
