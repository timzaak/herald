//
// Test SDK billing API against real API.
// Adapted for product_reduce: subscription uses entitlement_key instead of
// plan/planId/billingPeriod. Plan list/assignment tests removed as those
// endpoints no longer exist.
//

use herald_sdk::{Client, SubscriptionStatus};
use herald_test_support::SchemaTestContext;
use test_context::test_context;
use uuid::Uuid;

/// Billing API Scenarios
/// Test scenario: SDK get subscription - success
///
/// Verifies that the SDK correctly retrieves subscription details
/// with entitlement_key and payment_provider fields.
///
/// User Story: US-EM-005 (SDK queries subscription status via entitlement_key)
/// Covers: entitlement_key in response, payment_provider in response
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_sdk_get_subscription_success(ctx: &mut SchemaTestContext) {
    // 1. Setup: Create admin session with billing permissions
    let email = "billing_admin@example.com";
    let (_session_token, user_id) =
        herald_test_support::helpers::create_admin_session_with_user(ctx, email, 1800).await;
    herald_test_support::helpers::grant_realm_admin_role(ctx, &user_id).await;

    // 2. Create SDK client pointing to the test server
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{}", addr);

    // 3. Start the test server in the background
    let router = ctx.create_unified_test_router();
    let handle = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    // 4. Create a test API key for SDK authentication
    let (api_key, api_key_entity) =
        herald_test_support::helpers::create_test_api_key(ctx, "SDK Test Key", true, None).await;
    herald_test_support::helpers::grant_api_key_permissions(
        &ctx.app_state.pool,
        &ctx._realm_id,
        &ctx._client_id,
        &api_key_entity.id,
        &[("billing", "view")],
    )
    .await;

    // 5. Create a test subscription with entitlement_key (new schema)
    let realm_id = ctx._realm_id.clone();
    let client_app_id = ctx._client_app_id.clone();
    let client_app_uuid = Uuid::parse_str(&client_app_id).unwrap();
    herald_test_support::helpers::create_test_subscription_with_entitlement(
        ctx,
        &realm_id,
        client_app_uuid,
        "test-entitlement",
        "price_test_123",
    )
    .await;

    // 6. Create SDK client with the test API key and get subscription
    let client = Client::new(base_url, api_key, None);

    let result = client
        .get_subscription(&ctx._realm_id, "admin-web-console")
        .await;

    // 7. Verify: Subscription should be retrieved with entitlement_key
    if let Err(e) = &result {
        eprintln!("SDK Error: {:?}", e);
    }
    assert!(
        result.is_ok(),
        "Get subscription should succeed: {:?}",
        result.err()
    );
    let subscription = result.unwrap();
    assert_eq!(
        subscription.status,
        SubscriptionStatus::Active,
        "Subscription should be active"
    );
    assert_eq!(
        subscription.entitlement_key, "test-entitlement",
        "Subscription should have correct entitlement_key"
    );
    assert_eq!(
        subscription.payment_provider, "creem",
        "Subscription should have correct payment_provider"
    );

    // 8. Cleanup
    handle.abort();
}
