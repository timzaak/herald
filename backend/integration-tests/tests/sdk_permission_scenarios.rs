// SDK Permission Scenarios
//
// Test SDK permission checking against real API

use herald_sdk::{Client, PermissionCheckRequest, Rule};
use herald_test_support::SchemaTestContext;
use test_context::test_context;

/// Test scenario: SDK permission check - allowed
///
/// Verifies that the SDK correctly handles a successful permission check
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_sdk_permission_allowed(ctx: &mut SchemaTestContext) {
    // 1. Setup: Create a user with permission
    let email = "permission_user@example.com";
    let (_user_id, session_token) =
        herald_test_support::helpers::create_test_user_with_permissions(
            ctx,
            email,
            &[("article", "read")],
        )
        .await;

    // 2. Create API key for SDK
    let (api_key, _api_key_entity) = herald_test_support::helpers::create_test_api_key(
        ctx,
        "Test API Key for Permission",
        true,
        None,
    )
    .await;

    // 3. Create SDK client pointing to the test server
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{}", addr);

    // 4. Start the test server in the background
    let router = ctx.create_unified_test_router();
    let handle = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    // 5. Create SDK client and check permission
    let client = Client::new(base_url, api_key, None);

    let request = PermissionCheckRequest {
        access_token: session_token.clone(),
        rules: vec![Rule {
            resource: "article".to_string(),
            action: "read".to_string(),
        }],
    };

    // 6. Perform permission check
    let result = client.check_permission(request).await;

    // 7. Verify: Permission should be allowed
    assert!(result.is_ok(), "Permission check should succeed");
    let response = result.unwrap();
    assert!(response.allowed, "User should have article:read permission");
    assert!(
        response.user_id.is_some(),
        "Response should include user_id"
    );
    assert_eq!(response.error, None, "An allowed check carries no error");

    // 8. Cleanup
    handle.abort();
}

/// Test scenario: SDK permission check - denied
///
/// Verifies that the SDK correctly handles a denied permission check
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_sdk_permission_denied(ctx: &mut SchemaTestContext) {
    // 1. Setup: Create a user without the permission
    let email = "no_permission_user@example.com";
    let (_user_id, session_token) =
        herald_test_support::helpers::create_test_user_with_permissions(
            ctx,
            email,
            &[("other", "read")], // Different permission
        )
        .await;

    // 2. Create API key for SDK
    let (api_key, _api_key_entity) = herald_test_support::helpers::create_test_api_key(
        ctx,
        "Test API Key for Permission Denied",
        true,
        None,
    )
    .await;

    // 3. Create SDK client pointing to the test server
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{}", addr);

    // 4. Start the test server in the background
    let router = ctx.create_unified_test_router();
    let handle = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    // 5. Create SDK client and check permission
    let client = Client::new(base_url, api_key, None);

    let request = PermissionCheckRequest {
        access_token: session_token.clone(),
        rules: vec![Rule {
            resource: "article".to_string(), // User doesn't have this permission
            action: "read".to_string(),
        }],
    };

    // 6. Perform permission check
    let result = client.check_permission(request).await;

    // 7. Verify: Permission should be denied
    assert!(
        result.is_ok(),
        "Permission check should succeed even when denied"
    );
    let response = result.unwrap();
    assert!(
        !response.allowed,
        "User should not have article:read permission"
    );
    // A rule denial is an authenticated user: Herald sends neither `userId`
    // nor `error`. Integrators tell this (403) from an unusable token (401) by
    // exactly these two fields.
    assert_eq!(
        response.user_id, None,
        "A rule denial must not identify the user"
    );
    assert_eq!(response.error, None, "A rule denial carries no error");

    // 8. Cleanup
    handle.abort();
}

/// Test scenario: SDK permission check - unknown token
///
/// Verifies that an access token Herald never issued reaches the caller as a
/// structured refusal that names the reason, not as an SDK error
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_sdk_permission_unknown_token(ctx: &mut SchemaTestContext) {
    let unknown_token = "unknown_access_token_12345";

    let (api_key, _api_key_entity) = herald_test_support::helpers::create_test_api_key(
        ctx,
        "Test API Key for Unknown Token",
        true,
        None,
    )
    .await;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{}", addr);

    let router = ctx.create_unified_test_router();
    let handle = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    let client = Client::new(base_url, api_key, None);

    let request = PermissionCheckRequest {
        access_token: unknown_token.to_string(),
        rules: vec![Rule {
            resource: "article".to_string(),
            action: "read".to_string(),
        }],
    };

    let result = client.check_permission(request).await;

    // Herald answers 200 and names the reason in `error`. This value is what
    // an integrator maps to 401, where a rule denial (no `error`) maps to 403.
    let response = result.expect("An unknown token is a structured answer, not an SDK error");
    assert!(!response.allowed, "An unknown token must not be allowed");
    assert_eq!(
        response.user_id, None,
        "An unknown token must not identify a user"
    );
    assert_eq!(
        response.error.as_deref(),
        Some("token_not_found"),
        "An unknown token must be reported as token_not_found"
    );

    handle.abort();
}
