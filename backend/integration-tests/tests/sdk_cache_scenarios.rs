// =============================================================================
// SDK Cache Scenarios
// =============================================================================
//
// Test SDK caching behavior against real API
// =============================================================================

use herald_sdk::{Client, PermissionCheckRequest, Rule};
use herald_test_support::SchemaTestContext;
use std::time::Duration;
use test_context::test_context;

/// ============================================================================
/// Cache Scenarios
/// ============================================================================
/// Test scenario: SDK permission cache hit
///
/// Verifies that the SDK correctly caches permission check results
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_sdk_permission_cache_hit(ctx: &mut SchemaTestContext) {
    // 1. Setup: Create a user with permission
    let email = "cache_user@example.com";
    let (_user_id, session_token) =
        herald_test_support::helpers::create_test_user_with_permissions(
            ctx,
            email,
            &[("article", "read")],
        )
        .await;

    // 2. Create SDK client pointing to the test server with short cache TTL
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{}", addr);

    // 3. Start the test server in the background
    let router = ctx.create_unified_test_router();
    let handle = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    // 4. Create SDK client with caching enabled (5 seconds TTL)
    let (api_key, _api_key_entity) =
        herald_test_support::helpers::create_test_api_key(ctx, "SDK Test Key", true, None).await;
    let client = Client::new(base_url, api_key, Some(Duration::from_secs(5)));

    let request = PermissionCheckRequest {
        access_token: session_token.clone(),
        rules: vec![Rule {
            resource: "article".to_string(),
            action: "read".to_string(),
        }],
    };

    // 5. First permission check - should hit the server
    let result1 = client.check_permission(request.clone()).await;
    assert!(result1.is_ok(), "First permission check should succeed");
    let response1 = result1.unwrap();
    assert!(response1.allowed, "First check should allow permission");

    // 6. Second permission check immediately - should hit cache
    let result2 = client.check_permission(request.clone()).await;
    assert!(result2.is_ok(), "Second permission check should succeed");
    let response2 = result2.unwrap();
    assert!(response2.allowed, "Second check should allow permission");
    assert_eq!(
        response2.user_id, response1.user_id,
        "Cached response should match first response"
    );

    // 7. Cleanup
    handle.abort();
}

/// Test scenario: SDK cache invalidation
///
/// Verifies that the SDK correctly invalidates cache for a token
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_sdk_cache_invalidation(ctx: &mut SchemaTestContext) {
    // 1. Setup: Create a user with permission
    let email = "invalidate_user@example.com";
    let (_user_id, session_token) =
        herald_test_support::helpers::create_test_user_with_permissions(
            ctx,
            email,
            &[("article", "read")],
        )
        .await;

    // 2. Create SDK client pointing to the test server with caching
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{}", addr);

    // 3. Start the test server in the background
    let router = ctx.create_unified_test_router();
    let handle = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    // 4. Create SDK client with caching enabled
    let (api_key, _api_key_entity) =
        herald_test_support::helpers::create_test_api_key(ctx, "SDK Test Key", true, None).await;
    let client = Client::new(base_url, api_key, Some(Duration::from_secs(60)));

    let request = PermissionCheckRequest {
        access_token: session_token.clone(),
        rules: vec![Rule {
            resource: "article".to_string(),
            action: "read".to_string(),
        }],
    };

    // 5. First permission check - should populate cache
    let result1 = client.check_permission(request.clone()).await;
    assert!(result1.is_ok(), "First permission check should succeed");
    let _response1 = result1.unwrap();

    // 6. Invalidate cache for this token
    client.invalidate_cache(&session_token).await;

    // 7. Second permission check - should hit server again (cache was invalidated)
    let result2 = client.check_permission(request.clone()).await;
    assert!(
        result2.is_ok(),
        "Second permission check after invalidation should succeed"
    );
    let response2 = result2.unwrap();
    assert!(
        response2.allowed,
        "Check after invalidation should still allow permission"
    );

    // 8. Cleanup
    handle.abort();
}
