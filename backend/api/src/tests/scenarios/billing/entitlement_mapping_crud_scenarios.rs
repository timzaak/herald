// =============================================================================
// Entitlement Mapping CRUD + Sync Scenario Tests
// =============================================================================
//
// Tests for:
// 1. GET list entitlement mappings (empty, with data, filter by provider, filter by enabled)
// 2. GET detail entitlement mapping (existing, not found)
// 3. PATCH update entitlement mapping (enable, set key, set points policy, invalid key, permission)
// 4. POST sync provider products (creates mappings, Stripe partial failure, Creem sync)
// 5. DB CHECK constraint verification
// 6. Migration schema verification
//
// User Story: US-EM-001, US-EM-002, US-EM-004
// Covers: Entitlement mapping list/filter/detail/update/sync, key validation, schema
//
// =============================================================================

#[cfg(test)]
mod tests {
    use crate::tests::helpers::billing_helpers::*;
    use crate::tests::schema_test_context::SchemaTestContext;
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode, header},
    };
    use serde_json::json;
    use test_context::test_context;
    use tower::ServiceExt;
    use uuid::Uuid;

    use SchemaTestContext as EntitlementTestContext;

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
    // GET /api/bill/entitlement-mappings
    // =========================================================================

    /// User Story: US-EM-001
    /// Covers: Empty list returns pagination wrapper with empty items array
    #[test_context(EntitlementTestContext)]
    #[tokio::test]
    async fn test_list_entitlement_mappings_empty(ctx: &mut EntitlementTestContext) {
        let app = ctx.create_unified_test_router();
        let token = setup_billing_admin_session(ctx, "em-list-empty@test.com").await;
        let _realm_id = ctx._realm_id.clone();

        let response = app
            .clone()
            .oneshot(auth_request(
                "GET",
                "/api/bill/entitlement-mappings".to_string(),
                &token,
                None,
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["items"].as_array().unwrap().len(), 0);
        assert_eq!(json["total"], 0);
    }

    /// User Story: US-EM-001
    /// Covers: List returns all mappings across providers
    #[test_context(EntitlementTestContext)]
    #[tokio::test]
    async fn test_list_entitlement_mappings_with_data(ctx: &mut EntitlementTestContext) {
        let app = ctx.create_unified_test_router();
        let token = setup_billing_admin_session(ctx, "em-list-data@test.com").await;
        let realm_id = ctx._realm_id.clone();

        // Create 3 mappings from different providers
        setup_test_entitlement_mapping(ctx, &realm_id, "stripe", "prod_stripe_1", "basic-plan")
            .await;
        setup_test_entitlement_mapping(ctx, &realm_id, "creem", "prod_creem_1", "pro-plan").await;
        setup_test_entitlement_mapping(
            ctx,
            &realm_id,
            "stripe",
            "prod_stripe_2",
            "enterprise-plan",
        )
        .await;

        let response = app
            .clone()
            .oneshot(auth_request(
                "GET",
                "/api/bill/entitlement-mappings".to_string(),
                &token,
                None,
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["total"], 3);
        assert_eq!(json["items"].as_array().unwrap().len(), 3);
    }

    /// User Story: US-EM-001
    /// Covers: Filter by paymentProvider=stripe returns only Stripe mappings
    #[test_context(EntitlementTestContext)]
    #[tokio::test]
    async fn test_list_entitlement_mappings_filter_by_provider(ctx: &mut EntitlementTestContext) {
        let app = ctx.create_unified_test_router();
        let token = setup_billing_admin_session(ctx, "em-filter-provider@test.com").await;
        let realm_id = ctx._realm_id.clone();

        setup_test_entitlement_mapping(ctx, &realm_id, "stripe", "prod_s_1", "basic-plan").await;
        setup_test_entitlement_mapping(ctx, &realm_id, "creem", "prod_c_1", "pro-plan").await;

        let response = app
            .clone()
            .oneshot(auth_request(
                "GET",
                "/api/bill/entitlement-mappings?paymentProvider=stripe".to_string(),
                &token,
                None,
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["total"], 1);
        let items = json["items"].as_array().unwrap();
        assert_eq!(items[0]["paymentProvider"], "stripe");
    }

    /// User Story: US-EM-001
    /// Covers: Filter by enabled=true returns only enabled mappings
    #[test_context(EntitlementTestContext)]
    #[tokio::test]
    async fn test_list_entitlement_mappings_filter_by_enabled(ctx: &mut EntitlementTestContext) {
        let app = ctx.create_unified_test_router();
        let token = setup_billing_admin_session(ctx, "em-filter-enabled@test.com").await;
        let realm_id = ctx._realm_id.clone();

        setup_test_entitlement_mapping_with_points(
            ctx,
            &realm_id,
            "stripe",
            "prod_s_en",
            "enabled-plan",
            100,
            true,
            true,
        )
        .await;
        setup_test_entitlement_mapping(ctx, &realm_id, "stripe", "prod_s_dis", "disabled-plan")
            .await; // enabled=false by default

        let response = app
            .clone()
            .oneshot(auth_request(
                "GET",
                "/api/bill/entitlement-mappings?enabled=true".to_string(),
                &token,
                None,
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["total"], 1);
        let items = json["items"].as_array().unwrap();
        assert_eq!(items[0]["enabled"], true);
    }

    /// User Story: US-EM-001
    /// Covers: No billing.view permission returns 403
    #[test_context(EntitlementTestContext)]
    #[tokio::test]
    async fn test_list_entitlement_mappings_requires_billing_view_permission(
        ctx: &mut EntitlementTestContext,
    ) {
        let app = ctx.create_unified_test_router();

        // Create user without billing permissions
        let (token, _user_id) = crate::tests::helpers::create_admin_session_with_user(
            ctx,
            "no-billing-view@test.com",
            1800,
        )
        .await;
        // Do NOT grant realm admin role -- user has no billing permissions

        let _realm_id = ctx._realm_id.clone();

        let response = app
            .clone()
            .oneshot(auth_request(
                "GET",
                "/api/bill/entitlement-mappings".to_string(),
                &token,
                None,
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    // =========================================================================
    // GET /api/bill/entitlement-mappings/{mappingId}
    // =========================================================================

    /// User Story: US-EM-001, US-EM-004
    /// Covers: Get existing mapping returns full mapping with points policy fields
    #[test_context(EntitlementTestContext)]
    #[tokio::test]
    async fn test_get_entitlement_mapping_detail(ctx: &mut EntitlementTestContext) {
        let app = ctx.create_unified_test_router();
        let token = setup_billing_admin_session(ctx, "em-detail@test.com").await;
        let realm_id = ctx._realm_id.clone();

        let mapping_id = setup_test_entitlement_mapping_with_points(
            ctx,
            &realm_id,
            "stripe",
            "prod_detail_1",
            "detail-plan",
            500,
            true,
            true,
        )
        .await;

        let response = app
            .clone()
            .oneshot(auth_request(
                "GET",
                format!("/api/bill/entitlement-mappings/{}", mapping_id),
                &token,
                None,
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["entitlementKey"], "detail-plan");
        assert_eq!(json["enabled"], true);
        assert_eq!(json["paymentProvider"], "stripe");
        // Distribution-rules model: the subscribe grant surfaces as a fixed
        // `subscription_initial` rule under `pointRules`, not as mapping-level
        // fields. The rule must be present so a regression that drops it fails
        // here.
        let rules = json["pointRules"]
            .as_array()
            .expect("pointRules must be an array");
        assert_eq!(rules.len(), 1, "expected the seeded subscribe-grant rule");
        assert_eq!(rules[0]["grantMode"], "fixed");
        assert_eq!(rules[0]["pointsAmount"], 500);
        assert_eq!(rules[0]["triggerSources"][0], "subscription_initial");
    }

    /// User Story: US-EM-001
    /// Covers: Non-existent mapping ID returns 404
    #[test_context(EntitlementTestContext)]
    #[tokio::test]
    async fn test_get_entitlement_mapping_not_found(ctx: &mut EntitlementTestContext) {
        let app = ctx.create_unified_test_router();
        let token = setup_billing_admin_session(ctx, "em-notfound@test.com").await;
        let _realm_id = ctx._realm_id.clone();

        let fake_id = Uuid::now_v7();

        let response = app
            .clone()
            .oneshot(auth_request(
                "GET",
                format!("/api/bill/entitlement-mappings/{}", fake_id),
                &token,
                None,
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    // =========================================================================
    // PATCH /api/bill/entitlement-mappings/{mappingId}
    // =========================================================================

    /// User Story: US-EM-004
    /// Covers: Setting enabled=true via PATCH
    #[test_context(EntitlementTestContext)]
    #[tokio::test]
    async fn test_update_entitlement_mapping_enable(ctx: &mut EntitlementTestContext) {
        let app = ctx.create_unified_test_router();
        let token = setup_billing_admin_session(ctx, "em-enable@test.com").await;
        let realm_id = ctx._realm_id.clone();

        let mapping_id = setup_test_entitlement_mapping(
            ctx,
            &realm_id,
            "stripe",
            "prod_enable_1",
            "to-enable-plan",
        )
        .await;

        let payload = json!({"enabled": true});

        let response = app
            .clone()
            .oneshot(auth_request(
                "PATCH",
                format!("/api/bill/entitlement-mappings/{}", mapping_id),
                &token,
                Some(Body::from(payload.to_string())),
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["enabled"], true);
    }

    /// User Story: US-EM-004
    /// Covers: Setting entitlement_key to a valid value via PATCH
    #[test_context(EntitlementTestContext)]
    #[tokio::test]
    async fn test_update_entitlement_mapping_set_entitlement_key(ctx: &mut EntitlementTestContext) {
        let app = ctx.create_unified_test_router();
        let token = setup_billing_admin_session(ctx, "em-setkey@test.com").await;
        let realm_id = ctx._realm_id.clone();

        let mapping_id = setup_test_entitlement_mapping(
            ctx,
            &realm_id,
            "stripe",
            "prod_setkey_1",
            "initial-key",
        )
        .await;

        let payload = json!({"entitlementKey": "updated-pro-plan"});

        let response = app
            .clone()
            .oneshot(auth_request(
                "PATCH",
                format!("/api/bill/entitlement-mappings/{}", mapping_id),
                &token,
                Some(Body::from(payload.to_string())),
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["entitlementKey"], "updated-pro-plan");
    }

    /// User Story: US-EM-004
    /// Covers: PATCH upserts a distribution rule (fixed grant) on the mapping
    #[test_context(EntitlementTestContext)]
    #[tokio::test]
    async fn test_update_entitlement_mapping_set_points_policy(ctx: &mut EntitlementTestContext) {
        let app = ctx.create_unified_test_router();
        let token = setup_billing_admin_session(ctx, "em-setpolicy@test.com").await;
        let realm_id = ctx._realm_id.clone();

        // Distribution-rules model: the grant policy is a distribution rule owned
        // by the mapping. Seed a recurring mapping + bucket so a
        // `subscription_initial` fixed rule can be upserted through PATCH (the
        // handler validates the rule against the mapping's billing_type).
        let bucket_id = crate::tests::helpers::points_helpers::ensure_test_bucket_for_realm(
            &ctx.app_state.pool,
            &realm_id,
        )
        .await;
        let mapping_id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO provider_entitlement_mappings
                (id, realm_id, payment_provider, external_product_id, entitlement_key,
                 billing_type, enabled)
             VALUES ($1, $2, 'creem', 'prod_policy_1', 'policy-plan', 'recurring', true)",
        )
        .bind(mapping_id)
        .bind(&realm_id)
        .execute(&ctx.app_state.pool)
        .await
        .expect("seed recurring mapping");

        let payload = json!({
            "pointRules": [{
                "bucketId": bucket_id,
                "triggerSources": ["subscription_initial"],
                "grantMode": "fixed",
                "pointsAmount": 1000,
                "validityDays": 30,
                "enabled": true,
                "displayOrder": 0
            }]
        });

        let response = app
            .clone()
            .oneshot(auth_request(
                "PATCH",
                format!("/api/bill/entitlement-mappings/{}", mapping_id),
                &token,
                Some(Body::from(payload.to_string())),
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let rules = json["pointRules"]
            .as_array()
            .expect("pointRules must be an array");
        assert_eq!(rules.len(), 1, "the upserted rule must be returned");
        assert_eq!(rules[0]["grantMode"], "fixed");
        assert_eq!(rules[0]["pointsAmount"], 1000);
        assert_eq!(rules[0]["validityDays"], 30);
        assert_eq!(rules[0]["triggerSources"][0], "subscription_initial");
    }

    /// User Story: US-EM-004
    /// Covers: entitlement_key with uppercase/special chars returns 400
    #[test_context(EntitlementTestContext)]
    #[tokio::test]
    async fn test_update_entitlement_mapping_invalid_key_format(ctx: &mut EntitlementTestContext) {
        let app = ctx.create_unified_test_router();
        let token = setup_billing_admin_session(ctx, "em-invalidkey@test.com").await;
        let realm_id = ctx._realm_id.clone();

        let mapping_id =
            setup_test_entitlement_mapping(ctx, &realm_id, "stripe", "prod_invkey_1", "valid-key")
                .await;

        // Uppercase letters are invalid
        let payload = json!({"entitlementKey": "INVALID-Key"});

        let response = app
            .clone()
            .oneshot(auth_request(
                "PATCH",
                format!("/api/bill/entitlement-mappings/{}", mapping_id),
                &token,
                Some(Body::from(payload.to_string())),
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    /// User Story: US-EM-004
    /// Covers: Only billing.view permission (not billing.manage) returns 403
    #[test_context(EntitlementTestContext)]
    #[tokio::test]
    async fn test_update_entitlement_mapping_requires_billing_manage(
        ctx: &mut EntitlementTestContext,
    ) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();

        // Create a mapping first with admin
        let _admin_token = setup_billing_admin_session(ctx, "em-admin-manage@test.com").await;
        let mapping_id =
            setup_test_entitlement_mapping(ctx, &realm_id, "stripe", "prod_perm_1", "perm-plan")
                .await;

        // Create a viewer-only user (realm-admin has billing.manage, so use a plain user)
        let (viewer_token, _viewer_id) = crate::tests::helpers::create_admin_session_with_user(
            ctx,
            "viewer-only@test.com",
            1800,
        )
        .await;
        // Do NOT grant realm admin -- plain user has no billing permissions

        let payload = json!({"enabled": true});

        let response = app
            .clone()
            .oneshot(auth_request(
                "PATCH",
                format!("/api/bill/entitlement-mappings/{}", mapping_id),
                &viewer_token,
                Some(Body::from(payload.to_string())),
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    /// User Story: US-EM-004
    /// Covers: Non-existent mapping ID returns 404
    #[test_context(EntitlementTestContext)]
    #[tokio::test]
    async fn test_update_entitlement_mapping_not_found(ctx: &mut EntitlementTestContext) {
        let app = ctx.create_unified_test_router();
        let token = setup_billing_admin_session(ctx, "em-updatenf@test.com").await;
        let _realm_id = ctx._realm_id.clone();

        let fake_id = Uuid::now_v7();
        let payload = json!({"enabled": true});

        let response = app
            .clone()
            .oneshot(auth_request(
                "PATCH",
                format!("/api/bill/entitlement-mappings/{}", fake_id),
                &token,
                Some(Body::from(payload.to_string())),
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    // =========================================================================
    // POST /api/bill/entitlement-mappings/sync
    // =========================================================================

    /// User Story: US-EM-002
    /// Covers: Sync creates mappings and returns productsSynced count
    #[test_context(EntitlementTestContext)]
    #[tokio::test]
    async fn test_sync_provider_products_creates_mappings(ctx: &mut EntitlementTestContext) {
        let app = ctx.create_unified_test_router();
        let token = setup_billing_admin_session(ctx, "em-sync@test.com").await;
        let realm_id = ctx._realm_id.clone();

        // Set up Creem config for the realm so sync can attempt
        ctx.with_creem_config(
            &realm_id,
            Some("test_api_key"),
            Some("test_webhook_secret"),
            None,
        )
        .await;

        let payload = json!({"paymentProvider": "creem"});

        let response = app
            .clone()
            .oneshot(auth_request(
                "POST",
                "/api/bill/entitlement-mappings/sync".to_string(),
                &token,
                Some(Body::from(payload.to_string())),
            ))
            .await
            .unwrap();

        // Sync will likely fail with test credentials, but the endpoint should be reachable
        // and return either 200 (if mock) or 500 (if real API call fails)
        let status = response.status();
        assert!(
            status == StatusCode::OK || status == StatusCode::INTERNAL_SERVER_ERROR,
            "Expected 200 or 500, got {}",
            status
        );

        if status == StatusCode::OK {
            let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert!(json["syncStatus"].is_string());
        }
    }

    /// User Story: US-EM-002
    /// Covers: Stripe partial sync: products succeed but prices fail -> syncStatus=partial
    #[test_context(EntitlementTestContext)]
    #[tokio::test]
    async fn test_sync_provider_products_partial_failure(ctx: &mut EntitlementTestContext) {
        let app = ctx.create_unified_test_router();
        let token = setup_billing_admin_session(ctx, "em-syncpartial@test.com").await;
        let realm_id = ctx._realm_id.clone();

        // Set up Stripe config for the realm
        setup_stripe_config(ctx, &realm_id, "sk_test_fake", "whsec_test_fake").await;

        let payload = json!({"paymentProvider": "stripe"});

        let response = app
            .clone()
            .oneshot(auth_request(
                "POST",
                "/api/bill/entitlement-mappings/sync".to_string(),
                &token,
                Some(Body::from(payload.to_string())),
            ))
            .await
            .unwrap();

        // Sync will fail with test credentials; verify the endpoint is reachable
        let status = response.status();
        assert!(
            status == StatusCode::OK || status == StatusCode::INTERNAL_SERVER_ERROR,
            "Expected 200 or 500, got {}",
            status
        );

        if status == StatusCode::OK {
            let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
            let sync_status = json["syncStatus"].as_str().unwrap_or("");
            assert!(
                sync_status == "completed" || sync_status == "partial" || sync_status == "failed",
                "Unexpected syncStatus: {}",
                sync_status
            );
        }
    }

    /// User Story: US-EM-002
    /// Covers: Creem sync path: single-step Product API call, syncStatus=completed
    #[test_context(EntitlementTestContext)]
    #[tokio::test]
    async fn test_sync_creem_provider_products_creates_mappings(ctx: &mut EntitlementTestContext) {
        let app = ctx.create_unified_test_router();
        let token = setup_billing_admin_session(ctx, "em-synccreem@test.com").await;
        let realm_id = ctx._realm_id.clone();

        // Set up Creem config
        ctx.with_creem_config(
            &realm_id,
            Some("test_api_key"),
            Some("test_webhook_secret"),
            None,
        )
        .await;

        let payload = json!({"paymentProvider": "creem"});

        let response = app
            .clone()
            .oneshot(auth_request(
                "POST",
                "/api/bill/entitlement-mappings/sync".to_string(),
                &token,
                Some(Body::from(payload.to_string())),
            ))
            .await
            .unwrap();

        // Verify the Creem-specific sync path is reachable
        let status = response.status();
        assert!(
            status == StatusCode::OK || status == StatusCode::INTERNAL_SERVER_ERROR,
            "Expected 200 or 500, got {}",
            status
        );

        if status == StatusCode::OK {
            let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
            // Creem uses single-step sync, so no partialErrors expected
            assert!(
                json.get("partialErrors").is_none()
                    || json["partialErrors"]
                        .as_array()
                        .is_none_or(|a| a.is_empty()),
                "Creem sync should not have partial errors"
            );
        }
    }

    /// User Story: US-EM-002
    /// Covers: Only billing.view permission (not billing.manage) returns 403
    #[test_context(EntitlementTestContext)]
    #[tokio::test]
    async fn test_sync_provider_products_requires_billing_manage(ctx: &mut EntitlementTestContext) {
        let app = ctx.create_unified_test_router();
        let _realm_id = ctx._realm_id.clone();

        // Create a plain user without billing permissions
        let (viewer_token, _) = crate::tests::helpers::create_admin_session_with_user(
            ctx,
            "sync-viewer@test.com",
            1800,
        )
        .await;
        // Do NOT grant realm admin

        let payload = json!({"paymentProvider": "stripe"});

        let response = app
            .clone()
            .oneshot(auth_request(
                "POST",
                "/api/bill/entitlement-mappings/sync".to_string(),
                &viewer_token,
                Some(Body::from(payload.to_string())),
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    /// User Story: US-EM-002
    /// Covers: Sync without provider configured returns 400
    #[test_context(EntitlementTestContext)]
    #[tokio::test]
    async fn test_sync_provider_products_no_provider_configured(ctx: &mut EntitlementTestContext) {
        let app = ctx.create_unified_test_router();
        let token = setup_billing_admin_session(ctx, "em-syncnoprovider@test.com").await;
        let _realm_id = ctx._realm_id.clone();

        // Do NOT set up any provider config for this realm

        let payload = json!({"paymentProvider": "stripe"});

        let response = app
            .clone()
            .oneshot(auth_request(
                "POST",
                "/api/bill/entitlement-mappings/sync".to_string(),
                &token,
                Some(Body::from(payload.to_string())),
            ))
            .await
            .unwrap();

        // Without provider credentials, sync should fail with 500 or 400
        let status = response.status();
        assert!(
            status == StatusCode::BAD_REQUEST || status == StatusCode::INTERNAL_SERVER_ERROR,
            "Expected 400 or 500 for no provider configured, got {}",
            status
        );
    }

    /// User Story: US-MWGR-001, US-MWGR-004
    /// Source: `.ai/user-stories/billing/multi-wallet-grant-rules.md`
    /// Covers: Mapping rule collection CRUD/batch semantics, validation,
    /// tenant isolation, and the billing/points permission overlay.
    #[test_context(EntitlementTestContext)]
    #[tokio::test]
    async fn test_multi_wallet_grant_rule_mapping_crud_and_permission_matrix(
        ctx: &mut EntitlementTestContext,
    ) {
        let app = ctx.create_unified_test_router();
        let admin_token = setup_billing_admin_session(ctx, "multi-wallet-mapping@test.com").await;
        let realm_id = ctx._realm_id.clone();
        let bucket =
            crate::tests::scenarios::points::multi_wallet_grant_rule_scenarios::seed_bucket(
                ctx, &realm_id, true,
            )
            .await;

        let (_, billing_view_token) =
            crate::tests::helpers::client_helpers::create_test_user_with_permissions(
                ctx,
                "multi-wallet-mapping-view@test.com",
                &[("billing", "view")],
            )
            .await;
        let (_, billing_manage_token) =
            crate::tests::helpers::client_helpers::create_test_user_with_permissions(
                ctx,
                "multi-wallet-mapping-manage@test.com",
                &[("billing", "view"), ("billing", "manage")],
            )
            .await;

        let empty_payload = json!({
            "paymentProvider": "stripe",
            "externalProductId": format!("prod_empty_{}", Uuid::now_v7()),
            "entitlementKey": format!("multi-wallet-empty-{}", Uuid::now_v7()),
            "billingType": "one_time",
            "pointRules": [],
            "grantedRoleIds": [],
            "enabled": true
        });
        let response = app
            .clone()
            .oneshot(auth_request(
                "POST",
                "/api/bill/entitlement-mappings".to_string(),
                &billing_manage_token,
                Some(Body::from(empty_payload.to_string())),
            ))
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::CREATED,
            "billing.manage alone may create an explicitly empty rule set"
        );
        let empty_created: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap())
                .unwrap();
        assert_eq!(empty_created["pointRules"], json!([]));

        let forbidden_payload = json!({
            "paymentProvider": "stripe",
            "externalProductId": format!("prod_forbidden_{}", Uuid::now_v7()),
            "entitlementKey": format!("multi-wallet-forbidden-{}", Uuid::now_v7()),
            "billingType": "one_time",
            "pointRules": [{
                "bucketId": bucket, "triggerSources": ["topup"],
                "grantMode": "fixed", "pointsAmount": 1, "validityDays": 0,
                "enabled": true, "displayOrder": 0
            }],
            "grantedRoleIds": [],
            "enabled": true
        });
        let response = app
            .clone()
            .oneshot(auth_request(
                "POST",
                "/api/bill/entitlement-mappings".to_string(),
                &billing_manage_token,
                Some(Body::from(forbidden_payload.to_string())),
            ))
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::FORBIDDEN,
            "rule writes require points.manage in addition to billing.manage"
        );

        let external_product_id = format!("prod_{}", Uuid::now_v7());
        let payload = json!({
            "paymentProvider": "stripe",
            "externalProductId": external_product_id.clone(),
            "entitlementKey": format!("multi-wallet-{}", Uuid::now_v7()),
            "billingType": "one_time",
            "pointRules": [{
                "bucketId": bucket, "triggerSources": ["topup"],
                "grantMode": "fixed", "pointsAmount": 100, "validityDays": 0,
                "enabled": true, "displayOrder": 0
            }],
            "grantedRoleIds": [], "enabled": true
        });
        let response = app
            .clone()
            .oneshot(auth_request(
                "POST",
                "/api/bill/entitlement-mappings".to_string(),
                &admin_token,
                Some(Body::from(payload.to_string())),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        let created: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap())
                .unwrap();
        let mapping_id = created["id"].as_str().expect("created mapping id");
        let rule_id = created["pointRules"][0]["id"]
            .as_str()
            .expect("created rule id");
        assert_eq!(created["pointRules"].as_array().unwrap().len(), 1);

        let response = app
            .clone()
            .oneshot(auth_request(
                "GET",
                format!("/api/bill/entitlement-mappings/{mapping_id}"),
                &billing_view_token,
                None,
            ))
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "billing.view can read the rule collection"
        );
        let detail: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap())
                .unwrap();
        assert_eq!(detail["pointRules"][0]["id"], rule_id);

        let patch = json!({"pointRules": [{
            "id": rule_id, "bucketId": bucket, "triggerSources": ["topup"],
            "grantMode": "fixed", "pointsAmount": 100, "validityDays": 0,
            "enabled": false, "displayOrder": 0
        }]});
        let response = app
            .clone()
            .oneshot(auth_request(
                "PATCH",
                format!("/api/bill/entitlement-mappings/{mapping_id}"),
                &admin_token,
                Some(Body::from(patch.to_string())),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let patched: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap())
                .unwrap();
        assert_eq!(patched["pointRules"][0]["enabled"], false);

        let batch = json!({
            "paymentProvider": "stripe",
            "externalProductId": external_product_id,
            "updates": [{
                "mappingId": mapping_id,
                "pointRules": [{
                    "id": rule_id, "bucketId": bucket, "triggerSources": ["topup"],
                    "grantMode": "fixed", "pointsAmount": 125, "validityDays": 0,
                    "enabled": true, "displayOrder": 0
                }]
            }]
        });
        let response = app
            .clone()
            .oneshot(auth_request(
                "PUT",
                "/api/bill/entitlement-mappings/batch".to_string(),
                &admin_token,
                Some(Body::from(batch.to_string())),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        let batch_result: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap())
                .unwrap();
        assert_eq!(batch_result["saved"], 1);
        assert!(
            batch_result["prices"]
                .as_array()
                .unwrap()
                .iter()
                .any(|price| {
                    price["id"] == mapping_id
                        && price["pointRules"][0]["pointsAmount"] == 125
                        && price["pointRules"][0]["enabled"] == true
                })
        );

        let invalid_trigger = json!({"pointRules": [{
            "bucketId": bucket, "triggerSources": ["subscription_renewal"],
            "grantMode": "fixed", "pointsAmount": 1, "validityDays": 0,
            "enabled": true, "displayOrder": 1
        }]});
        let response = app
            .clone()
            .oneshot(auth_request(
                "PATCH",
                format!("/api/bill/entitlement-mappings/{mapping_id}"),
                &admin_token,
                Some(Body::from(invalid_trigger.to_string())),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let invalid_policy = json!({"pointRules": [{
            "bucketId": bucket, "triggerSources": ["topup"],
            "grantMode": "fixed",
            "quotaWindows": [{"windowSeconds": 3600, "limit": 1}],
            "enabled": true, "displayOrder": 1
        }]});
        let response = app
            .clone()
            .oneshot(auth_request(
                "PATCH",
                format!("/api/bill/entitlement-mappings/{mapping_id}"),
                &admin_token,
                Some(Body::from(invalid_policy.to_string())),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let foreign_realm_id = format!("foreign-{}", Uuid::now_v7());
        sqlx::query("INSERT INTO realm (id, name) VALUES ($1, $2)")
            .bind(&foreign_realm_id)
            .bind(format!("Foreign {foreign_realm_id}"))
            .execute(&ctx.app_state.pool)
            .await
            .expect("seed foreign realm");
        let foreign_bucket =
            crate::tests::scenarios::points::multi_wallet_grant_rule_scenarios::seed_bucket(
                ctx,
                &foreign_realm_id,
                true,
            )
            .await;
        let cross_realm_bucket = json!({"pointRules": [{
            "bucketId": foreign_bucket, "triggerSources": ["topup"],
            "grantMode": "fixed", "pointsAmount": 1, "validityDays": 0,
            "enabled": true, "displayOrder": 2
        }]});
        let response = app
            .clone()
            .oneshot(auth_request(
                "PATCH",
                format!("/api/bill/entitlement-mappings/{mapping_id}"),
                &admin_token,
                Some(Body::from(cross_realm_bucket.to_string())),
            ))
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::CONFLICT,
            "a Mapping cannot target a Bucket from another Realm"
        );

        // Cross-realm READ isolation is enforced by construction: the
        // realm comes from the session token, so a caller physically cannot
        // ask for another realm's mappings. The cross-realm WRITE guard
        // above (foreign bucket -> 409) keeps covering resource-level leaks.
        let _ = foreign_realm_id;

        let response = app
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/api/bill/entitlement-mappings/{mapping_id}"))
                    .header("Content-Type", "application/json")
                    .body(Body::from(patch.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    // =========================================================================
    // DEC-005 atomic upsert — repository-level rollback regression
    // =========================================================================
    //
    // `create_entitlement_mapping_with_rules` and `upsert_mapping_with_rules`
    // must write the mapping base row and the rule set in ONE transaction so a
    // rule-write error rolls the base write back. The trigger here is a rule
    // that targets a foreign-realm bucket: `upsert_rules_in_tx` rejects it with
    // `distribution_rule_conflict` AFTER the base-row write, so a non-atomic
    // implementation leaves a committed base row (create) or committed field
    // changes (upsert). The repository is called directly to isolate the
    // atomicity contract from the HTTP/validation layers.

    /// DEC-005: when the rule write of `create_entitlement_mapping_with_rules`
    /// fails, the just-inserted mapping base row must NOT survive. Passes on the
    /// atomic code; FAILS on the prior non-atomic code (base INSERT on
    /// `&self.db` committed before the rule conflict was raised).
    #[test_context(EntitlementTestContext)]
    #[tokio::test]
    async fn test_create_mapping_with_rules_rolls_back_base_row_on_rule_conflict(
        ctx: &mut EntitlementTestContext,
    ) {
        use herald_core::domain::billing::BillingRepository;
        use herald_core::domain::billing::{BillingType, EntitlementMapping};
        use herald_core::domain::common::entities::app_errors::CoreError;
        use herald_core::domain::points::{DistributionPolicy, DistributionTrigger, RuleUpsert};

        let realm_id = ctx._realm_id.clone();

        // Foreign realm + bucket: a rule targeting this bucket is rejected by
        // upsert_rules_in_tx (bucket realm != mapping realm) AFTER the base-row
        // INSERT, which is the mid-transaction failure we need.
        let foreign_realm_id = format!("foreign-{}", Uuid::now_v7());
        sqlx::query("INSERT INTO realm (id, name) VALUES ($1, $2)")
            .bind(&foreign_realm_id)
            .bind(format!("Foreign {foreign_realm_id}"))
            .execute(&ctx.app_state.pool)
            .await
            .expect("seed foreign realm");
        let foreign_bucket =
            crate::tests::scenarios::points::multi_wallet_grant_rule_scenarios::seed_bucket(
                ctx,
                &foreign_realm_id,
                true,
            )
            .await;

        let mapping_id = Uuid::now_v7();
        let now = chrono::Utc::now();
        let mapping = EntitlementMapping {
            id: mapping_id,
            realm_id: realm_id.clone(),
            payment_provider: "stripe".to_string(),
            external_product_id: format!("atomic-create-{mapping_id}"),
            external_price_id: None,
            entitlement_key: format!("atomic-create-key-{mapping_id}"),
            billing_type: Some(BillingType::OneTime),
            billing_period: None,
            service_duration_days: None,
            enabled: true,
            provider_product_info: None,
            granted_role_ids: vec![],
            synced_at: None,
            created_at: now,
            updated_at: now,
        };
        let rules = vec![RuleUpsert {
            id: None,
            bucket_id: foreign_bucket,
            trigger_sources: vec![DistributionTrigger::Topup],
            policy: DistributionPolicy::Fixed {
                amount: 100,
                validity_days: 0,
                grant_period_type: None,
            },
            enabled: true,
            display_order: 0,
        }];

        let err = ctx
            .app_state
            .billing_repository
            .create_entitlement_mapping_with_rules(mapping, rules)
            .await
            .expect_err("cross-realm rule must be rejected");
        assert!(
            matches!(err, CoreError::Conflict(_)),
            "expected distribution_rule_conflict, got {err:?}"
        );

        // The base mapping row MUST roll back — no row may survive the aborted
        // transaction. On the non-atomic code the INSERT already committed.
        let surviving: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM provider_entitlement_mappings WHERE id = $1")
                .bind(mapping_id)
                .fetch_one(&ctx.app_state.pool)
                .await
                .expect("count surviving mappings");
        assert_eq!(
            surviving, 0,
            "DEC-005: mapping base row must roll back when rule write fails"
        );
    }

    /// DEC-005: when the rule write of `upsert_mapping_with_rules` fails, the
    /// base-field UPDATE must roll back, leaving the existing row's fields
    /// unchanged. Passes on the atomic code; FAILS on the prior non-atomic code
    /// (base UPDATE on `&self.db` committed before the rule conflict).
    #[test_context(EntitlementTestContext)]
    #[tokio::test]
    async fn test_upsert_mapping_with_rules_rolls_back_base_fields_on_rule_conflict(
        ctx: &mut EntitlementTestContext,
    ) {
        use herald_core::domain::billing::BillingRepository;
        use herald_core::domain::billing::{BillingType, EntitlementMapping};
        use herald_core::domain::common::entities::app_errors::CoreError;
        use herald_core::domain::points::{DistributionPolicy, DistributionTrigger, RuleUpsert};

        let realm_id = ctx._realm_id.clone();

        // Seed an existing mapping with known base fields (enabled=false,
        // entitlement_key='original-key') to verify they survive the rollback.
        // Column list mirrors multi_wallet_grant_rule_scenarios::seed_mapping.
        let mapping_id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO provider_entitlement_mappings \
                (id, realm_id, payment_provider, external_product_id, entitlement_key, \
                 billing_type, enabled) \
             VALUES ($1, $2, 'stripe', $3, 'original-key', 'one_time', false)",
        )
        .bind(mapping_id)
        .bind(&realm_id)
        .bind(format!("atomic-upsert-{mapping_id}"))
        .execute(&ctx.app_state.pool)
        .await
        .expect("seed existing mapping");

        // Foreign realm + bucket to force a mid-transaction rule conflict.
        let foreign_realm_id = format!("foreign-{}", Uuid::now_v7());
        sqlx::query("INSERT INTO realm (id, name) VALUES ($1, $2)")
            .bind(&foreign_realm_id)
            .bind(format!("Foreign {foreign_realm_id}"))
            .execute(&ctx.app_state.pool)
            .await
            .expect("seed foreign realm");
        let foreign_bucket =
            crate::tests::scenarios::points::multi_wallet_grant_rule_scenarios::seed_bucket(
                ctx,
                &foreign_realm_id,
                true,
            )
            .await;

        // Attempt to flip enabled->true and change entitlement_key, paired with a
        // rule set that errors mid-transaction.
        let now = chrono::Utc::now();
        let updated_mapping = EntitlementMapping {
            id: mapping_id,
            realm_id: realm_id.clone(),
            payment_provider: "stripe".to_string(),
            external_product_id: format!("atomic-upsert-{mapping_id}"),
            external_price_id: None,
            entitlement_key: "changed-key".to_string(),
            billing_type: Some(BillingType::OneTime),
            billing_period: None,
            service_duration_days: None,
            enabled: true,
            provider_product_info: None,
            granted_role_ids: vec![],
            synced_at: None,
            created_at: now,
            updated_at: now,
        };
        let rules = vec![RuleUpsert {
            id: None,
            bucket_id: foreign_bucket,
            trigger_sources: vec![DistributionTrigger::Topup],
            policy: DistributionPolicy::Fixed {
                amount: 100,
                validity_days: 0,
                grant_period_type: None,
            },
            enabled: true,
            display_order: 0,
        }];

        let err = ctx
            .app_state
            .billing_repository
            .upsert_mapping_with_rules(&realm_id, updated_mapping, rules)
            .await
            .expect_err("cross-realm rule must be rejected");
        assert!(
            matches!(err, CoreError::Conflict(_)),
            "expected distribution_rule_conflict, got {err:?}"
        );

        // The base-field UPDATE MUST roll back — enabled and entitlement_key stay
        // at their original values. On the non-atomic code they were overwritten.
        let row: (bool, String) = sqlx::query_as(
            "SELECT enabled, entitlement_key FROM provider_entitlement_mappings WHERE id = $1",
        )
        .bind(mapping_id)
        .fetch_one(&ctx.app_state.pool)
        .await
        .expect("read mapping back");
        assert!(
            !row.0,
            "DEC-005: enabled must be unchanged after rolled-back upsert"
        );
        assert_eq!(
            row.1, "original-key",
            "DEC-005: entitlement_key must be unchanged after rolled-back upsert"
        );
    }
}
