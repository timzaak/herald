// =============================================================================
// Entitlement Subscription Endpoint Scenario Tests
// =============================================================================
//
// Tests for subscription list and detail endpoints with the new schema
// (entitlement_key, external_price_id, provider_metadata, synced_at).
//
// User Story: US-EM-006 (Subscription Projection List)
// Covers: GET subscriptions list with entitlement_key filter, status filter;
//         GET subscription detail with new fields, not found
//
// =============================================================================

#[cfg(test)]
mod tests {
    use crate::tests::helpers::billing_helpers::setup_billing_admin_session;
    use crate::tests::helpers::subscription_test_helpers::{
        create_test_subscription_full, create_test_subscription_with_entitlement_key,
    };
    use crate::tests::schema_test_context::SchemaTestContext;
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode, header},
    };
    use herald_core::domain::authentication::BrowserTokenService;
    use herald_core::domain::client::ports::ClientService;
    use herald_core::domain::user::UserRepository;
    use herald_core::infrastructure::authentication::RedisBrowserTokenService;
    use serde_json::json;
    use test_context::test_context;
    use tower::ServiceExt;
    use uuid::Uuid;

    use SchemaTestContext as SubTestContext;

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

    // =========================================================================
    // GET /api/bill/subscriptions (US-EM-006)
    // =========================================================================

    /// User Story: US-EM-006
    /// Covers: List returns entitlement_key, no planId/tier/billingPeriod
    #[test_context(SubTestContext)]
    #[tokio::test]
    async fn test_list_subscriptions_returns_entitlement_key(ctx: &mut SubTestContext) {
        let app = ctx.create_unified_test_router();
        let token = setup_billing_admin_session(ctx, "sub-list-ek@test.com").await;
        let realm_id = ctx._realm_id.clone();
        let client_app_id = Uuid::parse_str(&ctx._client_app_id).unwrap();

        // Create a subscription with entitlement_key
        let sub_id = create_test_subscription_with_entitlement_key(
            ctx,
            &realm_id,
            client_app_id,
            "pro-plan",
            "price_abc123",
            "stripe",
            "active",
        )
        .await;

        let response = app
            .clone()
            .oneshot(auth_request(
                "GET",
                "/api/bill/subscriptions".to_string(),
                &token,
                None,
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        // Should have items with entitlement_key
        let items = json["items"].as_array().expect("items should be an array");
        assert!(!items.is_empty(), "Should have at least one subscription");

        // Find our subscription
        let sub = items
            .iter()
            .find(|item| item["id"] == sub_id.to_string())
            .expect("Created subscription should appear in list");

        // Verify new fields are present
        assert_eq!(sub["entitlementKey"], "pro-plan");
        assert_eq!(sub["paymentProvider"], "stripe");

        // Verify old fields are NOT present
        assert!(
            sub.get("planId").is_none(),
            "planId should not be in response"
        );
        assert!(sub.get("tier").is_none(), "tier should not be in response");
        assert!(
            sub.get("billingPeriod").is_none(),
            "billingPeriod should not be in response"
        );
    }

    /// User Story: US-EM-006
    /// Covers: Filter by entitlementKey returns matching subscriptions only
    #[test_context(SubTestContext)]
    #[tokio::test]
    async fn test_list_subscriptions_filter_by_entitlement_key(ctx: &mut SubTestContext) {
        let app = ctx.create_unified_test_router();
        let token = setup_billing_admin_session(ctx, "sub-filter-ek@test.com").await;
        let realm_id = ctx._realm_id.clone();
        let client_app_id = Uuid::parse_str(&ctx._client_app_id).unwrap();

        // Create subscriptions with different entitlement_keys
        create_test_subscription_with_entitlement_key(
            ctx,
            &realm_id,
            client_app_id,
            "basic-plan",
            "price_basic",
            "creem",
            "active",
        )
        .await;

        // Use a different client_app_id for the second subscription to avoid UNIQUE conflict
        let client_app_id_2 = Uuid::now_v7();
        // Ensure a user row exists for the second client app
        sqlx::query(
            "INSERT INTO client_apps (id, realm_id, client_id, name, redirect_uris, enabled, created_at, updated_at)
             VALUES ($1, $2, $3, 'test-app-2', '[]', true, NOW(), NOW())
             ON CONFLICT (id) DO NOTHING",
        )
        .bind(client_app_id_2)
        .bind(&realm_id)
        .bind(format!("client-{}", client_app_id_2))
        .execute(&ctx.app_state.pool)
        .await
        .ok(); // Ignore error if already exists

        create_test_subscription_with_entitlement_key(
            ctx,
            &realm_id,
            client_app_id_2,
            "premium-plan",
            "price_premium",
            "stripe",
            "active",
        )
        .await;

        // Filter by entitlementKey=basic-plan
        let response = app
            .clone()
            .oneshot(auth_request(
                "GET",
                "/api/bill/subscriptions?entitlementKey=basic-plan".to_string(),
                &token,
                None,
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        let items = json["items"].as_array().expect("items should be an array");
        assert_eq!(items.len(), 1, "Should return exactly 1 subscription");
        assert_eq!(items[0]["entitlementKey"], "basic-plan");
    }

    /// User Story: US-EM-006
    /// Covers: Filter by status=active returns only active subscriptions
    #[test_context(SubTestContext)]
    #[tokio::test]
    async fn test_list_subscriptions_filter_by_status(ctx: &mut SubTestContext) {
        let app = ctx.create_unified_test_router();
        let token = setup_billing_admin_session(ctx, "sub-filter-status@test.com").await;
        let realm_id = ctx._realm_id.clone();
        let client_app_id = Uuid::parse_str(&ctx._client_app_id).unwrap();

        // Create an active subscription
        create_test_subscription_with_entitlement_key(
            ctx,
            &realm_id,
            client_app_id,
            "active-plan",
            "price_active",
            "creem",
            "active",
        )
        .await;

        // Create a canceled subscription with a different client_app_id
        let client_app_id_2 = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO client_apps (id, realm_id, client_id, name, redirect_uris, enabled, created_at, updated_at)
             VALUES ($1, $2, $3, 'test-app-canceled', '[]', true, NOW(), NOW())
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
            "canceled-plan",
            "price_canceled",
            "creem",
            "canceled",
        )
        .await;

        // Filter by status=active
        let response = app
            .clone()
            .oneshot(auth_request(
                "GET",
                "/api/bill/subscriptions?status=active".to_string(),
                &token,
                None,
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        let items = json["items"].as_array().expect("items should be an array");
        // All returned subscriptions should be active
        for item in items {
            assert_eq!(item["status"], "active");
        }
    }

    // =========================================================================
    // GET /api/bill/subscriptions/{subscriptionId} (US-EM-006)
    // =========================================================================

    /// User Story: US-EM-006
    /// Covers: Subscription detail includes entitlement_key, external_price_id,
    ///         provider_metadata, synced_at; no planId/tier/billingPeriod
    #[test_context(SubTestContext)]
    #[tokio::test]
    async fn test_get_subscription_detail_new_fields(ctx: &mut SubTestContext) {
        let app = ctx.create_unified_test_router();
        let token = setup_billing_admin_session(ctx, "sub-detail-new@test.com").await;
        let realm_id = ctx._realm_id.clone();
        let client_app_id = Uuid::parse_str(&ctx._client_app_id).unwrap();

        let metadata = json!({
            "stripe_product_name": "Pro Plan",
            "interval": "month"
        });

        let sub_id = create_test_subscription_full(
            ctx,
            &realm_id,
            client_app_id,
            "detail-plan",
            "price_detail_123",
            "stripe",
            "active",
            Some(metadata.clone()),
        )
        .await;

        let response = app
            .clone()
            .oneshot(auth_request(
                "GET",
                format!("/api/bill/subscriptions/{}", sub_id),
                &token,
                None,
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        // Verify new fields
        assert_eq!(json["entitlementKey"], "detail-plan");
        assert_eq!(json["externalPriceId"], "price_detail_123");
        assert_eq!(json["paymentProvider"], "stripe");
        assert_eq!(json["status"], "active");

        // providerMetadata should be present
        assert!(json.get("providerMetadata").is_some());

        // syncedAt should be present
        assert!(
            json.get("syncedAt").is_some(),
            "syncedAt should be present in response"
        );

        // Verify old fields are NOT present
        assert!(
            json.get("planId").is_none(),
            "planId should not be in response"
        );
        assert!(json.get("tier").is_none(), "tier should not be in response");
        assert!(
            json.get("billingPeriod").is_none(),
            "billingPeriod should not be in response"
        );
    }

    /// User Story: US-EM-006
    /// Covers: Non-existent subscription ID returns 404
    #[test_context(SubTestContext)]
    #[tokio::test]
    async fn test_get_subscription_detail_not_found(ctx: &mut SubTestContext) {
        let app = ctx.create_unified_test_router();
        let token = setup_billing_admin_session(ctx, "sub-notfound@test.com").await;
        let _realm_id = ctx._realm_id.clone();
        let fake_id = Uuid::now_v7();

        let response = app
            .clone()
            .oneshot(auth_request(
                "GET",
                format!("/api/bill/subscriptions/{}", fake_id),
                &token,
                None,
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    // =========================================================================
    // POST /api/bill/client/{clientAppId}/subscription/cancel
    // =========================================================================
    //
    // User self-service cancel. The route calls the provider cancel API and
    // deliberately does NOT mutate the local subscription row — local status is
    // updated by the provider webhook. These tests encode that contract: an
    // unsupported provider (Apple/Google) is rejected, and a provider failure
    // leaves the local row untouched.

    /// Mint a FirstParty browser-token family for `user_id` bound to the test
    /// client app. FirstParty tokens bypass scope checks but are still subject
    /// to `require_bound_client_app`, so they exercise the new browser route.
    async fn create_user_browser_token(ctx: &SubTestContext, user_id: Uuid) -> String {
        let user = ctx
            .app_state
            .user_repository
            .get_user_by_id(user_id)
            .await
            .expect("Failed to load test user");
        let client_app = ctx
            .app_state
            .service
            .client_service()
            .get_client_app_by_client_id(&ctx._realm_id, &ctx._client_id)
            .await
            .expect("Failed to load test client app");
        RedisBrowserTokenService::new(ctx.app_state.redis_manager.clone())
            .create_first_party_token_family(&user, &client_app, None, None)
            .await
            .expect("Failed to create user browser token family")
            .access_token
    }

    /// Create a subscription owned by `user_id` (so the ownership check passes)
    /// with a fixed external_subscription_id. Returns the local subscription id.
    async fn create_subscription_for_user(
        ctx: &SubTestContext,
        user_id: Uuid,
        payment_provider: &str,
    ) -> Uuid {
        let subscription_id = Uuid::now_v7();
        let external_subscription_id = format!("sub_test_{}", subscription_id);
        let client_app_id = Uuid::parse_str(&ctx._client_app_id).unwrap();
        sqlx::query(
            "INSERT INTO subscription
                (id, realm_id, user_id, client_app_id, status, entitlement_key, external_price_id,
                 external_subscription_id, external_product_id, payment_provider,
                 current_period_start, current_period_end,
                 provider_metadata, synced_at,
                 cancel_at_period_end, created_at, updated_at, billing_type)
             VALUES ($1, $2, $3, $4, 'active', $5, $6,
                     $7, $8, $9, NOW(), NOW() + INTERVAL '30 days',
                     NULL, NOW(),
                     false, NOW(), NOW(), 'recurring')",
        )
        .bind(subscription_id)
        .bind(&ctx._realm_id)
        .bind(user_id)
        .bind(client_app_id)
        .bind(format!("cancel-plan-{}", subscription_id))
        .bind(format!("price_{}", subscription_id))
        .bind(&external_subscription_id)
        .bind(format!("prod_{}", subscription_id))
        .bind(payment_provider)
        .execute(&ctx.app_state.pool)
        .await
        .expect("Failed to insert test subscription");
        subscription_id
    }

    /// Covers: Apple/Google subscriptions cannot be canceled via a developer
    /// API; the route must reject them with 400 instead of attempting a
    /// provider call. The local status must remain `active`.
    #[test_context(SubTestContext)]
    #[tokio::test]
    async fn test_cancel_subscription_apple_rejects_with_400(ctx: &mut SubTestContext) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();
        let client_app_id = Uuid::parse_str(&ctx._client_app_id).unwrap();

        // Create the owning user + a FirstParty browser token bound to the client app.
        let user_id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO account (id, realm_id, email, password, status)
             VALUES ($1, $2, $3, '$2a$12$dummy_password_hash', 1)",
        )
        .bind(user_id)
        .bind(&realm_id)
        .bind(format!("apple-cancel-owner-{}@test.com", user_id))
        .execute(&ctx.app_state.pool)
        .await
        .unwrap();
        let token = create_user_browser_token(ctx, user_id).await;
        let sub_id = create_subscription_for_user(ctx, user_id, "apple").await;

        let response = app
            .clone()
            .oneshot(auth_request(
                "POST",
                format!("/api/bill/client/{}/subscription/cancel", client_app_id),
                &token,
                Some(Body::from(
                    serde_json::to_vec(&json!({"cancelAtPeriodEnd": false})).unwrap(),
                )),
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        // Local status must be untouched (still active), per the webhook-driven model.
        let status: String =
            sqlx::query_scalar("SELECT status::text FROM subscription WHERE id = $1")
                .bind(sub_id)
                .fetch_one(&ctx.app_state.pool)
                .await
                .unwrap();
        assert_eq!(status, "active");
    }

    /// Covers: When the provider cancel API cannot be reached (Stripe not
    /// configured for the realm), the error surfaces and the local
    /// subscription row is left unchanged. This is the core "do not flip local
    /// state" contract of the provider-driven cancel.
    #[test_context(SubTestContext)]
    #[tokio::test]
    async fn test_cancel_subscription_stripe_failure_leaves_status_unchanged(
        ctx: &mut SubTestContext,
    ) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();
        let client_app_id = Uuid::parse_str(&ctx._client_app_id).unwrap();

        let user_id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO account (id, realm_id, email, password, status)
             VALUES ($1, $2, $3, '$2a$12$dummy_password_hash', 1)",
        )
        .bind(user_id)
        .bind(&realm_id)
        .bind(format!("stripe-cancel-owner-{}@test.com", user_id))
        .execute(&ctx.app_state.pool)
        .await
        .unwrap();
        let token = create_user_browser_token(ctx, user_id).await;
        let sub_id = create_subscription_for_user(ctx, user_id, "stripe").await;

        let response = app
            .clone()
            .oneshot(auth_request(
                "POST",
                format!("/api/bill/client/{}/subscription/cancel", client_app_id),
                &token,
                Some(Body::from(
                    serde_json::to_vec(&json!({"cancelAtPeriodEnd": false})).unwrap(),
                )),
            ))
            .await
            .unwrap();

        // Stripe is not configured in the test realm → CoreError → 5xx.
        assert!(
            response.status().is_server_error(),
            "expected 5xx when Stripe is unconfigured, got {}",
            response.status()
        );

        // The local row must NOT have been flipped — this is the contract.
        let status: String =
            sqlx::query_scalar("SELECT status::text FROM subscription WHERE id = $1")
                .bind(sub_id)
                .fetch_one(&ctx.app_state.pool)
                .await
                .unwrap();
        assert_eq!(
            status, "active",
            "local status must stay active when the provider call fails"
        );
    }
}
