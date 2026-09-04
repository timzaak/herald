// =============================================================================
// Invoice Apply-Eligibility Scenario Tests
// =============================================================================
//
// Per-resource, read-only apply-eligibility endpoint that lets the frontend
// gate the Apply Invoice button on a specific payment_attempt/subscription
// BEFORE submit, instead of relying on post-submit backend rejection.
//
// Decision is "External-if-synced" (confirmed by maintainer): the endpoint
// resolves ownership → provider → policy → seller config → external-invoice,
// then delegates to the pure `determine_invoice_apply_route` in
// `api-billing/src/invoice_eligibility.rs`.
//
// Covers:
//   - policy=none                  => disabled
//   - creem provider               => disabled
//   - no seller config             => disabled
//   - manual_only + no provider + seller + no external => manual_fallback (canApply=true)
//   - provider_first + stripe + seller + NO external invoice  => external_provider (canApply=false)
//     (Stripe invoices are pushed via webhook; users never apply manually)
//   - provider_first + stripe + WITH an external_sync invoice => external_provider (canApply=false)
//   - resource owned by another user => 403
//   - subscription with no user owner => 403 (not a database decode error)
//   - nonexistent resource            => 404
//
// =============================================================================

#[cfg(test)]
mod tests {
    use crate::tests::helpers::*;
    use crate::tests::schema_test_context::SchemaTestContext;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use chrono::Datelike;
    use serde_json::json;
    use test_context::test_context;
    use tower::ServiceExt;
    use uuid::Uuid;

    use SchemaTestContext as ApplyEligibilityTestContext;

    // -------------------------------------------------------------------------
    // Helpers
    // -------------------------------------------------------------------------

    async fn parse_body(body: axum::body::Body) -> serde_json::Value {
        let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    /// Create a regular user session (no admin role) and return (token, user_id).
    async fn create_regular_user_session(
        ctx: &ApplyEligibilityTestContext,
        email: &str,
    ) -> (String, Uuid) {
        let (token, user_id_str) =
            crate::tests::helpers::create_admin_session_with_user(ctx, email, 1800).await;
        let user_id = Uuid::parse_str(&user_id_str).expect("Invalid user_id format");
        // Do NOT grant realm-admin role — this is a regular user.
        (token, user_id)
    }

    /// Set the invoice policy for a realm by inserting/updating realm_config.
    /// Mirrors the helper in feature_availability_invoice_eligibility_scenarios.rs.
    async fn set_invoice_policy(
        ctx: &ApplyEligibilityTestContext,
        realm_id: &str,
        policy: &str,
        provider_capabilities: &str,
    ) {
        let config_value = json!({
            "policy": policy,
            "provider_capabilities": serde_json::from_str::<serde_json::Value>(provider_capabilities).unwrap(),
        })
        .to_string();

        sqlx::query(
            "INSERT INTO realm_config (realm_id, config_type, config_key, config_value, enabled, created_at, updated_at)
             VALUES ($1, 'invoice_policy', 'policy', $2, true, NOW(), NOW())
             ON CONFLICT (realm_id, config_type, config_key) DO UPDATE SET config_value = $2, enabled = true, updated_at = NOW()",
        )
        .bind(realm_id)
        .bind(&config_value)
        .execute(&ctx.app_state.pool)
        .await
        .unwrap();
    }

    /// Set up seller config for a realm via the admin API.
    async fn setup_seller_config(app: &axum::Router, admin_token: &str, realm_id: &str) {
        let put_payload = json!({
            "sellerName": "Apply Eligibility Seller",
            "sellerAddress": "1 Seller Way",
            "sellerEmail": "seller@apply-elig.com",
            "sellerPhone": "+1-555-0400",
            "sellerTaxId": "TAX-AE-001",
        });

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/bill/{}/invoice-seller-config", realm_id))
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {}", admin_token))
                    .body(Body::from(put_payload.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "Seller config setup failed"
        );
    }

    /// Create a payment_attempts row for a specific user with the given provider.
    async fn create_payment_attempt_with_provider(
        ctx: &ApplyEligibilityTestContext,
        realm_id: &str,
        user_id: Uuid,
        provider: &str,
    ) -> Uuid {
        let pa_id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO payment_attempts (id, realm_id, user_id, payment_provider, target_type, target_id, amount, currency, status, expires_at)
             VALUES ($1, $2, $3, $4, 'entitlement_mapping', $5, 5000, 'USD', 'Succeeded', NOW() + interval '1 hour')"
        )
        .bind(pa_id)
        .bind(realm_id)
        .bind(user_id)
        .bind(provider)
        .bind(Uuid::now_v7())
        .execute(&ctx.app_state.pool)
        .await
        .unwrap();
        pa_id
    }

    /// Create a subscription row for a specific user with the given provider.
    async fn create_subscription_with_provider(
        ctx: &ApplyEligibilityTestContext,
        realm_id: &str,
        user_id: Uuid,
        provider: &str,
    ) -> Uuid {
        let sub_id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO subscription (id, realm_id, external_subscription_id, external_product_id, payment_provider, status, entitlement_key, user_id, created_at, updated_at, billing_type)
             VALUES ($1, $2, $3, $4, $5, 'active', 'pro', $6, NOW(), NOW(), 'recurring')"
        )
        .bind(sub_id)
        .bind(realm_id)
        .bind(format!("sub_ext_{}", sub_id))
        .bind(format!("prod_ext_{}", sub_id))
        .bind(provider)
        .bind(user_id)
        .execute(&ctx.app_state.pool)
        .await
        .unwrap();
        sub_id
    }

    /// Insert an external_sync invoice tied to a payment_attempt_id.
    async fn seed_external_sync_invoice_for_payment_attempt(
        ctx: &ApplyEligibilityTestContext,
        realm_id: &str,
        payment_attempt_id: Uuid,
        provider: &str,
    ) {
        let year = chrono::Utc::now().year();
        let seq: i64 = sqlx::query_scalar(
            "INSERT INTO invoice_number_counter (realm_id, year, next_seq, updated_at)
             VALUES ($1, $2, 2, NOW())
             ON CONFLICT (realm_id, year) DO UPDATE SET next_seq = invoice_number_counter.next_seq + 1, updated_at = NOW()
             RETURNING next_seq - 1",
        )
        .bind(realm_id)
        .bind(year)
        .fetch_one(&ctx.app_state.pool)
        .await
        .unwrap();
        let invoice_number = format!("INV-{}-{:04}", year, seq);

        sqlx::query(
            "INSERT INTO invoice (
                id, realm_id, invoice_number, source, provider, payment_provider,
                payment_attempt_id, status, currency,
                subtotal, discount_amount, tax_amount, shipping_amount, total,
                created_at, updated_at
            ) VALUES (
                $1, $2, $3, 'external_sync', $4, $4,
                $5, 'issued', 'USD',
                5000, 0, 0, 0, 5000,
                NOW(), NOW()
            )",
        )
        .bind(Uuid::now_v7())
        .bind(realm_id)
        .bind(&invoice_number)
        .bind(provider)
        .bind(payment_attempt_id)
        .execute(&ctx.app_state.pool)
        .await
        .unwrap();
    }

    /// GET /api/bill/{realmId}/my/invoices/apply-eligibility?referenceType=...&referenceId=...
    async fn fetch_apply_eligibility(
        app: &axum::Router,
        token: &str,
        _realm_id: &str,
        reference_type: &str,
        reference_id: Uuid,
    ) -> axum::response::Response {
        app.clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!(
                        "/api/user/bill/invoices/apply-eligibility?referenceType={}&referenceId={}",
                        reference_type, reference_id
                    ))
                    .header("authorization", format!("Bearer {}", token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    // =========================================================================
    // Test: policy=none => disabled
    // =========================================================================
    // User Story: docs/user-stories/billing/invoice-fallback.md
    // Covers: policy=none surfaces as disabled route.
    //
    // Given: invoice_policy.policy = "none" (and seller config present)
    // When:  GET apply-eligibility for the user's payment_attempt
    // Then:  route == "disabled", canApply == false, reason mentions policy

    #[test_context(ApplyEligibilityTestContext)]
    #[tokio::test]
    async fn test_apply_eligibility_policy_none_is_disabled(ctx: &mut ApplyEligibilityTestContext) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();

        let admin_token =
            setup_billing_admin_session(ctx, "apply-elig-policy-none-admin@test.com").await;
        setup_seller_config(&app, &admin_token, &realm_id).await;

        let (user_token, user_id) =
            create_regular_user_session(ctx, "apply-elig-policy-none@test.com").await;

        // Even a stripe payment_attempt + policy=none must disable apply.
        let pa_id = create_payment_attempt_with_provider(ctx, &realm_id, user_id, "stripe").await;

        set_invoice_policy(
            ctx,
            &realm_id,
            "none",
            r#"{"stripe":{"external_invoice_enabled":false}}"#,
        )
        .await;

        let response =
            fetch_apply_eligibility(&app, &user_token, &realm_id, "payment_attempt", pa_id).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = parse_body(response.into_body()).await;

        assert_eq!(body["route"], "disabled");
        assert_eq!(body["canApply"], false);
        assert_eq!(body["referenceType"], "payment_attempt");
        assert_eq!(body["referenceId"], pa_id.to_string());
        let reason = body["reason"].as_str().unwrap_or("");
        assert!(reason.contains("disabled by policy"), "got: {}", reason);
    }

    // =========================================================================
    // Test: creem provider => disabled (regardless of policy/seller)
    // =========================================================================
    // Covers: Creem is MoR; mirrors `validate_not_mor_provider`.
    //
    // Given: a payment_attempt with payment_provider='creem'
    // When:  GET apply-eligibility
    // Then:  route == "disabled", provider == "creem", reason mentions MoR

    #[test_context(ApplyEligibilityTestContext)]
    #[tokio::test]
    async fn test_apply_eligibility_creem_is_disabled(ctx: &mut ApplyEligibilityTestContext) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();

        let admin_token = setup_billing_admin_session(ctx, "apply-elig-creem-admin@test.com").await;
        setup_seller_config(&app, &admin_token, &realm_id).await;

        let (user_token, user_id) =
            create_regular_user_session(ctx, "apply-elig-creem@test.com").await;

        let pa_id = create_payment_attempt_with_provider(ctx, &realm_id, user_id, "creem").await;

        let response =
            fetch_apply_eligibility(&app, &user_token, &realm_id, "payment_attempt", pa_id).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = parse_body(response.into_body()).await;

        assert_eq!(body["route"], "disabled");
        assert_eq!(body["canApply"], false);
        assert_eq!(body["provider"], "creem");
        let reason = body["reason"].as_str().unwrap_or("");
        assert!(reason.contains("Merchant of Record"), "got: {}", reason);
    }

    // =========================================================================
    // Test: no seller config => disabled
    // =========================================================================
    // Covers: missing seller config surfaces as disabled route.
    //
    // Given: no invoice_seller_config for the realm (and stripe payment_attempt)
    // When:  GET apply-eligibility
    // Then:  route == "disabled", reason mentions seller configuration

    #[test_context(ApplyEligibilityTestContext)]
    #[tokio::test]
    async fn test_apply_eligibility_no_seller_config_is_disabled(
        ctx: &mut ApplyEligibilityTestContext,
    ) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();

        // No admin setup -> no seller config. policy remains default provider_first.
        let (user_token, user_id) =
            create_regular_user_session(ctx, "apply-elig-no-seller@test.com").await;

        let pa_id = create_payment_attempt_with_provider(ctx, &realm_id, user_id, "stripe").await;

        let response =
            fetch_apply_eligibility(&app, &user_token, &realm_id, "payment_attempt", pa_id).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = parse_body(response.into_body()).await;

        assert_eq!(body["route"], "disabled");
        assert_eq!(body["canApply"], false);
        let reason = body["reason"].as_str().unwrap_or("");
        assert!(
            reason.contains("seller"),
            "expected reason to mention seller, got: {}",
            reason
        );
    }

    // =========================================================================
    // Test: manual_only + non-Creem provider + seller + no external => manual_fallback
    // =========================================================================
    // Covers: manual_only policy still allows a manual Herald
    // invoice on a non-Creem provider transaction when seller config is present.
    // (Both `payment_attempts.payment_provider` and `subscription.payment_provider`
    // are NOT NULL in the schema, so the real "no provider" case is unreachable;
    // the realistic equivalent is a non-Creem provider like stripe.)
    //
    // Given: invoice_policy.policy = "manual_only", seller config present,
    //        a subscription with payment_provider='stripe', no external invoice
    // When:  GET apply-eligibility for the subscription
    // Then:  route == "manual_fallback", canApply == true, reason null

    #[test_context(ApplyEligibilityTestContext)]
    #[tokio::test]
    async fn test_apply_eligibility_manual_only_non_creem_is_manual_fallback(
        ctx: &mut ApplyEligibilityTestContext,
    ) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();

        let admin_token =
            setup_billing_admin_session(ctx, "apply-elig-manual-admin@test.com").await;
        setup_seller_config(&app, &admin_token, &realm_id).await;
        set_invoice_policy(
            ctx,
            &realm_id,
            "manual_only",
            r#"{"stripe":{"external_invoice_enabled":false}}"#,
        )
        .await;

        let (user_token, user_id) =
            create_regular_user_session(ctx, "apply-elig-manual@test.com").await;

        let sub_id = create_subscription_with_provider(ctx, &realm_id, user_id, "stripe").await;

        let response =
            fetch_apply_eligibility(&app, &user_token, &realm_id, "subscription", sub_id).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = parse_body(response.into_body()).await;

        assert_eq!(body["route"], "manual_fallback");
        assert_eq!(body["canApply"], true);
        assert!(body["reason"].is_null(), "expected null reason");
        assert_eq!(body["provider"], "stripe");
    }

    // =========================================================================
    // Test: provider_first + stripe + external_sync invoice => external_provider
    // =========================================================================
    // Covers: an externally-synced invoice already exists for
    // this resource; the route is read-only external_provider. Stripe always
    // routes here regardless of webhook state (see test above).
    //
    // Given: stripe payment_attempt WITH an external_sync invoice tied to it,
    //        seller config present, default policy
    // When:  GET apply-eligibility
    // Then:  route == "external_provider", canApply == false

    #[test_context(ApplyEligibilityTestContext)]
    #[tokio::test]
    async fn test_apply_eligibility_stripe_with_external_is_external_provider(
        ctx: &mut ApplyEligibilityTestContext,
    ) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();

        let admin_token = setup_billing_admin_session(ctx, "apply-elig-ext-admin@test.com").await;
        setup_seller_config(&app, &admin_token, &realm_id).await;

        let (user_token, user_id) =
            create_regular_user_session(ctx, "apply-elig-ext@test.com").await;

        let pa_id = create_payment_attempt_with_provider(ctx, &realm_id, user_id, "stripe").await;
        // Seed an externally-synced invoice for this payment_attempt.
        seed_external_sync_invoice_for_payment_attempt(ctx, &realm_id, pa_id, "stripe").await;

        let response =
            fetch_apply_eligibility(&app, &user_token, &realm_id, "payment_attempt", pa_id).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = parse_body(response.into_body()).await;

        assert_eq!(body["route"], "external_provider");
        assert_eq!(body["canApply"], false);
        // reason is null at the eligibility layer; the frontend renders the
        // generic "Managed by Stripe — see My Invoices." text from the route.
        assert!(body["reason"].is_null());
    }

    // =========================================================================
    // Test: resource owned by another user => 403
    // =========================================================================
    // Covers: ownership boundary — a user may only check their own resources.
    //
    // Given: a payment_attempt owned by user A, caller is user B (same realm)
    // When:  user B GETs apply-eligibility for that payment_attempt
    // Then:  403 Forbidden

    #[test_context(ApplyEligibilityTestContext)]
    #[tokio::test]
    async fn test_apply_eligibility_other_users_resource_is_forbidden(
        ctx: &mut ApplyEligibilityTestContext,
    ) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();

        let admin_token = setup_billing_admin_session(ctx, "apply-elig-403-admin@test.com").await;
        setup_seller_config(&app, &admin_token, &realm_id).await;

        let (_user_a_token, user_a_id) =
            create_regular_user_session(ctx, "apply-elig-owner-a@test.com").await;
        let (user_b_token, _user_b_id) =
            create_regular_user_session(ctx, "apply-elig-caller-b@test.com").await;

        let pa_id = create_payment_attempt_with_provider(ctx, &realm_id, user_a_id, "stripe").await;

        let response =
            fetch_apply_eligibility(&app, &user_b_token, &realm_id, "payment_attempt", pa_id).await;
        assert_eq!(
            response.status(),
            StatusCode::FORBIDDEN,
            "Expected 403 when querying another user's payment_attempt"
        );
    }

    // =========================================================================
    // Test: subscription owned by a soft-deleted user => 403
    // =========================================================================
    // Covers: subscription.user_id is NOT NULL. Deleted users are soft-deleted
    // (account.status = Invalid), so subscriptions keep their owner FK.
    //
    // Given: a subscription row owned by a soft-deleted user
    // When:  a regular user GETs apply-eligibility for that subscription
    // Then:  403 Forbidden

    #[test_context(ApplyEligibilityTestContext)]
    #[tokio::test]
    async fn test_apply_eligibility_soft_deleted_owner_subscription_is_forbidden(
        ctx: &mut ApplyEligibilityTestContext,
    ) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();

        let (user_token, _user_id) =
            create_regular_user_session(ctx, "apply-elig-null-owner@test.com").await;
        let deleted_owner_id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO account (id, realm_id, email, password, status)
             VALUES ($1, $2, $3, '$2a$12$dummy_password_hash', 3)",
        )
        .bind(deleted_owner_id)
        .bind(&realm_id)
        .bind(format!("soft-deleted-owner-{}@test.com", deleted_owner_id))
        .execute(&ctx.app_state.pool)
        .await
        .unwrap();

        let sub_id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO subscription (id, realm_id, external_subscription_id, external_product_id, payment_provider, status, entitlement_key, user_id, created_at, updated_at, billing_type)
             VALUES ($1, $2, $3, $4, 'stripe', 'active', 'pro', $5, NOW(), NOW(), 'recurring')",
        )
        .bind(sub_id)
        .bind(&realm_id)
        .bind(format!("sub_ext_{}", sub_id))
        .bind(format!("prod_ext_{}", sub_id))
        .bind(deleted_owner_id)
        .execute(&ctx.app_state.pool)
        .await
        .unwrap();

        let response =
            fetch_apply_eligibility(&app, &user_token, &realm_id, "subscription", sub_id).await;
        assert_eq!(
            response.status(),
            StatusCode::FORBIDDEN,
            "Expected 403 for subscription owned by another soft-deleted user"
        );
    }

    // =========================================================================
    // Test: nonexistent resource => 404
    // =========================================================================
    // Covers: ownership boundary — a missing resource returns 404 (NOT 400),
    // distinguishing "not in realm" from "owned by someone else".
    //
    // Given: a random UUID that does not correspond to any payment_attempt
    // When:  GET apply-eligibility for that id
    // Then:  404 Not Found

    #[test_context(ApplyEligibilityTestContext)]
    #[tokio::test]
    async fn test_apply_eligibility_nonexistent_resource_is_not_found(
        ctx: &mut ApplyEligibilityTestContext,
    ) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();

        let admin_token = setup_billing_admin_session(ctx, "apply-elig-404-admin@test.com").await;
        setup_seller_config(&app, &admin_token, &realm_id).await;

        let (user_token, _user_id) =
            create_regular_user_session(ctx, "apply-elig-404@test.com").await;

        let bogus_pa_id = Uuid::now_v7();
        let response =
            fetch_apply_eligibility(&app, &user_token, &realm_id, "payment_attempt", bogus_pa_id)
                .await;
        assert_eq!(
            response.status(),
            StatusCode::NOT_FOUND,
            "Expected 404 for nonexistent resource"
        );
    }
}
