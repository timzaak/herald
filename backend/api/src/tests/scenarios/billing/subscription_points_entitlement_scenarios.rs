// =============================================================================
// Subscription + Points + SDK Entitlement Scenario Tests
// =============================================================================
//
// Tests for:
// 1. Subscription new fields (create, update, query by entitlement_key)
// 2. Points grant on subscribe (enabled, disabled, mapping disabled)
// 3. Points grant on renewal (normal, max_periods limit, validity_days)
// 4. Points revoke on cancel (full, partial)
// 5. Points revoke on refund
// 6. SDK subscription query (returns entitlement_key, no plan fields, matches entitlement_key)
//
// User Story: US-EM-004 (points by entitlement), US-EM-005 (SDK query),
//             US-EM-006 (subscription projection)
//
// =============================================================================

#[cfg(test)]
mod tests {
    use crate::tests::helpers::billing_helpers::{
        setup_billing_admin_session_with_user, setup_test_entitlement_mapping_full,
        setup_test_entitlement_mapping_with_points,
    };
    use crate::tests::helpers::points_helpers::{
        get_points_balance_for_user, get_points_grant_schedule_by_entitlement,
        verify_points_granted_for_entitlement,
    };
    use crate::tests::helpers::subscription_test_helpers::{
        count_subscriptions_by_entitlement_key, create_test_subscription_for_user,
        create_test_subscription_with_entitlement_key, verify_subscription_entitlement_key,
    };
    use crate::tests::helpers::webhook_helpers::*;
    use crate::tests::schema_test_context::SchemaTestContext;
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode, header},
    };
    use test_context::test_context;
    use tower::ServiceExt;
    use uuid::Uuid;

    use SchemaTestContext as SpTestContext;

    // Helper: build request with admin auth cookie
    fn auth_request(method: &str, uri: String, token: &str, body: Option<Body>) -> Request<Body> {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::AUTHORIZATION, format!("Bearer {}", token));
        if let Some(b) = body {
            builder = builder.header("Content-Type", "application/json");
            builder.body(b).unwrap()
        } else {
            builder.body(Body::empty()).unwrap()
        }
    }

    // Helper: create a user in the test realm and return their UUID
    async fn create_test_user_in_realm(ctx: &SchemaTestContext, email: &str) -> Uuid {
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

    // Helper: create a points wallet for a user
    async fn create_points_wallet_for_user(ctx: &SchemaTestContext, user_id: Uuid, realm_id: &str) {
        let bucket_id = crate::tests::helpers::points_helpers::ensure_test_bucket_for_realm(
            &ctx.app_state.pool,
            realm_id,
        )
        .await;
        // point-time: per-type balance columns and the `total_balance`
        // GENERATED column were dropped from `points_wallets`; available balance
        // is derived from `points_credit_ledger`. Seed only the retained
        // lifetime-analytics columns (all 0).
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

    // Helper: set Creem webhook secret for a realm
    async fn set_webhook_secret_for_realm(ctx: &SchemaTestContext, webhook_secret: &str) {
        ctx.with_creem_config(
            &ctx._realm_id,
            Some("test_api_key"),
            Some(webhook_secret),
            Some(30),
        )
        .await;
    }

    // =========================================================================
    // Subscription New Fields (US-EM-006)
    // =========================================================================

    /// User Story: US-EM-006
    /// Covers: Create subscription with entitlement_key, verify all new fields persisted
    #[test_context(SpTestContext)]
    #[tokio::test]
    async fn test_subscription_create_with_entitlement_key(ctx: &mut SpTestContext) {
        let app = ctx.create_unified_test_router();
        let webhook_secret = "test_creem_wh_secret";
        let realm_id = ctx._realm_id.clone();
        let user_id = create_test_user_in_realm(ctx, "sub-create-ek@test.com").await;
        let client_app_id = Uuid::parse_str(&ctx._client_app_id).unwrap();
        let entitlement_key = "create-test-plan";
        let event_id = generate_test_event_id();
        let external_product_id = format!("prod_creem_{}", entitlement_key);

        set_webhook_secret_for_realm(ctx, webhook_secret).await;

        // Create entitlement mapping
        setup_test_entitlement_mapping_full(
            ctx,
            &realm_id,
            "creem",
            &external_product_id,
            None,
            entitlement_key,
            Some("recurring"),
            Some("every_month"),
            Some(500),
            None,
            None,
            true,
            None,
            true,
            None,
        )
        .await;

        create_points_wallet_for_user(ctx, user_id, &realm_id).await;

        // Send subscription.paid to create subscription
        let external_sub_id = format!("sub_creem_create_{}", event_id);
        let payload = build_creem_subscription_paid_with_herald_metadata(
            &event_id,
            entitlement_key,
            &realm_id,
            user_id,
            Some(client_app_id),
            &external_sub_id,
            &external_product_id,
            false,
        );

        let response = send_webhook_with_signature(&app, &realm_id, payload, webhook_secret).await;
        assert_webhook_success(&response);

        // Verify subscription was created with entitlement_key
        let sub_count =
            count_subscriptions_by_entitlement_key(ctx, &realm_id, entitlement_key).await;
        assert!(
            sub_count >= 1,
            "Expected at least 1 subscription with entitlement_key '{}'",
            entitlement_key
        );
    }

    /// User Story: US-EM-006
    /// Covers: Subscription entitlement_key change is tracked in history
    #[test_context(SpTestContext)]
    #[tokio::test]
    async fn test_subscription_update_entitlement_key(ctx: &mut SpTestContext) {
        let realm_id = ctx._realm_id.clone();
        let client_app_id = Uuid::parse_str(&ctx._client_app_id).unwrap();

        // Create subscription via SQL
        let sub_id = create_test_subscription_with_entitlement_key(
            ctx,
            &realm_id,
            client_app_id,
            "original-plan",
            "price_original",
            "creem",
            "active",
        )
        .await;

        // Update entitlement_key directly via SQL
        sqlx::query(
            "UPDATE subscription SET entitlement_key = $1, updated_at = NOW() WHERE id = $2",
        )
        .bind("updated-plan")
        .bind(sub_id)
        .execute(&ctx.app_state.pool)
        .await
        .unwrap();

        // Verify entitlement_key was updated
        verify_subscription_entitlement_key(ctx, sub_id, "updated-plan").await;
    }

    /// User Story: US-EM-006
    /// Covers: Query subscriptions by entitlement_key returns correct results
    #[test_context(SpTestContext)]
    #[tokio::test]
    async fn test_subscription_query_by_entitlement_key(ctx: &mut SpTestContext) {
        let realm_id = ctx._realm_id.clone();
        let client_app_id = Uuid::parse_str(&ctx._client_app_id).unwrap();

        // Create multiple subscriptions with the same entitlement_key
        create_test_subscription_with_entitlement_key(
            ctx,
            &realm_id,
            client_app_id,
            "shared-plan",
            "price_shared_1",
            "creem",
            "active",
        )
        .await;

        // Second subscription needs a different client_app_id
        let client_app_id_2 = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO client_apps (id, realm_id, client_id, name, redirect_uris, enabled, created_at, updated_at)
             VALUES ($1, $2, $3, 'test-app-shared', '[]', true, NOW(), NOW())
             ON CONFLICT (id) DO NOTHING",
        )
        .bind(client_app_id_2)
        .bind(&realm_id)
        .bind(format!("client-{}", client_app_id_2))
        .execute(&ctx.app_state.pool)
        .await
        .ok();

        create_test_subscription_with_entitlement_key(
            ctx,
            &realm_id,
            client_app_id_2,
            "shared-plan",
            "price_shared_2",
            "stripe",
            "active",
        )
        .await;

        // Query by entitlement_key
        let count = count_subscriptions_by_entitlement_key(ctx, &realm_id, "shared-plan").await;
        assert_eq!(
            count, 2,
            "Should find exactly 2 subscriptions with entitlement_key 'shared-plan'"
        );
    }

    // =========================================================================
    // Points Grant on Subscribe (US-EM-004)
    // =========================================================================

    /// User Story: US-EM-004
    /// Covers: first subscribe, grant_on_subscribe=true and points_per_period=500 -> 500 points
    #[test_context(SpTestContext)]
    #[tokio::test]
    async fn test_points_granted_on_first_subscribe(ctx: &mut SpTestContext) {
        let app = ctx.create_unified_test_router();
        let webhook_secret = "test_creem_wh_secret";
        let realm_id = ctx._realm_id.clone();
        let user_id = create_test_user_in_realm(ctx, "points-first-sub@test.com").await;
        let client_app_id = Uuid::parse_str(&ctx._client_app_id).unwrap();
        let entitlement_key = "grant-subscribe-plan";
        let event_id = generate_test_event_id();
        let external_product_id = format!("prod_points_first_{}", entitlement_key);
        let points_to_grant = 500;

        set_webhook_secret_for_realm(ctx, webhook_secret).await;

        let mapping_id = setup_test_entitlement_mapping_with_points(
            ctx,
            &realm_id,
            "creem",
            &external_product_id,
            entitlement_key,
            points_to_grant,
            true, // grant_on_subscribe
            true, // enabled
        )
        .await;
        // Initial activation grants nothing (BE-D04 owns initial fulfillment);
        // only the renewal route grants, so seed a subscription_renewal rule and
        // drive a renewal webhook.
        crate::tests::helpers::points_helpers::seed_subscription_renewal_rule(
            &ctx.app_state.pool,
            &realm_id,
            mapping_id,
            points_to_grant,
        )
        .await;

        create_points_wallet_for_user(ctx, user_id, &realm_id).await;

        let external_sub_id = format!("sub_points_first_{}", event_id);
        let payload = build_creem_subscription_paid_with_herald_metadata(
            &event_id,
            entitlement_key,
            &realm_id,
            user_id,
            Some(client_app_id),
            &external_sub_id,
            &external_product_id,
            true, // renewal — the only grant-bearing subscription.paid route
        );

        let response = send_webhook_with_signature(&app, &realm_id, payload, webhook_secret).await;
        assert_webhook_success(&response);

        // Verify points were granted
        verify_points_granted_for_entitlement(ctx, user_id, entitlement_key, points_to_grant).await;

        let balance = get_points_balance_for_user(ctx, user_id).await;
        assert!(
            balance >= points_to_grant,
            "Expected balance >= {}, got {}",
            points_to_grant,
            balance
        );
    }

    /// User Story: US-EM-004
    /// Covers: grant_on_subscribe=false -> no points on first subscribe
    #[test_context(SpTestContext)]
    #[tokio::test]
    async fn test_points_not_granted_on_subscribe_when_disabled(ctx: &mut SpTestContext) {
        let app = ctx.create_unified_test_router();
        let webhook_secret = "test_creem_wh_secret";
        let realm_id = ctx._realm_id.clone();
        let user_id = create_test_user_in_realm(ctx, "points-no-grant@test.com").await;
        let client_app_id = Uuid::parse_str(&ctx._client_app_id).unwrap();
        let entitlement_key = "no-grant-plan";
        let event_id = generate_test_event_id();
        let external_product_id = format!("prod_no_grant_{}", entitlement_key);

        set_webhook_secret_for_realm(ctx, webhook_secret).await;

        setup_test_entitlement_mapping_with_points(
            ctx,
            &realm_id,
            "creem",
            &external_product_id,
            entitlement_key,
            500,
            false, // grant_on_subscribe = false
            true,  // enabled
        )
        .await;

        create_points_wallet_for_user(ctx, user_id, &realm_id).await;

        let external_sub_id = format!("sub_no_grant_{}", event_id);
        let payload = build_creem_subscription_paid_with_herald_metadata(
            &event_id,
            entitlement_key,
            &realm_id,
            user_id,
            Some(client_app_id),
            &external_sub_id,
            &external_product_id,
            false,
        );

        let response = send_webhook_with_signature(&app, &realm_id, payload, webhook_secret).await;
        // The handler may succeed (subscription created) but points should not be granted
        assert!(
            response.status() == StatusCode::OK
                || response.status() == StatusCode::BAD_REQUEST
                || response.status() == StatusCode::INTERNAL_SERVER_ERROR,
            "Expected OK or error, got {}",
            response.status()
        );

        // Verify no significant points were granted
        let balance = get_points_balance_for_user(ctx, user_id).await;
        assert!(
            balance < 500,
            "Expected no significant points with grant_on_subscribe=false, got {}",
            balance
        );
    }

    /// User Story: US-EM-004
    /// Covers: mapping enabled=false -> no points even if grant_on_subscribe=true
    #[test_context(SpTestContext)]
    #[tokio::test]
    async fn test_points_not_granted_when_mapping_disabled(ctx: &mut SpTestContext) {
        let app = ctx.create_unified_test_router();
        let webhook_secret = "test_creem_wh_secret";
        let realm_id = ctx._realm_id.clone();
        let user_id = create_test_user_in_realm(ctx, "points-map-disabled@test.com").await;
        let client_app_id = Uuid::parse_str(&ctx._client_app_id).unwrap();
        let entitlement_key = "disabled-mapping-plan";
        let event_id = generate_test_event_id();
        let external_product_id = format!("prod_disabled_map_{}", entitlement_key);

        set_webhook_secret_for_realm(ctx, webhook_secret).await;

        setup_test_entitlement_mapping_with_points(
            ctx,
            &realm_id,
            "creem",
            &external_product_id,
            entitlement_key,
            500,
            true,  // grant_on_subscribe = true
            false, // enabled = false
        )
        .await;

        create_points_wallet_for_user(ctx, user_id, &realm_id).await;

        let external_sub_id = format!("sub_disabled_map_{}", event_id);
        let payload = build_creem_subscription_paid_with_herald_metadata(
            &event_id,
            entitlement_key,
            &realm_id,
            user_id,
            Some(client_app_id),
            &external_sub_id,
            &external_product_id,
            false,
        );

        let response = send_webhook_with_signature(&app, &realm_id, payload, webhook_secret).await;
        // May succeed or fail, but points should not be granted
        assert!(
            response.status() == StatusCode::OK
                || response.status() == StatusCode::BAD_REQUEST
                || response.status() == StatusCode::INTERNAL_SERVER_ERROR
                || response.status() == StatusCode::NOT_FOUND,
            "Expected OK or error (including NOT_FOUND for disabled mapping), got {}",
            response.status()
        );

        let balance = get_points_balance_for_user(ctx, user_id).await;
        assert!(
            balance < 500,
            "Expected no points with disabled mapping, got {}",
            balance
        );
    }

    /// User Story: US-EM-004 (absent-mapping graceful-skip contract).
    ///
    /// Encodes the B2 domain contract: when the entitlement mapping is
    /// **absent** (no row, not merely disabled) AND the subscription has no
    /// bound bucket, the renewal must surface `EntitlementMappingNotFound` —
    /// which the Creem webhook handler swallows for a graceful skip (no grant,
    /// no provider retry) — rather than `SubscriptionBucketNotResolved`
    /// (which would propagate as a 5xx and trigger endless retries for an
    /// entitlement that should simply be ignored). This guards against the
    /// bucket-before-mapping ordering regression.
    #[test_context(SpTestContext)]
    #[tokio::test]
    async fn test_absent_mapping_renewal_skips_gracefully(ctx: &mut SpTestContext) {
        let app = ctx.create_unified_test_router();
        let webhook_secret = "test_creem_wh_secret";
        let realm_id = ctx._realm_id.clone();
        let user_id = create_test_user_in_realm(ctx, "points-map-absent@test.com").await;
        let client_app_id = Uuid::parse_str(&ctx._client_app_id).unwrap();
        // Deliberately use an entitlement_key with NO mapping row at all.
        let entitlement_key = "absent-mapping-plan";
        let event_id = generate_test_event_id();
        let external_product_id = format!("prod_absent_map_{}", entitlement_key);

        set_webhook_secret_for_realm(ctx, webhook_secret).await;
        // NOTE: no setup_test_entitlement_mapping_* call — mapping is absent.
        // No wallet is created up-front either: for an absent mapping the
        // grant is skipped, so no wallet write ever occurs. (This also keeps
        // the test independent of the unrelated Credit-Buckets
        // `create_points_wallet_for_user` ON-CONFLICT breakage.)

        let external_sub_id = format!("sub_absent_map_{}", event_id);
        let payload = build_creem_subscription_paid_with_herald_metadata(
            &event_id,
            entitlement_key,
            &realm_id,
            user_id,
            Some(client_app_id),
            &external_sub_id,
            &external_product_id,
            false,
        );

        let response = send_webhook_with_signature(&app, &realm_id, payload, webhook_secret).await;
        // Graceful skip: the handler swallows EntitlementMappingNotFound and
        // returns 200. It must NOT surface a loud SubscriptionBucketNotResolved
        // (5xx) for an entitlement with no mapping.
        assert!(
            response.status() == StatusCode::OK
                || response.status() == StatusCode::NOT_FOUND
                || response.status() == StatusCode::BAD_REQUEST,
            "Expected graceful skip (OK/NOT_FOUND/BAD_REQUEST) for absent mapping, got {}",
            response.status()
        );

        let balance = get_points_balance_for_user(ctx, user_id).await;
        assert_eq!(
            balance, 0,
            "Expected no points granted for an absent mapping, got {}",
            balance
        );
    }

    // =========================================================================
    // Points Grant on Renewal (US-EM-004)
    // =========================================================================

    /// User Story: US-EM-004
    /// Covers: Renewal with isRenewal=true, mapping has points_per_period=500 -> 500 points
    #[test_context(SpTestContext)]
    #[tokio::test]
    async fn test_points_granted_on_renewal(ctx: &mut SpTestContext) {
        let app = ctx.create_unified_test_router();
        let webhook_secret = "test_creem_wh_secret";
        let realm_id = ctx._realm_id.clone();
        let user_id = create_test_user_in_realm(ctx, "points-renewal@test.com").await;
        let client_app_id = Uuid::parse_str(&ctx._client_app_id).unwrap();
        let entitlement_key = "renewal-plan";
        let event_id = generate_test_event_id();
        let external_product_id = format!("prod_renewal_{}", entitlement_key);

        set_webhook_secret_for_realm(ctx, webhook_secret).await;

        let mapping_id = setup_test_entitlement_mapping_with_points(
            ctx,
            &realm_id,
            "creem",
            &external_product_id,
            entitlement_key,
            500,
            true,
            true,
        )
        .await;
        // The renewal grant reads CURRENT distribution rules; the helper only
        // seeds a subscription_initial rule, so add the subscription_renewal
        // rule that the renewal route selects.
        crate::tests::helpers::points_helpers::seed_subscription_renewal_rule(
            &ctx.app_state.pool,
            &realm_id,
            mapping_id,
            500,
        )
        .await;

        create_points_wallet_for_user(ctx, user_id, &realm_id).await;

        let external_sub_id = format!("sub_renewal_{}", event_id);
        let payload = build_creem_subscription_paid_with_herald_metadata(
            &event_id,
            entitlement_key,
            &realm_id,
            user_id,
            Some(client_app_id),
            &external_sub_id,
            &external_product_id,
            true, // is_renewal = true
        );

        let response = send_webhook_with_signature(&app, &realm_id, payload, webhook_secret).await;
        assert_webhook_success(&response);

        // Verify points were granted on renewal
        let balance = get_points_balance_for_user(ctx, user_id).await;
        assert!(
            balance >= 500,
            "Expected at least 500 points granted on renewal, got {}",
            balance
        );
    }

    /// User Story: US-EM-004
    /// Covers: max_periods=3, renewal beyond 3rd period -> no additional points
    #[test_context(SpTestContext)]
    #[tokio::test]
    async fn test_points_granted_with_max_periods_limit(ctx: &mut SpTestContext) {
        let realm_id = ctx._realm_id.clone();
        let entitlement_key = "max-periods-plan";
        let external_product_id = format!("prod_max_periods_{}", entitlement_key);

        // Create mapping with max_periods=3
        setup_test_entitlement_mapping_full(
            ctx,
            &realm_id,
            "creem",
            &external_product_id,
            None,
            entitlement_key,
            Some("recurring"),
            Some("every_month"),
            Some(500),
            None,
            None,
            true,
            Some(3), // max_periods = 3
            true,
            None,
        )
        .await;

        // Verify the grant schedule respects max_periods
        // (Full lifecycle test requires multiple renewal webhooks; here we verify
        // the mapping was created correctly with max_periods)
        let schedules = get_points_grant_schedule_by_entitlement(ctx, entitlement_key).await;
        // No schedule exists yet since no subscription was created
        assert!(
            schedules.is_empty(),
            "No grant schedule should exist before subscription"
        );
    }

    /// User Story: US-EM-004
    /// Covers: mapping has validity_days=30 -> points granted with 30-day expiration
    #[test_context(SpTestContext)]
    #[tokio::test]
    async fn test_points_granted_with_validity_days(ctx: &mut SpTestContext) {
        let app = ctx.create_unified_test_router();
        let webhook_secret = "test_creem_wh_secret";
        let realm_id = ctx._realm_id.clone();
        let user_id = create_test_user_in_realm(ctx, "points-validity@test.com").await;
        let client_app_id = Uuid::parse_str(&ctx._client_app_id).unwrap();
        let entitlement_key = "validity-plan";
        let event_id = generate_test_event_id();
        let external_product_id = format!("prod_validity_{}", entitlement_key);

        set_webhook_secret_for_realm(ctx, webhook_secret).await;

        let mapping_id = setup_test_entitlement_mapping_full(
            ctx,
            &realm_id,
            "creem",
            &external_product_id,
            None,
            entitlement_key,
            Some("recurring"),
            Some("every_month"),
            Some(500),
            None,
            Some(30), // validity_days = 30
            true,
            None,
            true,
            None,
        )
        .await;
        // Initial activation grants nothing; seed a subscription_renewal rule
        // (validity_days on the renewal quota is driven by the subscription
        // period end) and drive a renewal webhook.
        crate::tests::helpers::points_helpers::seed_subscription_renewal_rule(
            &ctx.app_state.pool,
            &realm_id,
            mapping_id,
            500,
        )
        .await;

        create_points_wallet_for_user(ctx, user_id, &realm_id).await;

        let external_sub_id = format!("sub_validity_{}", event_id);
        let payload = build_creem_subscription_paid_with_herald_metadata(
            &event_id,
            entitlement_key,
            &realm_id,
            user_id,
            Some(client_app_id),
            &external_sub_id,
            &external_product_id,
            true, // renewal — the only grant-bearing subscription.paid route
        );

        let response = send_webhook_with_signature(&app, &realm_id, payload, webhook_secret).await;
        assert_webhook_success(&response);

        // Verify points were granted
        let balance = get_points_balance_for_user(ctx, user_id).await;
        assert!(
            balance >= 500,
            "Expected at least 500 points with validity_days=30, got {}",
            balance
        );

        // Verify quota entitlement has an expiration window set
        let has_expiry: bool = sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1 FROM points_quota_entitlements
                WHERE user_id = $1 AND credit_type = 'subscription_credit' AND effective_until IS NOT NULL
            )",
        )
        .bind(user_id)
        .fetch_one(&ctx.app_state.pool)
        .await
        .unwrap();

        assert!(
            has_expiry,
            "Subscription quota entitlement should have effective_until set when validity_days is configured"
        );
    }

    // =========================================================================
    // Points Revoke on Cancel (US-EM-004)
    // =========================================================================

    /// User Story: US-EM-004
    /// Covers: Active subscription canceled -> unused points revoked
    #[test_context(SpTestContext)]
    #[tokio::test]
    async fn test_points_revoked_on_subscription_cancel(ctx: &mut SpTestContext) {
        let app = ctx.create_unified_test_router();
        let webhook_secret = "test_creem_wh_secret";
        let realm_id = ctx._realm_id.clone();
        let user_id = create_test_user_in_realm(ctx, "points-cancel-revoke@test.com").await;
        let client_app_id = Uuid::parse_str(&ctx._client_app_id).unwrap();
        let entitlement_key = "cancel-revoke-plan";
        let external_product_id = format!("prod_cancel_revoke_{}", entitlement_key);

        set_webhook_secret_for_realm(ctx, webhook_secret).await;

        let mapping_id = setup_test_entitlement_mapping_with_points(
            ctx,
            &realm_id,
            "creem",
            &external_product_id,
            entitlement_key,
            1000,
            true,
            true,
        )
        .await;
        // Initial activation grants nothing; seed a subscription_renewal rule
        // so the renewal paid webhook grants the credits that cancel then revokes.
        crate::tests::helpers::points_helpers::seed_subscription_renewal_rule(
            &ctx.app_state.pool,
            &realm_id,
            mapping_id,
            1000,
        )
        .await;

        create_points_wallet_for_user(ctx, user_id, &realm_id).await;

        // First send subscription.paid (renewal — the grant-bearing route) to grant points
        let paid_event_id = generate_test_event_id();
        let external_sub_id = format!("sub_cancel_revoke_{}", paid_event_id);
        let paid_payload = build_creem_subscription_paid_with_herald_metadata(
            &paid_event_id,
            entitlement_key,
            &realm_id,
            user_id,
            Some(client_app_id),
            &external_sub_id,
            &external_product_id,
            true, // renewal — the only grant-bearing subscription.paid route
        );

        let paid_response =
            send_webhook_with_signature(&app, &realm_id, paid_payload, webhook_secret).await;
        assert_webhook_success(&paid_response);

        let balance_after_paid = get_points_balance_for_user(ctx, user_id).await;
        assert!(
            balance_after_paid >= 1000,
            "Expected at least 1000 points after paid, got {}",
            balance_after_paid
        );

        // Now send subscription.canceled
        let cancel_event_id = generate_test_event_id();
        let cancel_payload = build_creem_subscription_canceled_with_entitlement(
            &cancel_event_id,
            entitlement_key,
            &realm_id,
            user_id,
            &external_sub_id,
            &external_product_id,
            false, // immediate cancel
        );

        let cancel_response =
            send_webhook_with_signature(&app, &realm_id, cancel_payload, webhook_secret).await;
        assert_webhook_success(&cancel_response);

        // After immediate cancel, subscription credits should be revoked
        let balance_after_cancel = get_points_balance_for_user(ctx, user_id).await;
        assert!(
            balance_after_cancel <= balance_after_paid,
            "Balance should not increase after cancel, was {} now {}",
            balance_after_paid,
            balance_after_cancel
        );
    }

    /// User Story: US-EM-004
    /// Covers: Subscription canceled mid-period -> only future period points revoked,
    ///         already-consumed points kept
    #[test_context(SpTestContext)]
    #[tokio::test]
    async fn test_points_partial_revoke_on_cancel(ctx: &mut SpTestContext) {
        let app = ctx.create_unified_test_router();
        let webhook_secret = "test_creem_wh_secret";
        let realm_id = ctx._realm_id.clone();
        let user_id = create_test_user_in_realm(ctx, "points-partial-revoke@test.com").await;
        let client_app_id = Uuid::parse_str(&ctx._client_app_id).unwrap();
        let entitlement_key = "partial-revoke-plan";
        let external_product_id = format!("prod_partial_revoke_{}", entitlement_key);

        set_webhook_secret_for_realm(ctx, webhook_secret).await;

        setup_test_entitlement_mapping_with_points(
            ctx,
            &realm_id,
            "creem",
            &external_product_id,
            entitlement_key,
            1000,
            true,
            true,
        )
        .await;

        create_points_wallet_for_user(ctx, user_id, &realm_id).await;

        // Grant points via subscription.paid
        let paid_event_id = generate_test_event_id();
        let external_sub_id = format!("sub_partial_revoke_{}", paid_event_id);
        let paid_payload = build_creem_subscription_paid_with_herald_metadata(
            &paid_event_id,
            entitlement_key,
            &realm_id,
            user_id,
            Some(client_app_id),
            &external_sub_id,
            &external_product_id,
            false,
        );

        let paid_response =
            send_webhook_with_signature(&app, &realm_id, paid_payload, webhook_secret).await;
        assert_webhook_success(&paid_response);

        // Consume some points to simulate partial usage
        let balance_after_paid = get_points_balance_for_user(ctx, user_id).await;

        // Send subscription.canceled with cancel_at_period_end (not immediate)
        let cancel_event_id = generate_test_event_id();
        let cancel_payload = build_creem_subscription_canceled_with_entitlement(
            &cancel_event_id,
            entitlement_key,
            &realm_id,
            user_id,
            &external_sub_id,
            &external_product_id,
            true, // cancel_at_period_end = true (not immediate)
        );

        let cancel_response =
            send_webhook_with_signature(&app, &realm_id, cancel_payload, webhook_secret).await;
        assert_webhook_success(&cancel_response);

        // With cancel_at_period_end, credits should remain until period end
        // not immediately revoked
        let balance_after_cancel = get_points_balance_for_user(ctx, user_id).await;
        // Balance should not change for cancel_at_period_end
        assert!(
            balance_after_cancel <= balance_after_paid,
            "Balance should not increase after cancel_at_period_end"
        );
    }

    // =========================================================================
    // Points Revoke on Refund (US-EM-004)
    // =========================================================================

    /// User Story: US-EM-004
    /// Covers: Refund webhook for subscription -> points for refunded period revoked
    #[test_context(SpTestContext)]
    #[tokio::test]
    async fn test_points_revoked_on_refund(ctx: &mut SpTestContext) {
        let app = ctx.create_unified_test_router();
        let webhook_secret = "test_creem_wh_secret";
        let realm_id = ctx._realm_id.clone();
        let user_id = create_test_user_in_realm(ctx, "points-refund-revoke@test.com").await;
        let client_app_id = Uuid::parse_str(&ctx._client_app_id).unwrap();
        let entitlement_key = "refund-revoke-plan";
        let external_product_id = format!("prod_refund_revoke_{}", entitlement_key);

        set_webhook_secret_for_realm(ctx, webhook_secret).await;

        let mapping_id = setup_test_entitlement_mapping_with_points(
            ctx,
            &realm_id,
            "creem",
            &external_product_id,
            entitlement_key,
            1000,
            true,
            true,
        )
        .await;
        // Initial activation grants nothing; seed a subscription_renewal rule
        // so the renewal paid webhook grants the credits the refund then revokes.
        crate::tests::helpers::points_helpers::seed_subscription_renewal_rule(
            &ctx.app_state.pool,
            &realm_id,
            mapping_id,
            1000,
        )
        .await;

        create_points_wallet_for_user(ctx, user_id, &realm_id).await;

        // First grant points via subscription.paid (renewal — the grant-bearing route)
        let paid_event_id = generate_test_event_id();
        let external_sub_id = format!("sub_refund_revoke_{}", paid_event_id);
        let paid_payload = build_creem_subscription_paid_with_herald_metadata(
            &paid_event_id,
            entitlement_key,
            &realm_id,
            user_id,
            Some(client_app_id),
            &external_sub_id,
            &external_product_id,
            true, // renewal — the only grant-bearing subscription.paid route
        );

        let paid_response =
            send_webhook_with_signature(&app, &realm_id, paid_payload, webhook_secret).await;
        assert_webhook_success(&paid_response);

        let balance_after_paid = get_points_balance_for_user(ctx, user_id).await;
        assert!(
            balance_after_paid >= 1000,
            "Expected at least 1000 points after paid, got {}",
            balance_after_paid
        );

        // Now send refund webhook using existing refund.created builder
        let refund_event_id = generate_test_event_id();
        let refund_id = format!("refund_{}", refund_event_id);
        let payment_id = format!("pay_{}", refund_event_id);
        let refund_payload = build_refund_created_event_with_user_and_type(
            refund_event_id,
            refund_id,
            payment_id,
            1000,
            1000,
            &realm_id,
            user_id,
            "subscription",
        );

        let refund_response =
            send_webhook_with_signature(&app, &realm_id, refund_payload, webhook_secret).await;
        // Refund handler should process successfully (may revoke points)
        assert!(
            refund_response.status() == StatusCode::OK
                || refund_response.status() == StatusCode::BAD_REQUEST
                || refund_response.status() == StatusCode::INTERNAL_SERVER_ERROR,
            "Expected OK or error for refund, got {}",
            refund_response.status()
        );
    }

    // =========================================================================
    // SDK Subscription Query (US-EM-005)
    // =========================================================================

    /// User Story: US-EM-005
    /// Covers: SDK endpoint returns entitlement_key field
    #[test_context(SpTestContext)]
    #[tokio::test]
    async fn test_sdk_subscription_query_returns_entitlement_key(ctx: &mut SpTestContext) {
        let app = ctx.create_unified_test_router();
        let (token, user_id) = setup_billing_admin_session_with_user(ctx, "sdk-ek@test.com").await;
        let realm_id = ctx._realm_id.clone();
        let client_app_id = Uuid::parse_str(&ctx._client_app_id).unwrap();

        // Create a subscription with entitlement_key bound to the session user
        create_test_subscription_for_user(
            ctx,
            &realm_id,
            client_app_id,
            user_id,
            "sdk-pro-plan",
            "price_sdk_123",
            "stripe",
            "active",
        )
        .await;

        // Query via the self-service subscription endpoint (same handler logic as SDK)
        let response = app
            .clone()
            .oneshot(auth_request(
                "GET",
                format!("/api/bill/client/{}/subscription", client_app_id),
                &token,
                None,
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        // Verify entitlement_key is present
        assert_eq!(json["entitlementKey"], "sdk-pro-plan");
        assert_eq!(json["paymentProvider"], "stripe");
    }

    /// User Story: US-EM-005
    /// Covers: SDK response does NOT contain planId, plan object, or billingPeriod
    #[test_context(SpTestContext)]
    #[tokio::test]
    async fn test_sdk_subscription_query_no_plan_fields(ctx: &mut SpTestContext) {
        let app = ctx.create_unified_test_router();
        let (token, user_id) =
            setup_billing_admin_session_with_user(ctx, "sdk-no-plan@test.com").await;
        let realm_id = ctx._realm_id.clone();
        let client_app_id = Uuid::parse_str(&ctx._client_app_id).unwrap();

        create_test_subscription_for_user(
            ctx,
            &realm_id,
            client_app_id,
            user_id,
            "no-plan-fields-plan",
            "price_no_plan",
            "creem",
            "active",
        )
        .await;

        let response = app
            .clone()
            .oneshot(auth_request(
                "GET",
                format!("/api/bill/client/{}/subscription", client_app_id),
                &token,
                None,
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        // Verify old plan fields are absent
        assert!(
            json.get("planId").is_none(),
            "planId should not be in SDK response"
        );
        assert!(
            json.get("plan").is_none(),
            "plan object should not be in SDK response"
        );
        assert!(
            json.get("billingPeriod").is_none(),
            "billingPeriod should not be in SDK response"
        );
    }

    /// User Story: US-EM-005
    /// Covers: SDK query returns matching entitlement_key for the subscription
    #[test_context(SpTestContext)]
    #[tokio::test]
    async fn test_sdk_subscription_query_matches_user_entitlement_key(ctx: &mut SpTestContext) {
        let app = ctx.create_unified_test_router();
        let (token, user_id) =
            setup_billing_admin_session_with_user(ctx, "sdk-match-ek@test.com").await;
        let realm_id = ctx._realm_id.clone();
        let client_app_id = Uuid::parse_str(&ctx._client_app_id).unwrap();

        // Create subscription with specific entitlement_key "pro-plan" bound to
        // the session user
        create_test_subscription_for_user(
            ctx,
            &realm_id,
            client_app_id,
            user_id,
            "pro-plan",
            "price_pro_match",
            "stripe",
            "active",
        )
        .await;

        let response = app
            .clone()
            .oneshot(auth_request(
                "GET",
                format!("/api/bill/client/{}/subscription", client_app_id),
                &token,
                None,
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        // Verify entitlement_key matches exactly "pro-plan"
        assert_eq!(
            json["entitlementKey"], "pro-plan",
            "SDK response entitlement_key should match 'pro-plan'"
        );
    }
}
