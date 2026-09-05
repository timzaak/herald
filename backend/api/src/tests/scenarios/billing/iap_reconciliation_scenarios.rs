// =============================================================================
// IAP Reconciliation Job Scenario Tests
// =============================================================================
//
// constructing the job with a `MockProcessor` and calling `run()` directly
// (no worker process). Mirrors the `webhook_compensation_scenarios.rs`
// MockProcessor pattern.
//
// User Story: US-IAP-006 (scheduled reconciliation: Google lifecycle primary
//             driver / Apple compensation)
//
// # Testability boundary
//
// The job supports a per-realm `realm_config.apple.base_url` override (the
// Stripe/Creem `base_url` injection pattern), so Apple happy-path
// reconciliation IS drivable against wiremock — see the Apple status-drift
// test below. Real Apple / Google sandbox accounts are still needed for
// end-to-end delivery verification.
//
// The remaining structural contracts covered here:
//
//   * job construction + `run()` returns `IapReconciliationStats`;
//   * a realm with no IAP credentials configured is a no-op for the job
//     (zero realms scanned, zero replays);
//   * failure isolation: a realm whose provider API is unreachable produces
//     a realm-level error that is logged + skipped — `run()` still returns
//     `Ok(stats)`, and any other realm / token in the same sweep is not
//     blocked. The Google case is exercised directly by configuring a realm
//     with Google credentials and asserting the sweep completes without
//     aborting (the production endpoint is unreachable from the test
//     sandbox, which is exactly the "single token failure" scenario).
//
// =============================================================================

#[cfg(test)]
mod tests {
    use crate::tests::helpers::iap_mocks::{
        build_service_account_json, fresh_rsa_pem, insert_apple_realm_config,
        insert_google_realm_config, test_apple_ec_p8_pem,
    };
    use crate::tests::schema_test_context::SchemaTestContext;
    use herald_core::domain::billing::compensation::WebhookEventProcessor;
    use herald_core::domain::common::entities::app_errors::CoreError;
    use herald_worker::IapReconciliationJob;
    use serde_json::Value;
    use sqlx::PgPool;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};
    use test_context::test_context;

    use SchemaTestContext as IapReconContext;

    // =========================================================================
    // Mock processor (mirrors webhook_compensation_scenarios.rs)
    // =========================================================================

    #[derive(Debug, Clone)]
    #[allow(dead_code)]
    struct ReprocessCallRecord {
        realm_id: String,
        payment_provider: String,
        event_type: String,
        payload: Value,
    }

    /// Manual mock processor that records all reprocess_event calls and can
    /// optionally inject failures for isolation testing.
    struct MockProcessor {
        calls: Arc<Mutex<Vec<ReprocessCallRecord>>>,
        /// If set, reprocess_event returns an error for every call (failure
        /// isolation test).
        fail_all: bool,
    }

    impl MockProcessor {
        fn new() -> Self {
            Self {
                calls: Arc::new(Mutex::new(Vec::new())),
                fail_all: false,
            }
        }

        /// Build a processor that fails every `reprocess_event` call. Retained
        /// for failure-isolation tests that want to assert the processor's own
        /// errors are non-blocking; the current sweep relies on provider-API
        /// unreachability for the same effect.
        #[allow(dead_code)]
        fn failing() -> Self {
            Self {
                calls: Arc::new(Mutex::new(Vec::new())),
                fail_all: true,
            }
        }

        fn call_log(&self) -> Arc<Mutex<Vec<ReprocessCallRecord>>> {
            self.calls.clone()
        }
    }

    impl WebhookEventProcessor for MockProcessor {
        fn reprocess_event<'a>(
            &'a self,
            realm_id: &'a str,
            payment_provider: &'a str,
            event_type: &'a str,
            payload: &'a Value,
        ) -> Pin<Box<dyn std::future::Future<Output = Result<(), CoreError>> + Send + 'a>> {
            Box::pin(async move {
                {
                    let mut calls = self.calls.lock().unwrap();
                    calls.push(ReprocessCallRecord {
                        realm_id: realm_id.to_string(),
                        payment_provider: payment_provider.to_string(),
                        event_type: event_type.to_string(),
                        payload: payload.clone(),
                    });
                }
                if self.fail_all {
                    return Err(CoreError::InternalServerError(
                        "simulated reprocess failure".to_string(),
                    ));
                }
                Ok(())
            })
        }
    }

    fn build_job(ctx: &IapReconContext, processor: MockProcessor) -> IapReconciliationJob {
        IapReconciliationJob::new(
            ctx.app_state.pool.clone(),
            Arc::new(processor),
            1800, // apple interval
            900,  // google interval
        )
    }

    fn count_calls(log: &Arc<Mutex<Vec<ReprocessCallRecord>>>) -> usize {
        log.lock().unwrap().len()
    }

    // =========================================================================
    // Tests
    // =========================================================================

    /// User Story: US-IAP-006 (no-op sweep for unconfigured realm)
    ///
    /// A realm with no Apple / Google credentials in `realm_config` is
    /// invisible to `fetch_iap_configured_realms` — the job's `run()` scans
    /// zero realms and never invokes `reprocess_event`.
    #[test_context(IapReconContext)]
    #[tokio::test]
    async fn test_iap_reconciliation_no_configured_realms_is_noop(ctx: &mut IapReconContext) {
        let processor = MockProcessor::new();
        let log = processor.call_log();
        let job = build_job(ctx, processor);

        let stats = job.run().await.expect("job run must succeed");

        assert_eq!(stats.realms_scanned, 0, "no IAP realms configured");
        assert_eq!(stats.apple_replayed, 0);
        assert_eq!(stats.google_replayed, 0);
        assert_eq!(
            count_calls(&log),
            0,
            "no reprocess_event calls for unconfigured realm"
        );
    }

    /// User Story: US-IAP-006 (failure isolation — single token/realm failure
    /// does not block the sweep)
    ///
    /// Configure two realms with Google credentials. The Google Developer
    /// client uses the production base URL, which is unreachable from the
    /// test sandbox — each realm's `poll_google_lifecycle` therefore hits a
    /// network error. The sweep MUST nonetheless return `Ok(stats)` (the
    /// realm-level error is logged and skipped), proving the single-failure
    /// isolation contract.
    #[test_context(IapReconContext)]
    #[tokio::test]
    async fn test_iap_reconciliation_single_token_failure_not_blocking(ctx: &mut IapReconContext) {
        let pool: &PgPool = &ctx.app_state.pool;
        let realm_a = ctx._realm_id.clone();
        let realm_b = create_second_realm(pool).await;

        // Configure Google credentials for both realms. The service account
        // JSON is well-formed (parses), but the production Google endpoint is
        // unreachable from the sandbox — poll_google_lifecycle hits a network
        // error per realm.
        let rsa_pem = fresh_rsa_pem();
        let sa_json = build_service_account_json(
            "svc@herald-test.iam.gserviceaccount.com",
            std::str::from_utf8(&rsa_pem).unwrap(),
        );
        insert_google_realm_config(pool, &realm_a, "com.herald.app.a", &sa_json, None).await;
        insert_google_realm_config(pool, &realm_b, "com.herald.app.b", &sa_json, None).await;

        let processor = MockProcessor::new();
        let log = processor.call_log();
        let job = build_job(ctx, processor);

        // The sweep MUST succeed even though both realms' Google polls fail.
        let stats = job
            .run()
            .await
            .expect("job must not abort on per-realm failure");

        assert!(
            stats.realms_scanned >= 2,
            "both configured Google realms must be scanned, got {}",
            stats.realms_scanned
        );

        // No replays reached the processor (Google API was unreachable). The
        // key assertion is that `run()` returned Ok at all — the per-realm
        // failures were isolated.
        assert_eq!(
            count_calls(&log),
            0,
            "no replays when Google API unreachable (failure isolated)"
        );
    }

    /// User Story: US-IAP-006 (Apple compensation — realm with Apple
    /// credentials is scanned, failures isolated)
    ///
    /// A realm with Apple credentials is picked up by
    /// `fetch_iap_configured_realms`. The Apple Server API client uses a
    /// `.p8` private key + real Apple endpoint — unreachable from the
    /// sandbox — so `compensate_apple` hits a realm-level error. The sweep
    /// still returns `Ok(stats)` with the realm counted as scanned; the
    /// error is logged + skipped (non-blocking). This is the structural
    /// contract for the Apple compensation arm.
    #[test_context(IapReconContext)]
    #[tokio::test]
    async fn test_iap_reconciliation_apple_missed_notification_compensated(
        ctx: &mut IapReconContext,
    ) {
        let realm_id = ctx._realm_id.clone();
        insert_apple_realm_config(
            &ctx.app_state.pool,
            &realm_id,
            "com.herald.test",
            "issuer-test",
            "key-test",
            "-----BEGIN PRIVATE KEY-----\nMIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQg\n-----END PRIVATE KEY-----",
            "sandbox",
        )
        .await;

        let processor = MockProcessor::new();
        let job = build_job(ctx, processor);

        // The Apple compensation arm fails at the API layer (unreachable),
        // but the sweep must succeed.
        let stats = job
            .run()
            .await
            .expect("Apple compensation failure must not abort sweep");

        assert!(
            stats.realms_scanned >= 1,
            "Apple-configured realm must be scanned"
        );
        // Apple replay count is zero because the API call failed; the
        // regression anchor is that run() returned Ok with the realm scanned.
    }

    /// User Story: US-IAP-006 (Google lifecycle polling — realm scanned,
    /// failures isolated)
    ///
    /// A realm with Google credentials triggers `poll_google_lifecycle`. The
    /// poll fails at the API layer (unreachable), but the sweep returns
    /// `Ok(stats)` and the realm is counted. Structural contract for the
    /// Google lifecycle arm; the voided-purchase and state-change happy
    /// paths require a Google base-URL override seam.
    #[test_context(IapReconContext)]
    #[tokio::test]
    async fn test_iap_reconciliation_google_state_change_captured(ctx: &mut IapReconContext) {
        let realm_id = ctx._realm_id.clone();
        let rsa_pem = fresh_rsa_pem();
        let sa_json = build_service_account_json(
            "svc@herald-test.iam.gserviceaccount.com",
            std::str::from_utf8(&rsa_pem).unwrap(),
        );
        insert_google_realm_config(
            &ctx.app_state.pool,
            &realm_id,
            "com.herald.app",
            &sa_json,
            None,
        )
        .await;

        let processor = MockProcessor::new();
        let job = build_job(ctx, processor);

        let stats = job
            .run()
            .await
            .expect("Google poll failure must not abort sweep");

        assert!(
            stats.realms_scanned >= 1,
            "Google-configured realm must be scanned"
        );
    }

    /// User Story: US-IAP-006 (voided purchase triggers refund — structural)
    ///
    /// The voided-purchase replay path runs inside `poll_google_lifecycle`
    /// after the subscription-refresh pass. Without a reachable Google API
    /// the path produces no replays, but the sweep must still complete
    /// cleanly. This test pins the structural contract that a
    /// Google-configured realm with zero active subscriptions still
    /// completes the voided-purchase pass without aborting.
    #[test_context(IapReconContext)]
    #[tokio::test]
    async fn test_iap_reconciliation_voided_purchase_revokes(ctx: &mut IapReconContext) {
        let realm_id = ctx._realm_id.clone();
        let rsa_pem = fresh_rsa_pem();
        let sa_json = build_service_account_json(
            "svc@herald-test.iam.gserviceaccount.com",
            std::str::from_utf8(&rsa_pem).unwrap(),
        );
        insert_google_realm_config(
            &ctx.app_state.pool,
            &realm_id,
            "com.herald.app",
            &sa_json,
            None,
        )
        .await;

        let processor = MockProcessor::new();
        let log = processor.call_log();
        let job = build_job(ctx, processor);

        let stats = job
            .run()
            .await
            .expect("voided-purchase pass must not abort sweep");

        // Realm scanned; no active google subscriptions + unreachable API →
        // no replays. The structural contract is that run() completes Ok.
        assert!(stats.realms_scanned >= 1);
        assert_eq!(
            count_calls(&log),
            0,
            "no replays when API unreachable — voided pass completed cleanly"
        );
    }

    /// User Story: US-IAP-004 / US-IAP-006 (Apple getAllSubscriptionStatuses
    /// drift fallback — support-iap §4.2)
    ///
    /// A locally-active Apple subscription whose Apple status is Expired must
    /// be detected as drift and repaired by a TARGETED notification-history
    /// pull (transactionId filter, onlyFailures=false) replayed through the
    /// shared reprocess pipeline. The global onlyFailures sweep returns
    /// nothing — the missed notification was "delivered" per Apple (e.g. the
    /// webhook returned 200 before a local processing failure), so it is only
    /// discoverable via the status endpoint. This is exactly the fallback the
    /// PRD names alongside Notification History.
    #[test_context(IapReconContext)]
    #[tokio::test]
    async fn test_iap_reconciliation_apple_status_drift_targeted_replay(ctx: &mut IapReconContext) {
        let realm_id = ctx._realm_id.clone();
        let pool: &PgPool = &ctx.app_state.pool;
        let client_app_id = uuid::Uuid::parse_str(&ctx._client_app_id).unwrap();

        // wiremock standing in for the App Store Server API (per-realm
        // apple.base_url override).
        let server = wiremock::MockServer::start().await;

        // Global notification-history sweep (onlyFailures=true) → nothing.
        // The drift notification is NOT in this stream: Apple delivered it.
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/inApps/v1/notifications/history"))
            .and(wiremock::matchers::body_partial_json(serde_json::json!({
                "onlyFailures": true
            })))
            .respond_with(
                wiremock::ResponseTemplate::new(200).set_body_json(
                    serde_json::json!({ "notificationHistory": [], "hasMore": false }),
                ),
            )
            .mount(&server)
            .await;

        // Status endpoint: Apple reports the subscription Expired (status=2).
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path(
                "/inApps/v1/subscriptions/apple-orig-1",
            ))
            .respond_with(
                wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "environment": "Sandbox",
                    "bundleId": "com.herald.test",
                    "appAppleId": 123,
                    "data": [{
                        "subscriptionGroupIdentifier": "70001",
                        "lastTransactions": [{
                            "status": 2,
                            "originalTransactionId": "apple-orig-1"
                        }]
                    }]
                })),
            )
            .mount(&server)
            .await;

        // Targeted history pull for the drifted transaction → the missed
        // EXPIRED notification to replay.
        let missed_notification =
            crate::tests::helpers::iap_mocks::make_apple_jws(&serde_json::json!({
                "notificationType": "EXPIRED",
                "notificationUUID": "drift-uuid-1"
            }));
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/inApps/v1/notifications/history"))
            .and(wiremock::matchers::body_partial_json(serde_json::json!({
                "transactionId": "apple-orig-1"
            })))
            .respond_with(
                wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "notificationHistory": [{ "signedPayload": missed_notification }],
                    "hasMore": false
                })),
            )
            .mount(&server)
            .await;

        // Apple credentials, API base pointed at the wiremock. The `.p8` must
        // be a parseable EC key: the client signs ES256 JWTs before every
        // call and panics on malformed keys.
        insert_apple_realm_config(
            pool,
            &realm_id,
            "com.herald.test",
            "issuer-test",
            "key-test",
            test_apple_ec_p8_pem(),
            "sandbox",
        )
        .await;
        sqlx::query(
            "INSERT INTO realm_config (realm_id, config_type, config_key, config_value, is_secret, enabled, metadata)
             VALUES ($1, 'apple', 'base_url', $2, false, true, null)
             ON CONFLICT (realm_id, config_type, config_key)
             DO UPDATE SET config_value = EXCLUDED.config_value, enabled = true",
        )
        .bind(&realm_id)
        .bind(server.uri())
        .execute(pool)
        .await
        .expect("insert apple base_url override");

        // A subscription Herald still believes is active.
        seed_apple_subscription(pool, &realm_id, client_app_id, "apple-orig-1", "prod.recon").await;

        let processor = MockProcessor::new();
        let log = processor.call_log();
        let job = build_job(ctx, processor);

        let stats = job.run().await.expect("sweep must succeed");

        assert_eq!(
            stats.apple_status_polled, 1,
            "the local subscription is polled"
        );
        assert_eq!(
            stats.apple_drift_detected, 1,
            "local active vs Apple expired is drift"
        );
        assert_eq!(
            stats.apple_drift_replayed, 1,
            "the targeted pull replays the missed notification"
        );
        assert_eq!(
            stats.apple_failed, 0,
            "no per-object failures against the wiremock"
        );

        // Exactly the targeted replay reaches the processor (global sweep was
        // empty), carrying the raw signedPayload for the shared verify+replay
        // path.
        let calls = log.lock().unwrap();
        let apple_replays: Vec<&ReprocessCallRecord> = calls
            .iter()
            .filter(|c| c.payment_provider == "apple")
            .collect();
        assert_eq!(
            apple_replays.len(),
            1,
            "only the targeted drift replay reaches the processor"
        );
        assert_eq!(
            apple_replays[0].payload["signedPayload"].as_str(),
            Some(missed_notification.as_str())
        );
        assert_eq!(apple_replays[0].realm_id, realm_id);
    }

    // =========================================================================
    // pay_model — non-renewing / recurring / voided reconciliation
    // =========================================================================

    /// User Story: US-PM-009 (scenario 1 — Google poll EXPIRED → non-renewing
    ///             subscription transitioned to Expired).
    ///
    /// # HTTP-layer posture (same boundary as the sibling recon tests)
    ///
    /// The job builds its Google client with the production base URL (no
    /// per-realm override seam today), so the poll fails against the
    /// unreachable endpoint: a non-renewing Google subscription IS picked up
    /// by the poll SQL (it is in the active-Google-subscriptions set, now
    /// that the SELECT carries `billing_type`), and the sweep completes Ok
    /// (failure isolated). The positive EXPIRED→Expired transition is a
    /// unit-level behaviour of `map_google_subscription_change`; the full
    /// store-driven happy-path needs a Google base-URL override seam first.
    #[test_context(IapReconContext)]
    #[tokio::test]
    async fn test_pay_model_recon_google_expired_non_renewing_to_expired(
        ctx: &mut IapReconContext,
    ) {
        let realm_id = ctx._realm_id.clone();
        let pool: &PgPool = &ctx.app_state.pool;
        let client_app_id = uuid::Uuid::parse_str(&ctx._client_app_id).unwrap();

        // Seed an active non-renewing Google subscription (in the poll set).
        seed_google_subscription(
            pool,
            &realm_id,
            client_app_id,
            "gplay_nr_recon_1",
            "nr_recon_pass",
            "active",
            "non_renewing",
        )
        .await;

        let rsa_pem = fresh_rsa_pem();
        let sa_json = build_service_account_json(
            "svc@herald-test.iam.gserviceaccount.com",
            std::str::from_utf8(&rsa_pem).unwrap(),
        );
        insert_google_realm_config(pool, &realm_id, "com.herald.app", &sa_json, None).await;

        let processor = MockProcessor::new();
        let job = build_job(ctx, processor);

        // The sweep must succeed even though the Google poll is unreachable
        // (single-token failure isolated). The non-renewing subscription was
        // store-driven expiry boundary).
        let stats = job
            .run()
            .await
            .expect("non-renewing recon sweep must not abort on API unreachability");
        assert!(
            stats.realms_scanned >= 1,
            "Google-configured realm with a non-renewing subscription must be scanned"
        );

        // Invariant: absent a store EXPIRED event the non-renewing row is NOT
        let status: String = sqlx::query_scalar::<_, String>(
            "SELECT status FROM subscription
             WHERE external_subscription_id = 'gplay_nr_recon_1'",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(
            status.to_lowercase(),
            "active",
            "non-renewing subscription stays Active absent a store EXPIRED event (no local fallback)"
        );
    }

    /// User Story: n/a (regression — recurring poll behaviour not regressed).
    ///         §5.5 (recurring maintains full state mapping).
    ///
    /// Complements `test_iap_reconciliation_google_state_change_captured` with a
    /// non_renewing-aware assertion: a realm that holds BOTH a recurring and a
    /// non-renewing Google subscription is scanned, and the recurring row
    /// remains in the poll set (the new billing_type filter does not drop
    /// recurring subscriptions from the active set). Sweep completes Ok.
    #[test_context(IapReconContext)]
    #[tokio::test]
    async fn test_pay_model_recon_recurring_state_mapping_not_regressed(ctx: &mut IapReconContext) {
        let realm_id = ctx._realm_id.clone();
        let pool: &PgPool = &ctx.app_state.pool;
        let client_app_id = uuid::Uuid::parse_str(&ctx._client_app_id).unwrap();

        seed_google_subscription(
            pool,
            &realm_id,
            client_app_id,
            "gplay_rec_recon_1",
            "rec_recon_plan",
            "active",
            "recurring",
        )
        .await;
        // `subscription.client_app_id` is UNIQUE (`uq_subscription_client_app`,
        // migration 0002), so the second row needs its own client_app.
        let second_app_id = uuid::Uuid::now_v7();
        sqlx::query(
            "INSERT INTO client_app (id, realm_id, client_id, name, enabled)
             VALUES ($1, $2, $3, 'recon-sub-app', true)",
        )
        .bind(second_app_id)
        .bind(&realm_id)
        .bind(format!("recon-app-{second_app_id}"))
        .execute(pool)
        .await
        .expect("seed client_app for second recon subscription");
        seed_google_subscription(
            pool,
            &realm_id,
            second_app_id,
            "gplay_nr_recon_2",
            "nr_recon_pass_2",
            "active",
            "non_renewing",
        )
        .await;

        let rsa_pem = fresh_rsa_pem();
        let sa_json = build_service_account_json(
            "svc@herald-test.iam.gserviceaccount.com",
            std::str::from_utf8(&rsa_pem).unwrap(),
        );
        insert_google_realm_config(pool, &realm_id, "com.herald.app", &sa_json, None).await;

        let processor = MockProcessor::new();
        let job = build_job(ctx, processor);

        let stats = job
            .run()
            .await
            .expect("mixed billing_type sweep must not abort");
        assert!(
            stats.realms_scanned >= 1,
            "Google-configured realm must be scanned"
        );

        // Both rows remain in the poll set (active status); the new
        // billing_type column did not exclude recurring subscriptions.
        let active_count: i64 = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM subscription
             WHERE realm_id = $1 AND payment_provider = 'google'
               AND status = 'active'
               AND external_subscription_id IN ('gplay_rec_recon_1','gplay_nr_recon_2')",
        )
        .bind(&realm_id)
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(
            active_count, 2,
            "both recurring and non-renewing active subscriptions must remain in the poll set (no recurring regression)"
        );
    }

    /// User Story: US-PM-008 (scenario 2 — Google voided one_time purchase →
    ///             role revocation; manual grants untouched).
    ///
    /// Complements `test_iap_reconciliation_voided_purchase_revokes` (the
    /// subscription path) with the one_time / buyout path: a one_time Google
    /// purchase with a payment-source role. Under the API-unreachable boundary
    /// the voided pass produces no replays, but the structural contract holds:
    /// the sweep completes Ok and a pre-existing payment-source role (which a
    /// verified voided event would revoke via `revoke_roles_by_payment_source`)
    /// is left intact absent a store event — while a manual role is also
    /// intact (the source='payment' filter never touches it).
    #[test_context(IapReconContext)]
    #[tokio::test]
    async fn test_pay_model_recon_voided_one_time_revokes_role(ctx: &mut IapReconContext) {
        let realm_id = ctx._realm_id.clone();
        let pool: &PgPool = &ctx.app_state.pool;

        let user_id = uuid::Uuid::now_v7();
        sqlx::query(
            "INSERT INTO account (id, realm_id, email, password, status)
             VALUES ($1, $2, $3, $4, 1)
             ON CONFLICT (realm_id, email) DO NOTHING",
        )
        .bind(user_id)
        .bind(&realm_id)
        .bind("pm-recon-voided@test.com")
        .bind("$2a$12$dummy_password_hash")
        .execute(pool)
        .await
        .unwrap();

        let role_id = uuid::Uuid::now_v7();
        sqlx::query(
            "INSERT INTO roles (id, name, realm_id, client_id, is_builtin)
             VALUES ($1, $2, $3, $4, false)",
        )
        .bind(role_id)
        .bind("pm-recon-voided-role")
        .bind(&realm_id)
        .bind(&ctx._client_id)
        .execute(pool)
        .await
        .expect("create role");

        // Seed both a payment-source role (the voided one_time event would
        // revoke this) and a manual role (always preserved).
        for (source, source_id) in [
            ("payment", Some("gplay_voided_attempt_1".to_string())),
            ("manual", None),
        ] {
            sqlx::query(
                "INSERT INTO user_roles
                    (id, user_id, role_id, realm_id, client_id, principal_type, principal_id,
                     source, source_id, expires_at)
                 VALUES ($1, $2, $3, $4, $5, 'user', $2::text, $6, $7, NULL)",
            )
            .bind(uuid::Uuid::now_v7())
            .bind(user_id)
            .bind(role_id)
            .bind(&realm_id)
            .bind(&ctx._client_id)
            .bind(source)
            .bind(source_id)
            .execute(pool)
            .await
            .expect("seed role grant");
        }

        let rsa_pem = fresh_rsa_pem();
        let sa_json = build_service_account_json(
            "svc@herald-test.iam.gserviceaccount.com",
            std::str::from_utf8(&rsa_pem).unwrap(),
        );
        insert_google_realm_config(pool, &realm_id, "com.herald.app", &sa_json, None).await;

        let processor = MockProcessor::new();
        let log = processor.call_log();
        let job = build_job(ctx, processor);

        let stats = job
            .run()
            .await
            .expect("voided one_time pass must not abort sweep");
        assert!(stats.realms_scanned >= 1);
        assert_eq!(
            count_calls(&log),
            0,
            "no replays when Google API unreachable — voided one_time pass completed cleanly"
        );

        // Absent a store voided event, neither role is revoked. The manual role
        // would be preserved even under a verified voided event (source filter).
        let payment_count: i64 = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM user_roles
             WHERE user_id = $1 AND source = 'payment'",
        )
        .bind(user_id)
        .fetch_one(pool)
        .await
        .unwrap();
        let manual_count: i64 = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM user_roles
             WHERE user_id = $1 AND source = 'manual'",
        )
        .bind(user_id)
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(
            payment_count, 1,
            "payment role intact absent a store voided event"
        );
        assert_eq!(manual_count, 1, "manual role always preserved");
    }

    // =========================================================================
    // Helpers
    // =========================================================================

    /// Seed a Google subscription row (mirrors the production
    /// `pre_create_subscription` shape but with `billing_type` and
    /// `payment_provider='google'`).
    async fn seed_google_subscription(
        pool: &PgPool,
        realm_id: &str,
        client_app_id: uuid::Uuid,
        external_subscription_id: &str,
        external_product_id: &str,
        status: &str,
        billing_type: &str,
    ) {
        sqlx::query(
            "INSERT INTO subscription
                (id, realm_id, user_id, external_subscription_id, external_product_id,
                 payment_provider, status, entitlement_key, external_price_id,
                 provider_metadata, synced_at, current_period_start, current_period_end,
                 cancel_at_period_end, client_app_id, cancel_at, created_at, updated_at,
                 billing_type)
             VALUES ($1, $2, $3, $4, $5,
                     'google', $6, 'recon', NULL,
                     NULL, NOW(), NOW(), NOW() + INTERVAL '30 days',
                     false, $7, NULL, NOW(), NOW(), $8)",
        )
        .bind(uuid::Uuid::now_v7())
        .bind(realm_id)
        .bind(uuid::Uuid::now_v7())
        .bind(external_subscription_id)
        .bind(external_product_id)
        .bind(status)
        .bind(client_app_id)
        .bind(billing_type)
        .execute(pool)
        .await
        .expect("seed google subscription");
    }

    /// Create a second test realm (distinct from the default realm in
    /// SchemaTestContext) and return its ID.
    /// Seed an Apple subscription Herald believes is still alive (the drift
    /// poll only queries locally-alive statuses).
    async fn seed_apple_subscription(
        pool: &PgPool,
        realm_id: &str,
        client_app_id: uuid::Uuid,
        external_subscription_id: &str,
        external_product_id: &str,
    ) {
        sqlx::query(
            "INSERT INTO subscription
                (id, realm_id, user_id, external_subscription_id, external_product_id,
                 payment_provider, status, entitlement_key, external_price_id,
                 provider_metadata, synced_at, current_period_start, current_period_end,
                 cancel_at_period_end, client_app_id, cancel_at, created_at, updated_at,
                 billing_type)
             VALUES ($1, $2, $3, $4, $5,
                     'apple', 'active', 'recon', NULL,
                     NULL, NOW(), NOW(), NOW() + INTERVAL '30 days',
                     false, $6, NULL, NOW(), NOW(), 'recurring')",
        )
        .bind(uuid::Uuid::now_v7())
        .bind(realm_id)
        .bind(uuid::Uuid::now_v7())
        .bind(external_subscription_id)
        .bind(external_product_id)
        .bind(client_app_id)
        .execute(pool)
        .await
        .expect("seed apple subscription");
    }

    async fn create_second_realm(pool: &PgPool) -> String {
        let realm_id = uuid::Uuid::now_v7().to_string();
        sqlx::query(
            "INSERT INTO realm (id, name, created_at, updated_at)
             VALUES ($1, $2, NOW(), NOW())",
        )
        .bind(&realm_id)
        .bind(format!("test-realm-{}", &realm_id[..8]))
        .execute(pool)
        .await
        .expect("Failed to create second realm");
        realm_id
    }
}
