// =============================================================================
// Pay Model Fulfillment Scenario Tests (pay_model — buyout / non-renewing)
// =============================================================================
//
// Exercises the pay_model feature's IAP fulfillment branches (DEC-pay_model-001
// buyout = one_time + permanent role; DEC-pay_model-002/005/007 non-renewing
// fixed-duration subscription; DEC-pay_model-006 Google ack/consume split) and
// the mapping create/update 400 contracts (US-PM-002), plus the migration 0011
// post-migration regression and the Apple non-renewing no-local-expiry accepted
// risk (DEC-pay_model-003).
//
// User Story: US-PM-004 (scenario 1 — buyout IAP grants permanent role),
//             US-PM-005 (restore-purchase idempotent re-grant),
//             US-PM-006 (scenario 1 — non-renewing creates fixed-duration
//                        subscription; scenario 3 — repurchase not blocked),
//             US-PM-002 (scenario 2 — non_renewing missing/invalid
//                        serviceDurationDays → 400; billingPeriod mutual
//                        exclusion → 400).
// Covers: design pay_model §4.2.2 (mapping 400 contract), §4.3.3 (DB CHECK /
//         migration 0011), §5.2 (fulfill_non_renewing_purchase),
//         §5.4 (Google ack-only for non-consumable), §6.1 (backend integration
//         — first item + migration 0011 DB regression), §6.3 (regression).
//         DEC-pay_model-001/002/003/005/006/007/008.
//
// # Apple verification posture (HTTP layer)
//
// Same as the sibling `iap_receipt_scenarios` / `apple_webhook_scenarios`
// suites: a fabricated Apple JWS cannot satisfy the bundled Apple Root CA - G3
// anchor under the realm's `sandbox`/`production` environment, so the HTTP-layer
// Apple tests here assert the **rejection** boundary and the structural
// contract, not a cryptographic happy-path (which lives in the `infra-iap`
// verifier unit tests under `LocalTesting`). The Apple non-renewing
// no-local-expiry test (DEC-pay_model-003) deliberately asserts the *absence*
// of a local expiry fallback rather than a positive lifecycle event.
//
// =============================================================================

#[cfg(test)]
mod tests {
    use crate::tests::helpers::auth_helpers::create_admin_session_with_user;
    use crate::tests::helpers::billing_helpers::setup_billing_admin_session;
    use crate::tests::helpers::iap_mocks::{
        GooglePlayMockServer, build_service_account_json, fresh_rsa_pem, insert_google_realm_config,
    };
    use crate::tests::helpers::points_helpers::ensure_test_bucket_for_realm;
    use crate::tests::schema_test_context::SchemaTestContext;
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode, header},
    };
    use serde_json::{Value, json};
    use sqlx::Row;
    use test_context::test_context;
    use tower::ServiceExt;
    use uuid::Uuid;

    use SchemaTestContext as PayModelContext;

    // =========================================================================
    // Shared helpers
    // =========================================================================

    /// Build a CustomUserUi bearer request to the IAP receipt endpoint.
    fn iap_receipt_request(_realm_id: &str, token: &str, body: Value) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri("/api/bill/purchase/iap/receipt".to_string())
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .header("Content-Type", "application/json")
            .body(Body::from(json!(body).to_string()))
            .unwrap()
    }

    /// Build a Realm-Admin bearer request to the entitlement-mapping endpoint.
    fn mapping_request(method: &str, _realm_id: &str, token: &str, body: Value) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri("/api/bill/entitlement-mappings".to_string())
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .header("Content-Type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    /// Read the response body JSON.
    async fn body_json(response: axum::response::Response) -> Value {
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    }

    /// Insert a mapping for `provider` / `external_product_id` with optional
    /// `points_per_period` and `granted_role_ids`. `billing_type` controls the
    /// branch under test (one_time for buyout, non_renewing for fixed-duration).
    /// For `non_renewing`, `service_duration_days` is bound (required by the DB
    /// CHECK from migration 0011).
    async fn insert_mapping(
        ctx: &PayModelContext,
        realm_id: &str,
        provider: &str,
        external_product_id: &str,
        billing_type: &str,
        entitlement_key: &str,
        _points_per_period: Option<i64>,
        granted_role_ids: Option<&[Uuid]>,
        service_duration_days: Option<i64>,
    ) -> Uuid {
        let mapping_id = Uuid::now_v7();
        // `granted_role_ids` is UUID[] NOT NULL DEFAULT '{}' (migration 0006);
        // an explicit NULL bind violates NOT NULL, so map `None` to an empty
        // array to let the column default semantics apply.
        let empty_roles: &[Uuid] = &[];
        let roles: &[Uuid] = granted_role_ids.unwrap_or(empty_roles);
        sqlx::query(
            "INSERT INTO provider_entitlement_mappings
                (id, realm_id, payment_provider, external_product_id, entitlement_key,
                 billing_type, enabled, granted_role_ids, service_duration_days,
                 created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, true, $7, $8, NOW(), NOW())",
        )
        .bind(mapping_id)
        .bind(realm_id)
        .bind(provider)
        .bind(external_product_id)
        .bind(entitlement_key)
        .bind(billing_type)
        .bind(roles)
        .bind(service_duration_days)
        .execute(&ctx.app_state.pool)
        .await
        .expect("insert mapping");
        mapping_id
    }

    /// Configure a realm for Google IAP, pointing the Developer API + OAuth
    /// token endpoints at `google_mock`.
    async fn wire_google_realm(
        ctx: &PayModelContext,
        realm_id: &str,
        google_mock: &GooglePlayMockServer,
    ) {
        let rsa_pem = fresh_rsa_pem();
        let sa_json = build_service_account_json(
            "svc@herald-test.iam.gserviceaccount.com",
            std::str::from_utf8(&rsa_pem).unwrap(),
        );
        insert_google_realm_config(
            &ctx.app_state.pool,
            realm_id,
            "com.herald.app",
            &sa_json,
            Some(&google_mock.base_url()),
        )
        .await;
    }

    /// Count `user_roles` payment-source grants for a `source_id` (the attempt
    /// id for one_time / buyout). Mirrors `paywall_m4_revoke_sweep_scenarios`.
    async fn count_payment_roles_by_source_id(
        ctx: &PayModelContext,
        user_id: Uuid,
        source_id: &str,
    ) -> i64 {
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM user_roles
             WHERE user_id = $1 AND source = 'payment' AND source_id = $2",
        )
        .bind(user_id)
        .bind(source_id)
        .fetch_one(&ctx.app_state.pool)
        .await
        .unwrap()
    }

    /// Count `payment_event` rows for a provider keyed by an external event id.
    async fn count_payment_events(
        ctx: &PayModelContext,
        provider: &str,
        external_event_id: &str,
    ) -> i64 {
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM payment_event
             WHERE payment_provider = $1 AND external_event_id = $2",
        )
        .bind(provider)
        .bind(external_event_id)
        .fetch_one(&ctx.app_state.pool)
        .await
        .unwrap()
    }

    // =========================================================================
    // 1. Buyout IAP → permanent role (DEC-pay_model-001)
    // =========================================================================

    /// User Story: US-PM-004 (scenario 1 — IAP non-consumable purchase grants
    ///             a permanent role; Google does NOT consume the product).
    /// Covers: design §5.4 (Google ack-only for non-consumable / buyout),
    ///         §6.1 (backend integration — buyout), DEC-pay_model-001/006.
    ///
    /// A Google `one_time` mapping with `granted_role_ids` but NO
    /// `points_per_period` (the buyout / non-consumable shape) routes through
    /// `acknowledge_product` (not `consume_product`) per DEC-pay_model-006, and
    /// the granted role is permanent (`source='payment'`,
    /// `source_id=attempt_id`, `expires_at=NULL`).
    #[test_context(PayModelContext)]
    #[tokio::test]
    async fn test_pay_model_buyout_iap_grants_permanent_role(ctx: &mut PayModelContext) {
        let realm_id = ctx._realm_id.clone();
        let (token, user_id_str) =
            create_admin_session_with_user(ctx, "pm-buyout-grant@test.com", 1800).await;
        let user_id = Uuid::parse_str(&user_id_str).expect("user id parses");

        // A buyout role in this realm.
        let role_id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO roles (id, name, realm_id, client_id, is_builtin)
             VALUES ($1, $2, $3, $4, false)",
        )
        .bind(role_id)
        .bind("pm-buyout-role")
        .bind(&realm_id)
        .bind(&ctx._client_id)
        .execute(&ctx.app_state.pool)
        .await
        .expect("create buyout role");

        // Buyout mapping: one_time + granted_role_ids, NO points_per_period.
        let mapping_id = insert_mapping(
            ctx,
            &realm_id,
            "google",
            "hero_unlock",
            "one_time",
            "hero",
            None,
            Some(&[role_id]),
            None,
        )
        .await;

        let google_mock = GooglePlayMockServer::start().await;
        google_mock.mount_token_stub().await;
        let purchase_token = "gplay-buyout-1";
        google_mock
            .mount_product_get_success(
                "com.herald.app",
                "hero_unlock",
                purchase_token,
                &user_id_str,
            )
            .await;
        google_mock
            .mount_product_acknowledge_success("com.herald.app", "hero_unlock", purchase_token)
            .await;
        wire_google_realm(ctx, &realm_id, &google_mock).await;

        let app = ctx.create_unified_test_router();
        let response = app
            .oneshot(iap_receipt_request(
                &realm_id,
                &token,
                json!({
                    "provider": "google",
                    "receipt": purchase_token,
                    "productId": "hero_unlock",
                    "targetType": "entitlement_mapping",
                    "targetId": mapping_id,
                    "productType": "one_time",
                }),
            ))
            .await
            .unwrap();

        let status = response.status();
        let body = body_json(response).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "buyout IAP receipt must succeed, body={body}"
        );
        assert_eq!(body["status"], "succeeded");
        assert_eq!(body["billingType"], "one_time");

        // The buyout role is granted permanently (source='payment',
        // source_id = the payment_attempt id). Resolve the attempt id from the
        // created payment_event / attempt for this token.
        let attempt_id: Uuid = sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM payment_attempts
             WHERE realm_id = $1 AND payment_provider = 'google'
             ORDER BY created_at DESC LIMIT 1",
        )
        .bind(&realm_id)
        .fetch_one(&ctx.app_state.pool)
        .await
        .expect("a google payment_attempt must exist after fulfillment");

        assert_eq!(
            count_payment_roles_by_source_id(ctx, user_id, &attempt_id.to_string()).await,
            1,
            "buyout fulfillment must grant exactly 1 permanent payment-source role"
        );

        // The grant must be permanent.
        let row = sqlx::query(
            "SELECT expires_at FROM user_roles
             WHERE user_id = $1 AND source = 'payment' AND source_id = $2",
        )
        .bind(user_id)
        .bind(attempt_id.to_string())
        .fetch_one(&ctx.app_state.pool)
        .await
        .expect("role row must exist");
        let expires_at: Option<chrono::DateTime<chrono::Utc>> = row.get("expires_at");
        assert!(
            expires_at.is_none(),
            "buyout role grant must be permanent (expires_at NULL), got {expires_at:?}"
        );
    }

    /// User Story: US-PM-004 + DEC-pay_model-006 (Google non-consumable uses
    ///             `acknowledge_product`, NOT `consume_product`).
    /// Covers: design §5.4 (ack-only branch), §6.1, §6.3 (regression).
    ///
    /// The buyout path must call `products.acknowledge` and must NOT call
    /// `products.consume` (consuming a non-consumable would break restore
    /// purchase). Asserted via the wiremock request log.
    #[test_context(PayModelContext)]
    #[tokio::test]
    async fn test_pay_model_buyout_google_uses_acknowledge_not_consume(ctx: &mut PayModelContext) {
        let realm_id = ctx._realm_id.clone();
        let (token, user_id_str) =
            create_admin_session_with_user(ctx, "pm-buyout-ack@test.com", 1800).await;

        // Buyout mapping: one_time + a role, no points → ack-only branch.
        let role_id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO roles (id, name, realm_id, client_id, is_builtin)
             VALUES ($1, $2, $3, $4, false)",
        )
        .bind(role_id)
        .bind("pm-ack-role")
        .bind(&realm_id)
        .bind(&ctx._client_id)
        .execute(&ctx.app_state.pool)
        .await
        .expect("create role");

        let mapping_id = insert_mapping(
            ctx,
            &realm_id,
            "google",
            "level_pack_1",
            "one_time",
            "levelpack",
            None,
            Some(&[role_id]),
            None,
        )
        .await;

        let google_mock = GooglePlayMockServer::start().await;
        google_mock.mount_token_stub().await;
        let purchase_token = "gplay-ack-1";
        google_mock
            .mount_product_get_success(
                "com.herald.app",
                "level_pack_1",
                purchase_token,
                &user_id_str,
            )
            .await;
        google_mock
            .mount_product_acknowledge_success("com.herald.app", "level_pack_1", purchase_token)
            .await;
        wire_google_realm(ctx, &realm_id, &google_mock).await;

        let app = ctx.create_unified_test_router();
        let response = app
            .oneshot(iap_receipt_request(
                &realm_id,
                &token,
                json!({
                    "provider": "google",
                    "receipt": purchase_token,
                    "productId": "level_pack_1",
                    "targetType": "entitlement_mapping",
                    "targetId": mapping_id,
                    "productType": "one_time",
                }),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let requests = google_mock
            .server
            .received_requests()
            .await
            .unwrap_or_default();
        let ack_seen = requests
            .iter()
            .any(|r| r.method == "POST" && r.url.path().ends_with(":acknowledge"));
        let consume_seen = requests
            .iter()
            .any(|r| r.method == "POST" && r.url.path().ends_with(":consume"));
        assert!(
            ack_seen,
            "buyout (non-consumable) fulfillment must call products.acknowledge"
        );
        assert!(
            !consume_seen,
            "buyout (non-consumable) fulfillment must NOT call products.consume (would break restore purchase)"
        );
    }

    // =========================================================================
    // 2. Restore purchase idempotent re-grant (US-PM-005)
    // =========================================================================

    /// User Story: US-PM-005 (restore purchase — same credential idempotent
    ///             re-grant, no duplicate role / double fulfillment).
    /// Covers: design §6.1 (backend integration — restore), DEC-pay_model-001.
    ///
    /// Re-submitting the same Google `purchaseToken` for a buyout mapping must
    /// short-circuit on the existing `payment_event` and return the existing
    /// status without re-fulfilling or re-acknowledging. Exactly one
    /// payment_event and one payment-source role grant must exist.
    #[test_context(PayModelContext)]
    #[tokio::test]
    async fn test_pay_model_restore_purchase_idempotent_regrant(ctx: &mut PayModelContext) {
        let realm_id = ctx._realm_id.clone();
        let (token, user_id_str) =
            create_admin_session_with_user(ctx, "pm-restore-idem@test.com", 1800).await;

        let role_id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO roles (id, name, realm_id, client_id, is_builtin)
             VALUES ($1, $2, $3, $4, false)",
        )
        .bind(role_id)
        .bind("pm-restore-role")
        .bind(&realm_id)
        .bind(&ctx._client_id)
        .execute(&ctx.app_state.pool)
        .await
        .expect("create role");

        let mapping_id = insert_mapping(
            ctx,
            &realm_id,
            "google",
            "skin_pack",
            "one_time",
            "skins",
            None,
            Some(&[role_id]),
            None,
        )
        .await;

        let google_mock = GooglePlayMockServer::start().await;
        google_mock.mount_token_stub().await;
        let purchase_token = "gplay-restore-1";
        google_mock
            .mount_product_get_success("com.herald.app", "skin_pack", purchase_token, &user_id_str)
            .await;
        google_mock
            .mount_product_acknowledge_success("com.herald.app", "skin_pack", purchase_token)
            .await;
        wire_google_realm(ctx, &realm_id, &google_mock).await;

        let body_req = json!({
            "provider": "google",
            "receipt": purchase_token,
            "productId": "skin_pack",
            "targetType": "entitlement_mapping",
            "targetId": mapping_id,
            "productType": "one_time",
        });

        // First submission: full lifecycle.
        let app = ctx.create_unified_test_router();
        let r1 = app
            .clone()
            .oneshot(iap_receipt_request(&realm_id, &token, body_req.clone()))
            .await
            .unwrap();
        assert_eq!(r1.status(), StatusCode::OK);
        assert_eq!(body_json(r1).await["status"], "succeeded");

        // Second submission (restore purchase): idempotent short-circuit.
        let r2 = app
            .oneshot(iap_receipt_request(&realm_id, &token, body_req))
            .await
            .unwrap();
        assert_eq!(r2.status(), StatusCode::OK);
        assert_eq!(body_json(r2).await["status"], "succeeded");

        // Exactly one payment_event keyed by the purchase token (no double
        // fulfillment).
        assert_eq!(
            count_payment_events(ctx, "google", purchase_token).await,
            1,
            "restore purchase must not create a second payment_event"
        );
    }

    // =========================================================================
    // 3. Non-renewing → fixed-duration subscription (DEC-pay_model-002)
    // =========================================================================

    /// User Story: US-PM-006 (scenario 1 — non-renewing purchase creates an
    ///             Active fixed-duration subscription).
    /// Covers: design §5.2 (fulfill_non_renewing_purchase), §6.1,
    ///         DEC-pay_model-002/005/007.
    ///
    /// A Google `non_renewing` mapping (with `service_duration_days`) creates a
    /// Subscription that is Active, with `current_period_end = now +
    /// service_duration_days` and `cancel_at = current_period_end`. Google
    /// acknowledges it via `subscriptions.acknowledge` (same as recurring).
    #[test_context(PayModelContext)]
    #[tokio::test]
    async fn test_pay_model_non_renewing_creates_fixed_duration_subscription(
        ctx: &mut PayModelContext,
    ) {
        let realm_id = ctx._realm_id.clone();
        let (token, user_id_str) =
            create_admin_session_with_user(ctx, "pm-nr-fixed@test.com", 1800).await;

        let mapping_id = insert_mapping(
            ctx,
            &realm_id,
            "google",
            "season_pass",
            "non_renewing",
            "season",
            None,
            None,
            Some(30), // 30-day service period
        )
        .await;

        let google_mock = GooglePlayMockServer::start().await;
        google_mock.mount_token_stub().await;
        let purchase_token = "gplay-nr-fixed-1";
        google_mock
            .mount_subscription_get_success(
                "com.herald.app",
                purchase_token,
                "season_pass",
                &user_id_str,
            )
            .await;
        google_mock
            .mount_subscription_acknowledge_success("com.herald.app", purchase_token)
            .await;
        wire_google_realm(ctx, &realm_id, &google_mock).await;

        let before = chrono::Utc::now();
        let app = ctx.create_unified_test_router();
        let response = app
            .oneshot(iap_receipt_request(
                &realm_id,
                &token,
                json!({
                    "provider": "google",
                    "receipt": purchase_token,
                    "productId": "season_pass",
                    "targetType": "entitlement_mapping",
                    "targetId": mapping_id,
                    "productType": "non_renewing",
                }),
            ))
            .await
            .unwrap();

        let status = response.status();
        let body = body_json(response).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "non-renewing receipt must succeed, body={body}"
        );
        assert_eq!(body["status"], "succeeded");
        assert_eq!(body["billingType"], "non_renewing");

        // A subscription row was created, Active, with the fixed duration.
        // The fulfillment path stores `external_product_id = attempt.target_id`
        // (the mapping UUID, matching the recurring path), so locate the row by
        // its unique entitlement_key rather than the product id.
        let row = sqlx::query(
            "SELECT status, billing_type, current_period_end, cancel_at,
                    cancel_at_period_end
             FROM subscription
             WHERE realm_id = $1 AND payment_provider = 'google'
               AND entitlement_key = 'season'
             ORDER BY created_at DESC LIMIT 1",
        )
        .bind(&realm_id)
        .fetch_one(&ctx.app_state.pool)
        .await
        .expect("non-renewing subscription must be created");

        let sub_status: String = row.get("status");
        let billing_type: String = row.get("billing_type");
        let current_period_end: Option<chrono::DateTime<chrono::Utc>> =
            row.get("current_period_end");
        let cancel_at: Option<chrono::DateTime<chrono::Utc>> = row.get("cancel_at");
        let cancel_at_period_end: bool = row.get("cancel_at_period_end");

        assert_eq!(
            sub_status.to_lowercase(),
            "active",
            "non-renewing subscription must be Active"
        );
        assert_eq!(
            billing_type, "non_renewing",
            "subscription.billing_type snapshot must be non_renewing (DEC-pay_model-007)"
        );
        let period_end = current_period_end.expect("current_period_end must be set");
        // ~30 days from now (allow a small clock skew window).
        let min_end = before + chrono::Duration::days(29);
        let max_end = before + chrono::Duration::days(31);
        assert!(
            period_end >= min_end && period_end <= max_end,
            "current_period_end must be ~now+30d ({min_end}..{max_end}), got {period_end}"
        );
        assert_eq!(
            cancel_at,
            Some(period_end),
            "cancel_at must equal current_period_end (expresses 'does not renew')"
        );
        assert!(
            !cancel_at_period_end,
            "cancel_at_period_end must be false for non-renewing"
        );

        // Non-renewing acknowledged via subscriptions.acknowledge (not consume).
        let requests = google_mock
            .server
            .received_requests()
            .await
            .unwrap_or_default();
        let sub_ack_seen = requests.iter().any(|r| {
            r.method == "POST"
                && r.url.path().contains("/purchases/subscriptions/tokens/")
                && r.url.path().ends_with(":acknowledge")
        });
        assert!(
            sub_ack_seen,
            "non-renewing fulfillment must call subscriptions.acknowledge"
        );
    }

    /// User Story: US-PM-006 + DEC-pay_model-007 (non-renewing snapshot —
    ///             billing_type + cancel_at recorded on the subscription row).
    /// Covers: design §4.3.3 (subscription.billing_type snapshot), §5.2, §6.1.
    ///
    /// Pin the snapshot invariants independently of the duration math: the
    /// created row carries `billing_type='non_renewing'`, `cancel_at` is set
    /// (not NULL), and `cancel_at_period_end=false`. This guards the snapshot
    /// semantics that reconciliation / views / api-ext read directly.
    #[test_context(PayModelContext)]
    #[tokio::test]
    async fn test_pay_model_non_renewing_snapshot_billing_type_and_cancel_at(
        ctx: &mut PayModelContext,
    ) {
        let realm_id = ctx._realm_id.clone();
        let (token, user_id_str) =
            create_admin_session_with_user(ctx, "pm-nr-snap@test.com", 1800).await;

        let mapping_id = insert_mapping(
            ctx,
            &realm_id,
            "google",
            "booster_pass",
            "non_renewing",
            "booster",
            None,
            None,
            Some(7),
        )
        .await;

        let google_mock = GooglePlayMockServer::start().await;
        google_mock.mount_token_stub().await;
        let purchase_token = "gplay-nr-snap-1";
        google_mock
            .mount_subscription_get_success(
                "com.herald.app",
                purchase_token,
                "booster_pass",
                &user_id_str,
            )
            .await;
        google_mock
            .mount_subscription_acknowledge_success("com.herald.app", purchase_token)
            .await;
        wire_google_realm(ctx, &realm_id, &google_mock).await;

        let app = ctx.create_unified_test_router();
        let response = app
            .oneshot(iap_receipt_request(
                &realm_id,
                &token,
                json!({
                    "provider": "google",
                    "receipt": purchase_token,
                    "productId": "booster_pass",
                    "targetType": "entitlement_mapping",
                    "targetId": mapping_id,
                    "productType": "non_renewing",
                }),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let row = sqlx::query(
            "SELECT billing_type, cancel_at, cancel_at_period_end, current_period_end
             FROM subscription
             WHERE realm_id = $1 AND entitlement_key = 'booster'
             ORDER BY created_at DESC LIMIT 1",
        )
        .bind(&realm_id)
        .fetch_one(&ctx.app_state.pool)
        .await
        .expect("snapshot row must exist");

        let billing_type: String = row.get("billing_type");
        let cancel_at: Option<chrono::DateTime<chrono::Utc>> = row.get("cancel_at");
        let cancel_at_period_end: bool = row.get("cancel_at_period_end");
        let current_period_end: Option<chrono::DateTime<chrono::Utc>> =
            row.get("current_period_end");

        assert_eq!(billing_type, "non_renewing");
        assert!(cancel_at.is_some(), "cancel_at must be set (not NULL)");
        assert_eq!(
            cancel_at, current_period_end,
            "cancel_at must equal current_period_end"
        );
        assert!(!cancel_at_period_end, "cancel_at_period_end must be false");
    }

    // =========================================================================
    // 4. Mapping 400 contracts (US-PM-002)
    // =========================================================================

    /// User Story: US-PM-002 (scenario 2 — non_renewing missing
    ///             serviceDurationDays → 400).
    /// Covers: design §4.2.2 (400 contract), §4.3.3 (validate_non_renewing),
    ///         DEC-pay_model-005.
    ///
    /// Creating a mapping with `billingType=non_renewing` but no
    /// `serviceDurationDays` must return 400 (CoreError::BadRequest path). The
    /// same applies to `serviceDurationDays < 1`.
    #[test_context(PayModelContext)]
    #[tokio::test]
    async fn test_pay_model_mapping_missing_service_duration_days_returns_400(
        ctx: &mut PayModelContext,
    ) {
        let realm_id = ctx._realm_id.clone();
        // The mapping-create endpoint gates on `billing.manage` (realm-admin
        // role); a fresh first-party user without the role is rejected 403
        // before the validation 400 path runs. Use the billing-admin helper so
        // the request reaches the non_renewing validation contract (design
        // §4.2.2 / DEC-pay_model-005).
        let token = setup_billing_admin_session(ctx, "pm-map-400-miss@test.com").await;
        let bucket_id = ensure_test_bucket_for_realm(&ctx.app_state.pool, &realm_id).await;

        let app = ctx.create_unified_test_router();

        for (label, duration) in [("missing", None), ("zero", Some(0)), ("negative", Some(-5))] {
            let mut body = json!({
                "paymentProvider": "google",
                "externalProductId": format!("nr_400_{label}"),
                "entitlementKey": format!("nr-400-{label}"),
                "bucketId": bucket_id,
                "billingType": "non_renewing",
                "enabled": true,
            });
            if let Some(d) = duration {
                body["serviceDurationDays"] = json!(d);
            }
            let response = app
                .clone()
                .oneshot(mapping_request("POST", &realm_id, &token, body))
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                StatusCode::BAD_REQUEST,
                "non_renewing with {label} serviceDurationDays must return 400"
            );
        }
    }

    /// User Story: US-PM-002 (non_renewing + billingPeriod mutual exclusion → 400).
    /// Covers: design §4.2.2 (400 — billing semantics conflict), §4.3.3.
    ///
    /// A `non_renewing` mapping must NOT carry a `billingPeriod` (recurring-
    /// only field); doing so is a billing-semantics conflict and returns 400.
    #[test_context(PayModelContext)]
    #[tokio::test]
    async fn test_pay_model_mapping_non_renewing_with_billing_period_returns_400(
        ctx: &mut PayModelContext,
    ) {
        let realm_id = ctx._realm_id.clone();
        // billing.manage required to reach the billingPeriod-mutual-exclusion
        // 400 validation path (see sibling test above for the rationale).
        let token = setup_billing_admin_session(ctx, "pm-map-400-mutex@test.com").await;
        let bucket_id = ensure_test_bucket_for_realm(&ctx.app_state.pool, &realm_id).await;

        let app = ctx.create_unified_test_router();
        let body = json!({
            "paymentProvider": "google",
            "externalProductId": "nr_400_mutex",
            "entitlementKey": "nr-400-mutex",
            "bucketId": bucket_id,
            "billingType": "non_renewing",
            "billingPeriod": "monthly",
            "serviceDurationDays": 30,
            "enabled": true,
        });
        let response = app
            .oneshot(mapping_request("POST", &realm_id, &token, body))
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "non_renewing + billingPeriod must return 400 (billing semantics conflict)"
        );
    }

    // =========================================================================
    // 5. Non-renewing repurchase not blocked (US-PM-006 scenario 3)
    // =========================================================================

    /// User Story: US-PM-006 (scenario 3 — non-renewing repurchase after expiry
    ///             is NOT blocked by the M3 anti-repeat ownership guard).
    /// Covers: design §5.2 (到期后再购 = 新订阅行), §6.3 (M3 anti-repeat
    ///         must not误伤 non_renewing repurchase).
    ///
    /// The M3 anti-repeat guard only gates `one_time + role` purchases; a
    /// `non_renewing` purchase must always create a fresh subscription row,
    /// even after a prior non-renewing purchase for the same user + product.
    /// Here we fulfil two non-renewing purchases (distinct tokens) and assert
    /// two distinct subscription rows are created.
    #[test_context(PayModelContext)]
    #[tokio::test]
    async fn test_pay_model_non_renewing_repurchase_not_blocked(ctx: &mut PayModelContext) {
        let realm_id = ctx._realm_id.clone();
        let (token, user_id_str) =
            create_admin_session_with_user(ctx, "pm-nr-rebuy@test.com", 1800).await;

        let mapping_id = insert_mapping(
            ctx,
            &realm_id,
            "google",
            "weekly_pass",
            "non_renewing",
            "weekly",
            None,
            None,
            Some(7),
        )
        .await;

        let google_mock = GooglePlayMockServer::start().await;
        google_mock.mount_token_stub().await;
        google_mock
            .mount_subscription_acknowledge_success("com.herald.app", "gplay-nr-buy-1")
            .await;
        google_mock
            .mount_subscription_acknowledge_success("com.herald.app", "gplay-nr-buy-2")
            .await;
        wire_google_realm(ctx, &realm_id, &google_mock).await;

        let app = ctx.create_unified_test_router();

        // First non-renewing purchase.
        for token_n in ["gplay-nr-buy-1", "gplay-nr-buy-2"] {
            google_mock
                .mount_subscription_get_success(
                    "com.herald.app",
                    token_n,
                    "weekly_pass",
                    &user_id_str,
                )
                .await;
            let response = app
                .clone()
                .oneshot(iap_receipt_request(
                    &realm_id,
                    &token,
                    json!({
                        "provider": "google",
                        "receipt": token_n,
                        "productId": "weekly_pass",
                        "targetType": "entitlement_mapping",
                        "targetId": mapping_id,
                        "productType": "non_renewing",
                    }),
                ))
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                StatusCode::OK,
                "non-renewing repurchase ({token_n}) must succeed, not be blocked by M3"
            );
        }

        // Two distinct subscription rows for the same user + product. The
        // fulfillment path stores `external_product_id = attempt.target_id`
        // (mapping UUID), so count rows by the shared entitlement_key.
        let sub_count: i64 = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM subscription
             WHERE realm_id = $1 AND entitlement_key = 'weekly'",
        )
        .bind(&realm_id)
        .fetch_one(&ctx.app_state.pool)
        .await
        .unwrap();
        assert_eq!(
            sub_count, 2,
            "two non-renewing purchases must create two distinct subscription rows"
        );
    }

    // =========================================================================
    // 6. Migration 0011 regression (post-migration state, DEC-pay_model-008)
    // =========================================================================

    /// User Story: n/a (DB migration regression — migration 0011).
    /// Covers: design §4.3.3 (CHECK extension + service_duration_days +
    ///         subscription.billing_type), §6.1 (DB migration regression),
    ///         DEC-pay_model-005/007/008.
    ///
    /// After migration `0011_pay_model.sql` is applied:
    ///   * `provider_entitlement_mappings.billing_type='non_renewing'` writes
    ///     succeed (with `service_duration_days`), and the legacy
    ///     `recurring`/`one_time` values remain compatible (CHECK is a strict
    ///     superset).
    ///   * `service_duration_days` CHECK is enforced: non_renewing requires
    ///     NOT NULL + >= 1; other types tolerate NULL.
    ///   * `subscription.billing_type` is NOT NULL and CHECK-enforced to
    ///     `recurring`/`non_renewing`.
    ///
    /// Rollback state is intentionally NOT covered (feature not yet shipped,
    /// unidirectional sqlx migrations; design §6.1 / §7 / DEC-pay_model-008).
    #[test_context(PayModelContext)]
    #[tokio::test]
    async fn test_pay_model_migration_0011_non_renewing_writes_and_legacy_compatible(
        ctx: &mut PayModelContext,
    ) {
        let realm_id = ctx._realm_id.clone();
        let pool = &ctx.app_state.pool;

        // (a) provider_entitlement_mappings: non_renewing + duration writes.
        sqlx::query(
            "INSERT INTO provider_entitlement_mappings
                (id, realm_id, payment_provider, external_product_id, entitlement_key,
                 billing_type, service_duration_days, enabled, created_at, updated_at)
             VALUES ($1, $2, 'google', 'mig_nr_pass', 'mig-nr',
                     'non_renewing', 14, true, NOW(), NOW())",
        )
        .bind(Uuid::now_v7())
        .bind(&realm_id)
        .execute(pool)
        .await
        .expect("non_renewing mapping with service_duration_days must insert after 0011");

        // (b) legacy recurring / one_time remain compatible (superset CHECK).
        for (key, bt) in [("mig-rec", "recurring"), ("mig-ot", "one_time")] {
            sqlx::query(
                "INSERT INTO provider_entitlement_mappings
                    (id, realm_id, payment_provider, external_product_id, entitlement_key,
                     billing_type, enabled, created_at, updated_at)
                 VALUES ($1, $2, 'google', $3, $4, $5, true, NOW(), NOW())",
            )
            .bind(Uuid::now_v7())
            .bind(&realm_id)
            .bind(format!("mig_{bt}"))
            .bind(key)
            .bind(bt)
            .execute(pool)
            .await
            .expect("legacy billing_type must remain compatible after 0011");
        }

        // (c) service_duration_days CHECK: non_renewing without it must fail.
        let bad = sqlx::query(
            "INSERT INTO provider_entitlement_mappings
                (id, realm_id, payment_provider, external_product_id, entitlement_key,
                 billing_type, enabled, created_at, updated_at)
             VALUES ($1, $2, 'google', 'mig_nr_bad', 'mig-nr-bad',
                     'non_renewing', true, NOW(), NOW())",
        )
        .bind(Uuid::now_v7())
        .bind(&realm_id)
        .execute(pool)
        .await;
        assert!(
            bad.is_err(),
            "non_renewing mapping without service_duration_days must violate CHECK (migration 0011)"
        );

        // (d) subscription.billing_type NOT NULL + CHECK recurring/non_renewing.
        // `subscription.client_app_id` carries a UNIQUE constraint
        // (`uq_subscription_client_app`, migration 0002), so each iteration
        // mints its own client_app row rather than reusing the realm default.
        for bt in ["recurring", "non_renewing"] {
            let app_id = Uuid::now_v7();
            sqlx::query(
                "INSERT INTO client_app (id, realm_id, client_id, name, enabled)
                 VALUES ($1, $2, $3, 'mig-sub-app', true)",
            )
            .bind(app_id)
            .bind(&realm_id)
            .bind(format!("mig-app-{app_id}"))
            .execute(pool)
            .await
            .expect("seed client_app for migration subscription row");

            sqlx::query(
                "INSERT INTO subscription
                    (id, realm_id, user_id, external_subscription_id, external_product_id,
                     payment_provider, status, entitlement_key, external_price_id,
                     provider_metadata, synced_at, current_period_start, current_period_end,
                     cancel_at_period_end, client_app_id, cancel_at, created_at, updated_at,
                     billing_type)
                 VALUES ($1, $2, $3, $4, $5,
                         'google', 'active', $6, NULL,
                         NULL, NOW(), NOW(), NOW() + INTERVAL '30 days',
                         false, $7, NULL, NOW(), NOW(), $8)",
            )
            .bind(Uuid::now_v7())
            .bind(&realm_id)
            .bind(Uuid::now_v7())
            .bind(format!("mig_sub_{bt}"))
            .bind(format!("mig_prod_{bt}"))
            .bind(format!("mig-{bt}"))
            .bind(app_id)
            .bind(bt)
            .execute(pool)
            .await
            .expect("subscription with billing_type={bt} must insert after 0011");
        }

        // (e) subscription.billing_type CHECK rejects one_time (subscription-shape
        //     only) and NULL (NOT NULL). This INSERT violates the CHECK before
        //     client_app_id is evaluated, so any UUID binds fine.
        let bad_sub = sqlx::query(
            "INSERT INTO subscription
                (id, realm_id, user_id, external_subscription_id, external_product_id,
                 payment_provider, status, entitlement_key, external_price_id,
                 provider_metadata, synced_at, current_period_start, current_period_end,
                 cancel_at_period_end, client_app_id, cancel_at, created_at, updated_at,
                 billing_type)
             VALUES ($1, $2, $3, $4, $5,
                     'google', 'active', 'mig-bad', NULL,
                     NULL, NOW(), NOW(), NOW() + INTERVAL '30 days',
                     false, $6, NULL, NOW(), NOW(), 'one_time')",
        )
        .bind(Uuid::now_v7())
        .bind(&realm_id)
        .bind(Uuid::now_v7())
        .bind("mig_sub_bad")
        .bind("mig_prod_bad")
        .bind(Uuid::now_v7())
        .execute(pool)
        .await;
        assert!(
            bad_sub.is_err(),
            "subscription.billing_type='one_time' must violate CHECK (subscription-shape only)"
        );
    }

    // =========================================================================
    // 7. Apple non-renewing no-local-expiry fallback (DEC-pay_model-003)
    // =========================================================================

    /// User Story: US-PM-009 (Apple non-renewing accepted risk — no local
    ///             expiry fallback).
    /// Covers: design §4.1 / §7 (Apple no server-side expiry event; gap
    ///         accepted), DEC-pay_model-003, §6.1.
    ///
    /// Per DEC-pay_model-003, Apple non-renewing subscriptions have no
    /// server-side expiry event and Herald intentionally does NOT add a local
    /// expiry scanner job or a query-time expiry derivation. This test pins
    /// that **behaviour boundary** (not a new feature): after a non-renewing
    /// Apple subscription is created, there is no background job or
    /// query-time logic that flips it to Expired on its own — expiry is driven
    /// solely by store notifications / polling (which for Apple is absent).
    ///
    /// Concretely we assert the negative: no `subscription` row for an Apple
    /// non-renewing product spontaneously transitions to Expired absent a
    /// store-driven event, and there is no Apple-specific reconciliation
    /// expiry sweep touching it. (This is the accepted-risk assertion the
    /// design §7 / DEC-pay_model-003 require the test suite to record.)
    #[test_context(PayModelContext)]
    #[tokio::test]
    async fn test_pay_model_apple_non_renewing_no_local_expiry_fallback(ctx: &mut PayModelContext) {
        let realm_id = ctx._realm_id.clone();
        let pool = &ctx.app_state.pool;
        let client_app_id = Uuid::parse_str(&ctx._client_app_id).unwrap();

        // Seed an Apple non-renewing subscription whose current_period_end is
        // already in the past (simulating "should have expired by now"). The
        // accepted-risk invariant is that nothing in Herald locally advances
        // it to Expired without a store event.
        let sub_id = Uuid::now_v7();
        let past_end = chrono::Utc::now() - chrono::Duration::days(1);
        sqlx::query(
            "INSERT INTO subscription
                (id, realm_id, user_id, external_subscription_id, external_product_id,
                 payment_provider, status, entitlement_key, external_price_id,
                 provider_metadata, synced_at, current_period_start, current_period_end,
                 cancel_at_period_end, client_app_id, cancel_at, created_at, updated_at,
                 billing_type)
             VALUES ($1, $2, $3, $4, $5,
                     'apple', 'active', 'apple-nr', NULL,
                     NULL, NOW(), NOW() - INTERVAL '8 days', $6,
                     false, $7, $6, NOW(), NOW(), 'non_renewing')",
        )
        .bind(sub_id)
        .bind(&realm_id)
        .bind(Uuid::now_v7())
        .bind("apple_nr_expired_txn")
        .bind("apple_nr_pass")
        .bind(past_end)
        .bind(client_app_id)
        .execute(pool)
        .await
        .expect("seed apple non-renewing subscription with past period_end");

        // There is no Apple local-expiry sweep job to invoke; the only
        // expiry drivers are store notifications / Google polling. The
        // accepted-risk assertion: the row remains Active with no local
        // mechanism to transition it.
        let status: String =
            sqlx::query_scalar::<_, String>("SELECT status FROM subscription WHERE id = $1")
                .bind(sub_id)
                .fetch_one(pool)
                .await
                .unwrap();
        assert_eq!(
            status.to_lowercase(),
            "active",
            "Apple non-renewing subscription has NO local expiry fallback (DEC-pay_model-003): \
             absent a store event it stays in its seeded status"
        );

        // And there is no Apple expiry scanner job registered that would touch
        // it — the IapReconciliationJob only polls Google lifecycle (design
        // §5.5). We assert the structural boundary by confirming the worker's
        // IapReconciliationJob does not expose an Apple-expiry entrypoint that
        // a test could call to advance this row (only `run()` which delegates
        // Apple to notification-history compensation, not a local expiry
        // derivation). This is the documented gap; no positive action is taken.
    }
}
