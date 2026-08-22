// =============================================================================
// One-Time Purchase Fulfillment Scenario Tests
// =============================================================================
//
// Tests for:
// 1. One-time fulfillment grants topup_credit
// 2. One-time fulfillment with validity_days (expiring points)
// 3. One-time fulfillment without validity_days (permanent points)
// 4. Idempotent fulfillment prevents double-grants (CRITICAL)
// 5. resolve_target: mapping not found returns error
// 6. resolve_target: mapping disabled returns error
// 7. resolve_target: mapping no provider_product_info returns error
// 8. Disabled mapping allows fulfillment of existing attempt
//
// User Story: US-PU-006 (one-time purchase), US-PA-001 (create payment attempt),
//             US-PA-003 (payment success fulfillment)
// Covers: Design section 5.1 "PurchaseService + FulfillmentService"
//
// =============================================================================

#[cfg(test)]
mod tests {
    use crate::tests::helpers::points_helpers::{create_points_wallet, get_points_wallet_by_user};
    use crate::tests::schema_test_context::SchemaTestContext as TestContext;
    use serde_json::json;
    use test_context::test_context;
    use uuid::Uuid;

    // =========================================================================
    // Helpers
    // =========================================================================

    /// Create a one-time entitlement mapping with points and validity_days.
    /// Returns the mapping ID.
    async fn create_one_time_mapping_with_points(
        ctx: &TestContext,
        realm_id: &str,
        points: i64,
        validity_days: Option<i64>,
        enabled: bool,
    ) -> Uuid {
        let mapping_id = Uuid::now_v7();
        let provider_product_info = json!({
            "name": format!("Test Package {}pts", points),
            "price": 999,
            "currency": "usd"
        });

        // Distribution-rules model: the mapping row carries no grant columns;
        // the points grant is a fixed `topup` rule owned by this mapping.
        let bucket_id = crate::tests::helpers::points_helpers::ensure_test_bucket_for_realm(
            &ctx.app_state.pool,
            realm_id,
        )
        .await;

        sqlx::query(
            "INSERT INTO provider_entitlement_mappings
                (id, realm_id, payment_provider, external_product_id, entitlement_key,
                 billing_type, enabled, provider_product_info, created_at, updated_at)
             VALUES ($1, $2, 'stripe', $3, $4, 'one_time', $5, $6, NOW(), NOW())",
        )
        .bind(mapping_id)
        .bind(realm_id)
        .bind(format!("prod_{}", mapping_id))
        .bind(format!("one-time-test-{}", mapping_id))
        .bind(enabled)
        .bind(provider_product_info)
        .execute(&ctx.app_state.pool)
        .await
        .expect("Failed to create one-time mapping");

        let rule_id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO points_distribution_rules
                (id, realm_id, owner_type, entitlement_mapping_id, bucket_id,
                 trigger_sources, grant_mode, points_amount, validity_days,
                 enabled, display_order)
             VALUES ($1, $2, 'entitlement_mapping', $3, $4, $5, 'fixed', $6, $7, $8, 0)",
        )
        .bind(rule_id)
        .bind(realm_id)
        .bind(mapping_id)
        .bind(bucket_id)
        .bind(&["topup"][..])
        .bind(points)
        .bind(validity_days.unwrap_or(0))
        .bind(enabled)
        .execute(&ctx.app_state.pool)
        .await
        .expect("Failed to seed mapping-owned topup distribution rule");
        mapping_id
    }

    /// Create a pending payment attempt targeting an entitlement mapping.
    /// Returns the attempt ID.
    async fn create_pending_attempt_for_mapping(
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
        // enabled `topup` rules so fulfillment replays them.
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

    /// Fulfill a payment attempt via the internal fulfill_payment handler.
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

    /// Get the expires_at of the most recent topup_credit ledger for a user.
    async fn get_latest_topup_expiry(
        ctx: &TestContext,
        user_id: Uuid,
    ) -> Option<chrono::DateTime<chrono::Utc>> {
        sqlx::query_scalar::<_, Option<chrono::DateTime<chrono::Utc>>>(
            "SELECT expires_at FROM points_credit_ledger
             WHERE user_id = $1 AND credit_type = 'topup_credit'
             ORDER BY created_at DESC LIMIT 1",
        )
        .bind(user_id)
        .fetch_one(&ctx.app_state.pool)
        .await
        .unwrap()
    }

    // =========================================================================
    // Test 1: One-time fulfillment grants topup_credit
    // =========================================================================

    /// User Story: US-PU-006, US-PA-003
    /// Covers: Design section 5.1 "one-time reads mapping.points_per_period"
    ///
    /// Scenario: One-time purchase fulfillment grants topup_credit points
    /// Given: A one-time mapping with points_per_period=1000 and validity_days=30
    /// And: A user with a points wallet
    /// And: A pending payment attempt targeting the mapping
    /// When: Fulfilling the payment attempt
    /// Then: Payment attempt status becomes Succeeded
    /// And: User's topup_balance increases by 1000
    /// And: subscription_balance remains 0
    /// And: Ledger entry shows TopupCredit type with 1000 amount
    #[test_context(TestContext)]
    #[tokio::test]
    async fn test_one_time_fulfillment_grants_topup_credit(ctx: &mut TestContext) {
        let realm_id = ctx._realm_id.clone();
        let user_id = Uuid::now_v7();

        // Given: A one-time mapping with 1000 points and 30-day validity
        let mapping_id =
            create_one_time_mapping_with_points(ctx, &realm_id, 1000, Some(30), true).await;

        // And: A user with a points wallet
        create_points_wallet(ctx, user_id, &realm_id).await;

        // And: A pending payment attempt
        let attempt_id =
            create_pending_attempt_for_mapping(ctx, &realm_id, user_id, mapping_id, 999, "USD")
                .await;

        // When: Fulfilling the payment attempt
        let provider_tx_id = format!("pi_test_{}", attempt_id);
        let result = fulfill_attempt(ctx, attempt_id, &provider_tx_id).await;
        assert!(result.is_ok(), "Fulfillment should succeed: {:?}", result);

        // Then: Payment attempt status becomes Succeeded
        let status = get_attempt_status(ctx, attempt_id).await;
        assert_eq!(status.as_deref(), Some("Succeeded"));

        // And: User's topup_balance increases by 1000
        let account = get_points_wallet_by_user(ctx, user_id).await;
        assert!(account.is_some(), "User should have a points wallet");
        let (_wallet_id, _total_balance, topup_balance, subscription_balance) = account.unwrap();
        assert_eq!(topup_balance, 1000, "User should have 1000 topup_credit");
        assert_eq!(
            subscription_balance, 0,
            "subscription_balance should remain 0"
        );

        // And: Ledger entry shows TopupCredit type with 1000 amount
        let ledger_count = count_ledger_entries_for_user(ctx, user_id, "topup_credit").await;
        assert_eq!(
            ledger_count, 1,
            "Should have exactly 1 topup_credit ledger entry"
        );
    }

    // =========================================================================
    // Test 2: One-time fulfillment with validity_days
    // =========================================================================

    /// User Story: US-PU-006
    /// Covers: Design section 5.1 "one-time reads mapping.validity_days"
    ///
    /// Scenario: Points are granted with the correct validity period
    /// Given: A one-time mapping with points_per_period=500 and validity_days=90
    /// And: A user with a points wallet
    /// And: A pending payment attempt
    /// When: Fulfilling the payment attempt
    /// Then: Points are granted with an expiration ~90 days from now
    #[test_context(TestContext)]
    #[tokio::test]
    async fn test_one_time_fulfillment_with_validity_days(ctx: &mut TestContext) {
        let realm_id = ctx._realm_id.clone();
        let user_id = Uuid::now_v7();

        // Given: A one-time mapping with 500 points and 90-day validity
        let mapping_id =
            create_one_time_mapping_with_points(ctx, &realm_id, 500, Some(90), true).await;

        // And: A user with a points wallet
        create_points_wallet(ctx, user_id, &realm_id).await;

        // And: A pending payment attempt
        let attempt_id =
            create_pending_attempt_for_mapping(ctx, &realm_id, user_id, mapping_id, 999, "USD")
                .await;

        // When: Fulfilling the payment attempt
        let provider_tx_id = format!("pi_test_{}", attempt_id);
        let result = fulfill_attempt(ctx, attempt_id, &provider_tx_id).await;
        assert!(result.is_ok(), "Fulfillment should succeed: {:?}", result);

        // Then: Points are granted with an expiration ~90 days from now
        let expires_at = get_latest_topup_expiry(ctx, user_id).await;
        assert!(
            expires_at.is_some(),
            "Ledger entry should have an expires_at"
        );

        let now = chrono::Utc::now();
        let expires = expires_at.unwrap();
        let diff = expires - now;
        let diff_days = diff.num_days();
        // Allow some tolerance (should be ~90 days, check it's between 89 and 91)
        assert!(
            (89..=91).contains(&diff_days),
            "Expiration should be ~90 days from now, got {} days",
            diff_days
        );
    }

    // =========================================================================
    // Test 3: One-time fulfillment without validity_days (permanent)
    // =========================================================================

    /// User Story: US-PU-006
    /// Covers: Design section 5.1 "validity_days null means permanent"
    ///
    /// Scenario: Points are granted as permanent (no expiration)
    /// Given: A one-time mapping with points_per_period=300 and validity_days=NULL
    /// And: A user with a points wallet
    /// And: A pending payment attempt
    /// When: Fulfilling the payment attempt
    /// Then: Points are granted with no expiration (permanent)
    #[test_context(TestContext)]
    #[tokio::test]
    async fn test_one_time_fulfillment_without_validity_days_permanent(ctx: &mut TestContext) {
        let realm_id = ctx._realm_id.clone();
        let user_id = Uuid::now_v7();

        // Given: A one-time mapping with 300 points and no validity_days
        let mapping_id = create_one_time_mapping_with_points(ctx, &realm_id, 300, None, true).await;

        // And: A user with a points wallet
        create_points_wallet(ctx, user_id, &realm_id).await;

        // And: A pending payment attempt
        let attempt_id =
            create_pending_attempt_for_mapping(ctx, &realm_id, user_id, mapping_id, 999, "USD")
                .await;

        // When: Fulfilling the payment attempt
        let provider_tx_id = format!("pi_test_{}", attempt_id);
        let result = fulfill_attempt(ctx, attempt_id, &provider_tx_id).await;
        assert!(result.is_ok(), "Fulfillment should succeed: {:?}", result);

        // Then: Points are granted with no expiration (permanent)
        let expires_at = get_latest_topup_expiry(ctx, user_id).await;
        assert!(
            expires_at.is_none(),
            "Permanent points should have NULL expires_at, got {:?}",
            expires_at
        );

        // And: Balance reflects 300 topup points
        let account = get_points_wallet_by_user(ctx, user_id).await.unwrap();
        assert_eq!(account.2, 300, "User should have 300 topup_credit");
    }

    // =========================================================================
    // Test 4: Idempotent fulfillment prevents double-grants (CRITICAL)
    // =========================================================================

    /// User Story: US-PU-006, US-PA-003
    /// Covers: Design section 5.1 "idempotent via payment attempt + ledger source_id"
    ///
    /// CRITICAL: This test prevents duplicate points grants from webhook retries.
    ///
    /// Scenario: Duplicate fulfillment calls should not double-grant points
    /// Given: A one-time mapping with points_per_period=750
    /// And: A user with a wallet and a pending payment attempt
    /// When: Calling fulfillment twice with the same attempt ID and provider tx ID
    /// Then: First fulfillment succeeds and grants 750 topup points
    /// And: Second fulfillment also returns OK (idempotent)
    /// And: User still has exactly 750 topup points (not 1500)
    /// And: Only 1 ledger entry exists
    #[test_context(TestContext)]
    #[tokio::test]
    async fn test_idempotent_one_time_fulfillment_prevents_double_grants(ctx: &mut TestContext) {
        let realm_id = ctx._realm_id.clone();
        let user_id = Uuid::now_v7();

        // Given: A one-time mapping with 750 points
        let mapping_id = create_one_time_mapping_with_points(ctx, &realm_id, 750, None, true).await;

        // And: A user with a wallet
        create_points_wallet(ctx, user_id, &realm_id).await;

        // And: A pending payment attempt
        let attempt_id =
            create_pending_attempt_for_mapping(ctx, &realm_id, user_id, mapping_id, 999, "USD")
                .await;

        let provider_tx_id = format!("pi_test_{}", attempt_id);

        // When: First fulfillment call
        let result1 = fulfill_attempt(ctx, attempt_id, &provider_tx_id).await;
        assert!(
            result1.is_ok(),
            "First fulfillment should succeed: {:?}",
            result1
        );

        // Then: User has 750 topup points
        let account = get_points_wallet_by_user(ctx, user_id).await.unwrap();
        assert_eq!(
            account.2, 750,
            "User should have 750 topup_credit after first fulfillment"
        );
        assert_eq!(account.3, 0, "subscription_balance should be 0");

        // When: Second fulfillment call (simulating webhook retry)
        let result2 = fulfill_attempt(ctx, attempt_id, &provider_tx_id).await;
        assert!(
            result2.is_ok(),
            "Second fulfillment should succeed (idempotent): {:?}",
            result2
        );

        // Then: User still has exactly 750 topup points (NOT 1500)
        let account = get_points_wallet_by_user(ctx, user_id).await.unwrap();
        assert_eq!(
            account.2, 750,
            "User should still have 750 topup_credit after second fulfillment (idempotency)"
        );
        assert_eq!(account.3, 0, "subscription_balance should still be 0");

        // And: Only 1 ledger entry exists
        let ledger_count = count_ledger_entries_for_user(ctx, user_id, "topup_credit").await;
        assert_eq!(
            ledger_count, 1,
            "Should have exactly 1 ledger entry (not doubled)"
        );
    }

    // =========================================================================
    // Test 5: resolve_target mapping not found returns error
    // =========================================================================

    /// User Story: US-PA-001
    /// Covers: Design section 5.1 "mapping not found -> error"
    ///
    /// Scenario: Creating a payment attempt for a non-existent mapping returns error
    /// Given: No mapping exists for the given target_id
    /// When: Creating a payment attempt via the purchase endpoint
    /// Then: Response returns an error (no attempt created)
    #[test_context(TestContext)]
    #[tokio::test]
    async fn test_resolve_target_mapping_not_found_returns_error(ctx: &mut TestContext) {
        use crate::tests::helpers::billing_helpers::setup_billing_admin_session;
        use axum::body::{Body, to_bytes};
        use axum::http::{Request, StatusCode};
        use tower::ServiceExt;

        let realm_id = ctx._realm_id.clone();
        let app = ctx.create_unified_test_router();

        // Given: No mapping exists for this random UUID
        let nonexistent_mapping_id = Uuid::now_v7();

        // And: An authenticated user
        let token = setup_billing_admin_session(ctx, "resolve-notfound@test.com").await;

        // When: Creating a payment attempt for the non-existent mapping
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/bill/{}/purchase/payment-attempts", realm_id))
                    .header("Content-Type", "application/json")
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::from(
                        json!({
                            "targetType": "entitlement_mapping",
                            "targetId": nonexistent_mapping_id.to_string(),
                            "paymentProvider": "stripe"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Then: Response returns an error (400/404/409)
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body_text = String::from_utf8(body.to_vec()).unwrap();

        assert!(
            status == StatusCode::CONFLICT
                || status == StatusCode::NOT_FOUND
                || status == StatusCode::BAD_REQUEST,
            "Expected error status (400/404/409) for non-existent mapping, got {}: {}",
            status,
            body_text
        );
    }

    // =========================================================================
    // Test 6: resolve_target mapping disabled returns error
    // =========================================================================

    /// User Story: US-PA-001
    /// Covers: Design section 5.1 "mapping disabled -> error"
    ///
    /// Scenario: Creating a payment attempt for a disabled mapping returns error
    /// Given: A one-time mapping with enabled=false
    /// When: Creating a payment attempt targeting the disabled mapping
    /// Then: Response returns a 409 error (mapping is not enabled)
    #[test_context(TestContext)]
    #[tokio::test]
    async fn test_resolve_target_mapping_disabled_returns_error(ctx: &mut TestContext) {
        use crate::tests::helpers::billing_helpers::setup_billing_admin_session;
        use axum::body::{Body, to_bytes};
        use axum::http::{Request, StatusCode};
        use tower::ServiceExt;

        let realm_id = ctx._realm_id.clone();
        let app = ctx.create_unified_test_router();

        // Given: A one-time mapping with enabled=false
        let mapping_id =
            create_one_time_mapping_with_points(ctx, &realm_id, 500, None, false).await;

        // And: An authenticated user
        let token = setup_billing_admin_session(ctx, "resolve-disabled@test.com").await;

        // When: Creating a payment attempt targeting the disabled mapping
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/bill/{}/purchase/payment-attempts", realm_id))
                    .header("Content-Type", "application/json")
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::from(
                        json!({
                            "targetType": "entitlement_mapping",
                            "targetId": mapping_id.to_string(),
                            "paymentProvider": "stripe"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Then: Response returns a 409 error
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body_text = String::from_utf8(body.to_vec()).unwrap();

        assert_eq!(
            status,
            StatusCode::CONFLICT,
            "Expected 409 Conflict for disabled mapping, got {}: {}",
            status,
            body_text
        );

        assert!(
            body_text.to_lowercase().contains("disabled"),
            "Error should mention 'disabled', got: {}",
            body_text
        );
    }

    // =========================================================================
    // Test 7: resolve_target mapping no provider_product_info returns error
    // =========================================================================

    /// User Story: US-PA-001
    /// Covers: Design section 5.1 "mapping no provider_product_info -> error"
    ///
    /// Scenario: Creating a payment attempt for a mapping without provider_product_info
    /// returns an error or creates an attempt with zero amount
    /// Given: A one-time mapping with provider_product_info=NULL
    /// When: Creating a payment attempt for this mapping
    /// Then: The request does not succeed with valid price/amount
    #[test_context(TestContext)]
    #[tokio::test]
    async fn test_resolve_target_mapping_no_provider_info_returns_error(ctx: &mut TestContext) {
        use crate::tests::helpers::billing_helpers::setup_billing_admin_session;
        use axum::body::{Body, to_bytes};
        use axum::http::{Request, StatusCode};
        use tower::ServiceExt;

        let realm_id = ctx._realm_id.clone();
        let app = ctx.create_unified_test_router();

        // Given: A one-time mapping with provider_product_info=NULL
        // Use setup_test_entitlement_mapping_with_points which does not set provider_product_info
        let mapping_id =
            crate::tests::helpers::billing_helpers::setup_test_entitlement_mapping_with_points(
                ctx,
                &realm_id,
                "stripe",
                &format!("prod_no_info_{}", Uuid::now_v7()),
                &format!("one-time-no-info-{}", Uuid::now_v7()),
                500,
                false, // grant_on_subscribe
                true,  // enabled
            )
            .await;

        // And: Set billing_type to one_time (the helper does not set it)
        sqlx::query(
            "UPDATE provider_entitlement_mappings SET billing_type = 'one_time' WHERE id = $1",
        )
        .bind(mapping_id)
        .execute(&ctx.app_state.pool)
        .await
        .expect("Failed to set billing_type");

        // And: An authenticated user
        let token = setup_billing_admin_session(ctx, "resolve-noinfo@test.com").await;

        // When: Creating a payment attempt for this mapping
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/bill/{}/purchase/payment-attempts", realm_id))
                    .header("Content-Type", "application/json")
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::from(
                        json!({
                            "targetType": "entitlement_mapping",
                            "targetId": mapping_id.to_string(),
                            "paymentProvider": "stripe"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body_text = String::from_utf8(body.to_vec()).unwrap();

        // Then: Stripe purchase resolution refuses a mapping row without
        // price/currency info (422 fail-loud). The historical silent fallback
        // (amount=0 / "usd") could never produce a legitimate charge and has
        // been replaced by an explicit error; store-priced providers
        // (apple/google/wechat/creem) are exempt and keep their sentinel
        // amount semantics.
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "stripe + no price info must fail 422, got {}: {}",
            status,
            body_text
        );
        assert!(
            body_text.contains("Price info missing"),
            "error should name the missing price info, got: {body_text}"
        );
    }

    // =========================================================================
    // Test 8: Disabled mapping allows fulfillment of existing attempt
    // =========================================================================

    /// User Story: US-PA-003
    /// Covers: Design section 5.1 "mapping disabled only affects new purchase creation, not fulfillment"
    ///
    /// Scenario: Fulfillment succeeds for an attempt created before mapping was disabled
    /// Given: A one-time mapping that was enabled when a payment attempt was created
    /// And: The mapping is then disabled
    /// And: The payment attempt is still pending
    /// When: Fulfilling the payment attempt
    /// Then: Fulfillment succeeds
    /// And: User receives the topup points
    #[test_context(TestContext)]
    #[tokio::test]
    async fn test_disabled_mapping_allows_fulfillment_of_existing_attempt(ctx: &mut TestContext) {
        let realm_id = ctx._realm_id.clone();
        let user_id = Uuid::now_v7();

        // Given: A one-time mapping that was enabled
        let mapping_id =
            create_one_time_mapping_with_points(ctx, &realm_id, 600, Some(30), true).await;

        // And: A user with a points wallet
        create_points_wallet(ctx, user_id, &realm_id).await;

        // And: A pending payment attempt created while mapping was enabled
        let attempt_id =
            create_pending_attempt_for_mapping(ctx, &realm_id, user_id, mapping_id, 999, "USD")
                .await;

        // And: The mapping is then disabled
        sqlx::query("UPDATE provider_entitlement_mappings SET enabled = false WHERE id = $1")
            .bind(mapping_id)
            .execute(&ctx.app_state.pool)
            .await
            .expect("Failed to disable mapping");

        // When: Fulfilling the payment attempt
        let provider_tx_id = format!("pi_test_{}", attempt_id);
        let result = fulfill_attempt(ctx, attempt_id, &provider_tx_id).await;

        // Then: Fulfillment succeeds
        assert!(
            result.is_ok(),
            "Fulfillment should succeed even with disabled mapping: {:?}",
            result
        );

        // And: User receives the topup points
        let account = get_points_wallet_by_user(ctx, user_id).await;
        assert!(account.is_some(), "User should have a points wallet");
        let (_wallet_id, _total_balance, topup_balance, subscription_balance) = account.unwrap();
        assert_eq!(topup_balance, 600, "User should have 600 topup_credit");
        assert_eq!(subscription_balance, 0, "subscription_balance should be 0");
    }
}

/// A top-up is one business event, so both target wallets must commit together.
#[test_context::test_context(crate::tests::schema_test_context::SchemaTestContext)]
#[tokio::test]
async fn test_multi_wallet_grant_rule_topup_two_accounts_atomically(
    ctx: &mut crate::tests::schema_test_context::SchemaTestContext,
) {
    crate::tests::scenarios::points::multi_wallet_grant_rule_scenarios::
        assert_two_account_fixed_event(
            ctx,
            herald_core::domain::points::DistributionTrigger::Topup,
        )
        .await;
}

/// Purchase-time rule capture prevents configuration races from rerouting paid attempts.
#[test_context::test_context(crate::tests::schema_test_context::SchemaTestContext)]
#[tokio::test]
async fn test_multi_wallet_grant_rule_payment_snapshot_survives_rule_disable(
    ctx: &mut crate::tests::schema_test_context::SchemaTestContext,
) {
    crate::tests::scenarios::points::multi_wallet_grant_rule_scenarios::
        assert_snapshot_survives_disable(ctx)
        .await;
}
