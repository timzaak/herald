// =============================================================================
// WeChat Pay v3 Webhook Scenario Tests
// =============================================================================
//
// Exercises `POST /api/third/pay/{realmId}/wechat/webhooks`
// (`api-billing/src/wechat_webhook_handlers.rs::handle_wechat_webhook`)
// end-to-end through the real verify → decrypt → idempotency → amount-check →
// unified-fulfilment pipeline.
//
// User Story: docs/user-stories/billing/wechat-support.md — US-WP-002 (PC scan
// Native pay), US-WP-004 (callback verify / decrypt / idempotent fulfilment),
// US-WP-003 (JSAPI openid requirement).
//
// Trust model: callbacks are signed with a generated RSA keypair whose public
// half is seeded as the realm's `platform_public_key` override, and the
// `resource` is AES-256-GCM encrypted with the realm's APIv3 Key. No network or
// real WeChat certificate is required.
//
// Covers the design's P0/P1 risks: signature/decrypt correctness, amount-guard
// rejection, duplicate-event idempotency, and OneTime / NonRenewing fulfilment.

#[cfg(test)]
mod tests {
    use crate::tests::helpers::billing_helpers::setup_billing_admin_session;
    use crate::tests::helpers::points_helpers::{
        ensure_test_bucket_for_realm, snapshot_attempt_rules_for_mapping,
    };
    use crate::tests::helpers::wechat_mocks::{
        TEST_V3_KEY, build_notification, insert_wechat_realm_config, wechat_webhook_request,
    };
    use crate::tests::helpers::{create_admin_session_with_user, grant_realm_admin_role};
    use crate::tests::schema_test_context::SchemaTestContext;
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode, header},
    };
    use serde_json::json;
    use test_context::test_context;
    use tower::ServiceExt;
    use uuid::Uuid;

    use SchemaTestContext as WechatWebhookContext;

    // =========================================================================
    // Shared setup helpers
    // =========================================================================

    async fn create_test_user(ctx: &WechatWebhookContext, email: &str) -> Uuid {
        let user_id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO account (id, realm_id, email, password, status)
             VALUES ($1, $2, $3, '$2a$12$dummy', 1)
             ON CONFLICT (realm_id, email) DO NOTHING",
        )
        .bind(user_id)
        .bind(&ctx._realm_id)
        .bind(email)
        .execute(&ctx.app_state.pool)
        .await
        .expect("create test user");
        user_id
    }

    async fn create_points_wallet(ctx: &WechatWebhookContext, user_id: Uuid, realm_id: &str) {
        let bucket_id = ensure_test_bucket_for_realm(&ctx.app_state.pool, realm_id).await;
        sqlx::query(
            "INSERT INTO points_wallets
                (id, user_id, realm_id, bucket_id, total_topup_granted,
                 total_subscription_granted, total_recharged, total_consumed, status, created_at, updated_at)
             VALUES ($1, $2, $3, $4, 0, 0, 0, 0, 'active', NOW(), NOW())
             ON CONFLICT (realm_id, user_id, bucket_id) DO NOTHING",
        )
        .bind(Uuid::now_v7())
        .bind(user_id)
        .bind(realm_id)
        .bind(bucket_id)
        .execute(&ctx.app_state.pool)
        .await
        .expect("create points wallet");
    }

    /// Seed a wechat one-time mapping + an owned topup distribution rule.
    async fn create_wechat_one_time_mapping(
        ctx: &WechatWebhookContext,
        realm_id: &str,
        external_product_id: &str,
        points: i64,
    ) -> Uuid {
        let mapping_id = Uuid::now_v7();
        let bucket_id = ensure_test_bucket_for_realm(&ctx.app_state.pool, realm_id).await;
        sqlx::query(
            "INSERT INTO provider_entitlement_mappings
                (id, realm_id, payment_provider, external_product_id, entitlement_key,
                 billing_type, enabled, created_at, updated_at)
             VALUES ($1, $2, 'wechat', $3, 'wechat-topup', 'one_time', true, NOW(), NOW())",
        )
        .bind(mapping_id)
        .bind(realm_id)
        .bind(external_product_id)
        .execute(&ctx.app_state.pool)
        .await
        .expect("insert wechat one-time mapping");

        sqlx::query(
            "INSERT INTO points_distribution_rules
                (id, realm_id, owner_type, entitlement_mapping_id, bucket_id,
                 trigger_sources, grant_mode, points_amount, validity_days,
                 enabled, display_order)
             VALUES ($1, $2, 'entitlement_mapping', $3, $4, $5, 'fixed', $6, 0, true, 0)",
        )
        .bind(Uuid::now_v7())
        .bind(realm_id)
        .bind(mapping_id)
        .bind(bucket_id)
        .bind(&["topup"][..])
        .bind(points)
        .execute(&ctx.app_state.pool)
        .await
        .expect("seed topup distribution rule");
        mapping_id
    }

    /// Seed a wechat non-renewing subscription mapping (service_duration_days drives
    /// the fixed subscription window).
    async fn create_wechat_non_renewing_mapping(
        ctx: &WechatWebhookContext,
        realm_id: &str,
        external_product_id: &str,
        service_days: i64,
    ) -> Uuid {
        let mapping_id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO provider_entitlement_mappings
                (id, realm_id, payment_provider, external_product_id, entitlement_key,
                 billing_type, service_duration_days, enabled, created_at, updated_at)
             VALUES ($1, $2, 'wechat', $3, 'wechat-pro', 'non_renewing', $4, true, NOW(), NOW())",
        )
        .bind(mapping_id)
        .bind(realm_id)
        .bind(external_product_id)
        .bind(service_days)
        .execute(&ctx.app_state.pool)
        .await
        .expect("insert wechat non-renewing mapping");
        mapping_id
    }

    /// Create a Pending wechat payment attempt bound to `out_trade_no`.
    async fn create_pending_wechat_attempt(
        ctx: &WechatWebhookContext,
        realm_id: &str,
        user_id: Uuid,
        mapping_id: Uuid,
        out_trade_no: &str,
        amount: i64,
        billing_type: &str,
    ) -> Uuid {
        let attempt_id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO payment_attempts
                (id, realm_id, user_id, payment_provider, target_type, target_id,
                 amount, currency, status, provider_reference, expires_at, created_at, updated_at)
             VALUES ($1, $2, $3, 'wechat', 'entitlement_mapping', $4,
                     $5, 'CNY', 'Pending', $6, NOW() + INTERVAL '2 hours', NOW(), NOW())",
        )
        .bind(attempt_id)
        .bind(realm_id)
        .bind(user_id)
        .bind(mapping_id)
        .bind(amount)
        .bind(out_trade_no)
        .execute(&ctx.app_state.pool)
        .await
        .expect("insert pending wechat payment attempt");

        let trigger = if billing_type == "one_time" {
            "topup"
        } else {
            "subscription_initial"
        };
        snapshot_attempt_rules_for_mapping(
            &ctx.app_state.pool,
            attempt_id,
            realm_id,
            mapping_id,
            trigger,
        )
        .await;
        attempt_id
    }

    async fn attempt_status(ctx: &WechatWebhookContext, attempt_id: Uuid) -> Option<String> {
        sqlx::query_scalar::<_, String>("SELECT status FROM payment_attempts WHERE id = $1")
            .bind(attempt_id)
            .fetch_optional(&ctx.app_state.pool)
            .await
            .unwrap_or(None)
    }

    async fn count_wechat_events(ctx: &WechatWebhookContext, realm_id: &str) -> i64 {
        sqlx::query_scalar(
            "SELECT COUNT(*) FROM payment_event
             WHERE payment_provider = 'wechat' AND realm_id = $1",
        )
        .bind(realm_id)
        .fetch_one(&ctx.app_state.pool)
        .await
        .unwrap()
    }

    async fn topup_granted(ctx: &WechatWebhookContext, realm_id: &str, user_id: Uuid) -> i64 {
        sqlx::query_scalar(
            "SELECT COALESCE(SUM(total_topup_granted), 0)::bigint FROM points_wallets
             WHERE realm_id = $1 AND user_id = $2",
        )
        .bind(realm_id)
        .bind(user_id)
        .fetch_one(&ctx.app_state.pool)
        .await
        .unwrap()
    }

    async fn count_wechat_subscriptions(
        ctx: &WechatWebhookContext,
        realm_id: &str,
        user_id: Uuid,
    ) -> i64 {
        sqlx::query_scalar(
            "SELECT COUNT(*) FROM subscription
             WHERE realm_id = $1 AND user_id = $2 AND payment_provider = 'wechat'",
        )
        .bind(realm_id)
        .bind(user_id)
        .fetch_one(&ctx.app_state.pool)
        .await
        .unwrap()
    }

    /// WHY: PRD wechat-support §4.1 requires WeChat config changes and payment
    /// operations to land in audit_events — fulfilments and rejections must be
    /// administrator-visible, not just server logs.
    async fn count_payment_audit_events(
        ctx: &WechatWebhookContext,
        realm_id: &str,
        action: &str,
        result: &str,
    ) -> i64 {
        sqlx::query_scalar(
            "SELECT COUNT(*) FROM audit_events
             WHERE realm_id = $1 AND action = $2 AND result = $3",
        )
        .bind(realm_id)
        .bind(action)
        .bind(result)
        .fetch_one(&ctx.app_state.pool)
        .await
        .unwrap()
    }

    // =========================================================================
    // Tests
    // =========================================================================

    /// User Story: docs/user-stories/billing/wechat-support.md (US-WP-002 / US-WP-004) — a valid SUCCESS callback fulfils a
    /// one-time (points pack) purchase: attempt flips to Succeeded and the
    /// topup grant is recorded.
    #[test_context(WechatWebhookContext)]
    #[tokio::test]
    async fn test_wechat_native_success_fulfills_one_time(ctx: &mut WechatWebhookContext) {
        let realm_id = ctx._realm_id.clone();
        insert_wechat_realm_config(&ctx.app_state.pool, &realm_id).await;

        let user_id = create_test_user(ctx, "wechat-onetime@test.com").await;
        create_points_wallet(ctx, user_id, &realm_id).await;
        let mapping_id = create_wechat_one_time_mapping(ctx, &realm_id, "wx-points-100", 100).await;
        let out_trade_no = "CAS_test_onetime_001";
        let attempt_id = create_pending_wechat_attempt(
            ctx,
            &realm_id,
            user_id,
            mapping_id,
            out_trade_no,
            1000,
            "one_time",
        )
        .await;

        let body = build_notification(
            "evt-onetime-1",
            out_trade_no,
            "wx-txn-onetime-1",
            "SUCCESS",
            1000,
            TEST_V3_KEY,
        );
        let app = ctx.create_unified_test_router();
        let response = app
            .oneshot(wechat_webhook_request(&realm_id, body, true))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let _ = to_bytes(response.into_body(), usize::MAX).await.unwrap();

        assert_eq!(
            attempt_status(ctx, attempt_id).await,
            Some("Succeeded".to_string()),
            "one-time callback must flip attempt to Succeeded"
        );
        assert_eq!(
            topup_granted(ctx, &realm_id, user_id).await,
            100,
            "one-time fulfilment must grant the topup points"
        );
        assert_eq!(count_wechat_events(ctx, &realm_id).await, 1);
        assert!(
            count_payment_audit_events(ctx, &realm_id, "payment.webhook", "success").await >= 1,
            "fulfilled callback must be audit-logged"
        );
    }

    /// User Story: docs/user-stories/billing/wechat-support.md (US-WP-002, scenario 4: NonRenewing subscription) — a valid
    /// SUCCESS callback fulfils a non-renewing subscription: attempt Succeeded
    /// and a fixed-window subscription row is created.
    #[test_context(WechatWebhookContext)]
    #[tokio::test]
    async fn test_wechat_native_success_fulfills_non_renewing(ctx: &mut WechatWebhookContext) {
        let realm_id = ctx._realm_id.clone();
        insert_wechat_realm_config(&ctx.app_state.pool, &realm_id).await;

        let user_id = create_test_user(ctx, "wechat-nr@test.com").await;
        let mapping_id = create_wechat_non_renewing_mapping(ctx, &realm_id, "wx-pro-30d", 30).await;
        let out_trade_no = "CAS_test_nonrenew_001";
        let attempt_id = create_pending_wechat_attempt(
            ctx,
            &realm_id,
            user_id,
            mapping_id,
            out_trade_no,
            3000,
            "non_renewing",
        )
        .await;

        let body = build_notification(
            "evt-nonrenew-1",
            out_trade_no,
            "wx-txn-nonrenew-1",
            "SUCCESS",
            3000,
            TEST_V3_KEY,
        );
        let app = ctx.create_unified_test_router();
        let response = app
            .oneshot(wechat_webhook_request(&realm_id, body, true))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let _ = to_bytes(response.into_body(), usize::MAX).await.unwrap();

        assert_eq!(
            attempt_status(ctx, attempt_id).await,
            Some("Succeeded".to_string()),
            "non-renewing callback must flip attempt to Succeeded"
        );
        assert_eq!(
            count_wechat_subscriptions(ctx, &realm_id, user_id).await,
            1,
            "non-renewing fulfilment must create a subscription row"
        );
    }

    /// User Story: docs/user-stories/billing/wechat-support.md (US-WP-004) — an unsigned / tampered callback must be rejected
    /// (422 FAIL) and must NOT record any payment_event or mutate the attempt.
    #[test_context(WechatWebhookContext)]
    #[tokio::test]
    async fn test_wechat_callback_invalid_signature_rejected(ctx: &mut WechatWebhookContext) {
        let realm_id = ctx._realm_id.clone();
        insert_wechat_realm_config(&ctx.app_state.pool, &realm_id).await;
        let user_id = create_test_user(ctx, "wechat-sig@test.com").await;
        create_points_wallet(ctx, user_id, &realm_id).await;
        let mapping_id = create_wechat_one_time_mapping(ctx, &realm_id, "wx-points-sig", 100).await;
        let _attempt_id = create_pending_wechat_attempt(
            ctx,
            &realm_id,
            user_id,
            mapping_id,
            "CAS_test_sig_001",
            1000,
            "one_time",
        )
        .await;

        let before = count_wechat_events(ctx, &realm_id).await;
        let body = build_notification(
            "evt-sig-bad",
            "CAS_test_sig_001",
            "wx-txn-sig",
            "SUCCESS",
            1000,
            TEST_V3_KEY,
        );
        // `signed = false` → no Wechatpay-Signature/Serial headers.
        let app = ctx.create_unified_test_router();
        let response = app
            .oneshot(wechat_webhook_request(&realm_id, body, false))
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "unsigned callback must be rejected"
        );
        let _ = to_bytes(response.into_body(), usize::MAX).await.unwrap();

        assert_eq!(
            count_wechat_events(ctx, &realm_id).await,
            before,
            "signature failure must NOT write a payment_event"
        );
    }

    /// User Story: docs/user-stories/billing/wechat-support.md (US-WP-004) — a callback whose decrypted amount does not match
    /// the attempt must be rejected and must NOT fulfil the attempt.
    #[test_context(WechatWebhookContext)]
    #[tokio::test]
    async fn test_wechat_callback_amount_mismatch_rejected(ctx: &mut WechatWebhookContext) {
        let realm_id = ctx._realm_id.clone();
        insert_wechat_realm_config(&ctx.app_state.pool, &realm_id).await;
        let user_id = create_test_user(ctx, "wechat-amt@test.com").await;
        create_points_wallet(ctx, user_id, &realm_id).await;
        let mapping_id = create_wechat_one_time_mapping(ctx, &realm_id, "wx-points-amt", 100).await;
        let attempt_id = create_pending_wechat_attempt(
            ctx,
            &realm_id,
            user_id,
            mapping_id,
            "CAS_test_amt_001",
            1000,
            "one_time",
        )
        .await;

        // Callback claims 9999 fen; the attempt is 1000 fen.
        let body = build_notification(
            "evt-amt-mismatch",
            "CAS_test_amt_001",
            "wx-txn-amt",
            "SUCCESS",
            9999,
            TEST_V3_KEY,
        );
        let app = ctx.create_unified_test_router();
        let response = app
            .oneshot(wechat_webhook_request(&realm_id, body, true))
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "amount mismatch must be rejected"
        );
        let _ = to_bytes(response.into_body(), usize::MAX).await.unwrap();

        assert_eq!(
            attempt_status(ctx, attempt_id).await,
            Some("Pending".to_string()),
            "amount mismatch must NOT fulfil the attempt"
        );
        assert_eq!(
            topup_granted(ctx, &realm_id, user_id).await,
            0,
            "amount mismatch must NOT grant points"
        );
        assert!(
            count_payment_audit_events(ctx, &realm_id, "payment.webhook", "failure").await >= 1,
            "amount-mismatch rejection must be audit-logged"
        );
    }

    /// PRD wechat-support §4.1 — "所有 WeChat 配置变更与支付操作必须记录审计日志":
    /// a WeChat config write through the generic configs API must land a
    /// payment_config.update audit row, while a non-payment config write on the
    /// same surface must not (payment credentials are the security-relevant
    /// surface; auditing every config row would bury them in noise).
    #[test_context(WechatWebhookContext)]
    #[tokio::test]
    async fn test_wechat_config_write_is_audited(ctx: &mut WechatWebhookContext) {
        let realm_id = ctx._realm_id.clone();
        let app = ctx.create_unified_test_router();
        let (token, user_id) =
            create_admin_session_with_user(ctx, "wechat-cfg-audit@test.com", 1800).await;
        grant_realm_admin_role(ctx, &user_id).await;

        let put_config = |config_type: &str, config_key: &str, config_value: &str| {
            Request::builder()
                .method("PUT")
                .uri(format!("/api/configs/{realm_id}"))
                .header("content-type", "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::from(
                    json!({
                        "configType": config_type,
                        "configKey": config_key,
                        "configValue": config_value,
                        "isSecret": false,
                        "enabled": true
                    })
                    .to_string(),
                ))
                .unwrap()
        };

        let resp = app
            .clone()
            .oneshot(put_config("wechat", "app_id", "wx-audit-test-app"))
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "wechat config write must succeed"
        );
        let _ = to_bytes(resp.into_body(), usize::MAX).await.unwrap();

        // Negative control on the same surface: a non-payment config write.
        let resp = app
            .clone()
            .oneshot(put_config("registration", "enabled", "true"))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let _ = to_bytes(resp.into_body(), usize::MAX).await.unwrap();

        let audited = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM audit_events
             WHERE realm_id = $1 AND action = 'payment_config.update'
               AND details->>'provider' = 'wechat' AND details->>'config_key' = 'app_id'",
        )
        .bind(&realm_id)
        .fetch_one(&ctx.app_state.pool)
        .await
        .unwrap();
        assert_eq!(
            audited, 1,
            "wechat config write must be audit-logged exactly once"
        );

        let other = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM audit_events
             WHERE realm_id = $1 AND action = 'payment_config.update'
               AND details->>'config_key' = 'enabled'",
        )
        .bind(&realm_id)
        .fetch_one(&ctx.app_state.pool)
        .await
        .unwrap();
        assert_eq!(
            other, 0,
            "non-payment config writes must not be audit-logged"
        );
    }

    /// PRD wechat-support §4.1 — manual compensation replays must be
    /// audit-logged as payment.replay so administrators can distinguish replay
    /// outcomes from live webhook processing. Drives the real
    /// `reprocess_wechat_event` entry (not a mock) on a pending attempt.
    #[test_context(WechatWebhookContext)]
    #[tokio::test]
    async fn test_wechat_compensation_replay_is_audited(ctx: &mut WechatWebhookContext) {
        let realm_id = ctx._realm_id.clone();
        insert_wechat_realm_config(&ctx.app_state.pool, &realm_id).await;

        let user_id = create_test_user(ctx, "wechat-replay@test.com").await;
        create_points_wallet(ctx, user_id, &realm_id).await;
        let mapping_id =
            create_wechat_one_time_mapping(ctx, &realm_id, "wx-points-replay", 100).await;
        let attempt_id = create_pending_wechat_attempt(
            ctx,
            &realm_id,
            user_id,
            mapping_id,
            "CAS_test_replay_001",
            1000,
            "one_time",
        )
        .await;

        let body = build_notification(
            "evt-replay-1",
            "CAS_test_replay_001",
            "wx-txn-replay-1",
            "SUCCESS",
            1000,
            TEST_V3_KEY,
        );
        let payload: serde_json::Value = serde_json::from_str(&body).unwrap();

        let outcome = herald_api_billing::wechat_webhook_handlers::reprocess_wechat_event(
            ctx.app_state.as_ref().clone(),
            realm_id.clone(),
            payload,
            "TRANSACTION.SUCCESS".to_string(),
        )
        .await;
        assert!(outcome.is_ok(), "replay must fulfil the pending attempt");

        assert_eq!(
            attempt_status(ctx, attempt_id).await,
            Some("Succeeded".to_string()),
            "replay must flip the attempt to Succeeded"
        );
        assert_eq!(topup_granted(ctx, &realm_id, user_id).await, 100);
        assert!(
            count_payment_audit_events(ctx, &realm_id, "payment.replay", "success").await >= 1,
            "compensation replay must be audit-logged"
        );
    }

    /// User Story: docs/user-stories/billing/wechat-support.md (US-WP-004) — delivering the same event twice fulfils exactly
    /// once (no double grant).
    #[test_context(WechatWebhookContext)]
    #[tokio::test]
    async fn test_wechat_callback_duplicate_event_no_double_grant(ctx: &mut WechatWebhookContext) {
        let realm_id = ctx._realm_id.clone();
        insert_wechat_realm_config(&ctx.app_state.pool, &realm_id).await;
        let user_id = create_test_user(ctx, "wechat-dup@test.com").await;
        create_points_wallet(ctx, user_id, &realm_id).await;
        let mapping_id = create_wechat_one_time_mapping(ctx, &realm_id, "wx-points-dup", 100).await;
        let out_trade_no = "CAS_test_dup_001";
        create_pending_wechat_attempt(
            ctx,
            &realm_id,
            user_id,
            mapping_id,
            out_trade_no,
            1000,
            "one_time",
        )
        .await;

        let event_id = "evt-dup-1";
        let make_body = || {
            build_notification(
                event_id,
                out_trade_no,
                "wx-txn-dup",
                "SUCCESS",
                1000,
                TEST_V3_KEY,
            )
        };

        for _ in 0..2 {
            let app = ctx.create_unified_test_router();
            let body = make_body();
            let response = app
                .oneshot(wechat_webhook_request(&realm_id, body, true))
                .await
                .unwrap();
            assert!(
                response.status() == StatusCode::OK
                    || response.status() == StatusCode::UNPROCESSABLE_ENTITY,
                "duplicate delivery: first=200 fulfilled, replay must not 5xx"
            );
            let _ = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        }

        assert_eq!(
            topup_granted(ctx, &realm_id, user_id).await,
            100,
            "duplicate event must fulfil exactly once (no double grant)"
        );
        assert_eq!(count_wechat_events(ctx, &realm_id).await, 1);
    }

    /// User Story: docs/user-stories/billing/wechat-support.md (US-WP-003) — a JSAPI purchase without an openid must be
    /// rejected at the API layer (400) before any WeChat v3 order is placed.
    #[test_context(WechatWebhookContext)]
    #[tokio::test]
    async fn test_wechat_jsapi_missing_openid_rejected(ctx: &mut WechatWebhookContext) {
        let realm_id = ctx._realm_id.clone();
        insert_wechat_realm_config(&ctx.app_state.pool, &realm_id).await;
        let token = setup_billing_admin_session(ctx, "wechat-jsapi@test.com").await;
        let mapping_id =
            create_wechat_one_time_mapping(ctx, &realm_id, "wx-points-jsapi", 100).await;

        let app = ctx.create_unified_test_router();
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/bill/{realm_id}/purchase/payment-attempts"))
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        json!({
                            "targetType": "entitlement_mapping",
                            "targetId": mapping_id.to_string(),
                            "paymentProvider": "wechat",
                            "paymentScene": "jsapi"
                            // openid intentionally omitted
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "jsapi purchase without openid must be rejected before placing an order"
        );
    }
}
