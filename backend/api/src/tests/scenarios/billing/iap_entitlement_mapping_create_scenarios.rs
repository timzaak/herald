// =============================================================================
// IAP Entitlement Mapping Create Scenario Tests
// =============================================================================
//
// Exercises `POST /api/bill/entitlement-mappings`
// (`api-billing/src/entitlement_mapping_handlers.rs::create_entitlement_mapping`),
// the generic mapping-create endpoint introduced for IAP (design A2). The
// endpoint is provider-agnostic, so these tests use `apple` / `google` as the
// IAP-motivating case but the contract covers Stripe / Creem too.
//
// User Story: US-IAP-002 (build IAP product → entitlement mapping)
// Covers: design support-iap §4.2.2 (mapping-create contract),
//         §4.3.3 (CHECK constraint extension lets apple/google rows insert),
//         §6.1 (backend integration), permission overlay (`billing.manage` +
//         credit fields requiring `points.manage`).
//
// =============================================================================

#[cfg(test)]
mod tests {
    use crate::tests::helpers::auth_helpers::create_admin_session_with_user;
    use crate::tests::helpers::billing_helpers::setup_billing_admin_session;
    use crate::tests::helpers::points_helpers::ensure_test_bucket_for_realm;
    use crate::tests::schema_test_context::SchemaTestContext;
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode, header},
    };
    use serde_json::{Value, json};
    use test_context::test_context;
    use tower::ServiceExt;
    use uuid::Uuid;

    use SchemaTestContext as MappingCreateContext;

    // =========================================================================
    // Shared helpers
    // =========================================================================

    fn auth_request(method: &str, uri: String, token: &str, body: Option<Body>) -> Request<Body> {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::AUTHORIZATION, format!("Bearer {token}"));
        if let Some(b) = body {
            builder = builder.header("Content-Type", "application/json");
            builder.body(b).unwrap()
        } else {
            builder.body(Body::empty()).unwrap()
        }
    }

    async fn body_json(response: axum::response::Response) -> Value {
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    }

    /// Build a base create-mapping request body for the IAP `apple` provider.
    fn apple_create_body(bucket_id: Uuid, external_product_id: &str) -> Value {
        json!({
            "paymentProvider": "apple",
            "externalProductId": external_product_id,
            "entitlementKey": "pro",
            "bucketId": bucket_id,
            "billingType": "recurring",
            "billingPeriod": "monthly",
            "enabled": true,
        })
    }

    /// Create a custom role granting ONLY `billing.manage` (no `points.manage`)
    /// and bind it to a fresh user. Returns the user's bearer token.
    ///
    /// Used to exercise the credit-fields-require-points.manage overlay: a
    /// billing-only admin must be allowed to create a non-credit mapping but
    /// rejected (403) when the body carries a non-empty `pointRules` array
    /// (the points config that triggers the points.manage gate; the legacy
    /// `pointsPerPeriod` / `grantOnSubscribe` / `validityDays` fields were
    /// removed by the distribution-rules refactor).
    async fn billing_only_session(ctx: &mut MappingCreateContext, email: &str) -> String {
        use herald_core::domain::authorization::principal_types;

        let (token, user_id_str) = create_admin_session_with_user(ctx, email, 1800).await;
        let user_id = Uuid::parse_str(&user_id_str).expect("user id");

        let role_id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO roles (id, name, realm_id, client_id, is_builtin)
             VALUES ($1, $2, $3, $4, false)",
        )
        .bind(role_id)
        .bind(format!("billing-only-{email}"))
        .bind(&ctx._realm_id)
        .bind(&ctx._client_id)
        .execute(&ctx.app_state.pool)
        .await
        .expect("create billing-only role");

        for (resource, action) in [("billing", "view"), ("billing", "manage")] {
            sqlx::query(
                "INSERT INTO role_policies (id, role_id, realm_id, resource, action)
                 VALUES ($1, $2, $3, $4, $5)",
            )
            .bind(Uuid::now_v7())
            .bind(role_id)
            .bind(&ctx._realm_id)
            .bind(resource)
            .bind(action)
            .execute(&ctx.app_state.pool)
            .await
            .expect("insert billing-only policy");
        }

        sqlx::query(
            "INSERT INTO user_roles
                (id, user_id, role_id, realm_id, client_id, principal_type, principal_id)
             VALUES ($1, $2, $3, $4, $5, $6, $2::text)
             ON CONFLICT DO NOTHING",
        )
        .bind(Uuid::now_v7())
        .bind(user_id)
        .bind(role_id)
        .bind(&ctx._realm_id)
        .bind(&ctx._client_id)
        .bind(principal_types::USER)
        .execute(&ctx.app_state.pool)
        .await
        .expect("bind billing-only role to user");

        token
    }

    // =========================================================================
    // Tests
    // =========================================================================

    /// User Story: US-IAP-002 (scenario 3 — create mapping 201)
    /// Covers: design §4.2.2 (POST → 201 Created + EntitlementMappingResponse),
    ///         §4.3.3 (apple/google rows insert after CHECK extension)
    ///
    /// A realm admin with `billing.manage` creates an Apple IAP mapping → the
    /// endpoint returns 201 and the persisted row's `payment_provider` is
    /// `apple` (which would have violated the pre-migration CHECK). Both
    /// `apple` and `google` are exercised.
    #[test_context(MappingCreateContext)]
    #[tokio::test]
    async fn test_iap_mapping_create_success_201(ctx: &mut MappingCreateContext) {
        let realm_id = ctx._realm_id.clone();
        let token = setup_billing_admin_session(ctx, "iap-map-create@test.com").await;
        let bucket_id = ensure_test_bucket_for_realm(&ctx.app_state.pool, &realm_id).await;

        for provider in ["apple", "google"] {
            let body = json!({
                "paymentProvider": provider,
                "externalProductId": format!("com.herald.test.{provider}.monthly"),
                "entitlementKey": format!("{provider}-pro"),
                "bucketId": bucket_id,
                "billingType": "recurring",
                "billingPeriod": "monthly",
                "enabled": true,
            });

            let app = ctx.create_unified_test_router();
            let response = app
                .oneshot(auth_request(
                    "POST",
                    "/api/bill/entitlement-mappings".to_string(),
                    &token,
                    Some(Body::from(body.to_string())),
                ))
                .await
                .unwrap();

            assert_eq!(
                response.status(),
                StatusCode::CREATED,
                "{provider} mapping create must return 201, body={}",
                body_json(response).await
            );

            let body = body_json(response).await;
            assert_eq!(body["paymentProvider"], provider);
            assert_eq!(body["billingType"], "recurring");
            assert!(body["id"].is_string(), "response must include mapping id");
        }
    }

    /// User Story: US-IAP-002 (scenario 3 — duplicate 409)
    /// Covers: design §4.2.2 (409 on uq_pem_realm_provider_product_price),
    ///         §4.3.3 (unique constraint still enforced for apple/google)
    ///
    /// Submitting the same (provider, product, price) twice must return 409
    /// on the second submission.
    #[test_context(MappingCreateContext)]
    #[tokio::test]
    async fn test_iap_mapping_create_duplicate_returns_409(ctx: &mut MappingCreateContext) {
        let realm_id = ctx._realm_id.clone();
        let token = setup_billing_admin_session(ctx, "iap-map-dup@test.com").await;
        let bucket_id = ensure_test_bucket_for_realm(&ctx.app_state.pool, &realm_id).await;

        let body = apple_create_body(bucket_id, "com.herald.test.dup.monthly");
        let uri = "/api/bill/entitlement-mappings".to_string();

        let app = ctx.create_unified_test_router();
        let r1 = app
            .clone()
            .oneshot(auth_request(
                "POST",
                uri.clone(),
                &token,
                Some(Body::from(body.to_string())),
            ))
            .await
            .unwrap();
        assert_eq!(r1.status(), StatusCode::CREATED);

        let r2 = app
            .oneshot(auth_request(
                "POST",
                uri,
                &token,
                Some(Body::from(body.to_string())),
            ))
            .await
            .unwrap();
        assert_eq!(
            r2.status(),
            StatusCode::CONFLICT,
            "duplicate (provider+product+price) must return 409"
        );
    }

    /// User Story: US-IAP-002 (permission guard — billing.manage required)
    /// Covers: design §4.2.2 (403 without billing.manage)
    ///
    /// A user with no `billing.manage` permission must be rejected with 403
    /// when attempting to create a mapping. We use a fresh first-party user
    /// with no role bindings at all (the default test user has no roles).
    #[test_context(MappingCreateContext)]
    #[tokio::test]
    async fn test_iap_mapping_create_without_billing_manage_returns_403(
        ctx: &mut MappingCreateContext,
    ) {
        let realm_id = ctx._realm_id.clone();
        // Fresh user with NO role bindings — no billing.manage.
        let (token, _user_id) =
            create_admin_session_with_user(ctx, "iap-map-nobilling@test.com", 1800).await;
        let bucket_id = ensure_test_bucket_for_realm(&ctx.app_state.pool, &realm_id).await;

        let body = apple_create_body(bucket_id, "com.herald.test.nobilling.monthly");
        let app = ctx.create_unified_test_router();
        let response = app
            .oneshot(auth_request(
                "POST",
                "/api/bill/entitlement-mappings".to_string(),
                &token,
                Some(Body::from(body.to_string())),
            ))
            .await
            .unwrap();

        assert_eq!(
            response.status(),
            StatusCode::FORBIDDEN,
            "create without billing.manage must be 403"
        );
    }

    /// User Story: US-IAP-002 (permission overlay — credit fields require
    /// points.manage)
    /// Covers: design §4.2.2 (credit fields → 403 without points.manage)
    ///
    /// A user with `billing.manage` but WITHOUT `points.manage` must be
    /// allowed to create a plain mapping (no `pointRules`) but rejected
    /// with 403 when the body carries a non-empty `pointRules` array (the
    /// points config that triggers the points.manage gate).
    #[test_context(MappingCreateContext)]
    #[tokio::test]
    async fn test_iap_mapping_create_credit_fields_without_points_manage_returns_403(
        ctx: &mut MappingCreateContext,
    ) {
        let realm_id = ctx._realm_id.clone();
        let token = billing_only_session(ctx, "iap-map-billingonly@test.com").await;
        let bucket_id = ensure_test_bucket_for_realm(&ctx.app_state.pool, &realm_id).await;
        let uri = "/api/bill/entitlement-mappings".to_string();

        // 1. Plain mapping (no credit fields) → 201 with billing.manage alone.
        let plain_body = apple_create_body(bucket_id, "com.herald.test.plain.monthly");
        let app = ctx.create_unified_test_router();
        let r_plain = app
            .clone()
            .oneshot(auth_request(
                "POST",
                uri.clone(),
                &token,
                Some(Body::from(plain_body.to_string())),
            ))
            .await
            .unwrap();
        assert_eq!(
            r_plain.status(),
            StatusCode::CREATED,
            "billing.manage alone must allow a non-credit mapping create"
        );

        // 2. Mapping WITH pointRules → 403 (credit config, requires
        //    points.manage which the billing-only user lacks). The legacy
        //    pointsPerPeriod/grantOnSubscribe/validityDays fields were removed;
        //    points config is now a non-empty `pointRules` array, which is the
        //    field that triggers the points.manage gate.
        let mut credit_body = apple_create_body(bucket_id, "com.herald.test.credit.monthly");
        credit_body["pointRules"] = json!([{
            "bucketId": bucket_id,
            "triggerSources": ["subscription_initial"],
            "grantMode": "fixed",
            "pointsAmount": 100
        }]);
        let r_credit = app
            .clone()
            .oneshot(auth_request(
                "POST",
                uri.clone(),
                &token,
                Some(Body::from(credit_body.to_string())),
            ))
            .await
            .unwrap();
        assert_eq!(
            r_credit.status(),
            StatusCode::FORBIDDEN,
            "pointRules without points.manage must be 403"
        );

        // 3. Mapping WITH pointRules (quota) → 403.
        let mut grant_body = apple_create_body(bucket_id, "com.herald.test.grant.monthly");
        grant_body["pointRules"] = json!([{
            "bucketId": bucket_id,
            "triggerSources": ["subscription_renewal"],
            "grantMode": "quota",
            "quotaWindows": [{"windowSeconds": 2592000, "limit": 1000, "key": "period"}]
        }]);
        let r_grant = app
            .clone()
            .oneshot(auth_request(
                "POST",
                uri.clone(),
                &token,
                Some(Body::from(grant_body.to_string())),
            ))
            .await
            .unwrap();
        assert_eq!(
            r_grant.status(),
            StatusCode::FORBIDDEN,
            "pointRules without points.manage must be 403"
        );

        // 4. one_time mapping WITH pointRules → 403.
        let validity_body = json!({
            "paymentProvider": "apple",
            "externalProductId": "com.herald.test.validity.onetime",
            "entitlementKey": "credits",
            "bucketId": bucket_id,
            "billingType": "one_time",
            "pointRules": [{
                "bucketId": bucket_id,
                "triggerSources": ["topup"],
                "grantMode": "fixed",
                "pointsAmount": 500,
                "validityDays": 30
            }],
            "enabled": true,
        });
        let r_validity = app
            .oneshot(auth_request(
                "POST",
                uri,
                &token,
                Some(Body::from(validity_body.to_string())),
            ))
            .await
            .unwrap();
        assert_eq!(
            r_validity.status(),
            StatusCode::FORBIDDEN,
            "pointRules without points.manage must be 403"
        );
    }
}
