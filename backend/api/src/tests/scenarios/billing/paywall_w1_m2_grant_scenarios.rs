// =============================================================================
// Paywall W1 + M2 — Payment-Driven Role Grant + Idempotency Scenario Tests
// =============================================================================
//
// Proves the payment-driven role grant infra (design §5.3) and its W1 wedge
// (design §5.1):
//   1. W1: a one-time mapping with NO points_per_period no longer 500s and
//      still grants the configured role (graceful-skip aligned to recurring).
//   2. one-time payment grants the mapping's roles with source='payment',
//      source_id=attempt_id, expires_at=NULL (permanent).
//   3. subscription payment grants roles with source_id=subscription_id,
//      expires_at=current_period_end.
//   4. duplicate fulfillment is idempotent (AlreadyExists → skip, no dup row).
//   5. duplicate subscription webhook is idempotent for the role grant.
//   6. pre-existing manual grants (source='manual') are untouched by the
//      payment-grant path (§6.3 regression: the manual/payment partial unique
//      indexes are fully disjoint — migration 0006).
//   7. a one-time mapping with empty granted_role_ids is a pure-points
//      package (role-grant loop is a no-op, points still granted).
//
// Mirrors `one_time_fulfillment_scenarios.rs` (the `fulfill_attempt`
// direct-handler helper, `create_pending_attempt_for_mapping`,
// `create_one_time_mapping_with_points`) and
// `webhook_grant_idempotency_scenarios.rs` (the duplicate-event idempotency
// pattern + `build_creem_checkout_completed_with_herald_metadata`).
//
// User Story: US-PW-002 (W1: one-time pure-entitlement no 500),
//             US-PW-003 (payment-driven role grant + source traceability +
//             idempotency; manual grants untouched)
// Covers: design §5.1 (W1 graceful-skip align to recurring),
//         §5.3 (one-time + subscription grant loops; source/source_id/expires_at;
//         GrantRoleOutcome::AlreadyExists skip),
//         §6.1 (W1 + M2),
//         §6.3 (user_roles unique constraint change; manual/payment partial
//         indexes; fulfillment_service new UserRoleRepository dep regression)
//
// =============================================================================

#[cfg(test)]
mod tests {
    use crate::tests::helpers::billing_helpers::setup_stripe_config;
    use crate::tests::helpers::points_helpers::{
        create_points_wallet, ensure_test_bucket_for_realm, get_points_wallet_by_user,
        snapshot_attempt_rules_for_mapping,
    };
    use crate::tests::helpers::rbac_helpers::create_role;
    use crate::tests::helpers::webhook_helpers::{
        assert_webhook_success, build_stripe_invoice_with_herald_metadata, generate_test_event_id,
        send_stripe_webhook_with_signature, setup_test_entitlement_mapping_for_webhook,
    };
    use crate::tests::schema_test_context::SchemaTestContext as TestContext;
    use herald_core::domain::authorization::principal_types;
    use serde_json::json;
    use test_context::test_context;
    use uuid::Uuid;

    // =========================================================================
    // Helpers
    // =========================================================================

    /// Thin wrapper over `rbac_helpers::create_role`. Requires an admin token
    /// (the role-definition endpoint needs `roles.manage`).
    async fn create_role_in_realm(
        ctx: &TestContext,
        realm_id: &str,
        token: &str,
        name: &str,
    ) -> Uuid {
        create_role(ctx, realm_id, token, name, "paywall role-grant test role").await
    }

    /// Create a one-time entitlement mapping that grants `role_ids` on payment.
    /// When `points` is `None` no distribution rule is seeded — the W1
    /// pure-entitlement case (no points, role only). When `points` is
    /// `Some(n)`, a fixed `topup` rule owned by this mapping is seeded so the
    /// one-time fulfillment grants `n` topup points (mirrors the grant
    /// semantics the old mapping-level `points_per_period` encoded).
    async fn create_one_time_mapping_with_role(
        ctx: &TestContext,
        realm_id: &str,
        points: Option<i64>,
        role_ids: &[Uuid],
        enabled: bool,
    ) -> Uuid {
        let mapping_id = Uuid::now_v7();
        let provider_product_info = json!({
            "name": format!("Test Package {}-{}", mapping_id, role_ids.len()),
            "price": 999,
            "currency": "usd"
        });

        // `bucket_id` is only needed when a distribution rule is seeded (the
        // rule targets a credit bucket); resolve it lazily for that case.
        sqlx::query(
            "INSERT INTO provider_entitlement_mappings
                (id, realm_id, payment_provider, external_product_id, entitlement_key,
                 billing_type, enabled, provider_product_info, granted_role_ids,
                 created_at, updated_at)
             VALUES ($1, $2, 'stripe', $3, $4, 'one_time', $5, $6, $7, NOW(), NOW())",
        )
        .bind(mapping_id)
        .bind(realm_id)
        .bind(format!("prod_{}", mapping_id))
        .bind(format!("one-time-role-{}", mapping_id))
        .bind(enabled)
        .bind(provider_product_info)
        .bind(role_ids) // Vec<Uuid> → Postgres uuid[]
        .execute(&ctx.app_state.pool)
        .await
        .expect("Failed to create one-time mapping with granted_role_ids");

        if let Some(points_amount) = points {
            let bucket_id = ensure_test_bucket_for_realm(&ctx.app_state.pool, realm_id).await;
            let rule_id = Uuid::now_v7();
            sqlx::query(
                "INSERT INTO points_distribution_rules
                    (id, realm_id, owner_type, entitlement_mapping_id, bucket_id,
                     trigger_sources, grant_mode, points_amount, validity_days,
                     enabled, display_order)
                 VALUES ($1, $2, 'entitlement_mapping', $3, $4, $5, 'fixed', $6, 0, true, 0)",
            )
            .bind(rule_id)
            .bind(realm_id)
            .bind(mapping_id)
            .bind(bucket_id)
            .bind(&["topup"][..])
            .bind(points_amount)
            .execute(&ctx.app_state.pool)
            .await
            .expect("Failed to seed mapping-owned topup distribution rule");
        }
        mapping_id
    }

    /// Create a pending payment attempt targeting an entitlement mapping.
    /// Copied verbatim from `one_time_fulfillment_scenarios.rs`.
    async fn create_pending_attempt(
        ctx: &TestContext,
        realm_id: &str,
        user_id: Uuid,
        mapping_id: Uuid,
        amount: i64,
        currency: &str,
    ) -> Uuid {
        let attempt_id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO payment_attempts
                (id, realm_id, user_id, payment_provider, target_type, target_id,
                 amount, currency, status, expires_at, created_at, updated_at)
             VALUES ($1, $2, $3, 'stripe', 'entitlement_mapping', $4,
                     $5, $6, 'Pending', NOW() + INTERVAL '2 hours', NOW(), NOW())",
        )
        .bind(attempt_id)
        .bind(realm_id)
        .bind(user_id)
        .bind(mapping_id)
        .bind(amount)
        .bind(currency)
        .execute(&ctx.app_state.pool)
        .await
        .expect("Failed to create pending payment attempt");
        // Mirror production `create_payment_attempt`: snapshot the mapping's
        // enabled `topup` rules (one-time trigger) so first fulfillment replays
        // them via `CapturedPaymentRules`. A no-points mapping captures nothing
        // (valid zero-result event).
        snapshot_attempt_rules_for_mapping(
            &ctx.app_state.pool,
            attempt_id,
            realm_id,
            mapping_id,
            "topup",
        )
        .await;
        attempt_id
    }

    /// Fulfill a payment attempt via the internal `fulfill_payment` handler.
    /// Copied verbatim from `one_time_fulfillment_scenarios.rs` — calls the
    /// direct handler with `State((*ctx.app_state).clone())`, `Path(attempt_id)`,
    /// and `Json(payload)`. This is the construction-site exercise for
    /// `PostgresFulfillmentService::new(points, billing, user_role,
    /// permission_service)` (the 4-arg ctor BE-D03 changed).
    async fn fulfill_attempt(
        ctx: &TestContext,
        attempt_id: Uuid,
        provider_tx_id: &str,
    ) -> Result<serde_json::Value, String> {
        let payload = json!({
            "realmId": ctx._realm_id,
            "providerStatus": "success",
            "providerTransactionId": provider_tx_id,
            "completedAt": chrono::Utc::now().to_rfc3339(),
        });

        let response = crate::application::http::billing::purchase_handlers::fulfill_payment(
            axum::extract::State((*ctx.app_state).clone()),
            axum::extract::Path(attempt_id),
            axum::Json(serde_json::from_value(payload).unwrap()),
        )
        .await;

        match response {
            Ok(axum::Json(result)) => Ok(serde_json::to_value(result).unwrap()),
            Err(e) => Err(format!("{:?}", e)),
        }
    }

    /// Get the status of a payment attempt.
    async fn get_attempt_status(ctx: &TestContext, attempt_id: Uuid) -> Option<String> {
        sqlx::query_scalar::<_, String>("SELECT status FROM payment_attempts WHERE id = $1")
            .bind(attempt_id)
            .fetch_optional(&ctx.app_state.pool)
            .await
            .unwrap()
    }

    /// Count credit ledger entries for a user with a given credit type.
    /// Mirrors `one_time_fulfillment_scenarios.rs`.
    async fn count_ledger_entries_for_user(
        ctx: &TestContext,
        user_id: Uuid,
        credit_type: &str,
    ) -> i64 {
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM points_credit_ledger WHERE user_id = $1 AND credit_type = $2",
        )
        .bind(user_id)
        .bind(credit_type)
        .fetch_one(&ctx.app_state.pool)
        .await
        .unwrap()
    }

    /// Count `user_roles` rows for a user/role with `source='payment'`.
    async fn count_payment_role_grants(ctx: &TestContext, user_id: Uuid, role_id: Uuid) -> i64 {
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM user_roles
             WHERE user_id = $1 AND role_id = $2 AND source = 'payment'",
        )
        .bind(user_id)
        .bind(role_id)
        .fetch_one(&ctx.app_state.pool)
        .await
        .unwrap()
    }

    /// Count `user_roles` rows for a user/role with `source='manual'`.
    async fn count_manual_role_grants(ctx: &TestContext, user_id: Uuid, role_id: Uuid) -> i64 {
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM user_roles
             WHERE user_id = $1 AND role_id = $2 AND source = 'manual'",
        )
        .bind(user_id)
        .bind(role_id)
        .fetch_one(&ctx.app_state.pool)
        .await
        .unwrap()
    }

    /// Fetch the payment-grant row's traceability columns
    /// (`source`, `source_id`, `expires_at`). Returns `None` if no payment
    /// grant row exists for this user/role.
    async fn get_payment_role_grant(
        ctx: &TestContext,
        user_id: Uuid,
        role_id: Uuid,
    ) -> Option<(
        String,
        Option<String>,
        Option<chrono::DateTime<chrono::Utc>>,
    )> {
        let row = sqlx::query(
            "SELECT source, source_id, expires_at FROM user_roles
             WHERE user_id = $1 AND role_id = $2 AND source = 'payment'",
        )
        .bind(user_id)
        .bind(role_id)
        .fetch_optional(&ctx.app_state.pool)
        .await
        .unwrap()?;

        use sqlx::Row;
        let source: String = row.get("source");
        let source_id: Option<String> = row.get("source_id");
        let expires_at: Option<chrono::DateTime<chrono::Utc>> = row.get("expires_at");
        Some((source, source_id, expires_at))
    }

    /// Insert a `user_roles` row with `source='manual'` (explicit about source;
    /// relies on the same schema shape as `rbac_helpers::assign_role_to_user`).
    /// Used to prove manual grants survive the payment-grant path (§6.3).
    async fn seed_manual_role_grant(
        ctx: &TestContext,
        realm_id: &str,
        user_id: Uuid,
        role_id: Uuid,
    ) {
        let user_role_id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO user_roles
                (id, user_id, role_id, realm_id, client_id, principal_type, principal_id,
                 source, source_id, expires_at)
             VALUES ($1, $2, $3, $4, $5, $6, $2::text, 'manual', NULL, NULL)",
        )
        .bind(user_role_id)
        .bind(user_id)
        .bind(role_id)
        .bind(realm_id)
        .bind(&ctx._client_id)
        .bind(principal_types::USER)
        .execute(&ctx.app_state.pool)
        .await
        .expect("Failed to seed manual role grant");
    }

    /// Look up the internal subscription id + period end created by a Stripe
    /// `invoice.payment_succeeded` webhook, by the external subscription id.
    /// Returns `(subscription_id, current_period_end)`.
    async fn get_subscription_by_external_id(
        ctx: &TestContext,
        external_subscription_id: &str,
    ) -> Option<(Uuid, Option<chrono::DateTime<chrono::Utc>>)> {
        let row = sqlx::query(
            "SELECT id, current_period_end FROM subscription
             WHERE external_subscription_id = $1 AND payment_provider = 'stripe'",
        )
        .bind(external_subscription_id)
        .fetch_optional(&ctx.app_state.pool)
        .await
        .unwrap()?;

        use sqlx::Row;
        let id: Uuid = row.get("id");
        let period_end: Option<chrono::DateTime<chrono::Utc>> = row.get("current_period_end");
        Some((id, period_end))
    }

    // =========================================================================
    // Test 1: W1 — one-time no-points fulfillment does not error and grants role
    // =========================================================================

    /// User Story: US-PW-002 (W1: one-time pure-entitlement no 500),
    ///             US-PW-003 (payment grants the configured role)
    /// Covers: design §5.1 (W1 graceful-skip align to recurring),
    ///         §5.3 (one-time grant loop), §6.1 W1+M2, §6.3 (new dep doesn't break)
    ///
    /// Scenario: A one-time mapping with NO points_per_period and a
    /// granted_role_ids no longer 500s; it grants the role permanently and
    /// writes no topup_credit ledger entry.
    #[test_context(TestContext)]
    #[tokio::test]
    async fn test_one_time_no_points_fulfillment_does_not_error_and_grants_role(
        ctx: &mut TestContext,
    ) {
        let realm_id = ctx._realm_id.clone();
        let user_id = Uuid::now_v7();

        // Create a role + an admin token (needed to define the role).
        let token = crate::tests::helpers::billing_helpers::setup_billing_admin_session(
            ctx,
            "pw2-w1-nopts@test.com",
        )
        .await;
        let role_id = create_role_in_realm(ctx, &realm_id, &token, "pw2-w1-role").await;

        // Given: a one-time mapping with points=None (W1 pure-entitlement) and
        // granted_role_ids=[role_id].
        let mapping_id =
            create_one_time_mapping_with_role(ctx, &realm_id, None, &[role_id], true).await;

        // Wallet creation mirrors the existing one-time test; the role-grant
        // path does not strictly need a wallet, but the fulfillment flow reads
        // the mapping uniformly.
        create_points_wallet(ctx, user_id, &realm_id).await;

        // And: a pending payment attempt.
        let attempt_id =
            create_pending_attempt(ctx, &realm_id, user_id, mapping_id, 999, "USD").await;

        // When: fulfilling the payment attempt.
        let provider_tx_id = format!("pi_test_{}", attempt_id);
        let result = fulfill_attempt(ctx, attempt_id, &provider_tx_id).await;

        // Then: fulfillment returns Ok — NOT an error (this is the W1 fix;
        // previously it 500'd with "no points_per_period configured").
        assert!(
            result.is_ok(),
            "W1: one-time no-points fulfillment must NOT error: {:?}",
            result
        );

        // And: the attempt status is Succeeded.
        let status = get_attempt_status(ctx, attempt_id).await;
        assert_eq!(status.as_deref(), Some("Succeeded"));

        // And: exactly one payment-source role grant was written.
        assert_eq!(
            count_payment_role_grants(ctx, user_id, role_id).await,
            1,
            "expected exactly 1 payment role grant"
        );

        // And: the grant row is permanent: source='payment',
        // source_id=attempt_id, expires_at=None.
        let (source, source_id, expires_at) = get_payment_role_grant(ctx, user_id, role_id)
            .await
            .expect("payment role grant row must exist after a successful one-time fulfillment");
        assert_eq!(source, "payment");
        assert_eq!(
            source_id.as_deref(),
            Some(attempt_id.to_string()).as_deref()
        );
        assert!(
            expires_at.is_none(),
            "one-time grant must be permanent (expires_at NULL), got {:?}",
            expires_at
        );

        // And: NO topup_credit ledger entry was created (no points granted).
        assert_eq!(
            count_ledger_entries_for_user(ctx, user_id, "topup_credit").await,
            0,
            "W1 no-points mapping must not write any topup_credit ledger entry"
        );
    }

    // =========================================================================
    // Test 2: one-time fulfillment grants role with source='payment' (points+role)
    // =========================================================================

    /// User Story: US-PW-003 (source=payment, source_id=attempt_id, expires_at=NULL permanent)
    /// Covers: design §5.3 (one-time: expires_at=None, source_id=attempt_id),
    ///         §6.1 M2, §6.3 (traceable)
    ///
    /// Scenario: A one-time mapping with BOTH points=500 and granted_role_ids
    /// grants the role (permanent) AND the points — proves the two dimensions
    /// are orthogonal.
    #[test_context(TestContext)]
    #[tokio::test]
    async fn test_one_time_fulfillment_grants_role_with_payment_source(ctx: &mut TestContext) {
        let realm_id = ctx._realm_id.clone();
        let user_id = Uuid::now_v7();

        let token = crate::tests::helpers::billing_helpers::setup_billing_admin_session(
            ctx,
            "pw2-src-pay@test.com",
        )
        .await;
        let role_id = create_role_in_realm(ctx, &realm_id, &token, "pw2-src-role").await;

        // Given: a one-time mapping with points=500 AND granted_role_ids=[role_id].
        let mapping_id =
            create_one_time_mapping_with_role(ctx, &realm_id, Some(500), &[role_id], true).await;

        create_points_wallet(ctx, user_id, &realm_id).await;

        let attempt_id =
            create_pending_attempt(ctx, &realm_id, user_id, mapping_id, 999, "USD").await;

        // When: fulfilling.
        let provider_tx_id = format!("pi_test_{}", attempt_id);
        let result = fulfill_attempt(ctx, attempt_id, &provider_tx_id).await;
        assert!(result.is_ok(), "Fulfillment should succeed: {:?}", result);

        // Then: status Succeeded.
        let status = get_attempt_status(ctx, attempt_id).await;
        assert_eq!(status.as_deref(), Some("Succeeded"));

        // And: exactly 1 payment-source role grant, permanent.
        assert_eq!(count_payment_role_grants(ctx, user_id, role_id).await, 1);
        let (source, source_id, expires_at) =
            get_payment_role_grant(ctx, user_id, role_id).await.unwrap();
        assert_eq!(source, "payment");
        assert_eq!(
            source_id.as_deref(),
            Some(attempt_id.to_string()).as_deref()
        );
        assert!(expires_at.is_none(), "one-time grant must be permanent");

        // And: points ALSO granted (role grant did not displace points grant).
        assert_eq!(
            count_ledger_entries_for_user(ctx, user_id, "topup_credit").await,
            1,
            "points must still be granted alongside the role"
        );
        let account = get_points_wallet_by_user(ctx, user_id).await;
        assert!(account.is_some(), "user should have a points wallet");
        let (_wallet_id, _total, topup_balance, _subscription_balance) = account.unwrap();
        assert_eq!(topup_balance, 500, "topup balance must be 500");
    }

    // =========================================================================
    // Test 3: subscription fulfillment grants role with period expiry
    // =========================================================================

    /// User Story: US-PW-003 (subscription: expires_at=period_end, source_id=subscription_id)
    /// Covers: design §5.3 (subscription grant loop), §6.1 M2
    ///
    /// Scenario: A successful Stripe `invoice.payment_succeeded` webhook for a
    /// recurring mapping with granted_role_ids grants the role with
    /// source_id=subscription_id and expires_at ≈ current_period_end (future).
    #[test_context(TestContext)]
    #[tokio::test]
    async fn test_subscription_fulfillment_grants_role_with_period_expiry(ctx: &mut TestContext) {
        let realm_id = ctx._realm_id.clone();
        let user_id = crate::tests::scenarios::points::fixtures::create_test_user_with_auth(
            &ctx.app_state.pool,
            &realm_id,
            "pw2-sub-period@test.com",
            "password123",
        )
        .await;

        // Create a role to grant.
        let token = crate::tests::helpers::billing_helpers::setup_billing_admin_session(
            ctx,
            "pw2-sub-role-admin@test.com",
        )
        .await;
        let role_id = create_role_in_realm(ctx, &realm_id, &token, "pw2-sub-role").await;

        // Stripe config so the webhook verifies.
        let webhook_secret = "test_stripe_wh_secret";
        setup_stripe_config(ctx, &realm_id, "sk_test_key", webhook_secret).await;

        // Recurring mapping with points; we then attach granted_role_ids=[role_id].
        let entitlement_key = "pw2-sub-plan";
        let external_product_id = "prod_pw2_sub";
        let mapping_id = setup_test_entitlement_mapping_for_webhook(
            ctx,
            &realm_id,
            "stripe",
            external_product_id,
            entitlement_key,
            1000,
            true, // grant_on_subscribe
            true, // enabled
        )
        .await;

        // The shared webhook mapping seeder does not set billing_type/granted_role_ids;
        // set them directly (recurring + the role-grant dimension).
        sqlx::query(
            "UPDATE provider_entitlement_mappings
             SET billing_type = 'recurring', billing_period = 'monthly',
                 granted_role_ids = $1
             WHERE id = $2",
        )
        .bind(vec![role_id]) // Vec<Uuid> → uuid[]
        .bind(mapping_id)
        .execute(&ctx.app_state.pool)
        .await
        .expect("Failed to set billing_type + granted_role_ids");

        create_points_wallet(ctx, user_id, &realm_id).await;

        // Build + send the Stripe invoice.payment_succeeded event (creates a
        // subscription and fulfills it). Mirror webhook_grant_idempotency
        // scenarios test 1.
        let event_id = generate_test_event_id();
        let stripe_subscription_id = format!("sub_pw2_{}", event_id);
        let event = build_stripe_invoice_with_herald_metadata(
            &event_id,
            &stripe_subscription_id,
            &realm_id,
            user_id,
            entitlement_key,
            2500,
        );

        let app = ctx.create_unified_test_router();
        let response =
            send_stripe_webhook_with_signature(&app, &realm_id, event, webhook_secret).await;
        assert_webhook_success(&response);

        // Resolve the subscription created by the webhook to its internal id +
        // period end (the grant's source_id / expires_at derive from these).
        let (subscription_id, period_end) =
            get_subscription_by_external_id(ctx, &stripe_subscription_id)
                .await
                .expect("subscription must be created by the webhook");

        // Then: exactly 1 payment-source role grant.
        assert_eq!(
            count_payment_role_grants(ctx, user_id, role_id).await,
            1,
            "expected exactly 1 payment role grant from the subscription webhook"
        );

        // And: source='payment', source_id=subscription_id, expires_at=Some(future).
        let (source, source_id, expires_at) =
            get_payment_role_grant(ctx, user_id, role_id).await.unwrap();
        assert_eq!(source, "payment");
        assert_eq!(
            source_id.as_deref(),
            Some(subscription_id.to_string()).as_deref(),
            "subscription grant source_id must be the subscription id"
        );
        let expires_at = expires_at.expect("subscription grant must have an expires_at");
        let now = chrono::Utc::now();
        assert!(
            expires_at > now,
            "subscription role expires_at must be in the future, got {:?}",
            expires_at
        );
        // Tolerance ±2 days around the period end (monthly ≈ 30 days out).
        if let Some(pe) = period_end {
            let diff = (expires_at - pe).num_days().abs();
            assert!(
                diff <= 2,
                "expires_at must be within ±2 days of the subscription period end, got {} days",
                diff
            );
        }
    }

    // =========================================================================
    // Test 4: duplicate fulfillment is idempotent for the role grant
    // =========================================================================

    /// User Story: US-PW-003 (idempotent, no double-grant)
    /// Covers: design §5.3 (GrantRoleOutcome::AlreadyExists skip),
    ///         §6.1 M2, §6.3 (regression: idempotency)
    ///
    /// Scenario: Calling fulfillment twice with the same attempt_id + provider
    /// tx id (simulating a webhook retry) does not create a second role grant.
    #[test_context(TestContext)]
    #[tokio::test]
    async fn test_duplicate_fulfillment_is_idempotent_for_role_grant(ctx: &mut TestContext) {
        let realm_id = ctx._realm_id.clone();
        let user_id = Uuid::now_v7();

        let token = crate::tests::helpers::billing_helpers::setup_billing_admin_session(
            ctx,
            "pw2-idem-fulfill@test.com",
        )
        .await;
        let role_id = create_role_in_realm(ctx, &realm_id, &token, "pw2-idem-role").await;

        let mapping_id =
            create_one_time_mapping_with_role(ctx, &realm_id, Some(750), &[role_id], true).await;

        create_points_wallet(ctx, user_id, &realm_id).await;

        let attempt_id =
            create_pending_attempt(ctx, &realm_id, user_id, mapping_id, 999, "USD").await;
        let provider_tx_id = format!("pi_test_{}", attempt_id);

        // First fulfillment.
        let result1 = fulfill_attempt(ctx, attempt_id, &provider_tx_id).await;
        assert!(
            result1.is_ok(),
            "first fulfillment should succeed: {:?}",
            result1
        );
        assert_eq!(count_payment_role_grants(ctx, user_id, role_id).await, 1);

        // Second fulfillment with the SAME attempt_id + provider_tx_id
        // (simulating a webhook retry).
        let result2 = fulfill_attempt(ctx, attempt_id, &provider_tx_id).await;
        assert!(
            result2.is_ok(),
            "duplicate fulfillment must return Ok (idempotent), not an error: {:?}",
            result2
        );

        // Still exactly 1 payment role grant (no duplicate row).
        assert_eq!(
            count_payment_role_grants(ctx, user_id, role_id).await,
            1,
            "duplicate fulfillment must NOT create a second role grant"
        );
    }

    // =========================================================================
    // Test 5: duplicate subscription webhook is idempotent for the role grant
    // =========================================================================

    /// User Story: US-PW-003 (webhook duplicate idempotent)
    /// Covers: design §5.3 + §5.5 (three-layer idempotency), §6.1 M2
    ///
    /// Scenario: Sending the Stripe invoice.payment_succeeded webhook TWICE
    /// with the SAME event_id does not create a second role grant.
    #[test_context(TestContext)]
    #[tokio::test]
    async fn test_duplicate_subscription_webhook_is_idempotent_for_role_grant(
        ctx: &mut TestContext,
    ) {
        let realm_id = ctx._realm_id.clone();
        let user_id = crate::tests::scenarios::points::fixtures::create_test_user_with_auth(
            &ctx.app_state.pool,
            &realm_id,
            "pw2-idem-webhook@test.com",
            "password123",
        )
        .await;

        let token = crate::tests::helpers::billing_helpers::setup_billing_admin_session(
            ctx,
            "pw2-idem-wh-admin@test.com",
        )
        .await;
        let role_id = create_role_in_realm(ctx, &realm_id, &token, "pw2-idem-wh-role").await;

        let webhook_secret = "test_stripe_wh_secret";
        setup_stripe_config(ctx, &realm_id, "sk_test_key", webhook_secret).await;

        let entitlement_key = "pw2-idem-plan";
        let external_product_id = "prod_pw2_idem";
        let mapping_id = setup_test_entitlement_mapping_for_webhook(
            ctx,
            &realm_id,
            "stripe",
            external_product_id,
            entitlement_key,
            1000,
            true,
            true,
        )
        .await;
        sqlx::query(
            "UPDATE provider_entitlement_mappings
             SET billing_type = 'recurring', billing_period = 'monthly',
                 granted_role_ids = $1
             WHERE id = $2",
        )
        .bind(vec![role_id])
        .bind(mapping_id)
        .execute(&ctx.app_state.pool)
        .await
        .expect("Failed to set billing_type + granted_role_ids");

        create_points_wallet(ctx, user_id, &realm_id).await;

        // Build the event once; send it twice with the SAME event_id.
        let event_id = generate_test_event_id();
        let stripe_subscription_id = format!("sub_pw2_idem_{}", event_id);
        let event = build_stripe_invoice_with_herald_metadata(
            &event_id,
            &stripe_subscription_id,
            &realm_id,
            user_id,
            entitlement_key,
            2500,
        );

        let app = ctx.create_unified_test_router();
        let response1 =
            send_stripe_webhook_with_signature(&app, &realm_id, event.clone(), webhook_secret)
                .await;
        assert_webhook_success(&response1);
        let response2 =
            send_stripe_webhook_with_signature(&app, &realm_id, event, webhook_secret).await;
        assert_webhook_success(&response2);

        // Exactly 1 payment role grant (payment_event unique key +
        // GrantRoleOutcome::AlreadyExists prevent a second row).
        assert_eq!(
            count_payment_role_grants(ctx, user_id, role_id).await,
            1,
            "duplicate webhook must NOT create a second role grant"
        );
    }

    // =========================================================================
    // Test 6: payment grant does not touch manual role grants (§6.3 regression)
    // =========================================================================

    /// User Story: US-PW-003 (manual grants untouched)
    /// Covers: design §4.1 (source isolation), §4.3.2 (manual/payment partial
    ///         unique indexes admit the same role from both sources), §6.3
    ///         (historical source='manual' regression)
    ///
    /// Scenario: A pre-existing manual grant of role R survives a payment grant
    /// of the SAME role — the disjoint manual (`source='manual'`) and payment
    /// (`source='payment'`) partial unique indexes admit both (migration 0006).
    #[test_context(TestContext)]
    #[tokio::test]
    async fn test_payment_grant_does_not_touch_manual_role_grants(ctx: &mut TestContext) {
        let realm_id = ctx._realm_id.clone();
        let user_id = Uuid::now_v7();

        let token = crate::tests::helpers::billing_helpers::setup_billing_admin_session(
            ctx,
            "pw2-manual-coexist@test.com",
        )
        .await;
        let role_id = create_role_in_realm(ctx, &realm_id, &token, "pw2-manual-role").await;

        // Given: a pre-existing MANUAL grant of role R.
        seed_manual_role_grant(ctx, &realm_id, user_id, role_id).await;
        assert_eq!(
            count_manual_role_grants(ctx, user_id, role_id).await,
            1,
            "manual grant must be seeded"
        );

        // And: a one-time mapping granting the SAME role.
        let mapping_id =
            create_one_time_mapping_with_role(ctx, &realm_id, Some(500), &[role_id], true).await;

        create_points_wallet(ctx, user_id, &realm_id).await;

        let attempt_id =
            create_pending_attempt(ctx, &realm_id, user_id, mapping_id, 999, "USD").await;

        // When: fulfilling.
        let provider_tx_id = format!("pi_test_{}", attempt_id);
        let result = fulfill_attempt(ctx, attempt_id, &provider_tx_id).await;
        assert!(result.is_ok(), "fulfillment should succeed: {:?}", result);

        // Then: exactly 1 payment-source grant coexists.
        assert_eq!(
            count_payment_role_grants(ctx, user_id, role_id).await,
            1,
            "new payment grant must coexist with the manual grant"
        );

        // And: the manual grant is UNTOUCHED (still exactly 1).
        assert_eq!(
            count_manual_role_grants(ctx, user_id, role_id).await,
            1,
            "manual grant must remain untouched by the payment-grant path (§6.3)"
        );
    }

    // =========================================================================
    // Test 7: one-time no-role mapping fulfillment succeeds without grant
    // =========================================================================

    /// User Story: US-PW-002 (pure points package, no role — inverse regression)
    /// Covers: design §1.3 (orthogonal: empty role array), §6.1 W1, §6.3
    ///
    /// Scenario: A one-time mapping with points=300 and an empty
    /// granted_role_ids fulfills successfully, grants points, and grants NO
    /// role — the role-grant loop is a no-op on an empty array.
    #[test_context(TestContext)]
    #[tokio::test]
    async fn test_one_time_no_role_mapping_fulfillment_succeeds_without_grant(
        ctx: &mut TestContext,
    ) {
        let realm_id = ctx._realm_id.clone();
        let user_id = Uuid::now_v7();

        let token = crate::tests::helpers::billing_helpers::setup_billing_admin_session(
            ctx,
            "pw2-norole@test.com",
        )
        .await;
        // Create a role purely so we can assert it was NOT granted to the user.
        let role_id = create_role_in_realm(ctx, &realm_id, &token, "pw2-norole-role").await;

        // Given: a one-time mapping with points=300 and empty granted_role_ids.
        let mapping_id =
            create_one_time_mapping_with_role(ctx, &realm_id, Some(300), &[], true).await;

        create_points_wallet(ctx, user_id, &realm_id).await;

        let attempt_id =
            create_pending_attempt(ctx, &realm_id, user_id, mapping_id, 999, "USD").await;

        // When: fulfilling.
        let provider_tx_id = format!("pi_test_{}", attempt_id);
        let result = fulfill_attempt(ctx, attempt_id, &provider_tx_id).await;
        assert!(result.is_ok(), "fulfillment should succeed: {:?}", result);

        // Then: status Succeeded.
        let status = get_attempt_status(ctx, attempt_id).await;
        assert_eq!(status.as_deref(), Some("Succeeded"));

        // And: no role granted (empty granted_role_ids → role loop no-op).
        assert_eq!(
            count_payment_role_grants(ctx, user_id, role_id).await,
            0,
            "no role should be granted when granted_role_ids is empty"
        );

        // And: points still granted.
        assert_eq!(
            count_ledger_entries_for_user(ctx, user_id, "topup_credit").await,
            1,
            "points must still be granted"
        );
        let account = get_points_wallet_by_user(ctx, user_id).await.unwrap();
        assert_eq!(account.2, 300, "topup balance must be 300");
    }

    // =========================================================================
    // NOTE on construction-site coverage (item test 8):
    // `PostgresFulfillmentService::new(points, billing, user_role, permission_service)`
    // (the 4-arg ctor BE-D03 changed in `schema_test_context.rs`) is exercised
    // by EVERY `fulfill_attempt` call in tests 1-7 (one-time path) and by the
    // two Stripe webhook tests (3, 5) on the subscription path. A standalone
    // construction smoke test would be redundant; coverage is implicit.
    // =========================================================================
}
