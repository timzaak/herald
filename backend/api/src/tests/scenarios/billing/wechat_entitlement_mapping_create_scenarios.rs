// =============================================================================
// WeChat Entitlement Mapping Manual-Price Scenario Tests
// =============================================================================
//
// Exercises `POST/PATCH /api/bill/{realmId}/entitlement-mappings[...]` for the
// WeChat provider. WeChat Pay v3 has no hosted product catalog, so the PRD
// (`docs/prd/billing/wechat-support.md` §2.2 / §8.1) pins the product binding
// to a manual configuration: the admin sets the mapping's
// external_product_id / price by hand instead of syncing from the provider.
//
// The manual price lands in the same `provider_product_info` JSONB keys the
// Stripe/Creem sync writes (`price` minor units + `currency`), because every
// read path (purchase snapshot, purchase options, WeChat create-order amount)
// consumes those keys — a WeChat mapping without a positive price can never
// produce a valid order, so the write must fail loud instead of checkout.
//
// User Story: US-WP-001 (WeChat product binding via manual mapping config)
//
// =============================================================================

#[cfg(test)]
mod tests {
    use crate::tests::helpers::billing_helpers::{
        setup_billing_admin_session, setup_test_entitlement_mapping,
    };
    use crate::tests::schema_test_context::SchemaTestContext;
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode, header},
    };
    use serde_json::{Value, json};
    use test_context::test_context;
    use tower::ServiceExt;

    use SchemaTestContext as WechatMappingContext;

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

    /// Base create body for a WeChat `non_renewing` mapping with a manual price.
    fn wechat_create_body(external_product_id: &str) -> Value {
        json!({
            "paymentProvider": "wechat",
            "externalProductId": external_product_id,
            "entitlementKey": "wechat-pro",
            "billingType": "non_renewing",
            "serviceDurationDays": 30,
            "price": 1990,
            "currency": "CNY",
            "enabled": true,
        })
    }

    /// PRD §2.2: the manual price must persist under the same JSONB keys the
    /// Stripe/Creem sync writes — otherwise the WeChat order flow reads a
    /// zero amount and every checkout fails.
    #[test_context(WechatMappingContext)]
    #[tokio::test]
    async fn test_wechat_mapping_create_with_price_persists_sync_compatible_info(
        ctx: &mut WechatMappingContext,
    ) {
        let realm_id = ctx._realm_id.clone();
        let token = setup_billing_admin_session(ctx, "wechat-map-create@test.com").await;

        let app = ctx.create_unified_test_router();
        let response = app
            .oneshot(auth_request(
                "POST",
                format!("/api/bill/{realm_id}/entitlement-mappings"),
                &token,
                Some(Body::from(
                    wechat_create_body("wx_product_monthly").to_string(),
                )),
            ))
            .await
            .unwrap();

        assert_eq!(
            response.status(),
            StatusCode::CREATED,
            "WeChat mapping create must return 201, body={}",
            body_json(response).await
        );

        let body = body_json(response).await;
        assert_eq!(body["paymentProvider"], "wechat");
        assert_eq!(body["providerProductInfo"]["price"], 1990);
        assert_eq!(body["providerProductInfo"]["currency"], "CNY");
        // `name` backs the purchase-page display_name; WeChat has no catalog
        // name, so it is seeded from the entitlement key.
        assert_eq!(body["providerProductInfo"]["name"], "wechat-pro");

        // The stored JSONB (what resolve_target and the WeChat create-order
        // call read) must match the response.
        let stored: Option<Value> = sqlx::query_scalar(
            "SELECT provider_product_info FROM provider_entitlement_mappings WHERE id = $1",
        )
        .bind(body["id"].as_str().unwrap().parse::<uuid::Uuid>().unwrap())
        .fetch_one(&ctx.app_state.pool)
        .await
        .unwrap();
        let stored = stored.expect("provider_product_info must be stored");
        assert_eq!(stored["price"], 1990);
        assert_eq!(stored["currency"], "CNY");
    }

    /// Fail-loud contract: a WeChat mapping without a positive price can never
    /// be purchased (create-order requires a positive amount), so the write
    /// itself must be rejected instead of deferring the failure to checkout.
    #[test_context(WechatMappingContext)]
    #[tokio::test]
    async fn test_wechat_mapping_create_without_price_returns_400(ctx: &mut WechatMappingContext) {
        let realm_id = ctx._realm_id.clone();
        let token = setup_billing_admin_session(ctx, "wechat-map-noprice@test.com").await;

        for (label, body) in [
            ("missing both", {
                let mut b = wechat_create_body("wx_product_noprice");
                let obj = b.as_object_mut().unwrap();
                obj.remove("price");
                obj.remove("currency");
                b
            }),
            ("zero price", {
                let mut b = wechat_create_body("wx_product_zero");
                b["price"] = json!(0);
                b
            }),
            ("missing currency", {
                let mut b = wechat_create_body("wx_product_nocurr");
                b.as_object_mut().unwrap().remove("currency");
                b
            }),
            ("lowercase currency", {
                let mut b = wechat_create_body("wx_product_lowercurr");
                b["currency"] = json!("cny");
                b
            }),
        ] {
            let app = ctx.create_unified_test_router();
            let response = app
                .oneshot(auth_request(
                    "POST",
                    format!("/api/bill/{realm_id}/entitlement-mappings"),
                    &token,
                    Some(Body::from(body.to_string())),
                ))
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                StatusCode::BAD_REQUEST,
                "{label}: invalid WeChat price config must return 400"
            );
        }
    }

    /// PRD §8.1: WeChat subscriptions are non-renewing (no merchant-initiated
    /// deduction in scope). A recurring mapping could never be fulfilled as
    /// configured, so the combination is rejected at write time.
    #[test_context(WechatMappingContext)]
    #[tokio::test]
    async fn test_wechat_mapping_create_recurring_returns_400(ctx: &mut WechatMappingContext) {
        let realm_id = ctx._realm_id.clone();
        let token = setup_billing_admin_session(ctx, "wechat-map-recurring@test.com").await;

        let mut body = wechat_create_body("wx_product_recurring");
        body["billingType"] = json!("recurring");
        body["billingPeriod"] = json!("monthly");
        body.as_object_mut().unwrap().remove("serviceDurationDays");

        let app = ctx.create_unified_test_router();
        let response = app
            .oneshot(auth_request(
                "POST",
                format!("/api/bill/{realm_id}/entitlement-mappings"),
                &token,
                Some(Body::from(body.to_string())),
            ))
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "WeChat recurring billing_type must be rejected"
        );
    }

    /// Same channel×billing_type whitelist, batch path: the bulk
    /// PUT /entitlement-mappings/batch writes `billing_type` directly (COALESCE
    /// per row), so it must run the same `validate_provider_billing_type`
    /// check the create path enforces — otherwise a valid WeChat
    /// non_renewing mapping could be flipped to recurring via batch, producing
    /// a subscription mapping with no renewal event source.
    #[test_context(WechatMappingContext)]
    #[tokio::test]
    async fn test_wechat_mapping_batch_recurring_returns_400(ctx: &mut WechatMappingContext) {
        let realm_id = ctx._realm_id.clone();
        let token = setup_billing_admin_session(ctx, "wechat-map-batch-recurring@test.com").await;

        let app = ctx.create_unified_test_router();
        let created = app
            .clone()
            .oneshot(auth_request(
                "POST",
                format!("/api/bill/{realm_id}/entitlement-mappings"),
                &token,
                Some(Body::from(
                    wechat_create_body("wx_product_batch_recurring").to_string(),
                )),
            ))
            .await
            .unwrap();
        assert_eq!(created.status(), StatusCode::CREATED);
        let mapping_id = body_json(created).await["id"].clone();

        let batch = json!({
            "paymentProvider": "wechat",
            "externalProductId": "wx_product_batch_recurring",
            "updates": [{
                "mappingId": mapping_id,
                "billingType": "recurring",
            }]
        });
        let response = app
            .clone()
            .oneshot(auth_request(
                "PUT",
                format!("/api/bill/{realm_id}/entitlement-mappings/batch"),
                &token,
                Some(Body::from(batch.to_string())),
            ))
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "batch must not bypass the channel×billing_type whitelist"
        );

        // The stored billing_type is untouched by the rejected batch.
        let stored: Option<String> = sqlx::query_scalar(
            "SELECT billing_type FROM provider_entitlement_mappings WHERE id = $1",
        )
        .bind(mapping_id.as_str().unwrap().parse::<uuid::Uuid>().unwrap())
        .fetch_one(&ctx.app_state.pool)
        .await
        .unwrap();
        assert_eq!(
            stored.as_deref(),
            Some("non_renewing"),
            "rejected batch must leave the original billing_type in place"
        );
    }

    /// Mirror of the create-side non_renewing rejection for Stripe/Creem:
    /// non-renewing is an IAP/WeChat product form, and a Stripe/Creem
    /// non_renewing row would be clobbered back by the next product sync.
    #[test_context(WechatMappingContext)]
    #[tokio::test]
    async fn test_stripe_mapping_batch_non_renewing_returns_400(ctx: &mut WechatMappingContext) {
        let realm_id = ctx._realm_id.clone();
        let token = setup_billing_admin_session(ctx, "stripe-map-batch-nonrenewing@test.com").await;

        let mapping_id = setup_test_entitlement_mapping(
            ctx,
            &realm_id,
            "stripe",
            "prod_stripe_batch_nonrenewing",
            "stripe-batch-plan",
        )
        .await;

        let app = ctx.create_unified_test_router();
        let batch = json!({
            "paymentProvider": "stripe",
            "externalProductId": "prod_stripe_batch_nonrenewing",
            "updates": [{
                "mappingId": mapping_id.to_string(),
                "billingType": "non_renewing",
            }]
        });
        let response = app
            .oneshot(auth_request(
                "PUT",
                format!("/api/bill/{realm_id}/entitlement-mappings/batch"),
                &token,
                Some(Body::from(batch.to_string())),
            ))
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "batch must reject a Stripe non_renewing billing_type like create does"
        );
    }

    /// Price truth for every other provider lives in its catalog (synced), so
    /// a manual price would create a second, unsynced source of truth.
    #[test_context(WechatMappingContext)]
    #[tokio::test]
    async fn test_stripe_mapping_create_with_price_returns_400(ctx: &mut WechatMappingContext) {
        let realm_id = ctx._realm_id.clone();
        let token = setup_billing_admin_session(ctx, "stripe-map-price@test.com").await;

        let body = json!({
            "paymentProvider": "stripe",
            "externalProductId": "prod_stripe_manual_price",
            "externalPriceId": "price_manual",
            "entitlementKey": "stripe-pro",
            "billingType": "recurring",
            "billingPeriod": "monthly",
            "price": 1990,
            "currency": "USD",
            "enabled": true,
        });

        let app = ctx.create_unified_test_router();
        let response = app
            .oneshot(auth_request(
                "POST",
                format!("/api/bill/{realm_id}/entitlement-mappings"),
                &token,
                Some(Body::from(body.to_string())),
            ))
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "manual price on a non-WeChat provider must be rejected"
        );
    }

    /// Price changes are a normal admin operation (and the only way to price
    /// SQL-seeded WeChat rows predating this endpoint). PATCH carries
    /// price/currency independently and merges into the stored JSONB.
    #[test_context(WechatMappingContext)]
    #[tokio::test]
    async fn test_wechat_mapping_patch_price_merges_into_stored_info(
        ctx: &mut WechatMappingContext,
    ) {
        let realm_id = ctx._realm_id.clone();
        let token = setup_billing_admin_session(ctx, "wechat-map-patch@test.com").await;

        let app = ctx.create_unified_test_router();
        let created = app
            .clone()
            .oneshot(auth_request(
                "POST",
                format!("/api/bill/{realm_id}/entitlement-mappings"),
                &token,
                Some(Body::from(
                    wechat_create_body("wx_product_patch").to_string(),
                )),
            ))
            .await
            .unwrap();
        assert_eq!(created.status(), StatusCode::CREATED);
        let mapping_id = body_json(created).await["id"].clone();

        // Price-only PATCH: currency stays as stored.
        let response = app
            .clone()
            .oneshot(auth_request(
                "PATCH",
                format!(
                    "/api/bill/{realm_id}/entitlement-mappings/{}",
                    mapping_id.as_str().unwrap()
                ),
                &token,
                Some(Body::from(json!({"price": 2990}).to_string())),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = body_json(response).await;
        assert_eq!(body["providerProductInfo"]["price"], 2990);
        assert_eq!(body["providerProductInfo"]["currency"], "CNY");
        assert_eq!(body["providerProductInfo"]["name"], "wechat-pro");

        // Currency-only PATCH with a malformed code (format check only — no
        // ISO dictionary, so "RMB" would pass; "cny" fails the format) → 400,
        // price untouched.
        let response = app
            .clone()
            .oneshot(auth_request(
                "PATCH",
                format!(
                    "/api/bill/{realm_id}/entitlement-mappings/{}",
                    mapping_id.as_str().unwrap()
                ),
                &token,
                Some(Body::from(json!({"currency": "cny"}).to_string())),
            ))
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "malformed currency code must be rejected"
        );
    }

    /// Same catalog-truth rule as create: PATCHing a price onto a synced
    /// provider must be rejected, not silently merged.
    #[test_context(WechatMappingContext)]
    #[tokio::test]
    async fn test_stripe_mapping_patch_price_returns_400(ctx: &mut WechatMappingContext) {
        let realm_id = ctx._realm_id.clone();
        let token = setup_billing_admin_session(ctx, "stripe-map-patchprice@test.com").await;

        let mapping_id = setup_test_entitlement_mapping(
            ctx,
            &realm_id,
            "stripe",
            "prod_stripe_patch_price",
            "stripe-patch-plan",
        )
        .await;

        let app = ctx.create_unified_test_router();
        let response = app
            .oneshot(auth_request(
                "PATCH",
                format!("/api/bill/{realm_id}/entitlement-mappings/{mapping_id}"),
                &token,
                Some(Body::from(json!({"price": 1990}).to_string())),
            ))
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "PATCHing a manual price onto a synced provider must be rejected"
        );
    }
}
