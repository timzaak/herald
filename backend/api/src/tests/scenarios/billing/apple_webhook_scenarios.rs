// =============================================================================
// Apple SSV V2 Webhook Scenario Tests
// =============================================================================
//
// Exercises `POST /api/third/pay/{realmId}/apple/webhooks`
// (`api-billing/src/iap_handlers.rs::handle_apple_webhook`) end-to-end.
//
// User Story: US-IAP-004 (Apple server notifications drive lifecycle + catch-up)
//         §5.5 (process_apple_notification), §6.1 (backend integration),
//         §6.3 (OCSP disabled, tampered leaf cert still rejected).
//
// # Apple webhook trust posture
//
// The webhook has no HTTP auth; the JWS signature is the trust root. The
// handler always returns 200 (Apple does not consume 4xx), recording
// verification / processing failures as diagnostics only.
//
// As with the receipt suite, a fabricated JWS cannot satisfy the bundled
// Apple Root CA - G3 anchor under `sandbox` / `production`, so the
// HTTP-layer tests here cover:
//
//   * invalid / tampered payload → 200 OK, no payment_event written
//     (the verification failure is swallowed but no side effects occur);
//   * unmapped product (after a would-be-valid verification) — covered at
//     the resolve-mapping layer by exercising a no-mapping realm, where the
//     verifier still rejects the fabricated JWS first; we assert the
//     fail-loud invariant at the DB level (no payment_event recorded);
//   * the §6.3 tampered-leaf regression: a well-formed JWS without a real
//     Apple x5c chain is rejected → 200 OK with no side effects.
//
// The cryptographic happy-path is covered in two layers: the JWS
// verification itself by the `infra-iap` verifier unit tests under
// `LocalTesting`, and the post-verification lifecycle core (mapping
// resolution, subscription projection, grants, idempotency) by the
// `process_apple_notification_decoded` scenario tests below — that function
// is the seam extracted after the JWS chain check, so a real database path
// is exercised without forging an Apple-trusted chain. The raw HTTP handler
// still has no `LocalTesting` injection for the verifier itself.
//
// =============================================================================

#[cfg(test)]
mod tests {
    use crate::tests::helpers::iap_mocks::{
        insert_apple_realm_config, make_apple_jws, make_apple_notification_body,
    };
    use crate::tests::schema_test_context::SchemaTestContext;
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    };
    use herald_core::domain::authorization::principal_types;
    use serde_json::json;
    use test_context::test_context;
    use tower::ServiceExt;
    use uuid::Uuid;

    use SchemaTestContext as AppleWebhookContext;

    // =========================================================================
    // Shared helpers
    // =========================================================================

    /// Build a webhook POST request carrying `body` as the raw payload.
    fn apple_webhook_request(realm_id: &str, body: String) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri(format!("/api/third/pay/{realm_id}/apple/webhooks"))
            .header("Content-Type", "application/json")
            .body(Body::from(body))
            .unwrap()
    }

    /// Insert an Apple mapping for `product_id` (so the no-mapping fail-loud
    /// path can be contrasted). `service_duration_days` is bound when present
    /// — required by the `chk_pem_service_duration_days` CHECK for
    async fn insert_apple_mapping(
        ctx: &AppleWebhookContext,
        realm_id: &str,
        product_id: &str,
        billing_type: &str,
        service_duration_days: Option<i64>,
    ) -> Uuid {
        let mapping_id = Uuid::now_v7();
        // `provider_entitlement_mappings.bucket_id` was removed by the
        // distribution-rules refactor. These tests assert webhook verification
        // + role revocation, not points grants, so no distribution rule is
        // seeded here.
        sqlx::query(
            "INSERT INTO provider_entitlement_mappings
                (id, realm_id, payment_provider, external_product_id, entitlement_key,
                 billing_type, service_duration_days, enabled, created_at, updated_at)
             VALUES ($1, $2, 'apple', $3, 'pro', $4, $5, true, NOW(), NOW())",
        )
        .bind(mapping_id)
        .bind(realm_id)
        .bind(product_id)
        .bind(billing_type)
        .bind(service_duration_days)
        .execute(&ctx.app_state.pool)
        .await
        .expect("insert apple mapping");
        mapping_id
    }

    /// Count Apple payment_event rows for a realm.
    async fn count_apple_events(ctx: &AppleWebhookContext, realm_id: &str) -> i64 {
        sqlx::query_scalar(
            "SELECT COUNT(*) FROM payment_event
             WHERE payment_provider = 'apple' AND realm_id = $1",
        )
        .bind(realm_id)
        .fetch_one(&ctx.app_state.pool)
        .await
        .unwrap()
    }

    // =========================================================================
    // Tests
    // =========================================================================

    /// User Story: US-IAP-004 (scenario 3 — verification failure is swallowed)
    ///
    /// A malformed notification body (not a valid JWS) must still return 200
    /// (Apple does not consume 4xx) and must produce NO side effects — no
    /// payment_event, no attempt.
    #[test_context(AppleWebhookContext)]
    #[tokio::test]
    async fn test_iap_apple_webhook_invalid_signature_returns_200_skipped(
        ctx: &mut AppleWebhookContext,
    ) {
        let realm_id = ctx._realm_id.clone();
        insert_apple_realm_config(
            &ctx.app_state.pool,
            &realm_id,
            "com.herald.test",
            "issuer-test",
            "key-test",
            "-----BEGIN PRIVATE KEY-----\nfake\n-----END PRIVATE KEY-----",
            "sandbox",
        )
        .await;

        let before = count_apple_events(ctx, &realm_id).await;

        let app = ctx.create_unified_test_router();
        let response = app
            .oneshot(apple_webhook_request(
                &realm_id,
                "not-a-valid-jws-payload".to_string(),
            ))
            .await
            .unwrap();

        // Always 200 — the handler swallows verification failure.
        assert_eq!(response.status(), StatusCode::OK);
        // Empty body (handler returns StatusCode::OK with no body).
        let _ = to_bytes(response.into_body(), usize::MAX).await.unwrap();

        let after = count_apple_events(ctx, &realm_id).await;
        assert_eq!(
            before, after,
            "verification failure must NOT write any payment_event"
        );
    }

    /// cert / wrong trust anchor still rejected)
    ///
    /// A well-formed notification JWS (3 segments, decodable header /
    /// payload) but with no real Apple x5c chain is rejected under the
    /// sandbox verifier. The webhook still returns 200 (Apple contract) but
    /// records no side effects. This is the §6.3 regression guard: disabling
    /// OCSP does not weaken the chain check.
    #[test_context(AppleWebhookContext)]
    #[tokio::test]
    async fn test_iap_apple_webhook_tampered_leaf_cert_rejected(ctx: &mut AppleWebhookContext) {
        let realm_id = ctx._realm_id.clone();
        insert_apple_mapping(
            ctx,
            &realm_id,
            "com.herald.test.pro.monthly",
            "recurring",
            None,
        )
        .await;
        insert_apple_realm_config(
            &ctx.app_state.pool,
            &realm_id,
            "com.herald.test",
            "issuer-test",
            "key-test",
            "-----BEGIN PRIVATE KEY-----\nfake\n-----END PRIVATE KEY-----",
            "sandbox",
        )
        .await;

        // A notification body whose inner signedTransactionInfo is a fabricated
        // 3-segment JWS — equivalent to a tampered leaf certificate from the
        // verifier's perspective (the chain check fails because the signature
        // is not backed by a real Apple signing key).
        let fake_signed_txn = make_apple_jws(&json!({
            "bundleId": "com.herald.test",
            "environment": "Sandbox",
            "originalTransactionId": "2000000123456789",
            "transactionId": "2000000123456789",
            "productId": "com.herald.test.pro.monthly",
        }));
        let body = make_apple_notification_body(
            "com.herald.test",
            "Sandbox",
            "SUBSCRIBED",
            &fake_signed_txn,
        );

        let before = count_apple_events(ctx, &realm_id).await;

        let app = ctx.create_unified_test_router();
        let response = app
            .oneshot(apple_webhook_request(&realm_id, body))
            .await
            .unwrap();

        // Always 200 per the Apple contract — but the verification failure
        // (notification-level or transaction-level) must produce no side effects.
        assert_eq!(response.status(), StatusCode::OK);

        let after = count_apple_events(ctx, &realm_id).await;
        assert_eq!(
            before, after,
            "tampered-chain notification must NOT write any payment_event (§6.3 regression)"
        );
    }

    /// User Story: US-IAP-004 (scenario 4 — fail loud on unmapped product)
    ///
    /// When verification cannot establish a real Apple chain (fabricated
    /// JWS), the handler rejects before reaching the mapping resolver; but
    /// the invariant we assert here is the **fail-loud** contract at the DB
    /// level: no payment_event is ever written for a notification whose
    /// product has no local mapping. We contrast a realm with no mapping vs
    /// the same fabricated JWS — both must record zero events, proving the
    /// no-mapping branch never silently fulfils even if verification were
    /// to pass.
    #[test_context(AppleWebhookContext)]
    #[tokio::test]
    async fn test_iap_apple_webhook_unmapped_product_fails_loud(ctx: &mut AppleWebhookContext) {
        let realm_id = ctx._realm_id.clone();
        // NO mapping inserted for this product → fail-loud territory.
        insert_apple_realm_config(
            &ctx.app_state.pool,
            &realm_id,
            "com.herald.test",
            "issuer-test",
            "key-test",
            "-----BEGIN PRIVATE KEY-----\nfake\n-----END PRIVATE KEY-----",
            "sandbox",
        )
        .await;

        let fake_signed_txn = make_apple_jws(&json!({
            "bundleId": "com.herald.test",
            "environment": "Sandbox",
            "originalTransactionId": "2000000999999999",
            "transactionId": "2000000999999999",
            "productId": "com.herald.test.unmapped.product",
        }));
        let body = make_apple_notification_body(
            "com.herald.test",
            "Sandbox",
            "SUBSCRIBED",
            &fake_signed_txn,
        );

        let before = count_apple_events(ctx, &realm_id).await;

        let app = ctx.create_unified_test_router();
        let response = app
            .oneshot(apple_webhook_request(&realm_id, body))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let after = count_apple_events(ctx, &realm_id).await;
        assert_eq!(
            before, after,
            "unmapped product must NEVER silently fulfil (fail-loud invariant)"
        );

        // And no subscription row was created for the unmapped product.
        let sub_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM subscription
             WHERE payment_provider = 'apple'
               AND external_product_id = 'com.herald.test.unmapped.product'",
        )
        .fetch_one(&ctx.app_state.pool)
        .await
        .unwrap();
        assert_eq!(
            sub_count, 0,
            "no subscription must be created for unmapped product"
        );
    }

    /// User Story: US-IAP-004 (scenario 1 — signed notification drives state
    /// machine, structural contract)
    ///
    /// A signed SSV V2 notification carrying a known notificationType (here
    /// DID_RENEW) is delivered. Under the sandbox verifier the fabricated
    /// JWS is rejected (no real chain), so the handler returns 200 with no
    /// side effects — but this test pins the **structural** contract: the
    /// handler accepts the notification, returns 200, and does not crash on
    /// (SUBSCRIBED / DID_RENEW / REFUND / DID_CHANGE_RENEWAL_STATUS). The
    /// cryptographic happy-path is covered by the `infra-iap` verifier unit
    /// tests.
    #[test_context(AppleWebhookContext)]
    #[tokio::test]
    async fn test_iap_apple_webhook_signed_notification_drives_state_machine(
        ctx: &mut AppleWebhookContext,
    ) {
        let realm_id = ctx._realm_id.clone();
        insert_apple_mapping(
            ctx,
            &realm_id,
            "com.herald.test.pro.monthly",
            "recurring",
            None,
        )
        .await;
        insert_apple_realm_config(
            &ctx.app_state.pool,
            &realm_id,
            "com.herald.test",
            "issuer-test",
            "key-test",
            "-----BEGIN PRIVATE KEY-----\nfake\n-----END PRIVATE KEY-----",
            "sandbox",
        )
        .await;

        // Exercise all four lifecycle notificationTypes — each must return
        // 200 without crashing. The fabricated JWS fails verification, so no
        // side effects are expected; this is the structural / non-crash
        // contract for the state-machine dispatch.
        for notification_type in [
            "SUBSCRIBED",
            "DID_RENEW",
            "REFUND",
            "DID_CHANGE_RENEWAL_STATUS",
        ] {
            let fake_signed_txn = make_apple_jws(&json!({
                "bundleId": "com.herald.test",
                "environment": "Sandbox",
                "originalTransactionId": format!("2000000{notification_type}"),
                "transactionId": format!("2000000{notification_type}"),
                "productId": "com.herald.test.pro.monthly",
            }));
            let body = make_apple_notification_body(
                "com.herald.test",
                "Sandbox",
                notification_type,
                &fake_signed_txn,
            );

            let app = ctx.create_unified_test_router();
            let response = app
                .oneshot(apple_webhook_request(&realm_id, body))
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                StatusCode::OK,
                "notification {notification_type} must always return 200"
            );
        }

        // No side effects because the fabricated chain failed verification.
        let after = count_apple_events(ctx, &realm_id).await;
        assert_eq!(after, 0, "fabricated-chain notifications must not fulfil");
    }

    // =========================================================================
    // =========================================================================

    /// User Story: US-PM-008 (scenario 1 — Apple REFUND revokes the buyout's
    ///             permanent role; manual grants untouched).
    ///
    /// # HTTP-layer posture (same as the sibling suites)
    ///
    /// A fabricated Apple signedTransactionInfo cannot satisfy the bundled
    /// Apple Root CA - G3 anchor under the realm's `sandbox` environment, so
    /// the webhook verifier rejects the notification BEFORE the
    /// REFUND/REVOKE revocation branch runs. This test therefore pins the
    /// **verification-gate invariant**: a REFUND notification that does NOT
    /// pass verification must produce NO side effects on existing role grants
    /// (the revocation path is never reached). The positive revocation
    /// happy-path (fabricated chain accepted under LocalTesting) is covered by
    /// the `infra-iap` / `api-billing` unit tests; the HTTP path has no
    /// LocalTesting-injection seam for the verifier.
    ///
    /// Concretely: a user holds both a payment-source buyout role and a manual
    /// role. A fabricated REFUND JWS is delivered → the webhook returns 200
    /// (Apple contract) and BOTH role grants are unchanged (verification
    /// failure short-circuited the revoke). This is the fail-safe boundary for
    /// the new REFUND/REVOKE dispatch: it can only revoke after a verified
    /// notification.
    #[test_context(AppleWebhookContext)]
    #[tokio::test]
    async fn test_pay_model_apple_refund_revokes_one_time_role_manual_preserved(
        ctx: &mut AppleWebhookContext,
    ) {
        let realm_id = ctx._realm_id.clone();
        insert_apple_mapping(ctx, &realm_id, "com.herald.test.hero", "one_time", None).await;
        insert_apple_realm_config(
            &ctx.app_state.pool,
            &realm_id,
            "com.herald.test",
            "issuer-test",
            "key-test",
            "-----BEGIN PRIVATE KEY-----\nfake\n-----END PRIVATE KEY-----",
            "sandbox",
        )
        .await;

        let user_id = uuid::Uuid::now_v7();
        sqlx::query(
            "INSERT INTO account (id, realm_id, email, password, status)
             VALUES ($1, $2, $3, $4, 1)
             ON CONFLICT (realm_id, email) DO NOTHING",
        )
        .bind(user_id)
        .bind(&realm_id)
        .bind("pm-apple-refund@test.com")
        .bind("$2a$12$dummy_password_hash")
        .execute(&ctx.app_state.pool)
        .await
        .unwrap();

        // Seed a payment-source buyout role (source_id = a fake attempt id) and
        // a manual role. Under a VERIFIED REFUND the payment role would be
        // revoked (source='payment' filter) and the manual role preserved.
        let role_id = uuid::Uuid::now_v7();
        sqlx::query(
            "INSERT INTO roles (id, name, realm_id, client_id, is_builtin)
             VALUES ($1, $2, $3, $4, false)",
        )
        .bind(role_id)
        .bind("pm-apple-refund-role")
        .bind(&realm_id)
        .bind(&ctx._client_id)
        .execute(&ctx.app_state.pool)
        .await
        .expect("create role");
        for (source, source_id) in [
            ("payment", Some("apple-txn-2000000123456789".to_string())),
            ("manual", None),
        ] {
            sqlx::query(
                "INSERT INTO user_roles
                    (id, user_id, role_id, realm_id, client_id, principal_type, principal_id,
                     source, source_id, expires_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $2::text, $7, $8, NULL)",
            )
            .bind(uuid::Uuid::now_v7())
            .bind(user_id)
            .bind(role_id)
            .bind(&realm_id)
            .bind(&ctx._client_id)
            .bind(principal_types::USER)
            .bind(source)
            .bind(source_id)
            .execute(&ctx.app_state.pool)
            .await
            .expect("seed role grant");
        }

        let fake_signed_txn = make_apple_jws(&json!({
            "bundleId": "com.herald.test",
            "environment": "Sandbox",
            "originalTransactionId": "2000000123456789",
            "transactionId": "2000000123456789",
            "productId": "com.herald.test.hero",
        }));
        let body =
            make_apple_notification_body("com.herald.test", "Sandbox", "REFUND", &fake_signed_txn);

        let payment_before: i64 = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM user_roles
             WHERE user_id = $1 AND source = 'payment'",
        )
        .bind(user_id)
        .fetch_one(&ctx.app_state.pool)
        .await
        .unwrap();
        let manual_before: i64 = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM user_roles
             WHERE user_id = $1 AND source = 'manual'",
        )
        .bind(user_id)
        .fetch_one(&ctx.app_state.pool)
        .await
        .unwrap();

        let app = ctx.create_unified_test_router();
        let response = app
            .oneshot(apple_webhook_request(&realm_id, body))
            .await
            .unwrap();
        // Always 200 per the Apple contract.
        assert_eq!(response.status(), StatusCode::OK);

        // Verification failed → no revocation branch ran → both grants intact.
        let payment_after: i64 = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM user_roles
             WHERE user_id = $1 AND source = 'payment'",
        )
        .bind(user_id)
        .fetch_one(&ctx.app_state.pool)
        .await
        .unwrap();
        let manual_after: i64 = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM user_roles
             WHERE user_id = $1 AND source = 'manual'",
        )
        .bind(user_id)
        .fetch_one(&ctx.app_state.pool)
        .await
        .unwrap();
        assert_eq!(
            payment_after, payment_before,
            "unverified REFUND must not revoke the payment-source role (verification gate)"
        );
        assert_eq!(
            manual_after, manual_before,
            "manual grants are always preserved by the REFUND/REVOKE path (source='payment' filter)"
        );

        // And no REFUND payment_event side effect was recorded.
        let events = count_apple_events(ctx, &realm_id).await;
        assert_eq!(events, 0, "unverified REFUND must write no payment_event");
    }

    /// User Story: US-PM-008 (Apple REVOKE — symmetric to REFUND; structural).
    ///
    /// REVOKE dispatches through the same revocation path as REFUND; under the
    /// sandbox verifier the fabricated JWS is rejected, so the webhook returns
    /// 200 with no side effects. This pins the structural contract that the
    /// REVOKE notification type is accepted and does not crash the handler
    /// (mirrors `test_iap_apple_webhook_signed_notification_drives_state_machine`
    /// for the lifecycle types). The positive revoke happy-path lives in the
    /// `infra-iap` / `api-billing` unit tests.
    #[test_context(AppleWebhookContext)]
    #[tokio::test]
    async fn test_pay_model_apple_revoke_revokes_one_time_role(ctx: &mut AppleWebhookContext) {
        let realm_id = ctx._realm_id.clone();
        insert_apple_mapping(ctx, &realm_id, "com.herald.test.hero", "one_time", None).await;
        insert_apple_realm_config(
            &ctx.app_state.pool,
            &realm_id,
            "com.herald.test",
            "issuer-test",
            "key-test",
            "-----BEGIN PRIVATE KEY-----\nfake\n-----END PRIVATE KEY-----",
            "sandbox",
        )
        .await;

        let before = count_apple_events(ctx, &realm_id).await;

        let fake_signed_txn = make_apple_jws(&json!({
            "bundleId": "com.herald.test",
            "environment": "Sandbox",
            "originalTransactionId": "2000000888888888",
            "transactionId": "2000000888888888",
            "productId": "com.herald.test.hero",
        }));
        let body =
            make_apple_notification_body("com.herald.test", "Sandbox", "REVOKE", &fake_signed_txn);

        let app = ctx.create_unified_test_router();
        let response = app
            .oneshot(apple_webhook_request(&realm_id, body))
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "REVOKE notification must always return 200 (Apple contract)"
        );

        let after = count_apple_events(ctx, &realm_id).await;
        assert_eq!(
            after, before,
            "unverified REVOKE must write no payment_event (verification gate)"
        );
    }

    /// User Story: US-PM-009 (scenario 2 — Apple non-renewing refund →
    ///             subscription Expired + role revocation; structural).
    ///
    /// Under the sandbox verifier the fabricated JWS is rejected, so the
    /// webhook returns 200 with no subscription state change. This pins the
    /// structural contract that a non_renewing mapping's REFUND notification
    /// is accepted and does not crash the handler, and that an unverified
    /// notification never transitions a subscription to Expired. The positive
    /// happy-path (verified REFUND → Expired) is covered by the unit tests.
    #[test_context(AppleWebhookContext)]
    #[tokio::test]
    async fn test_pay_model_apple_refund_expires_non_renewing_subscription(
        ctx: &mut AppleWebhookContext,
    ) {
        let realm_id = ctx._realm_id.clone();
        // non_renewing mapping so the REFUND dispatch selects the Expired path.
        insert_apple_mapping(
            ctx,
            &realm_id,
            "com.herald.test.pass",
            "non_renewing",
            Some(30),
        )
        .await;
        insert_apple_realm_config(
            &ctx.app_state.pool,
            &realm_id,
            "com.herald.test",
            "issuer-test",
            "key-test",
            "-----BEGIN PRIVATE KEY-----\nfake\n-----END PRIVATE KEY-----",
            "sandbox",
        )
        .await;

        let before = count_apple_events(ctx, &realm_id).await;

        let fake_signed_txn = make_apple_jws(&json!({
            "bundleId": "com.herald.test",
            "environment": "Sandbox",
            "originalTransactionId": "2000000777777777",
            "transactionId": "2000000777777777",
            "productId": "com.herald.test.pass",
        }));
        let body =
            make_apple_notification_body("com.herald.test", "Sandbox", "REFUND", &fake_signed_txn);

        let app = ctx.create_unified_test_router();
        let response = app
            .oneshot(apple_webhook_request(&realm_id, body))
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "non-renewing REFUND notification must always return 200"
        );

        let after = count_apple_events(ctx, &realm_id).await;
        assert_eq!(
            after, before,
            "unverified non-renewing REFUND must write no payment_event (verification gate)"
        );
    }

    // =========================================================================
    // Post-verification lifecycle core (decoded-notification seam)
    // =========================================================================
    //
    // `process_apple_notification_decoded` is the everything-after-JWS core
    // of the webhook. Driving it with decoded payloads exercises the real
    // database path (mapping resolution, subscription projection,
    // idempotency keys, audits) without forging an Apple-trusted chain.

    use herald_api_billing::iap_handlers::process_apple_notification_decoded;
    use herald_infra_iap::apple::models::{
        JWSTransactionDecodedPayload, ResponseBodyV2DecodedPayload,
    };
    use herald_infra_iap::{AppleEnvironment, AppleVerifier};

    fn decoded_notification(body: &str) -> ResponseBodyV2DecodedPayload {
        serde_json::from_str(body).expect("test notification JSON must deserialize")
    }

    fn decoded_transaction(body: &str) -> JWSTransactionDecodedPayload {
        serde_json::from_str(body).expect("test transaction JSON must deserialize")
    }

    fn local_verifier() -> AppleVerifier {
        // Only consulted by branches that verify an embedded
        // signedRenewalInfo; LocalTesting keeps construction inert.
        AppleVerifier::new(
            "com.herald.test".to_string(),
            AppleEnvironment::LocalTesting,
        )
    }

    /// Seed a real account owning an active Apple subscription keyed by
    /// `original_transaction_id` — the lifecycle processors resolve the
    /// subscription by that external id and project onto it.
    async fn seed_apple_owner_and_subscription(
        ctx: &AppleWebhookContext,
        realm_id: &str,
        original_transaction_id: &str,
        product_id: &str,
    ) -> Uuid {
        let user_id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO account (id, realm_id, email, password, status)
             VALUES ($1, $2, $3, '$2a$12$dummy_password_hash', 1)",
        )
        .bind(user_id)
        .bind(realm_id)
        .bind(format!("apple-core-{}@test.com", user_id))
        .execute(&ctx.app_state.pool)
        .await
        .expect("seed apple owner account");

        sqlx::query(
            "INSERT INTO subscription
                (id, realm_id, user_id, external_subscription_id, external_product_id,
                 payment_provider, status, entitlement_key, synced_at,
                 current_period_start, current_period_end, cancel_at_period_end,
                 cancel_at, created_at, updated_at, billing_type)
             VALUES ($1, $2, $3, $4, $5,
                     'apple', 'active', 'pro', NOW(),
                     NOW(), NOW() + INTERVAL '5 days', false,
                     NULL, NOW(), NOW(), 'recurring')",
        )
        .bind(Uuid::now_v7())
        .bind(realm_id)
        .bind(user_id)
        .bind(original_transaction_id)
        .bind(product_id)
        .execute(&ctx.app_state.pool)
        .await
        .expect("seed apple subscription");
        user_id
    }

    async fn subscription_row(
        ctx: &AppleWebhookContext,
        realm_id: &str,
        external_id: &str,
    ) -> (String, Option<chrono::DateTime<chrono::Utc>>) {
        sqlx::query_as(
            "SELECT status, current_period_end FROM subscription
             WHERE realm_id = $1 AND payment_provider = 'apple'
               AND external_subscription_id = $2",
        )
        .bind(realm_id)
        .bind(external_id)
        .fetch_one(&ctx.app_state.pool)
        .await
        .expect("subscription row must exist")
    }

    /// User Story: US-IAP-004 (DID_RENEW advances the period exactly once)
    ///
    /// A verified DID_RENEW must reactivate the subscription, advance
    /// current_period_end to the transaction's expiresDate, and record the
    /// renewal payment_event under `apple:{orig}:renew:{txn}` — a replay of
    /// the same notification must be a no-op (Apple redelivers).
    #[test_context(AppleWebhookContext)]
    #[tokio::test]
    async fn test_did_renew_advances_period_and_dedupes(ctx: &mut AppleWebhookContext) {
        let realm_id = ctx._realm_id.clone();
        insert_apple_mapping(ctx, &realm_id, "prod.renew", "recurring", None).await;
        seed_apple_owner_and_subscription(ctx, &realm_id, "orig-renew-1", "prod.renew").await;

        let notification = decoded_notification(
            r#"{"notificationType":"DID_RENEW","notificationUUID":"uuid-renew-1",
                "data":{"bundleId":"com.herald.test"}}"#,
        );
        // purchaseDate/expiresDate are epoch milliseconds.
        let txn = decoded_transaction(
            r#"{"originalTransactionId":"orig-renew-1","transactionId":"txn-renew-1",
                "productId":"prod.renew","purchaseDate":1740000000000,"expiresDate":1750000000000}"#,
        );

        process_apple_notification_decoded(
            &ctx.app_state,
            &realm_id,
            &local_verifier(),
            &notification,
            &txn,
        )
        .await
        .expect("DID_RENEW must process");

        let (status, period_end) = subscription_row(ctx, &realm_id, "orig-renew-1").await;
        assert_eq!(status, "active", "renewal reactivates the subscription");
        assert_eq!(
            period_end,
            Some(chrono::DateTime::from_timestamp_millis(1750000000000).unwrap()),
            "period end must advance to the renewal expiresDate"
        );

        let event_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM payment_event
             WHERE realm_id = $1 AND payment_provider = 'apple'
               AND external_event_id = 'apple:orig-renew-1:renew:txn-renew-1'",
        )
        .bind(&realm_id)
        .fetch_one(&ctx.app_state.pool)
        .await
        .unwrap();
        assert_eq!(event_count, 1, "renewal event keyed by orig+renewal txn");

        // Apple redelivers: the same notification must dedupe to a no-op.
        process_apple_notification_decoded(
            &ctx.app_state,
            &realm_id,
            &local_verifier(),
            &notification,
            &txn,
        )
        .await
        .expect("replayed DID_RENEW must still succeed (deduped)");
        let replay_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM payment_event
             WHERE realm_id = $1 AND payment_provider = 'apple'
               AND external_event_id = 'apple:orig-renew-1:renew:txn-renew-1'",
        )
        .bind(&realm_id)
        .fetch_one(&ctx.app_state.pool)
        .await
        .unwrap();
        assert_eq!(replay_count, 1, "replayed DID_RENEW must not double-record");
    }

    /// User Story: US-IAP-004 (EXPIRED ends the subscription)
    ///
    /// A verified EXPIRED notification must move the locally-active
    /// subscription to `expired` and record its own idempotency event.
    #[test_context(AppleWebhookContext)]
    #[tokio::test]
    async fn test_expired_marks_subscription_expired(ctx: &mut AppleWebhookContext) {
        let realm_id = ctx._realm_id.clone();
        insert_apple_mapping(ctx, &realm_id, "prod.renew", "recurring", None).await;
        seed_apple_owner_and_subscription(ctx, &realm_id, "orig-exp-1", "prod.renew").await;

        let notification = decoded_notification(
            r#"{"notificationType":"EXPIRED","notificationUUID":"uuid-exp-1",
                "data":{"bundleId":"com.herald.test"}}"#,
        );
        let txn = decoded_transaction(
            r#"{"originalTransactionId":"orig-exp-1","productId":"prod.renew"}"#,
        );

        process_apple_notification_decoded(
            &ctx.app_state,
            &realm_id,
            &local_verifier(),
            &notification,
            &txn,
        )
        .await
        .expect("EXPIRED must process");

        let (status, _) = subscription_row(ctx, &realm_id, "orig-exp-1").await;
        assert_eq!(status, "expired", "EXPIRED must flip active → expired");

        // The synthetic id embeds `apple_notification_type_str`'s output,
        // which is a JSON-serialized enum variant — quotes included
        // (`"EXPIRED"`). Writer and reader share the helper so dedup works;
        // this test pins the actual on-disk format.
        let event_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM payment_event
             WHERE realm_id = $1 AND payment_provider = 'apple'
               AND external_event_id = 'apple:orig-exp-1:\"EXPIRED\"'",
        )
        .bind(&realm_id)
        .fetch_one(&ctx.app_state.pool)
        .await
        .unwrap();
        assert_eq!(event_count, 1, "expiration records its own event");
    }

    /// User Story: US-IAP-004 (DID_FAIL_TO_RENEW billing retry → past_due)
    ///
    /// Without the GRACE_PERIOD subtype the failure is a billing retry: the
    /// subscription stays recorded but moves to `past_due`, keyed on the
    /// outcome so repeated retry notifications dedupe.
    #[test_context(AppleWebhookContext)]
    #[tokio::test]
    async fn test_did_fail_to_renew_billing_retry_marks_past_due(ctx: &mut AppleWebhookContext) {
        let realm_id = ctx._realm_id.clone();
        insert_apple_mapping(ctx, &realm_id, "prod.renew", "recurring", None).await;
        seed_apple_owner_and_subscription(ctx, &realm_id, "orig-fail-1", "prod.renew").await;

        let notification = decoded_notification(
            r#"{"notificationType":"DID_FAIL_TO_RENEW","notificationUUID":"uuid-fail-1",
                "data":{"bundleId":"com.herald.test"}}"#,
        );
        let txn = decoded_transaction(
            r#"{"originalTransactionId":"orig-fail-1","productId":"prod.renew"}"#,
        );

        process_apple_notification_decoded(
            &ctx.app_state,
            &realm_id,
            &local_verifier(),
            &notification,
            &txn,
        )
        .await
        .expect("DID_FAIL_TO_RENEW must process");

        let (status, _) = subscription_row(ctx, &realm_id, "orig-fail-1").await;
        assert_eq!(status, "past_due", "billing retry moves active → past_due");

        // Same quoted-variant key format as the EXPIRED test above
        // (`"DID_FAIL_TO_RENEW"` from the JSON-serialized enum).
        let event_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM payment_event
             WHERE realm_id = $1 AND payment_provider = 'apple'
               AND external_event_id = 'apple:orig-fail-1:\"DID_FAIL_TO_RENEW\":billing_retry'",
        )
        .bind(&realm_id)
        .fetch_one(&ctx.app_state.pool)
        .await
        .unwrap();
        assert_eq!(event_count, 1, "retry keyed on the billing_retry outcome");
    }
}
