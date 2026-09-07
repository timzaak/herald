// =============================================================================
// Creem Webhook Patch Scenario Tests
// =============================================================================
//
// Tests for Creem webhook handlers added in the billing patch:
// - Subscription lifecycle: active, trialing, paused, past_due, scheduled_cancel, expired
// - Disputes: dispute.created
// - Idempotency: subscription.scheduled_cancel duplicate event_id
//
// Covers: subscription lifecycle events, dispute.created
// =============================================================================

#[cfg(test)]
mod tests {
    use crate::tests::helpers::billing_helpers::setup_test_entitlement_mapping_full;
    use crate::tests::helpers::webhook_helpers::{
        generate_test_event_id, send_webhook_with_signature,
    };
    use crate::tests::schema_test_context::SchemaTestContext;
    use axum::http::StatusCode;
    use serde_json::json;
    use test_context::test_context;
    use uuid::Uuid;

    use SchemaTestContext as WebhookPatchTestContext;

    // =========================================================================
    // Shared Helpers
    // =========================================================================

    /// Create a test user in the test realm and return their UUID.
    async fn create_test_user(ctx: &SchemaTestContext, realm_id: &str, email: &str) -> Uuid {
        let user_id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO account (id, realm_id, email, password, status)
             VALUES ($1, $2, $3, $4, 1)
             ON CONFLICT (realm_id, email) DO NOTHING",
        )
        .bind(user_id)
        .bind(realm_id)
        .bind(email)
        .bind("$2a$12$dummy_password_hash")
        .execute(&ctx.app_state.pool)
        .await
        .expect("Failed to create test user");
        user_id
    }

    /// Create a points wallet for a user with zero balances.
    async fn create_points_wallet(ctx: &SchemaTestContext, user_id: Uuid, realm_id: &str) {
        let bucket_id = crate::tests::helpers::points_helpers::ensure_test_bucket_for_realm(
            &ctx.app_state.pool,
            realm_id,
        )
        .await;
        // point-time: per-type balance columns were dropped; the ledger
        // row inserted below by `create_subscription_credit_with_ledger` is the
        // source of truth for the derived subscription_balance.
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

    #[allow(dead_code)]
    /// Create a points wallet for a user with an initial subscription_balance.
    async fn create_points_wallet_with_sub_balance(
        ctx: &SchemaTestContext,
        user_id: Uuid,
        realm_id: &str,
        subscription_balance: i64,
    ) {
        let bucket_id = crate::tests::helpers::points_helpers::ensure_test_bucket_for_realm(
            &ctx.app_state.pool,
            realm_id,
        )
        .await;
        sqlx::query(
            "INSERT INTO points_wallets (id, user_id, realm_id, bucket_id, total_topup_granted, total_subscription_granted, total_recharged, total_consumed, status, created_at, updated_at)
             VALUES ($1, $2, $3, $4, 0, $5, $5, 0, 'active', NOW(), NOW())
             ON CONFLICT (realm_id, user_id, bucket_id) DO NOTHING",
        )
        .bind(Uuid::now_v7())
        .bind(user_id)
        .bind(realm_id)
        .bind(bucket_id)
        .bind(subscription_balance)
        .execute(&ctx.app_state.pool)
        .await
        .expect("Failed to create points wallet with sub balance");

        // The subscription_balance is no longer Stored on points_wallets;
        // seed a points_credit_ledger row so the derived balance reflects the
        // requested initial subscription_balance (mirrors
        // create_points_wallet_with_balance in points_helpers.rs).
        if subscription_balance > 0 {
            sqlx::query(
                "INSERT INTO points_credit_ledger (id, user_id, realm_id, bucket_id, credit_type, source_type, source_id, granted_amount, used_amount, revoked_amount, status, created_at, updated_at)
                 VALUES ($1, $2, $3, $4, 'subscription_credit', 'system_grant', $5, $6, 0, 0, 'active', NOW(), NOW())",
            )
            .bind(Uuid::now_v7())
            .bind(user_id)
            .bind(realm_id)
            .bind(bucket_id)
            .bind(format!("seed-sub-{}", user_id))
            .bind(subscription_balance)
            .execute(&ctx.app_state.pool)
            .await
            .expect("Failed to seed subscription ledger for create_points_wallet_with_sub_balance");
        }
    }

    /// Create a quota-entitlement-backed subscription credit entry attributed
    /// to a completed subscription-period distribution event + rule, mirroring
    /// production `handle_subscription_paid` output. The attributed event/rule
    /// are required for a subsequent cancel/refund/expiry/dispute webhook's
    /// `revoke_distribution_source_in_tx` to find and revoke the entitlement
    /// (it JOINs `q.distribution_event_id = e.id` and requires
    /// `q.distribution_rule_id IS NOT NULL`).
    async fn create_subscription_credit_with_ledger(
        ctx: &SchemaTestContext,
        user_id: Uuid,
        realm_id: &str,
        amount: i64,
        source_id: &str,
    ) {
        let bucket_id = crate::tests::helpers::points_helpers::ensure_test_bucket_for_realm(
            &ctx.app_state.pool,
            realm_id,
        )
        .await;
        // `source_id` arrives as `"{entitlement_key}:{subscription_uuid}"; the
        // subscription UUID is the revoke key production matches on.
        let subscription_id = source_id
            .rsplit(':')
            .next()
            .filter(|part| Uuid::parse_str(part).is_ok())
            .map(|part| Uuid::parse_str(part).expect("subscription id suffix is a valid UUID"))
            .expect("source_id must carry a subscription UUID suffix");
        let effective_from = chrono::Utc::now() - chrono::Duration::seconds(1);
        let effective_until = Some(chrono::Utc::now() + chrono::Duration::days(30));

        crate::tests::helpers::points_helpers::seed_attributed_subscription_quota(
            ctx,
            realm_id,
            user_id,
            subscription_id,
            bucket_id,
            herald_core::domain::points::entities::CreditType::SubscriptionCredit,
            herald_core::domain::points::entities::QuotaSourceType::SubscriptionInitial,
            &[(2_592_000, amount, "period")],
            effective_from,
            effective_until,
        )
        .await;

        sqlx::query(
            "UPDATE points_wallets
             SET total_subscription_granted = total_subscription_granted + $1,
                 updated_at = NOW()
             WHERE user_id = $2 AND realm_id = $3",
        )
        .bind(amount)
        .bind(user_id)
        .bind(realm_id)
        .execute(&ctx.app_state.pool)
        .await
        .expect("Failed to update wallet balance");
    }

    #[allow(dead_code)]
    /// Create a one-time entitlement mapping with points.
    /// Returns the mapping ID.
    async fn create_one_time_mapping(
        ctx: &mut SchemaTestContext,
        realm_id: &str,
        provider: &str,
        entitlement_key: &str,
        points_per_period: i64,
    ) -> Uuid {
        setup_test_entitlement_mapping_full(
            ctx,
            realm_id,
            provider,
            &format!("prod_{}_{}", provider, entitlement_key),
            None,
            entitlement_key,
            Some("one_time"),
            None,
            Some(points_per_period),
            None,
            None,
            false,
            None,
            true,
            None,
        )
        .await
    }

    /// Create a recurring entitlement mapping with points.
    /// Returns the mapping ID.
    async fn create_recurring_mapping(
        ctx: &mut SchemaTestContext,
        realm_id: &str,
        provider: &str,
        entitlement_key: &str,
        points_per_period: i64,
    ) -> Uuid {
        setup_test_entitlement_mapping_full(
            ctx,
            realm_id,
            provider,
            &format!("prod_{}_{}", provider, entitlement_key),
            None,
            entitlement_key,
            Some("recurring"),
            Some("monthly"),
            Some(points_per_period),
            None,
            None,
            true,
            None,
            true,
            None,
        )
        .await
    }

    /// Pre-create a subscription via SQL with the given fields.
    /// Returns the subscription ID (internal UUID).
    async fn pre_create_subscription(
        ctx: &SchemaTestContext,
        realm_id: &str,
        user_id: Uuid,
        client_app_id: Uuid,
        external_subscription_id: &str,
        external_product_id: &str,
        payment_provider: &str,
        status: &str,
        entitlement_key: &str,
    ) -> Uuid {
        let subscription_id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO subscription
                (id, realm_id, user_id, external_subscription_id, external_product_id,
                 payment_provider, status, entitlement_key, external_price_id,
                 provider_metadata, synced_at, current_period_start, current_period_end,
                 cancel_at_period_end, client_app_id, cancel_at, created_at, updated_at,
                 billing_type)
             VALUES ($1, $2, $3, $4, $5,
                     $6, $7, $8, NULL,
                     NULL, NOW(), NOW(), NOW() + INTERVAL '30 days',
                     false, $9, NULL, NOW(), NOW(), 'recurring')",
        )
        .bind(subscription_id)
        .bind(realm_id)
        .bind(user_id)
        .bind(external_subscription_id)
        .bind(external_product_id)
        .bind(payment_provider)
        .bind(status)
        .bind(entitlement_key)
        .bind(client_app_id)
        .execute(&ctx.app_state.pool)
        .await
        .expect("Failed to pre-create subscription");
        subscription_id
    }

    /// Get subscription status by internal ID.
    async fn get_subscription_status(ctx: &SchemaTestContext, subscription_id: Uuid) -> String {
        sqlx::query_scalar::<_, String>("SELECT status FROM subscription WHERE id = $1")
            .bind(subscription_id)
            .fetch_one(&ctx.app_state.pool)
            .await
            .expect("Subscription not found")
    }

    /// Get cancel_at_period_end for a subscription.
    async fn get_cancel_at_period_end(ctx: &SchemaTestContext, subscription_id: Uuid) -> bool {
        sqlx::query_scalar::<_, bool>(
            "SELECT COALESCE(cancel_at_period_end, false) FROM subscription WHERE id = $1",
        )
        .bind(subscription_id)
        .fetch_one(&ctx.app_state.pool)
        .await
        .unwrap_or(false)
    }

    #[allow(dead_code)]
    /// Get topup_balance for a user's wallet.
    /// `points_wallets.topup_balance` was dropped; the available topup
    /// balance is the derived SUM over `points_credit_ledger` topup/registration/
    /// free_periodic rows (same predicate as production).
    async fn get_topup_balance(ctx: &SchemaTestContext, user_id: Uuid, realm_id: &str) -> i64 {
        sqlx::query_scalar::<_, i64>(
            "SELECT COALESCE(SUM(l.remaining_amount) FILTER (
                        WHERE l.status = 'active' AND l.remaining_amount > 0
                          AND (l.effective_at IS NULL OR l.effective_at <= NOW())
                          AND (l.expires_at  IS NULL OR l.expires_at  >  NOW())
                    ), 0)::BIGINT
             FROM points_credit_ledger l
             WHERE l.user_id = $1 AND l.realm_id = $2
               AND l.credit_type IN ('topup_credit','registration_credit','free_periodic_credit')",
        )
        .bind(user_id)
        .bind(realm_id)
        .fetch_one(&ctx.app_state.pool)
        .await
        .unwrap_or(0)
    }

    /// Get subscription window availability for a user's wallet.
    async fn get_subscription_balance(
        ctx: &SchemaTestContext,
        user_id: Uuid,
        realm_id: &str,
    ) -> i64 {
        let bucket_id = crate::tests::helpers::points_helpers::ensure_test_bucket_for_realm(
            &ctx.app_state.pool,
            realm_id,
        )
        .await;
        crate::tests::helpers::points_helpers::compute_window_available(
            ctx,
            realm_id,
            user_id,
            bucket_id,
            herald_core::domain::points::entities::CreditType::SubscriptionCredit,
        )
        .await
    }

    #[allow(dead_code)]
    /// Create a pending payment attempt targeting the given mapping.
    /// Returns the attempt ID.
    async fn create_pending_payment_attempt(
        ctx: &SchemaTestContext,
        realm_id: &str,
        user_id: Uuid,
        mapping_id: Uuid,
        payment_provider: &str,
    ) -> Uuid {
        let attempt_id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO payment_attempts
                (id, realm_id, user_id, payment_provider, target_type, target_id,
                 amount, currency, status, expires_at, created_at, updated_at)
             VALUES ($1, $2, $3, $4, 'entitlement_mapping', $5,
                 1000, 'usd', 'Pending', NOW() + INTERVAL '1 hour', NOW(), NOW())",
        )
        .bind(attempt_id)
        .bind(realm_id)
        .bind(user_id)
        .bind(payment_provider)
        .bind(mapping_id)
        .execute(&ctx.app_state.pool)
        .await
        .expect("Failed to create pending payment attempt");
        attempt_id
    }

    #[allow(dead_code)]
    /// Get payment attempt status by ID.
    async fn get_payment_attempt_status(
        ctx: &SchemaTestContext,
        attempt_id: Uuid,
    ) -> Option<String> {
        sqlx::query_scalar::<_, String>("SELECT status FROM payment_attempts WHERE id = $1")
            .bind(attempt_id)
            .fetch_optional(&ctx.app_state.pool)
            .await
            .unwrap_or(None)
    }

    /// Count subscription_history entries for a given subscription_id and event_type.
    async fn count_history_events(
        ctx: &SchemaTestContext,
        subscription_id: Uuid,
        event_type: &str,
    ) -> i64 {
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM subscription_history WHERE subscription_id = $1 AND event_type = $2",
        )
        .bind(subscription_id)
        .bind(event_type)
        .fetch_one(&ctx.app_state.pool)
        .await
        .unwrap_or(0)
    }

    #[allow(dead_code)]
    /// Count payment_event rows for a given external_event_id.
    async fn count_payment_events(ctx: &SchemaTestContext, external_event_id: &str) -> i64 {
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM payment_event WHERE external_event_id = $1",
        )
        .bind(external_event_id)
        .fetch_one(&ctx.app_state.pool)
        .await
        .unwrap_or(0)
    }

    /// Get entitlement_key for a subscription by internal ID.
    async fn get_entitlement_key(ctx: &SchemaTestContext, subscription_id: Uuid) -> String {
        sqlx::query_scalar::<_, String>("SELECT entitlement_key FROM subscription WHERE id = $1")
            .bind(subscription_id)
            .fetch_one(&ctx.app_state.pool)
            .await
            .expect("Subscription not found")
    }

    // =========================================================================
    // Creem Payload Builders
    // =========================================================================

    fn build_creem_subscription_lifecycle(
        event_id: &str,
        event_type: &str,
        external_subscription_id: &str,
        external_product_id: &str,
        realm_id: &str,
        user_id: Uuid,
        status: &str,
        cancel_at_period_end: bool,
        entitlement_key: &str,
    ) -> serde_json::Value {
        let period_end = chrono::Utc::now() + chrono::Duration::days(30);
        json!({
            "id": event_id,
            "eventType": event_type,
            "data": {
                "object": {
                    "subscriptionId": external_subscription_id,
                    "productId": external_product_id,
                    "userId": user_id.to_string(),
                    "status": status,
                    "cancelAtPeriodEnd": cancel_at_period_end,
                    "currentPeriodStart": chrono::Utc::now().to_rfc3339(),
                    "currentPeriodEnd": period_end.to_rfc3339(),
                    "metadata": {
                        "herald_entitlement_key": entitlement_key,
                        "herald_realm_id": realm_id,
                        "herald_user_id": user_id.to_string(),
                    }
                }
            }
        })
    }

    fn build_creem_dispute_created(
        event_id: &str,
        external_subscription_id: &str,
        external_product_id: &str,
    ) -> serde_json::Value {
        json!({
            "id": event_id,
            "eventType": "dispute.created",
            "data": {
                "object": {
                    "id": format!("dispute_{}", Uuid::now_v7()),
                    "amount": 1000,
                    "currency": "USD",
                    "subscription": {
                        "id": external_subscription_id,
                        "product": external_product_id,
                    },
                    "transaction": {
                        "subscription": external_subscription_id,
                    }
                }
            }
        })
    }

    fn build_creem_subscription_canceled(
        event_id: &str,
        external_subscription_id: &str,
        external_product_id: &str,
        realm_id: &str,
        user_id: Uuid,
        cancel_at_period_end: bool,
        entitlement_key: &str,
    ) -> serde_json::Value {
        let period_end = chrono::Utc::now() + chrono::Duration::days(30);
        json!({
            "id": event_id,
            "eventType": "subscription.canceled",
            "data": {
                "object": {
                    "subscriptionId": external_subscription_id,
                    "productId": external_product_id,
                    "userId": user_id.to_string(),
                    "status": "canceled",
                    "cancelAtPeriodEnd": cancel_at_period_end,
                    "currentPeriodStart": chrono::Utc::now().to_rfc3339(),
                    "currentPeriodEnd": period_end.to_rfc3339(),
                    "metadata": {
                        "herald_entitlement_key": entitlement_key,
                        "herald_realm_id": realm_id,
                        "herald_user_id": user_id.to_string(),
                    }
                }
            }
        })
    }

    fn build_creem_subscription_update(
        event_id: &str,
        external_subscription_id: &str,
        external_product_id: &str,
        realm_id: &str,
        user_id: Uuid,
        new_entitlement_key: &str,
        old_entitlement_key: &str,
    ) -> serde_json::Value {
        let period_end = chrono::Utc::now() + chrono::Duration::days(30);
        json!({
            "id": event_id,
            "eventType": "subscription.update",
            "data": {
                "object": {
                    "subscriptionId": external_subscription_id,
                    "productId": external_product_id,
                    "userId": user_id.to_string(),
                    "status": "active",
                    "cancelAtPeriodEnd": false,
                    "currentPeriodStart": chrono::Utc::now().to_rfc3339(),
                    "currentPeriodEnd": period_end.to_rfc3339(),
                    "metadata": {
                        "herald_entitlement_key": new_entitlement_key,
                        "herald_realm_id": realm_id,
                        "herald_user_id": user_id.to_string(),
                    }
                },
                "previousAttributes": {
                    "herald_entitlement_key": old_entitlement_key
                }
            }
        })
    }

    // =========================================================================
    // Test 11: Creem subscription.active syncs status
    // =========================================================================

    /// Covers: subscription.active -> reactivate from paused
    ///
    /// Given: Creem config, a recurring mapping, a user, and a subscription in 'paused' status
    /// When: Creem subscription.active arrives matching the external_subscription_id
    /// Then: Subscription status becomes "active" and history has "reactivated" event.
    #[test_context(WebhookPatchTestContext)]
    #[tokio::test]
    async fn test_creem_subscription_active_syncs_status(ctx: &mut WebhookPatchTestContext) {
        let app = ctx.create_unified_test_router();
        let webhook_secret = "whsec_creem_active";
        let realm_id = ctx._realm_id.clone();
        let client_app_id = Uuid::parse_str(&ctx._client_app_id).unwrap();
        let entitlement_key = "patch-creem-active";

        ctx.with_creem_config(
            &realm_id,
            Some("test_api_key"),
            Some(webhook_secret),
            Some(30),
        )
        .await;
        create_recurring_mapping(ctx, &realm_id, "creem", entitlement_key, 500).await;
        let user_id = create_test_user(ctx, &realm_id, "patch-creem-active@test.com").await;

        let ext_sub_id = format!("sub_creem_{}", Uuid::now_v7());
        let sub_id = pre_create_subscription(
            ctx,
            &realm_id,
            user_id,
            client_app_id,
            &ext_sub_id,
            &format!("prod_creem_{}", entitlement_key),
            "creem",
            "paused",
            entitlement_key,
        )
        .await;

        let event_id = generate_test_event_id();
        let payload = build_creem_subscription_lifecycle(
            &event_id,
            "subscription.active",
            &ext_sub_id,
            &format!("prod_creem_{}", entitlement_key),
            &realm_id,
            user_id,
            "active",
            false,
            entitlement_key,
        );
        let response = send_webhook_with_signature(&app, &realm_id, payload, webhook_secret).await;
        assert_eq!(response.status(), StatusCode::OK, "Expected 200 OK");

        let status = get_subscription_status(ctx, sub_id).await;
        assert_eq!(status, "active", "Subscription status should be 'active'");

        let history_count = count_history_events(ctx, sub_id, "reactivated").await;
        assert_eq!(
            history_count, 1,
            "Expected exactly 1 'reactivated' history event"
        );
    }

    // =========================================================================
    // Test 12: Creem subscription.trialing syncs status
    // =========================================================================

    /// Covers: subscription.trialing -> status=trialing, history=created
    ///
    /// Given: Creem config, a recurring mapping, a user, and a subscription
    /// When: Creem subscription.trialing arrives
    /// Then: Subscription status becomes "trialing" and history has "created" event.
    #[test_context(WebhookPatchTestContext)]
    #[tokio::test]
    async fn test_creem_subscription_trialing_syncs_status(ctx: &mut WebhookPatchTestContext) {
        let app = ctx.create_unified_test_router();
        let webhook_secret = "whsec_creem_trial";
        let realm_id = ctx._realm_id.clone();
        let client_app_id = Uuid::parse_str(&ctx._client_app_id).unwrap();
        let entitlement_key = "patch-creem-trial";

        ctx.with_creem_config(
            &realm_id,
            Some("test_api_key"),
            Some(webhook_secret),
            Some(30),
        )
        .await;
        create_recurring_mapping(ctx, &realm_id, "creem", entitlement_key, 500).await;
        let user_id = create_test_user(ctx, &realm_id, "patch-creem-trial@test.com").await;

        let ext_sub_id = format!("sub_creem_{}", Uuid::now_v7());
        let sub_id = pre_create_subscription(
            ctx,
            &realm_id,
            user_id,
            client_app_id,
            &ext_sub_id,
            &format!("prod_creem_{}", entitlement_key),
            "creem",
            // pending is the only legal predecessor of trialing under the
            // webhook status-transition guard (active -> trialing is rejected)
            "pending",
            entitlement_key,
        )
        .await;

        let event_id = generate_test_event_id();
        let payload = build_creem_subscription_lifecycle(
            &event_id,
            "subscription.trialing",
            &ext_sub_id,
            &format!("prod_creem_{}", entitlement_key),
            &realm_id,
            user_id,
            "trialing",
            false,
            entitlement_key,
        );
        let response = send_webhook_with_signature(&app, &realm_id, payload, webhook_secret).await;
        assert_eq!(response.status(), StatusCode::OK, "Expected 200 OK");

        let status = get_subscription_status(ctx, sub_id).await;
        assert_eq!(
            status, "trialing",
            "Subscription status should be 'trialing'"
        );

        let history_count = count_history_events(ctx, sub_id, "created").await;
        assert_eq!(
            history_count, 1,
            "Expected exactly 1 'created' history event"
        );
    }

    // =========================================================================
    // Test 13: Creem subscription.paused syncs status
    // =========================================================================

    /// Covers: subscription.paused -> status=paused, history=paused
    ///
    /// Given: Creem config, a recurring mapping, a user, and a subscription in 'active' status
    /// When: Creem subscription.paused arrives
    /// Then: Subscription status becomes "paused" and history has "paused" event.
    #[test_context(WebhookPatchTestContext)]
    #[tokio::test]
    async fn test_creem_subscription_paused_syncs_status(ctx: &mut WebhookPatchTestContext) {
        let app = ctx.create_unified_test_router();
        let webhook_secret = "whsec_creem_paused";
        let realm_id = ctx._realm_id.clone();
        let client_app_id = Uuid::parse_str(&ctx._client_app_id).unwrap();
        let entitlement_key = "patch-creem-paused";

        ctx.with_creem_config(
            &realm_id,
            Some("test_api_key"),
            Some(webhook_secret),
            Some(30),
        )
        .await;
        create_recurring_mapping(ctx, &realm_id, "creem", entitlement_key, 500).await;
        let user_id = create_test_user(ctx, &realm_id, "patch-creem-paused@test.com").await;

        let ext_sub_id = format!("sub_creem_{}", Uuid::now_v7());
        let sub_id = pre_create_subscription(
            ctx,
            &realm_id,
            user_id,
            client_app_id,
            &ext_sub_id,
            &format!("prod_creem_{}", entitlement_key),
            "creem",
            "active",
            entitlement_key,
        )
        .await;

        let event_id = generate_test_event_id();
        let payload = build_creem_subscription_lifecycle(
            &event_id,
            "subscription.paused",
            &ext_sub_id,
            &format!("prod_creem_{}", entitlement_key),
            &realm_id,
            user_id,
            "paused",
            false,
            entitlement_key,
        );
        let response = send_webhook_with_signature(&app, &realm_id, payload, webhook_secret).await;
        assert_eq!(response.status(), StatusCode::OK, "Expected 200 OK");

        let status = get_subscription_status(ctx, sub_id).await;
        assert_eq!(status, "paused", "Subscription status should be 'paused'");

        let history_count = count_history_events(ctx, sub_id, "paused").await;
        assert_eq!(
            history_count, 1,
            "Expected exactly 1 'paused' history event"
        );
    }

    // =========================================================================
    // Test 14: Creem subscription.past_due syncs status (no credit revoke)
    // =========================================================================

    /// Covers: subscription.past_due -> status=past_due, no credit revoke
    ///
    /// Given: Creem config, a recurring mapping, a user, and a subscription in 'active' status
    /// When: Creem subscription.past_due arrives
    /// Then: Subscription status becomes "past_due" and history has "past_due" event.
    /// Note: past_due does NOT revoke credits -- only Expired does.
    #[test_context(WebhookPatchTestContext)]
    #[tokio::test]
    async fn test_creem_subscription_past_due_syncs_status(ctx: &mut WebhookPatchTestContext) {
        let app = ctx.create_unified_test_router();
        let webhook_secret = "whsec_creem_past_due";
        let realm_id = ctx._realm_id.clone();
        let client_app_id = Uuid::parse_str(&ctx._client_app_id).unwrap();
        let entitlement_key = "patch-creem-past-due";

        ctx.with_creem_config(
            &realm_id,
            Some("test_api_key"),
            Some(webhook_secret),
            Some(30),
        )
        .await;
        create_recurring_mapping(ctx, &realm_id, "creem", entitlement_key, 500).await;
        let user_id = create_test_user(ctx, &realm_id, "patch-creem-past-due@test.com").await;

        let ext_sub_id = format!("sub_creem_{}", Uuid::now_v7());
        let sub_id = pre_create_subscription(
            ctx,
            &realm_id,
            user_id,
            client_app_id,
            &ext_sub_id,
            &format!("prod_creem_{}", entitlement_key),
            "creem",
            "active",
            entitlement_key,
        )
        .await;

        let event_id = generate_test_event_id();
        let payload = build_creem_subscription_lifecycle(
            &event_id,
            "subscription.past_due",
            &ext_sub_id,
            &format!("prod_creem_{}", entitlement_key),
            &realm_id,
            user_id,
            "past_due",
            false,
            entitlement_key,
        );
        let response = send_webhook_with_signature(&app, &realm_id, payload, webhook_secret).await;
        assert_eq!(response.status(), StatusCode::OK, "Expected 200 OK");

        let status = get_subscription_status(ctx, sub_id).await;
        assert_eq!(
            status, "past_due",
            "Subscription status should be 'past_due'"
        );

        let history_count = count_history_events(ctx, sub_id, "past_due").await;
        assert_eq!(
            history_count, 1,
            "Expected exactly 1 'past_due' history event"
        );
    }

    // =========================================================================
    // Test 15: Creem subscription.scheduled_cancel syncs status
    // =========================================================================

    /// Covers: subscription.scheduled_cancel -> status=scheduled_cancel + cancel_at_period_end=true
    ///
    /// Given: Creem config, a recurring mapping, a user, and a subscription in 'active' status
    /// When: Creem subscription.scheduled_cancel arrives with currentPeriodEnd in the future
    /// Then: Subscription status becomes "scheduled_cancel", cancel_at_period_end = true,
    ///       and history has "scheduled_cancel" event.
    #[test_context(WebhookPatchTestContext)]
    #[tokio::test]
    async fn test_creem_subscription_scheduled_cancel_syncs_status(
        ctx: &mut WebhookPatchTestContext,
    ) {
        let app = ctx.create_unified_test_router();
        let webhook_secret = "whsec_creem_sched";
        let realm_id = ctx._realm_id.clone();
        let client_app_id = Uuid::parse_str(&ctx._client_app_id).unwrap();
        let entitlement_key = "patch-creem-sched";

        ctx.with_creem_config(
            &realm_id,
            Some("test_api_key"),
            Some(webhook_secret),
            Some(30),
        )
        .await;
        create_recurring_mapping(ctx, &realm_id, "creem", entitlement_key, 500).await;
        let user_id = create_test_user(ctx, &realm_id, "patch-creem-sched@test.com").await;

        let ext_sub_id = format!("sub_creem_{}", Uuid::now_v7());
        let sub_id = pre_create_subscription(
            ctx,
            &realm_id,
            user_id,
            client_app_id,
            &ext_sub_id,
            &format!("prod_creem_{}", entitlement_key),
            "creem",
            "active",
            entitlement_key,
        )
        .await;

        let event_id = generate_test_event_id();
        let payload = build_creem_subscription_lifecycle(
            &event_id,
            "subscription.scheduled_cancel",
            &ext_sub_id,
            &format!("prod_creem_{}", entitlement_key),
            &realm_id,
            user_id,
            "scheduled_cancel",
            true,
            entitlement_key,
        );
        let response = send_webhook_with_signature(&app, &realm_id, payload, webhook_secret).await;
        assert_eq!(response.status(), StatusCode::OK, "Expected 200 OK");

        let status = get_subscription_status(ctx, sub_id).await;
        assert_eq!(
            status, "scheduled_cancel",
            "Subscription status should be 'scheduled_cancel'"
        );

        let cancel_at_end = get_cancel_at_period_end(ctx, sub_id).await;
        assert!(
            cancel_at_end,
            "cancel_at_period_end should be true for scheduled_cancel"
        );

        let history_count = count_history_events(ctx, sub_id, "scheduled_cancel").await;
        assert_eq!(
            history_count, 1,
            "Expected exactly 1 'scheduled_cancel' history event"
        );
    }

    // =========================================================================
    // Test 16: Creem subscription.expired revokes credits
    // =========================================================================

    /// Covers: subscription.expired -> status=expired, subscription_balance=0 (credits revoked)
    ///
    /// Given: Creem config, a recurring mapping, a user + wallet with subscription_balance > 0,
    ///        and a subscription in 'active' status
    /// When: Creem subscription.expired arrives
    /// Then: Subscription status becomes "expired", subscription_balance = 0 (credits revoked),
    ///       and history has "expired" event.
    #[test_context(WebhookPatchTestContext)]
    #[tokio::test]
    async fn test_creem_subscription_expired_revokes_credits(ctx: &mut WebhookPatchTestContext) {
        let app = ctx.create_unified_test_router();
        let webhook_secret = "whsec_creem_expired";
        let realm_id = ctx._realm_id.clone();
        let client_app_id = Uuid::parse_str(&ctx._client_app_id).unwrap();
        let entitlement_key = "patch-creem-expired";

        ctx.with_creem_config(
            &realm_id,
            Some("test_api_key"),
            Some(webhook_secret),
            Some(30),
        )
        .await;
        create_recurring_mapping(ctx, &realm_id, "creem", entitlement_key, 500).await;
        let user_id = create_test_user(ctx, &realm_id, "patch-creem-expired@test.com").await;
        create_points_wallet(ctx, user_id, &realm_id).await;

        let ext_sub_id = format!("sub_creem_{}", Uuid::now_v7());
        let sub_id = pre_create_subscription(
            ctx,
            &realm_id,
            user_id,
            client_app_id,
            &ext_sub_id,
            &format!("prod_creem_{}", entitlement_key),
            "creem",
            "active",
            entitlement_key,
        )
        .await;

        create_subscription_credit_with_ledger(
            ctx,
            user_id,
            &realm_id,
            500,
            &format!("{}:{}", entitlement_key, sub_id),
        )
        .await;

        let event_id = generate_test_event_id();
        let payload = build_creem_subscription_lifecycle(
            &event_id,
            "subscription.expired",
            &ext_sub_id,
            &format!("prod_creem_{}", entitlement_key),
            &realm_id,
            user_id,
            "expired",
            false,
            entitlement_key,
        );
        let response = send_webhook_with_signature(&app, &realm_id, payload, webhook_secret).await;
        assert_eq!(response.status(), StatusCode::OK, "Expected 200 OK");

        let status = get_subscription_status(ctx, sub_id).await;
        assert_eq!(status, "expired", "Subscription status should be 'expired'");

        let sub_balance = get_subscription_balance(ctx, user_id, &realm_id).await;
        assert_eq!(
            sub_balance, 0,
            "subscription_balance should be 0 after subscription expired (credits revoked)"
        );

        let history_count = count_history_events(ctx, sub_id, "expired").await;
        assert_eq!(
            history_count, 1,
            "Expected exactly 1 'expired' history event"
        );
    }

    // =========================================================================
    // Test 17: Creem dispute.created sets dispute status
    // =========================================================================

    /// Covers: Creem dispute.created -> status=dispute
    ///
    /// Given: Creem config, a recurring mapping, a user, and a subscription in 'active' status
    /// When: Creem dispute.created arrives with subscription.id matching external_subscription_id
    /// Then: Subscription status becomes "dispute" and history has "disputed" event.
    #[test_context(WebhookPatchTestContext)]
    #[tokio::test]
    async fn test_creem_dispute_created_sets_dispute_status(ctx: &mut WebhookPatchTestContext) {
        let app = ctx.create_unified_test_router();
        let webhook_secret = "whsec_creem_dispute";
        let realm_id = ctx._realm_id.clone();
        let client_app_id = Uuid::parse_str(&ctx._client_app_id).unwrap();
        let entitlement_key = "patch-creem-dispute";

        ctx.with_creem_config(
            &realm_id,
            Some("test_api_key"),
            Some(webhook_secret),
            Some(30),
        )
        .await;
        create_recurring_mapping(ctx, &realm_id, "creem", entitlement_key, 500).await;
        let user_id = create_test_user(ctx, &realm_id, "patch-creem-dispute@test.com").await;

        let ext_sub_id = format!("sub_creem_{}", Uuid::now_v7());
        let ext_product_id = format!("prod_creem_{}", entitlement_key);
        let sub_id = pre_create_subscription(
            ctx,
            &realm_id,
            user_id,
            client_app_id,
            &ext_sub_id,
            &ext_product_id,
            "creem",
            "active",
            entitlement_key,
        )
        .await;

        let event_id = generate_test_event_id();
        let payload = build_creem_dispute_created(&event_id, &ext_sub_id, &ext_product_id);
        let response = send_webhook_with_signature(&app, &realm_id, payload, webhook_secret).await;
        assert_eq!(response.status(), StatusCode::OK, "Expected 200 OK");

        let status = get_subscription_status(ctx, sub_id).await;
        assert_eq!(status, "dispute", "Subscription status should be 'dispute'");

        let history_count = count_history_events(ctx, sub_id, "disputed").await;
        assert_eq!(
            history_count, 1,
            "Expected exactly 1 'disputed' history event"
        );
    }

    // =========================================================================
    // Test 19: Creem subscription.scheduled_cancel idempotent
    // =========================================================================

    /// Covers: Duplicate subscription.scheduled_cancel with same event_id -> no double history
    ///
    /// Given: Creem config, a recurring mapping, a user, and a subscription
    /// When: subscription.scheduled_cancel is sent twice with the SAME event_id
    /// Then: Both return 200 OK and exactly 1 subscription_history entry for "scheduled_cancel".
    #[test_context(WebhookPatchTestContext)]
    #[tokio::test]
    async fn test_creem_subscription_scheduled_cancel_idempotent(
        ctx: &mut WebhookPatchTestContext,
    ) {
        let app = ctx.create_unified_test_router();
        let webhook_secret = "whsec_creem_idemp";
        let realm_id = ctx._realm_id.clone();
        let client_app_id = Uuid::parse_str(&ctx._client_app_id).unwrap();
        let entitlement_key = "patch-creem-idemp";

        ctx.with_creem_config(
            &realm_id,
            Some("test_api_key"),
            Some(webhook_secret),
            Some(30),
        )
        .await;
        create_recurring_mapping(ctx, &realm_id, "creem", entitlement_key, 500).await;
        let user_id = create_test_user(ctx, &realm_id, "patch-creem-idemp@test.com").await;

        let ext_sub_id = format!("sub_creem_{}", Uuid::now_v7());
        let sub_id = pre_create_subscription(
            ctx,
            &realm_id,
            user_id,
            client_app_id,
            &ext_sub_id,
            &format!("prod_creem_{}", entitlement_key),
            "creem",
            "active",
            entitlement_key,
        )
        .await;

        // Use the SAME event_id for both sends
        let event_id = format!("evt_idemp_sched_{}", Uuid::now_v7());
        let payload = build_creem_subscription_lifecycle(
            &event_id,
            "subscription.scheduled_cancel",
            &ext_sub_id,
            &format!("prod_creem_{}", entitlement_key),
            &realm_id,
            user_id,
            "scheduled_cancel",
            true,
            entitlement_key,
        );

        let response1 =
            send_webhook_with_signature(&app, &realm_id, payload.clone(), webhook_secret).await;
        assert_eq!(
            response1.status(),
            StatusCode::OK,
            "First send should return 200 OK"
        );

        let response2 = send_webhook_with_signature(&app, &realm_id, payload, webhook_secret).await;
        assert_eq!(
            response2.status(),
            StatusCode::OK,
            "Duplicate send should return 200 OK (idempotent)"
        );

        // Exactly 1 history entry for scheduled_cancel
        let history_count = count_history_events(ctx, sub_id, "scheduled_cancel").await;
        assert_eq!(
            history_count, 1,
            "Expected exactly 1 'scheduled_cancel' history event for idempotent send, got {}",
            history_count
        );
    }

    // =========================================================================
    // Test B4: Creem subscription.canceled immediate revokes credits
    // =========================================================================

    /// Covers: subscription.canceled with cancelAtPeriodEnd=false -> immediate cancel,
    ///         credits revoked, status=canceled
    ///
    /// Given: Creem config, a recurring mapping (500 points), a user + wallet with ledger-backed
    ///        subscription credits, and an active subscription
    /// When: subscription.canceled arrives with cancelAtPeriodEnd=false
    /// Then: Subscription status becomes "canceled", subscription_balance=0 (credits revoked),
    ///       and history has "canceled" event.
    #[test_context(WebhookPatchTestContext)]
    #[tokio::test]
    async fn test_creem_subscription_canceled_immediate_revokes_credits(
        ctx: &mut WebhookPatchTestContext,
    ) {
        let app = ctx.create_unified_test_router();
        let webhook_secret = "whsec_creem_imm_cancel";
        let realm_id = ctx._realm_id.clone();
        let client_app_id = Uuid::parse_str(&ctx._client_app_id).unwrap();
        let entitlement_key = "patch-creem-cancel";

        ctx.with_creem_config(
            &realm_id,
            Some("test_api_key"),
            Some(webhook_secret),
            Some(30),
        )
        .await;
        create_recurring_mapping(ctx, &realm_id, "creem", entitlement_key, 500).await;
        let user_id = create_test_user(ctx, &realm_id, "patch-creem-cancel@test.com").await;
        create_points_wallet(ctx, user_id, &realm_id).await;

        let ext_sub_id = format!("sub_creem_{}", Uuid::now_v7());
        let sub_id = pre_create_subscription(
            ctx,
            &realm_id,
            user_id,
            client_app_id,
            &ext_sub_id,
            &format!("prod_creem_{}", entitlement_key),
            "creem",
            "active",
            entitlement_key,
        )
        .await;

        create_subscription_credit_with_ledger(
            ctx,
            user_id,
            &realm_id,
            500,
            &format!("{}:{}", entitlement_key, sub_id),
        )
        .await;

        let event_id = generate_test_event_id();
        let payload = build_creem_subscription_canceled(
            &event_id,
            &ext_sub_id,
            &format!("prod_creem_{}", entitlement_key),
            &realm_id,
            user_id,
            false,
            entitlement_key,
        );
        let response = send_webhook_with_signature(&app, &realm_id, payload, webhook_secret).await;
        assert_eq!(response.status(), StatusCode::OK, "Expected 200 OK");

        let status = get_subscription_status(ctx, sub_id).await;
        assert_eq!(
            status, "canceled",
            "Subscription status should be 'canceled'"
        );

        let sub_balance = get_subscription_balance(ctx, user_id, &realm_id).await;
        assert_eq!(
            sub_balance, 0,
            "subscription_balance should be 0 after immediate cancel (credits revoked)"
        );

        let history_count = count_history_events(ctx, sub_id, "canceled").await;
        assert_eq!(
            history_count, 1,
            "Expected exactly 1 'canceled' history event"
        );
    }

    // =========================================================================
    // Test B5: Creem subscription.update changes entitlement key
    // =========================================================================

    /// Covers: subscription.update -> entitlement_key change from key-A to key-B,
    ///         upgrade detected (500 -> 1000 points), history has "upgraded" event
    ///
    /// Given: Creem config, TWO recurring mappings (key-A with 500 pts, key-B with 1000 pts),
    ///        a user + wallet, and an active subscription with entitlement_key=A
    /// When: subscription.update arrives with new entitlement_key=B and previousAttributes.entitlement_key=A
    /// Then: Subscription entitlement_key changes to B, history has "upgraded" event.
    #[test_context(WebhookPatchTestContext)]
    #[tokio::test]
    async fn test_creem_subscription_update_changes_entitlement_key(
        ctx: &mut WebhookPatchTestContext,
    ) {
        let app = ctx.create_unified_test_router();
        let webhook_secret = "whsec_creem_update";
        let realm_id = ctx._realm_id.clone();
        let client_app_id = Uuid::parse_str(&ctx._client_app_id).unwrap();
        let entitlement_key_a = "patch-creem-key-a";
        let entitlement_key_b = "patch-creem-key-b";

        ctx.with_creem_config(
            &realm_id,
            Some("test_api_key"),
            Some(webhook_secret),
            Some(30),
        )
        .await;
        create_recurring_mapping(ctx, &realm_id, "creem", entitlement_key_a, 500).await;
        create_recurring_mapping(ctx, &realm_id, "creem", entitlement_key_b, 1000).await;
        let user_id = create_test_user(ctx, &realm_id, "patch-creem-update@test.com").await;
        create_points_wallet(ctx, user_id, &realm_id).await;

        let ext_sub_id = format!("sub_creem_{}", Uuid::now_v7());
        let sub_id = pre_create_subscription(
            ctx,
            &realm_id,
            user_id,
            client_app_id,
            &ext_sub_id,
            &format!("prod_creem_{}", entitlement_key_a),
            "creem",
            "active",
            entitlement_key_a,
        )
        .await;

        let event_id = generate_test_event_id();
        let payload = build_creem_subscription_update(
            &event_id,
            &ext_sub_id,
            &format!("prod_creem_{}", entitlement_key_b),
            &realm_id,
            user_id,
            entitlement_key_b,
            entitlement_key_a,
        );
        let response = send_webhook_with_signature(&app, &realm_id, payload, webhook_secret).await;
        assert_eq!(response.status(), StatusCode::OK, "Expected 200 OK");

        let entitlement_key = get_entitlement_key(ctx, sub_id).await;
        assert_eq!(
            entitlement_key, entitlement_key_b,
            "Subscription entitlement_key should be updated to key-B"
        );

        let history_count = count_history_events(ctx, sub_id, "upgraded").await;
        assert_eq!(
            history_count, 1,
            "Expected exactly 1 'upgraded' history event"
        );
    }

    // =========================================================================
    // Test C4: Creem subscription paused then active reactivation sequence
    // =========================================================================

    /// Sequence test: Active -> Paused -> Active (reactivation via lifecycle events)
    ///
    /// Covers: subscription.paused followed by subscription.active (reactivation),
    ///         credits preserved throughout pause/reactivation cycle
    ///
    /// Given: Creem config, a recurring mapping with 500 points, a user + wallet with
    ///        ledger-backed subscription credits, and an active subscription
    /// When: First, subscription.paused arrives (status -> paused)
    ///       Then, subscription.active arrives (status -> active, reactivated)
    /// Then: After the sequence, subscription is "active" with 2 history events:
    ///       1 "paused" and 1 "reactivated". Credits are preserved (balance=500).
    #[test_context(WebhookPatchTestContext)]
    #[tokio::test]
    async fn test_creem_subscription_paused_then_active_reactivation_sequence(
        ctx: &mut WebhookPatchTestContext,
    ) {
        let app = ctx.create_unified_test_router();
        let webhook_secret = "whsec_creem_seq_pause";
        let realm_id = ctx._realm_id.clone();
        let client_app_id = Uuid::parse_str(&ctx._client_app_id).unwrap();
        let entitlement_key = "patch-creem-seq-pause";

        ctx.with_creem_config(
            &realm_id,
            Some("test_api_key"),
            Some(webhook_secret),
            Some(30),
        )
        .await;
        create_recurring_mapping(ctx, &realm_id, "creem", entitlement_key, 500).await;
        let user_id = create_test_user(ctx, &realm_id, "patch-creem-seq-pause@test.com").await;
        create_points_wallet(ctx, user_id, &realm_id).await;

        let ext_sub_id = format!("sub_creem_{}", Uuid::now_v7());
        let sub_id = pre_create_subscription(
            ctx,
            &realm_id,
            user_id,
            client_app_id,
            &ext_sub_id,
            &format!("prod_creem_{}", entitlement_key),
            "creem",
            "active",
            entitlement_key,
        )
        .await;

        create_subscription_credit_with_ledger(
            ctx,
            user_id,
            &realm_id,
            500,
            &format!("{}:{}", entitlement_key, sub_id),
        )
        .await;

        // Step 1: Pause the subscription
        let event_id_1 = generate_test_event_id();
        let pause_payload = build_creem_subscription_lifecycle(
            &event_id_1,
            "subscription.paused",
            &ext_sub_id,
            &format!("prod_creem_{}", entitlement_key),
            &realm_id,
            user_id,
            "paused",
            false,
            entitlement_key,
        );
        let response =
            send_webhook_with_signature(&app, &realm_id, pause_payload, webhook_secret).await;
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "Pause webhook should return 200 OK"
        );

        let status = get_subscription_status(ctx, sub_id).await;
        assert_eq!(
            status, "paused",
            "Subscription status should be 'paused' after pause event"
        );

        let sub_balance_after_pause = get_subscription_balance(ctx, user_id, &realm_id).await;
        assert_eq!(
            sub_balance_after_pause, 500,
            "Credits should NOT be revoked on pause, only on cancel/expired"
        );

        // Step 2: Reactivate the subscription via subscription.active
        let event_id_2 = generate_test_event_id();
        let active_payload = build_creem_subscription_lifecycle(
            &event_id_2,
            "subscription.active",
            &ext_sub_id,
            &format!("prod_creem_{}", entitlement_key),
            &realm_id,
            user_id,
            "active",
            false,
            entitlement_key,
        );
        let response =
            send_webhook_with_signature(&app, &realm_id, active_payload, webhook_secret).await;
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "Active webhook should return 200 OK"
        );

        let status = get_subscription_status(ctx, sub_id).await;
        assert_eq!(
            status, "active",
            "Subscription status should be 'active' after reactivation"
        );

        // Final assertions
        let sub_balance = get_subscription_balance(ctx, user_id, &realm_id).await;
        assert_eq!(
            sub_balance, 500,
            "Credits should be preserved throughout pause/reactivation cycle"
        );

        let paused_count = count_history_events(ctx, sub_id, "paused").await;
        assert_eq!(paused_count, 1, "Expected exactly 1 'paused' history event");

        let reactivated_count = count_history_events(ctx, sub_id, "reactivated").await;
        assert_eq!(
            reactivated_count, 1,
            "Expected exactly 1 'reactivated' history event"
        );
    }
}
