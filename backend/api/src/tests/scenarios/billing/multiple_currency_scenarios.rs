// =============================================================================
// Multiple-Currency Resolution Scenario Tests
// =============================================================================
//
// Backend contract for the multiple-currency feature:
// 1. api-ext exposes per-entitlement currency sets and by-currency default
//    price resolution (hit / 404 no-match / 409 ambiguous), fail-loud without
//    secondary-currency fallback; stored provider codes are lowercase Stripe
//    codes and resolution matches them ASCII-case-insensitively.
// 2. Purchase creation against a mapping row whose product info has no
//    price/currency fails loud (422) instead of falling back to a fabricated
//    amount/currency.
//
// User Story: docs/user-stories/billing/payment-attempt.md (US-PA-001,
//             purchase attempt creation baseline),
//             docs/user-stories/billing/entitlement-mapping.md (US-EM-008,
//             multi-price purchase resolution baseline)
// Covers: currency resolution behavior extending those baselines
//
// =============================================================================

#[cfg(test)]
mod tests {
    use crate::tests::helpers::billing_helpers::{
        setup_billing_admin_session, setup_test_entitlement_mapping_full,
    };
    use crate::tests::helpers::client_helpers::create_test_api_key;
    use crate::tests::schema_test_context::SchemaTestContext as TestContext;
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode, header},
    };
    use serde_json::json;
    use test_context::test_context;
    use tower::ServiceExt;

    // =========================================================================
    // Helpers
    // =========================================================================

    /// Create an API key for the context realm with billing.view permission.
    async fn create_api_key_with_billing_view(ctx: &TestContext) -> String {
        let (api_key_plaintext, api_key_entity) =
            create_test_api_key(ctx, "multiple-currency-test-key", true, None).await;

        herald_test_support::helpers::grant_api_key_permissions(
            &ctx._app_state.pool,
            &ctx._realm_id,
            &ctx._client_id,
            &api_key_entity.id,
            &[("billing", "view")],
        )
        .await;

        api_key_plaintext
    }

    async fn send_json(
        app: &axum::Router,
        method: &str,
        uri: String,
        bearer: Option<&str>,
        api_key: Option<&str>,
        body: Option<serde_json::Value>,
    ) -> (StatusCode, serde_json::Value) {
        let mut builder = Request::builder().method(method).uri(uri);
        if let Some(token) = bearer {
            builder = builder.header(header::AUTHORIZATION, format!("Bearer {}", token));
        }
        if let Some(key) = api_key {
            builder = builder.header("X-API-Key", key);
        }
        let request = match body {
            Some(payload) => builder
                .header("Content-Type", "application/json")
                .body(Body::from(payload.to_string()))
                .unwrap(),
            None => builder.body(Body::empty()).unwrap(),
        };

        let response = app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body_json: serde_json::Value =
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
        (status, body_json)
    }

    // =========================================================================
    // api-ext: entitlement currency set + by-currency default price
    // =========================================================================

    /// Currency aggregation reflects enabled Stripe rows across billing types
    /// (deduped, uppercase-normalized) and excludes Creem rows; resolution
    /// returns the unique row, 404 on no match (no fallback to another
    /// currency), 409 on same-currency multiple periods until narrowed.
    #[test_context(TestContext)]
    #[tokio::test]
    async fn test_ext_currency_set_and_default_price_resolution(ctx: &mut TestContext) {
        let realm_id = ctx._realm_id.clone();
        let api_key = create_api_key_with_billing_view(ctx).await;
        let app = ctx.create_unified_test_router();

        let key = "mc-multi";
        // Stripe stores lowercase codes; resolution must bridge the case gap.
        let usd_month = setup_test_entitlement_mapping_full(
            ctx,
            &realm_id,
            "stripe",
            "prod_mc_1",
            Some("price_mc_month"),
            key,
            Some("recurring"),
            Some("monthly"),
            Some(100),
            None,
            None,
            false,
            None,
            true,
            Some(json!({"name": "MC Pro Monthly", "price": 1000, "currency": "usd"})),
        )
        .await;
        let _usd_year = setup_test_entitlement_mapping_full(
            ctx,
            &realm_id,
            "stripe",
            "prod_mc_2",
            Some("price_mc_year"),
            key,
            Some("recurring"),
            Some("yearly"),
            Some(1000),
            None,
            None,
            false,
            None,
            true,
            Some(json!({"name": "MC Pro Yearly", "price": 10000, "currency": "usd"})),
        )
        .await;
        let _eur_once = setup_test_entitlement_mapping_full(
            ctx,
            &realm_id,
            "stripe",
            "prod_mc_3",
            Some("price_mc_once"),
            key,
            Some("one_time"),
            None,
            Some(50),
            None,
            None,
            false,
            None,
            true,
            Some(json!({"name": "MC Credits EUR", "price": 500, "currency": "eur"})),
        )
        .await;
        // Creem rows carry pricing provider-side and must not join the set.
        let _creem = setup_test_entitlement_mapping_full(
            ctx,
            &realm_id,
            "creem",
            "prod_mc_4",
            None,
            key,
            None,
            None,
            None,
            None,
            None,
            false,
            None,
            true,
            Some(json!({"name": "MC Creem", "price": 700, "currency": "jpy"})),
        )
        .await;
        // Disabled rows are invisible.
        let _disabled = setup_test_entitlement_mapping_full(
            ctx,
            &realm_id,
            "stripe",
            "prod_mc_5",
            Some("price_mc_dis"),
            key,
            Some("one_time"),
            None,
            Some(10),
            None,
            None,
            false,
            None,
            false,
            Some(json!({"name": "MC Disabled", "price": 300, "currency": "gbp"})),
        )
        .await;

        // Currency set: USD + EUR only (creem/disabled excluded, deduped,
        // uppercase).
        let (status, body) = send_json(
            &app,
            "GET",
            format!("/api/ext/{}/entitlements/{}/currencies", realm_id, key),
            None,
            Some(&api_key),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "got {status}: {body}");
        let currencies = body["currencies"].as_array().expect("currencies array");
        assert_eq!(currencies.len(), 2, "got {currencies:?}");
        let codes: Vec<&str> = currencies.iter().map(|c| c.as_str().unwrap()).collect();
        assert!(
            codes.contains(&"USD") && codes.contains(&"EUR"),
            "got {codes:?}"
        );

        // Same currency with two billing periods → ambiguous (409)
        let (status, body) = send_json(
            &app,
            "GET",
            format!(
                "/api/ext/{}/entitlements/{}/default-price?currency=USD",
                realm_id, key
            ),
            None,
            Some(&api_key),
            None,
        )
        .await;
        assert_eq!(
            status,
            StatusCode::CONFLICT,
            "same-currency multi-period must be 409, got {status}: {body}"
        );

        // Narrowed by billing period → unique row
        let (status, body) = send_json(
            &app,
            "GET",
            format!(
                "/api/ext/{}/entitlements/{}/default-price?currency=USD&billingPeriod=monthly",
                realm_id, key
            ),
            None,
            Some(&api_key),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "got {status}: {body}");
        assert_eq!(body["mappingId"], usd_month.to_string());
        assert_eq!(body["amount"], 1000);
        assert_eq!(body["currency"], "USD");
        assert_eq!(body["billingType"], "recurring");
        assert_eq!(body["externalPriceId"], "price_mc_month");

        // Single EUR row resolves without extra filters
        let (status, body) = send_json(
            &app,
            "GET",
            format!(
                "/api/ext/{}/entitlements/{}/default-price?currency=EUR",
                realm_id, key
            ),
            None,
            Some(&api_key),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "got {status}: {body}");
        assert_eq!(body["amount"], 500);

        // No match → 404, never a fallback to another currency
        let (status, _body) = send_json(
            &app,
            "GET",
            format!(
                "/api/ext/{}/entitlements/{}/default-price?currency=GBP",
                realm_id, key
            ),
            None,
            Some(&api_key),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        // Malformed currency param → 400
        let (status, _body) = send_json(
            &app,
            "GET",
            format!(
                "/api/ext/{}/entitlements/{}/default-price?currency=usd",
                realm_id, key
            ),
            None,
            Some(&api_key),
            None,
        )
        .await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "lowercase input must be rejected"
        );
    }

    /// The currency endpoints stay realm-scoped: an API key from another
    /// realm gets 403, not the other realm's data.
    #[test_context(TestContext)]
    #[tokio::test]
    async fn test_ext_currency_endpoints_cross_realm_forbidden(ctx: &mut TestContext) {
        let api_key = create_api_key_with_billing_view(ctx).await;
        let app = ctx.create_unified_test_router();

        for uri in [
            "/api/ext/other-realm/entitlements/pro/currencies".to_string(),
            "/api/ext/other-realm/entitlements/pro/default-price?currency=USD".to_string(),
        ] {
            let (status, _body) = send_json(&app, "GET", uri, None, Some(&api_key), None).await;
            assert_eq!(status, StatusCode::FORBIDDEN);
        }
    }

    // =========================================================================
    // resolve_target fail-loud on missing price info
    // =========================================================================

    /// User Story: US-PA-001
    /// Covers: purchase creation must refuse mappings without price/currency
    ///
    /// A mapping row without readable price/currency must NOT produce a
    /// fabricated "usd"/amount-0 attempt (previously the silent fallback);
    /// it fails 422 and no payment_attempts row is written.
    #[test_context(TestContext)]
    #[tokio::test]
    async fn test_create_payment_attempt_without_price_info_fails_loud(ctx: &mut TestContext) {
        let realm_id = ctx._realm_id.clone();
        let token = setup_billing_admin_session(ctx, "mc-no-price@test.com").await;

        let mapping_id = setup_test_entitlement_mapping_full(
            ctx,
            &realm_id,
            "stripe",
            "prod_mc_noprice",
            Some("price_mc_noprice"),
            "mc-noprice",
            Some("one_time"),
            None,
            Some(100),
            None,
            None,
            false,
            None,
            true,
            Some(json!({"name": "No Price Package"})),
        )
        .await;

        let app = ctx.create_unified_test_router();
        let (status, body) = send_json(
            &app,
            "POST",
            "/api/bill/purchase/payment-attempts".to_string(),
            Some(&token),
            None,
            Some(json!({
                "targetType": "entitlement_mapping",
                "targetId": mapping_id.to_string(),
                "paymentProvider": "stripe"
            })),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "missing price info must fail 422, got {status}: {body}"
        );
        assert!(
            body["message"]
                .as_str()
                .is_some_and(|m| m.contains("Price info missing")),
            "error body should name the missing price info, got {body}"
        );

        let attempts: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM payment_attempts WHERE target_id = $1")
                .bind(mapping_id)
                .fetch_one(&ctx._app_state.pool)
                .await
                .unwrap();
        assert_eq!(
            attempts, 0,
            "no attempt row may be created from a price-less mapping"
        );
    }
}
