// =============================================================================
// Feature-Availability Invoice Eligibility Scenario Tests
// =============================================================================
//
// Phase A of P0-2 (.ai/future/invoice_ux.md):
// Extends `feature-availability` with a realm-level invoice eligibility block
// so the frontend can gate Create/Apply invoice buttons BEFORE submit
// (policy=none, missing seller config) instead of relying on post-submit
// backend rejection.
//
// Principle (from the design doc): regular users consume a backend-provided
// eligibility *result*; they do NOT read admin config/policy APIs directly.
//
// Covers:
//   - Default unconfigured realm: provider_first, no seller config, can-create
//     and can-apply true (policy != none), reason mentions seller config.
//   - Realm with policy=none: can-create and can-apply false, reason mentions
//     Herald invoices / disabled.
//   - Realm with a seller config saved: hasSellerConfig true, reason None.
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
    use serde_json::json;
    use test_context::test_context;
    use tower::ServiceExt;

    use SchemaTestContext as FeatureEligibilityTestContext;

    // Helper: parse response body to JSON
    async fn parse_body(body: axum::body::Body) -> serde_json::Value {
        let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    /// GET /api/bill/feature-availability as the given token.
    async fn fetch_feature_availability(
        app: &axum::Router,
        token: &str,
        _realm_id: &str,
    ) -> serde_json::Value {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/bill/feature-availability".to_string())
                    .header("authorization", format!("Bearer {}", token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "feature-availability should return 200"
        );
        parse_body(response.into_body()).await
    }

    /// Set the invoice policy for a realm by inserting/updating realm_config.
    /// Mirrors the helper in invoice_provider_policy_scenarios.rs so we wire
    /// policy=none exactly the same way existing invoice policy tests do.
    async fn set_invoice_policy(
        ctx: &FeatureEligibilityTestContext,
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

    // =========================================================================
    // Test: Default unconfigured realm reports provider_first + missing seller
    // =========================================================================
    // User Story: docs/user-stories/billing/invoice-fallback.md
    // Covers: P0-2 Phase A -- default eligibility for an unconfigured realm.
    //
    // Given: A realm with no invoice_policy row and no seller config
    // When: GET /feature-availability
    // Then: invoiceEligibility.policy == "provider_first"
    //  And: invoiceEligibility.hasSellerConfig == false
    //  And: invoiceEligibility.canCreateManualInvoice == true
    //  And: invoiceEligibility.canApplyInvoice == true
    //  And: invoiceEligibility.reason mentions seller info not configured

    #[test_context(FeatureEligibilityTestContext)]
    #[tokio::test]
    async fn test_feature_availability_default_unconfigured_realm(
        ctx: &mut FeatureEligibilityTestContext,
    ) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();
        let admin_token = setup_billing_admin_session(ctx, "feature-elig-default@test.com").await;

        let body = fetch_feature_availability(&app, &admin_token, &realm_id).await;
        let eligibility = &body["invoiceEligibility"];

        assert_eq!(
            eligibility["policy"], "provider_first",
            "Default policy should be provider_first when unconfigured"
        );
        assert_eq!(
            eligibility["hasSellerConfig"], false,
            "hasSellerConfig should be false when no seller config saved"
        );
        assert_eq!(
            eligibility["canCreateManualInvoice"], true,
            "Default policy allows manual invoice creation"
        );
        assert_eq!(
            eligibility["canApplyInvoice"], true,
            "Default policy allows applying for an invoice at realm level"
        );
        let reason = eligibility["reason"].as_str().unwrap_or("");
        assert!(
            reason.contains("Seller information"),
            "Reason should mention seller information, got: {}",
            reason
        );
    }

    // =========================================================================
    // Test: policy=none disables creation and apply
    // =========================================================================
    // User Story: docs/user-stories/billing/invoice-fallback.md
    // Covers: P0-2 Phase A -- policy=none is surfaced as disabled eligibility.
    //
    // Given: invoice_policy.policy = "none"
    // When: GET /feature-availability
    // Then: invoiceEligibility.canCreateManualInvoice == false
    //  And: invoiceEligibility.canApplyInvoice == false
    //  And: invoiceEligibility.reason mentions Herald invoices

    #[test_context(FeatureEligibilityTestContext)]
    #[tokio::test]
    async fn test_feature_availability_policy_none_disables_invoices(
        ctx: &mut FeatureEligibilityTestContext,
    ) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();
        let admin_token = setup_billing_admin_session(ctx, "feature-elig-none@test.com").await;

        set_invoice_policy(
            ctx,
            &realm_id,
            "none",
            r#"{"stripe":{"external_invoice_enabled":false}}"#,
        )
        .await;

        let body = fetch_feature_availability(&app, &admin_token, &realm_id).await;
        let eligibility = &body["invoiceEligibility"];

        assert_eq!(eligibility["policy"], "none");
        assert_eq!(
            eligibility["canCreateManualInvoice"], false,
            "policy=none must disable manual invoice creation"
        );
        assert_eq!(
            eligibility["canApplyInvoice"], false,
            "policy=none must disable applying for an invoice"
        );
        let reason = eligibility["reason"].as_str().unwrap_or("");
        assert!(
            reason.contains("Herald invoices"),
            "Reason should mention Herald invoices, got: {}",
            reason
        );
    }

    // =========================================================================
    // Test: realm with seller config saved clears the seller-config reason
    // =========================================================================
    // User Story: docs/user-stories/billing/invoice-fallback.md
    // Covers: P0-2 Phase A -- seller config present means reason is null.
    //
    // Given: A seller config saved via PUT /invoice-seller-config
    //   And: policy left as default (provider_first)
    // When: GET /feature-availability
    // Then: invoiceEligibility.hasSellerConfig == true
    //  And: invoiceEligibility.reason is null

    #[test_context(FeatureEligibilityTestContext)]
    #[tokio::test]
    async fn test_feature_availability_with_seller_config_no_reason(
        ctx: &mut FeatureEligibilityTestContext,
    ) {
        let app = ctx.create_unified_test_router();
        let realm_id = ctx._realm_id.clone();
        let admin_token = setup_billing_admin_session(ctx, "feature-elig-seller@test.com").await;

        // Save a seller config via the admin endpoint (same path users will configure).
        let put_payload = json!({
            "sellerName": "Acme Corp",
            "sellerAddress": "123 Main St, Springfield",
            "sellerEmail": "billing@acme.com",
            "sellerPhone": "+1-555-0100",
            "sellerTaxId": "TAX-ACME-001",
        });
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/bill/invoice-seller-config".to_string())
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {}", admin_token))
                    .body(Body::from(put_payload.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = fetch_feature_availability(&app, &admin_token, &realm_id).await;
        let eligibility = &body["invoiceEligibility"];

        assert_eq!(
            eligibility["hasSellerConfig"], true,
            "hasSellerConfig should be true after saving seller config"
        );
        assert_eq!(
            eligibility["policy"], "provider_first",
            "Policy remains provider_first when not explicitly configured"
        );
        assert_eq!(eligibility["canCreateManualInvoice"], true);
        assert_eq!(eligibility["canApplyInvoice"], true);
        assert!(
            eligibility["reason"].is_null(),
            "Reason should be null when seller config is present and policy allows creation, got: {:?}",
            eligibility["reason"]
        );
    }
}
