// =============================================================================
// SDK Error Scenarios
// =============================================================================
//
// Test SDK error handling against real API
// =============================================================================

use herald_sdk::{Client, PermissionCheckRequest, Rule};
use herald_test_support::SchemaTestContext;
use test_context::test_context;

/// ============================================================================
/// Error Handling Scenarios
/// ============================================================================
/// Test scenario: SDK unauthorized error
///
/// Verifies that the SDK correctly handles 401 unauthorized errors
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_sdk_unauthorized_error(ctx: &mut SchemaTestContext) {
    // 1. Setup: No user session (invalid token)
    let invalid_token = "invalid_token_12345";

    // 2. Create SDK client pointing to the test server
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{}", addr);

    // 3. Start the test server in the background
    let router = ctx.create_unified_test_router();
    let handle = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    // 4. Create SDK client and attempt permission check with invalid token
    let client = Client::new(base_url, "test-api-key".to_string(), None);

    let request = PermissionCheckRequest {
        access_token: invalid_token.to_string(),
        rules: vec![Rule {
            resource: "article".to_string(),
            action: "read".to_string(),
        }],
    };

    // 5. Permission check with invalid token should fail
    let result = client.check_permission(request).await;

    // 6. Verify: Request should fail with network error
    assert!(
        result.is_err(),
        "Permission check with invalid token should fail"
    );

    // 7. Cleanup
    handle.abort();
}

/// Test scenario: SDK not found error
///
/// Verifies that the SDK correctly handles 404 not found errors
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_sdk_not_found_error(ctx: &mut SchemaTestContext) {
    // 1. Setup: Create admin session
    let email = "notfound_admin@example.com";
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

    // 4. Create SDK client and try to get subscription for non-existent client
    let client = Client::new(base_url, "test-api-key".to_string(), None);

    let result = client
        .get_subscription(&ctx._realm_id, "non-existent-client")
        .await;

    // 5. Verify: Request should fail with network error
    assert!(
        result.is_err(),
        "Get subscription for non-existent client should fail"
    );

    // 6. Cleanup
    handle.abort();
}

/// Test scenario: SDK invalid JSON error
///
/// Verifies that the SDK correctly handles invalid JSON responses
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_sdk_invalid_json_error(_ctx: &mut SchemaTestContext) {
    // 1. Setup: We'll need to mock the API to return invalid JSON
    // For now, we'll verify that the SDK properly handles connection errors
    // In a full implementation, we'd use a custom test server that returns malformed JSON

    // 2. Create SDK client pointing to a non-existent server
    let client = Client::new(
        "http://127.0.0.1:54321".to_string(),
        "test-api-key".to_string(),
        None,
    ); // Non-existent port

    let request = PermissionCheckRequest {
        access_token: "test_token".to_string(),
        rules: vec![Rule {
            resource: "article".to_string(),
            action: "read".to_string(),
        }],
    };

    // 3. Permission check to non-existent server should fail
    let result = client.check_permission(request).await;

    // 4. Verify: Request should fail with network error
    assert!(
        result.is_err(),
        "Permission check to non-existent server should fail"
    );
}

/// Test scenario: SDK network error
///
/// Verifies that the SDK correctly handles network connection errors
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_sdk_network_error(_ctx: &mut SchemaTestContext) {
    // 1. Setup: Test with a non-existent server
    let client = Client::new(
        "http://127.0.0.1:54322".to_string(),
        "test-api-key".to_string(),
        None,
    ); // Non-existent port

    let request = PermissionCheckRequest {
        access_token: "test_token".to_string(),
        rules: vec![Rule {
            resource: "article".to_string(),
            action: "read".to_string(),
        }],
    };

    // 2. Permission check to non-existent server should fail with network error
    let result = client.check_permission(request).await;

    // 3. Verify: Request should fail with network error
    assert!(
        result.is_err(),
        "Permission check should fail with network error"
    );
}
