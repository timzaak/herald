// =============================================================================
// Creem Webhook One-Time Dispatch Scenario Tests
// =============================================================================
//
// Tests for:
// 1. One-time checkout.completed -> immediate fulfillment (topup_credit)
// 2. Recurring checkout.completed -> deferred to subscription.paid
// 3. billing_type resolution via 3-tier fallback (metadata > product > mapping)
// 4. Creem "onetime" normalization to domain "one_time"
// 5. One-time checkout without attemptId -> audit only, no fulfillment
//
// User Story: US-PA-003 (payment success fulfillment), US-PU-006 (one-time purchase)
//
// =============================================================================

#[cfg(test)]
mod tests {
    use crate::tests::helpers::webhook_helpers::{
        assert_webhook_success, generate_test_event_id, send_webhook_with_signature,
    };
    use crate::tests::schema_test_context::SchemaTestContext;
    use axum::http::StatusCode;
    use serde_json::json;
    use test_context::test_context;
    use uuid::Uuid;

    use SchemaTestContext as CreemOneTimeTestContext;

    // =========================================================================
    // Helpers
    // =========================================================================

    /// Set Creem webhook secret for a realm.
    async fn set_webhook_secret(ctx: &SchemaTestContext, webhook_secret: &str) {
        ctx.with_creem_config(
            &ctx._realm_id,
            Some("test_api_key"),
            Some(webhook_secret),
            Some(30),
        )
        .await;
    }

    /// Create a user in the test realm and return their UUID.
    async fn create_test_user(ctx: &SchemaTestContext, email: &str) -> Uuid {
        let user_id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO account (id, realm_id, email, password, status)
             VALUES ($1, $2, $3, $4, 1)
             ON CONFLICT (realm_id, email) DO NOTHING",
        )
        .bind(user_id)
        .bind(&ctx._realm_id)
        .bind(email)
        .bind("$2a$12$dummy_password_hash")
        .execute(&ctx.app_state.pool)
        .await
        .expect("Failed to create test user");
        user_id
    }

    /// Create a points wallet for a user.
    async fn create_points_wallet(ctx: &SchemaTestContext, user_id: Uuid, realm_id: &str) {
        let bucket_id = crate::tests::helpers::points_helpers::ensure_test_bucket_for_realm(
            &ctx.app_state.pool,
            realm_id,
        )
        .await;
        sqlx::query(
            "INSERT INTO points_wallets (id, user_id, realm_id, bucket_id, total_topup_granted, total_subscription_granted, total_recharged, total_consumed, status, created_at, updated_at)
             VALUES ($1, $2, $3, $4, 0, 0, 0, 0, 'active', NOW(), NOW())
             ON CONFLICT (realm_id, user_id, bucket_id) DO NOTHING",
        )
        .bind(Uuid::now_v7())
        .bind(user_id)
        .bind(realm_id)
        .bind(bucket_id)
        .execute(&ctx.app_state.pool)
        .await
        .expect("Failed to create points wallet");
    }

    /// Create a one-time entitlement mapping for Creem provider.
    /// Returns the mapping ID.
    async fn create_one_time_mapping(
        ctx: &SchemaTestContext,
        realm_id: &str,
        external_product_id: &str,
        entitlement_key: &str,
        points_per_period: i64,
        enabled: bool,
    ) -> Uuid {
        let mapping_id = Uuid::now_v7();
        let bucket_id = crate::tests::helpers::points_helpers::ensure_test_bucket_for_realm(
            &ctx.app_state.pool,
            realm_id,
        )
        .await;
        // Distribution-rules model: the grant is a fixed `topup` rule owned by
        // this mapping (the old `points_per_period` / `grant_on_subscribe`).
        sqlx::query(
            "INSERT INTO provider_entitlement_mappings
                (id, realm_id, payment_provider, external_product_id, entitlement_key,
                 billing_type, enabled, created_at, updated_at)
             VALUES ($1, $2, 'creem', $3, $4, 'one_time', $5, NOW(), NOW())",
        )
        .bind(mapping_id)
        .bind(realm_id)
        .bind(external_product_id)
        .bind(entitlement_key)
        .bind(enabled)
        .execute(&ctx.app_state.pool)
        .await
        .expect("Failed to create one-time entitlement mapping");

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
        .bind(points_per_period)
        .execute(&ctx.app_state.pool)
        .await
        .expect("Failed to seed mapping-owned topup distribution rule");
        mapping_id
    }

    /// Create a recurring entitlement mapping for Creem provider.
    /// Returns the mapping ID.
    async fn create_recurring_mapping(
        ctx: &SchemaTestContext,
        realm_id: &str,
        external_product_id: &str,
        entitlement_key: &str,
        _points_per_period: i64,
        enabled: bool,
    ) -> Uuid {
        let mapping_id = Uuid::now_v7();
        // Distribution-rules model: bare mapping row. These callers assert
        // billing-type resolution / deferred-grant behavior, not points balances,
        // so no distribution rule is seeded.
        sqlx::query(
            "INSERT INTO provider_entitlement_mappings
                (id, realm_id, payment_provider, external_product_id, entitlement_key,
                 billing_type, billing_period, enabled, created_at, updated_at)
             VALUES ($1, $2, 'creem', $3, $4, 'recurring', 'monthly', $5, NOW(), NOW())",
        )
        .bind(mapping_id)
        .bind(realm_id)
        .bind(external_product_id)
        .bind(entitlement_key)
        .bind(enabled)
        .execute(&ctx.app_state.pool)
        .await
        .expect("Failed to create recurring entitlement mapping");
        mapping_id
    }

    /// Create a pending payment attempt targeting an entitlement mapping.
    /// Returns the attempt ID.
    async fn create_pending_payment_attempt(
        ctx: &SchemaTestContext,
        realm_id: &str,
        user_id: Uuid,
        mapping_id: Uuid,
        amount: i64,
    ) -> Uuid {
        let attempt_id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO payment_attempts
                (id, realm_id, user_id, payment_provider, target_type, target_id,
                 amount, currency, status, expires_at, created_at, updated_at)
             VALUES ($1, $2, $3, 'creem', 'entitlement_mapping', $4,
                     $5, 'USD', 'Pending', NOW() + INTERVAL '2 hours', NOW(), NOW())",
        )
        .bind(attempt_id)
        .bind(realm_id)
        .bind(user_id)
        .bind(mapping_id)
        .bind(amount)
        .execute(&ctx.app_state.pool)
        .await
        .expect("Failed to create pending payment attempt");
        // Mirror production `create_payment_attempt`: snapshot the mapping's
        // enabled `topup` rules so checkout.completed fulfillment replays them.
        crate::tests::helpers::points_helpers::snapshot_attempt_rules_for_mapping(
            &ctx.app_state.pool,
            attempt_id,
            realm_id,
            mapping_id,
            "topup",
        )
        .await;
        attempt_id
    }

    /// Get the status of a payment attempt.
    async fn get_payment_attempt_status(
        ctx: &SchemaTestContext,
        attempt_id: Uuid,
    ) -> Option<String> {
        sqlx::query_scalar::<_, String>("SELECT status FROM payment_attempts WHERE id = $1")
            .bind(attempt_id)
            .fetch_optional(&ctx.app_state.pool)
            .await
            .unwrap()
    }

    /// Get wallet balances for a user.
    async fn get_wallet_balances(
        ctx: &SchemaTestContext,
        user_id: Uuid,
        realm_id: &str,
    ) -> (i64, i64) {
        let row: Option<(i64, i64)> = sqlx::query_as(
            "SELECT COALESCE(SUM(l.remaining_amount) FILTER (
                        WHERE l.status = 'active' AND l.remaining_amount > 0
                          AND l.credit_type IN ('topup_credit','registration_credit','free_periodic_credit')
                          AND (l.effective_at IS NULL OR l.effective_at <= NOW())
                          AND (l.expires_at  IS NULL OR l.expires_at  >  NOW())
                    ), 0)::BIGINT AS topup_balance,
                    COALESCE(SUM(l.remaining_amount) FILTER (
                        WHERE l.status = 'active' AND l.remaining_amount > 0
                          AND l.credit_type = 'subscription_credit'
                          AND (l.effective_at IS NULL OR l.effective_at <= NOW())
                          AND (l.expires_at  IS NULL OR l.expires_at  >  NOW())
                    ), 0)::BIGINT AS subscription_balance
             FROM points_wallets w
             LEFT JOIN points_credit_ledger l
               ON l.realm_id = w.realm_id AND l.user_id = w.user_id AND l.bucket_id = w.bucket_id
             WHERE w.user_id = $1 AND w.realm_id = $2
             GROUP BY w.id",
        )
        .bind(user_id)
        .bind(realm_id)
        .fetch_optional(&ctx.app_state.pool)
        .await
        .unwrap();
        row.unwrap_or((0, 0))
    }

    /// Count points_credit_ledger entries for a user with a given credit_type.
    async fn count_ledger_entries(
        ctx: &SchemaTestContext,
        user_id: Uuid,
        realm_id: &str,
        credit_type: &str,
    ) -> i64 {
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM points_credit_ledger WHERE user_id = $1 AND realm_id = $2 AND credit_type = $3",
        )
        .bind(user_id)
        .bind(realm_id)
        .bind(credit_type)
        .fetch_one(&ctx.app_state.pool)
        .await
        .unwrap()
    }

    /// Count payment_event entries for a given external event ID.
    async fn count_payment_events(ctx: &SchemaTestContext, event_id: &str) -> i64 {
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM payment_event WHERE external_event_id = $1 AND payment_provider = 'creem'",
        )
        .bind(event_id)
        .fetch_one(&ctx.app_state.pool)
        .await
        .unwrap()
    }

    /// Build a checkout.completed payload with herald_billing_kind in metadata.
    fn build_checkout_completed_with_billing_kind(
        event_id: &str,
        entitlement_key: &str,
        realm_id: &str,
        user_id: Uuid,
        client_app_id: Uuid,
        external_product_id: &str,
        billing_kind: &str,
        attempt_id: Option<Uuid>,
        product_billing_type: Option<&str>,
    ) -> serde_json::Value {
        let mut metadata = json!({
            "herald_entitlement_key": entitlement_key,
            "herald_realm_id": realm_id,
            "herald_user_id": user_id.to_string(),
            "herald_client_app_id": client_app_id.to_string(),
            "clientAppId": client_app_id.to_string(),
            "herald_billing_kind": billing_kind,
        });
        if let Some(aid) = attempt_id {
            metadata["attemptId"] = json!(aid.to_string());
        }

        let mut product = json!({
            "id": external_product_id,
        });
        if let Some(bt) = product_billing_type {
            product["billing_type"] = json!(bt);
        }

        json!({
            "id": event_id,
            "eventType": "checkout.completed",
            "object": {
                "id": format!("checkout_{}", event_id),
                "status": "completed",
                "product": product,
                "metadata": metadata,
            }
        })
    }

    /// Build a checkout.completed payload without herald_billing_kind metadata
    /// but with product billing_type field.
    fn build_checkout_completed_no_metadata_billing_kind(
        event_id: &str,
        entitlement_key: &str,
        realm_id: &str,
        user_id: Uuid,
        client_app_id: Uuid,
        external_product_id: &str,
        product_billing_type: &str,
        attempt_id: Option<Uuid>,
    ) -> serde_json::Value {
        let mut metadata = json!({
            "herald_entitlement_key": entitlement_key,
            "herald_realm_id": realm_id,
            "herald_user_id": user_id.to_string(),
            "herald_client_app_id": client_app_id.to_string(),
            "clientAppId": client_app_id.to_string(),
        });
        if let Some(aid) = attempt_id {
            metadata["attemptId"] = json!(aid.to_string());
        }

        json!({
            "id": event_id,
            "eventType": "checkout.completed",
            "object": {
                "id": format!("checkout_{}", event_id),
                "status": "completed",
                "product": {
                    "id": external_product_id,
                    "billing_type": product_billing_type,
                },
                "metadata": metadata,
            }
        })
    }

    /// Build a checkout.completed payload without any billing_type information
    /// (neither in metadata nor in product field).
    fn build_checkout_completed_no_billing_info(
        event_id: &str,
        entitlement_key: &str,
        realm_id: &str,
        user_id: Uuid,
        client_app_id: Uuid,
        external_product_id: &str,
        attempt_id: Option<Uuid>,
    ) -> serde_json::Value {
        let mut metadata = json!({
            "herald_entitlement_key": entitlement_key,
            "herald_realm_id": realm_id,
            "herald_user_id": user_id.to_string(),
            "herald_client_app_id": client_app_id.to_string(),
            "clientAppId": client_app_id.to_string(),
        });
        if let Some(aid) = attempt_id {
            metadata["attemptId"] = json!(aid.to_string());
        }

        json!({
            "id": event_id,
            "eventType": "checkout.completed",
            "object": {
                "id": format!("checkout_{}", event_id),
                "status": "completed",
                "product": {
                    "id": external_product_id,
                },
                "metadata": metadata,
            }
        })
    }

    // =========================================================================
    // Test 1: One-time fulfillment
    // =========================================================================

    /// User Story: US-PA-003, US-PU-006
    ///
    /// Given a one-time entitlement mapping with 800 points and a pending payment attempt,
    /// when checkout.completed arrives with herald_billing_kind=one_time and the attempt's ID,
    /// the payment attempt completes and 800 topup_credit is granted.
    #[test_context(CreemOneTimeTestContext)]
    #[tokio::test]
    async fn test_creem_checkout_completed_one_time_fulfills_immediately(
        ctx: &mut CreemOneTimeTestContext,
    ) {
        let app = ctx.create_unified_test_router();
        let webhook_secret = "test_creem_wh_secret_1time";
        let realm_id = ctx._realm_id.clone();
        let event_id = generate_test_event_id();
        let client_app_id = Uuid::parse_str(&ctx._client_app_id).unwrap();
        let entitlement_key = "one-time-800";
        let external_product_id = format!("prod_1time_{}", entitlement_key);
        let points_amount = 800;

        set_webhook_secret(ctx, webhook_secret).await;

        let user_id = create_test_user(ctx, "creem-1time-fulfill@test.com").await;
        create_points_wallet(ctx, user_id, &realm_id).await;

        let mapping_id = create_one_time_mapping(
            ctx,
            &realm_id,
            &external_product_id,
            entitlement_key,
            points_amount,
            true,
        )
        .await;

        let attempt_id = create_pending_payment_attempt(
            ctx, &realm_id, user_id, mapping_id, 999, // amount in cents
        )
        .await;

        let payload = build_checkout_completed_with_billing_kind(
            &event_id,
            entitlement_key,
            &realm_id,
            user_id,
            client_app_id,
            &external_product_id,
            "one_time",
            Some(attempt_id),
            Some("onetime"),
        );

        let response = send_webhook_with_signature(&app, &realm_id, payload, webhook_secret).await;
        assert_webhook_success(&response);

        // Verify payment attempt status is Succeeded
        let status = get_payment_attempt_status(ctx, attempt_id)
            .await
            .expect("Payment attempt should exist");
        assert_eq!(
            status, "Succeeded",
            "Payment attempt should be marked Succeeded after one-time fulfillment"
        );

        // Verify topup_balance increased by 800
        let (topup_balance, subscription_balance) =
            get_wallet_balances(ctx, user_id, &realm_id).await;
        assert!(
            topup_balance >= points_amount,
            "Expected topup_balance >= {} after one-time fulfillment, got {}",
            points_amount,
            topup_balance,
        );

        // Verify subscription_balance remains 0
        assert_eq!(
            subscription_balance, 0,
            "subscription_balance should remain 0 for one-time purchase"
        );

        // Verify points_credit_ledger has at least 1 topup_credit entry
        let ledger_count = count_ledger_entries(ctx, user_id, &realm_id, "topup_credit").await;
        assert!(
            ledger_count >= 1,
            "Expected at least 1 topup_credit ledger entry, got {}",
            ledger_count,
        );
    }

    // =========================================================================
    // Test 2: Recurring defers to subscription.paid
    // =========================================================================

    /// User Story: US-PA-003
    ///
    /// Given a recurring entitlement mapping, when checkout.completed arrives with
    /// herald_billing_kind=subscription, no points are granted and no payment attempt
    /// is completed (defers to subscription.paid).
    #[test_context(CreemOneTimeTestContext)]
    #[tokio::test]
    async fn test_creem_checkout_completed_recurring_defers_to_subscription_paid(
        ctx: &mut CreemOneTimeTestContext,
    ) {
        let app = ctx.create_unified_test_router();
        let webhook_secret = "test_creem_wh_secret_recurring";
        let realm_id = ctx._realm_id.clone();
        let event_id = generate_test_event_id();
        let client_app_id = Uuid::parse_str(&ctx._client_app_id).unwrap();
        let entitlement_key = "recurring-defer";
        let external_product_id = format!("prod_recurring_{}", entitlement_key);

        set_webhook_secret(ctx, webhook_secret).await;

        let user_id = create_test_user(ctx, "creem-recurring-defer@test.com").await;
        create_points_wallet(ctx, user_id, &realm_id).await;

        create_recurring_mapping(
            ctx,
            &realm_id,
            &external_product_id,
            entitlement_key,
            1000,
            true,
        )
        .await;

        let payload = build_checkout_completed_with_billing_kind(
            &event_id,
            entitlement_key,
            &realm_id,
            user_id,
            client_app_id,
            &external_product_id,
            "subscription",
            None, // no attempt_id -- recurring checkout typically has none
            Some("recurring"),
        );

        let response = send_webhook_with_signature(&app, &realm_id, payload, webhook_secret).await;
        assert_webhook_success(&response);

        // Verify no points were granted (balance remains 0)
        let (topup_balance, subscription_balance) =
            get_wallet_balances(ctx, user_id, &realm_id).await;
        assert_eq!(
            topup_balance, 0,
            "topup_balance should remain 0 when recurring defers to subscription.paid"
        );
        assert_eq!(
            subscription_balance, 0,
            "subscription_balance should remain 0 when recurring defers to subscription.paid"
        );

        // Verify payment event was recorded (audit trail)
        let event_count = count_payment_events(ctx, &event_id).await;
        assert_eq!(
            event_count, 1,
            "Payment event should be recorded for recurring checkout.completed"
        );
    }

    // =========================================================================
    // Test 3: billing_type from metadata priority
    // =========================================================================

    /// User Story: US-PA-003
    ///
    /// Given a mapping configured as billing_type=recurring in DB, when checkout.completed
    /// arrives with herald_billing_kind=one_time in metadata, the handler interprets the
    /// event as one-time (metadata overrides DB mapping).
    #[test_context(CreemOneTimeTestContext)]
    #[tokio::test]
    async fn test_creem_checkout_completed_billing_type_from_metadata_priority(
        ctx: &mut CreemOneTimeTestContext,
    ) {
        let app = ctx.create_unified_test_router();
        let webhook_secret = "test_creem_wh_secret_meta";
        let realm_id = ctx._realm_id.clone();
        let event_id = generate_test_event_id();
        let client_app_id = Uuid::parse_str(&ctx._client_app_id).unwrap();
        let entitlement_key = "meta-priority";
        let external_product_id = format!("prod_meta_{}", entitlement_key);
        let points_amount = 500;

        set_webhook_secret(ctx, webhook_secret).await;

        let user_id = create_test_user(ctx, "creem-meta-priority@test.com").await;
        create_points_wallet(ctx, user_id, &realm_id).await;

        // Create mapping as billing_type=recurring in DB
        let mapping_id = create_recurring_mapping(
            ctx,
            &realm_id,
            &external_product_id,
            entitlement_key,
            points_amount,
            true,
        )
        .await;

        // But send webhook with herald_billing_kind=one_time (should override)
        let attempt_id =
            create_pending_payment_attempt(ctx, &realm_id, user_id, mapping_id, 999).await;

        let payload = build_checkout_completed_with_billing_kind(
            &event_id,
            entitlement_key,
            &realm_id,
            user_id,
            client_app_id,
            &external_product_id,
            "one_time", // metadata overrides DB recurring
            Some(attempt_id),
            Some("recurring"), // product field says recurring, but metadata should win
        );

        let response = send_webhook_with_signature(&app, &realm_id, payload, webhook_secret).await;
        assert_webhook_success(&response);

        // Verify the one-time path was taken: payment attempt succeeded
        let status = get_payment_attempt_status(ctx, attempt_id)
            .await
            .expect("Payment attempt should exist");
        assert_eq!(
            status, "Succeeded",
            "Payment attempt should be Succeeded when metadata overrides to one_time"
        );
    }

    // =========================================================================
    // Test 4: billing_type from product field fallback
    // =========================================================================

    /// User Story: US-PA-003
    ///
    /// Given no herald_billing_kind in metadata, when the Creem product has
    /// billing_type=onetime, the handler normalizes it to one_time and triggers
    /// the one-time fulfillment path.
    #[test_context(CreemOneTimeTestContext)]
    #[tokio::test]
    async fn test_creem_checkout_completed_billing_type_from_product_field_fallback(
        ctx: &mut CreemOneTimeTestContext,
    ) {
        let app = ctx.create_unified_test_router();
        let webhook_secret = "test_creem_wh_secret_prod";
        let realm_id = ctx._realm_id.clone();
        let event_id = generate_test_event_id();
        let client_app_id = Uuid::parse_str(&ctx._client_app_id).unwrap();
        let entitlement_key = "prod-fallback";
        let external_product_id = format!("prod_fallback_{}", entitlement_key);
        let points_amount = 300;

        set_webhook_secret(ctx, webhook_secret).await;

        let user_id = create_test_user(ctx, "creem-prod-fallback@test.com").await;
        create_points_wallet(ctx, user_id, &realm_id).await;

        // Create mapping (billing_type does not matter -- we are testing product field fallback)
        let mapping_id = create_one_time_mapping(
            ctx,
            &realm_id,
            &external_product_id,
            entitlement_key,
            points_amount,
            true,
        )
        .await;

        let attempt_id =
            create_pending_payment_attempt(ctx, &realm_id, user_id, mapping_id, 999).await;

        // No herald_billing_kind in metadata, product field has billing_type=onetime
        let payload = build_checkout_completed_no_metadata_billing_kind(
            &event_id,
            entitlement_key,
            &realm_id,
            user_id,
            client_app_id,
            &external_product_id,
            "onetime", // Creem's raw string
            Some(attempt_id),
        );

        let response = send_webhook_with_signature(&app, &realm_id, payload, webhook_secret).await;
        assert_webhook_success(&response);

        // Verify one-time fulfillment path: payment attempt succeeded
        let status = get_payment_attempt_status(ctx, attempt_id)
            .await
            .expect("Payment attempt should exist");
        assert_eq!(
            status, "Succeeded",
            "Payment attempt should be Succeeded when product billing_type=onetime triggers one-time path"
        );
    }

    // =========================================================================
    // Test 5: onetime normalized to one_time
    // =========================================================================

    /// User Story: US-PA-003
    ///
    /// Given Creem product sends billing_type=onetime, the handler correctly
    /// normalizes it to the domain BillingType::OneTime without parsing errors
    /// or fallback to recurring.
    #[test_context(CreemOneTimeTestContext)]
    #[tokio::test]
    async fn test_creem_onetime_normalized_to_domain_one_time(ctx: &mut CreemOneTimeTestContext) {
        let app = ctx.create_unified_test_router();
        let webhook_secret = "test_creem_wh_secret_norm";
        let realm_id = ctx._realm_id.clone();
        let event_id = generate_test_event_id();
        let client_app_id = Uuid::parse_str(&ctx._client_app_id).unwrap();
        let entitlement_key = "norm-test";
        let external_product_id = format!("prod_norm_{}", entitlement_key);
        let points_amount = 400;

        set_webhook_secret(ctx, webhook_secret).await;

        let user_id = create_test_user(ctx, "creem-norm@test.com").await;
        create_points_wallet(ctx, user_id, &realm_id).await;

        let mapping_id = create_one_time_mapping(
            ctx,
            &realm_id,
            &external_product_id,
            entitlement_key,
            points_amount,
            true,
        )
        .await;

        let attempt_id =
            create_pending_payment_attempt(ctx, &realm_id, user_id, mapping_id, 999).await;

        // Use herald_billing_kind=onetime (Creem's raw format) to test normalization
        let payload = build_checkout_completed_with_billing_kind(
            &event_id,
            entitlement_key,
            &realm_id,
            user_id,
            client_app_id,
            &external_product_id,
            "onetime", // Creem raw format, should normalize to one_time
            Some(attempt_id),
            None, // no product billing_type to isolate metadata normalization
        );

        let response = send_webhook_with_signature(&app, &realm_id, payload, webhook_secret).await;
        assert_webhook_success(&response);

        // Verify the one-time path was taken: payment attempt succeeded
        let status = get_payment_attempt_status(ctx, attempt_id)
            .await
            .expect("Payment attempt should exist");
        assert_eq!(
            status, "Succeeded",
            "onetime should normalize to one_time and trigger one-time fulfillment"
        );

        // Verify points were granted as topup_credit
        let (topup_balance, _) = get_wallet_balances(ctx, user_id, &realm_id).await;
        assert!(
            topup_balance >= points_amount,
            "Expected topup_balance >= {} after onetime normalization, got {}",
            points_amount,
            topup_balance,
        );
    }

    // =========================================================================
    // Test 6: billing_type from mapping lookup fallback
    // =========================================================================

    /// User Story: US-PA-003
    ///
    /// Given no herald_billing_kind in metadata AND no billing_type in product field,
    /// when the handler looks up the mapping by provider product ID and finds
    /// billing_type=one_time, the one-time fulfillment path is triggered.
    #[test_context(CreemOneTimeTestContext)]
    #[tokio::test]
    async fn test_creem_checkout_completed_billing_type_from_mapping_lookup_fallback(
        ctx: &mut CreemOneTimeTestContext,
    ) {
        let app = ctx.create_unified_test_router();
        let webhook_secret = "test_creem_wh_secret_map";
        let realm_id = ctx._realm_id.clone();
        let event_id = generate_test_event_id();
        let client_app_id = Uuid::parse_str(&ctx._client_app_id).unwrap();
        let entitlement_key = "map-lookup";
        let external_product_id = format!("prod_maplookup_{}", entitlement_key);
        let points_amount = 600;

        set_webhook_secret(ctx, webhook_secret).await;

        let user_id = create_test_user(ctx, "creem-map-lookup@test.com").await;
        create_points_wallet(ctx, user_id, &realm_id).await;

        // Create mapping with billing_type=one_time in DB
        let mapping_id = create_one_time_mapping(
            ctx,
            &realm_id,
            &external_product_id,
            entitlement_key,
            points_amount,
            true,
        )
        .await;

        let attempt_id =
            create_pending_payment_attempt(ctx, &realm_id, user_id, mapping_id, 999).await;

        // No herald_billing_kind and no product billing_type -- falls back to mapping lookup
        let payload = build_checkout_completed_no_billing_info(
            &event_id,
            entitlement_key,
            &realm_id,
            user_id,
            client_app_id,
            &external_product_id,
            Some(attempt_id),
        );

        let response = send_webhook_with_signature(&app, &realm_id, payload, webhook_secret).await;
        assert_webhook_success(&response);

        // Verify one-time fulfillment path: payment attempt succeeded
        let status = get_payment_attempt_status(ctx, attempt_id)
            .await
            .expect("Payment attempt should exist");
        assert_eq!(
            status, "Succeeded",
            "Payment attempt should be Succeeded when mapping lookup billing_type=one_time triggers one-time path"
        );
    }

    /// User Story: US-PA-003
    ///
    /// Given checkout.completed whose product.billing_type carries an
    /// UNRECOGNIZED word form (and no herald_billing_kind metadata), the
    /// unknown form must not silently force the recurring default and
    /// short-circuit the mapping lookup — the operator-configured mapping's
    /// billing_type (one_time) wins and drives one-time fulfillment.
    #[test_context(CreemOneTimeTestContext)]
    #[tokio::test]
    async fn test_creem_checkout_completed_unknown_word_form_falls_back_to_mapping(
        ctx: &mut CreemOneTimeTestContext,
    ) {
        let app = ctx.create_unified_test_router();
        let webhook_secret = "test_creem_wh_secret_unknown_word";
        let realm_id = ctx._realm_id.clone();
        let event_id = generate_test_event_id();
        let client_app_id = Uuid::parse_str(&ctx._client_app_id).unwrap();
        let entitlement_key = "unknown-word";
        let external_product_id = format!("prod_unknownword_{}", entitlement_key);
        let points_amount = 600;

        set_webhook_secret(ctx, webhook_secret).await;

        let user_id = create_test_user(ctx, "creem-unknown-word@test.com").await;
        create_points_wallet(ctx, user_id, &realm_id).await;

        // Mapping carries the operator-configured billing_type=one_time
        let mapping_id = create_one_time_mapping(
            ctx,
            &realm_id,
            &external_product_id,
            entitlement_key,
            points_amount,
            true,
        )
        .await;

        let attempt_id =
            create_pending_payment_attempt(ctx, &realm_id, user_id, mapping_id, 999).await;

        // product.billing_type is present but NOT a recognized Creem word form
        let payload = build_checkout_completed_no_metadata_billing_kind(
            &event_id,
            entitlement_key,
            &realm_id,
            user_id,
            client_app_id,
            &external_product_id,
            "usage_based_pricing",
            Some(attempt_id),
        );

        let response = send_webhook_with_signature(&app, &realm_id, payload, webhook_secret).await;
        assert_webhook_success(&response);

        let status = get_payment_attempt_status(ctx, attempt_id)
            .await
            .expect("Payment attempt should exist");
        assert_eq!(
            status, "Succeeded",
            "an unrecognized provider word form must defer to the mapping's billing_type=one_time, not force the recurring default"
        );
    }

    // =========================================================================
    // Test 7: No attemptId -- audit only
    // =========================================================================

    /// User Story: US-PA-003
    ///
    /// Given checkout.completed with herald_billing_kind=one_time but no attemptId
    /// in metadata, the handler returns OK but does not grant points or complete
    /// any payment attempt. An audit event is recorded.
    #[test_context(CreemOneTimeTestContext)]
    #[tokio::test]
    async fn test_creem_checkout_completed_no_attempt_id_logs_only(
        ctx: &mut CreemOneTimeTestContext,
    ) {
        let app = ctx.create_unified_test_router();
        let webhook_secret = "test_creem_wh_secret_noatt";
        let realm_id = ctx._realm_id.clone();
        let event_id = generate_test_event_id();
        let client_app_id = Uuid::parse_str(&ctx._client_app_id).unwrap();
        let entitlement_key = "no-attempt";
        let external_product_id = format!("prod_noattempt_{}", entitlement_key);
        let points_amount = 200;

        set_webhook_secret(ctx, webhook_secret).await;

        let user_id = create_test_user(ctx, "creem-no-attempt@test.com").await;
        create_points_wallet(ctx, user_id, &realm_id).await;

        create_one_time_mapping(
            ctx,
            &realm_id,
            &external_product_id,
            entitlement_key,
            points_amount,
            true,
        )
        .await;

        // No attemptId in metadata
        let payload = build_checkout_completed_with_billing_kind(
            &event_id,
            entitlement_key,
            &realm_id,
            user_id,
            client_app_id,
            &external_product_id,
            "one_time",
            None, // no attemptId
            Some("onetime"),
        );

        let response = send_webhook_with_signature(&app, &realm_id, payload, webhook_secret).await;
        // Handler returns OK (audit-only path)
        assert!(
            response.status() == StatusCode::OK || response.status() == StatusCode::ACCEPTED,
            "Expected OK or ACCEPTED for checkout.completed without attemptId, got {}",
            response.status(),
        );

        // Verify no points were granted
        let (topup_balance, _) = get_wallet_balances(ctx, user_id, &realm_id).await;
        assert_eq!(
            topup_balance, 0,
            "topup_balance should remain 0 when no attemptId is provided"
        );

        // Verify audit event was recorded (payment_event exists)
        let event_count = count_payment_events(ctx, &event_id).await;
        assert_eq!(
            event_count, 1,
            "Payment event should be recorded as audit even without attemptId"
        );
    }

    // =========================================================================
    // =========================================================================

    /// Create a Creem one-time entitlement mapping that grants `role_ids` on
    /// payment (the buyout shape). Mirrors the file-local
    /// `create_one_time_mapping` but also binds `granted_role_ids`, which the
    /// role-revocation test requires.
    async fn create_creem_one_time_mapping_with_role(
        ctx: &CreemOneTimeTestContext,
        realm_id: &str,
        external_product_id: &str,
        entitlement_key: &str,
        points_per_period: i64,
        role_ids: &[Uuid],
    ) -> Uuid {
        let mapping_id = Uuid::now_v7();
        let bucket_id = crate::tests::helpers::points_helpers::ensure_test_bucket_for_realm(
            &ctx.app_state.pool,
            realm_id,
        )
        .await;
        // Distribution-rules model: bare mapping row + a fixed `topup` rule for
        // the points grant (the role-grant dimension stays on `granted_role_ids`).
        sqlx::query(
            "INSERT INTO provider_entitlement_mappings
                (id, realm_id, payment_provider, external_product_id, entitlement_key,
                 billing_type, enabled, granted_role_ids, created_at, updated_at)
             VALUES ($1, $2, 'creem', $3, $4, 'one_time', true, $5, NOW(), NOW())",
        )
        .bind(mapping_id)
        .bind(realm_id)
        .bind(external_product_id)
        .bind(entitlement_key)
        .bind(role_ids)
        .execute(&ctx.app_state.pool)
        .await
        .expect("Failed to create Creem one-time mapping with granted_role_ids");

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
        .bind(points_per_period)
        .execute(&ctx.app_state.pool)
        .await
        .expect("Failed to seed mapping-owned topup distribution rule");
        mapping_id
    }

    /// Create a Creem `payment_attempts` snapshot in the Succeeded state with
    /// `provider_reference = payment_id` so the Creem refund handler can resolve
    /// the attempt + routing bucket (mirrors the Stripe
    /// `create_stripe_succeeded_attempt` but for `payment_provider='creem'`).
    async fn create_creem_succeeded_attempt(
        ctx: &CreemOneTimeTestContext,
        realm_id: &str,
        user_id: Uuid,
        mapping_id: Uuid,
        payment_id: &str,
        amount: i64,
    ) -> Uuid {
        let attempt_id = Uuid::now_v7();
        // `payment_attempts.bucket_id` was removed by the distribution-rules
        // refactor; refund bucket resolution uses the rule snapshot
        // (`payment_attempt_point_rules`), which direct-SQL setup does not
        // materialize (async-payment balance-assertion deferral).
        sqlx::query(
            "INSERT INTO payment_attempts
                (id, realm_id, user_id, payment_provider, target_type, target_id,
                 amount, currency, status, provider_reference,
                 provider_status, expires_at, created_at, updated_at)
             VALUES ($1, $2, $3, 'creem', 'entitlement_mapping', $4,
                     $5, 'USD', 'Succeeded', $6,
                     'succeeded', NOW() + INTERVAL '1 hour', NOW(), NOW())",
        )
        .bind(attempt_id)
        .bind(realm_id)
        .bind(user_id)
        .bind(mapping_id)
        .bind(amount)
        .bind(payment_id)
        .execute(&ctx.app_state.pool)
        .await
        .expect("Failed to create Creem succeeded payment_attempt snapshot");
        attempt_id
    }

    /// Count `user_roles` rows for a user with `source='payment'` and a given
    /// `source_id`. Mirrors `paywall_m4_revoke_sweep_scenarios`.
    async fn count_payment_roles_by_source_id(
        ctx: &CreemOneTimeTestContext,
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

    /// User Story: US-PM-008 (Creem one_time refund → permanent role revoked;
    ///             points revoked; manual grants untouched).
    ///
    /// A Succeeded Creem one_time (buyout) payment grants a permanent payment-
    /// source role + topup points. A `refund.created` webhook (refundType=topup)
    /// full-channel role clawback). This is the Creem counterpart to the
    /// Stripe `test_pay_model_stripe_one_time_refund_revokes_role` test.
    #[test_context(CreemOneTimeTestContext)]
    #[tokio::test]
    async fn test_pay_model_creem_one_time_refund_revokes_role(ctx: &mut CreemOneTimeTestContext) {
        let app = ctx.create_unified_test_router();
        let webhook_secret = "test_creem_wh_secret_pm_refund";
        let realm_id = ctx._realm_id.clone();
        set_webhook_secret(ctx, webhook_secret).await;

        // Buyout role in this realm.
        let token = crate::tests::helpers::billing_helpers::setup_billing_admin_session(
            ctx,
            "pm-creem-refund-role-admin@test.com",
        )
        .await;
        let role_id = crate::tests::helpers::rbac_helpers::create_role(
            ctx,
            &realm_id,
            &token,
            "pm-creem-refund-role",
            "pay_model creem one_time refund role",
        )
        .await;

        // Creem one-time mapping: grants the role + 500 points.
        let entitlement_key = "pm-creem-refund-role";
        let external_product_id = format!("prod_creem_{entitlement_key}");
        let mapping_id = create_creem_one_time_mapping_with_role(
            ctx,
            &realm_id,
            &external_product_id,
            entitlement_key,
            500,
            &[role_id],
        )
        .await;

        let user_id = create_test_user(ctx, "pm-creem-refund-role@test.com").await;
        create_points_wallet(ctx, user_id, &realm_id).await;

        // Seed a Succeeded Creem attempt with provider_reference = payment_id.
        let payment_id = format!("pay_creem_refund_{}", Uuid::now_v7());
        let attempt_id =
            create_creem_succeeded_attempt(ctx, &realm_id, user_id, mapping_id, &payment_id, 500)
                .await;

        // Seed the payment-source role grant (source_id = attempt.id) + a
        // topup_credit ledger entry (500) the proportional revocation claws back.
        sqlx::query(
            "INSERT INTO user_roles
                (id, user_id, role_id, realm_id, client_id, principal_type, principal_id,
                 source, source_id, expires_at)
             VALUES ($1, $2, $3, $4, $5, 'user', $2::text, 'payment', $6, NULL)",
        )
        .bind(Uuid::now_v7())
        .bind(user_id)
        .bind(role_id)
        .bind(&realm_id)
        .bind(&ctx._client_id)
        .bind(attempt_id.to_string())
        .execute(&ctx.app_state.pool)
        .await
        .expect("seed payment role grant");

        // Seed a rule-attributed topup_credit ledger mirroring
        // `fulfill_one_time_purchase` output, so the refund's
        // `revoke_topup_source_proportional(source_id = attempt.id)` finds and
        // revokes the grant. A raw unattributed ledger would be silently
        // skipped (the revoke JOINs distribution_events on source_id and
        // requires distribution_rule_id IS NOT NULL).
        let _ledger_id =
            crate::tests::helpers::points_helpers::seed_fulfilled_topup_ledger_for_attempt(
                ctx, &realm_id, user_id, attempt_id, 500, None,
            )
            .await;

        // Pre-condition: 1 payment role grant + 500 topup.
        assert_eq!(
            count_payment_roles_by_source_id(ctx, user_id, &attempt_id.to_string()).await,
            1,
            "pre-condition: payment role grant present"
        );
        let (topup_before, _) = get_wallet_balances(ctx, user_id, &realm_id).await;
        assert_eq!(topup_before, 500, "pre-condition: 500 topup granted");

        // Exercise: refund.created (topup). Uses the shared
        // `build_refund_created_event_with_user_and_type` builder.
        let refund_event_id = generate_test_event_id();
        let refund_id = format!("re_pm_creem_{refund_event_id}");
        let payload =
            crate::tests::helpers::webhook_helpers::build_refund_created_event_with_user_and_type(
                refund_event_id,
                refund_id,
                payment_id,
                500, // refund amount
                500, // original amount
                &realm_id,
                user_id,
                "topup",
            );
        let response = send_webhook_with_signature(&app, &realm_id, payload, webhook_secret).await;
        assert!(
            response.status() == StatusCode::OK || response.status() == StatusCode::ACCEPTED,
            "refund webhook must return success, got {}",
            response.status()
        );

        assert_eq!(
            count_payment_roles_by_source_id(ctx, user_id, &attempt_id.to_string()).await,
            0,
            "Creem one_time refund must revoke the permanent payment-source role (DEC-pay_model-004)"
        );

        // And: the topup points were also revoked (decoupled revocation).
        let (topup_after, _) = get_wallet_balances(ctx, user_id, &realm_id).await;
        assert_eq!(
            topup_after, 0,
            "Creem one_time refund must also revoke the topup points"
        );
    }
}
