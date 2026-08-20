// =============================================================================
// SDK Realm Scenarios
// =============================================================================
//
// Test SDK realm management API against real API.
// Covers US-TP-012: Realm CRUD via SDK (create, list, detail).
//
// =============================================================================

use herald_core::domain::authorization::permission_service::PermissionService;
use herald_core::domain::client_api_keys::services::ClientApiKeyService;
use herald_sdk::{AdminUserSdkInput, Client, CreateRealmSdkRequest, Error};
use herald_test_support::SchemaTestContext;
use sqlx::query;
use test_context::test_context;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Create an API key in a specific realm, optionally granting specific permissions
/// through a dedicated role bound to the API key principal.
///
/// This mirrors the design doc S8.2 fixture strategy:
/// 1. Create a role in the given realm.
/// 2. Insert permissions into role_policies.
/// 3. Create an API key in the given realm.
/// 4. Insert user_roles with principal_type="api_key" and principal_id = api_key.id.
///
/// Returns (api_key_plaintext, api_key_entity_id, realm_id).
async fn setup_api_key_with_permissions(
    ctx: &SchemaTestContext,
    realm_id: &str,
    client_id: &str,
    permissions: &[(&str, &str)],
) -> (String, String, String) {
    // 1. Create a role
    let role_id = Uuid::now_v7();
    query(
        "INSERT INTO roles (id, name, realm_id, client_id, is_builtin)
         VALUES ($1, $2, $3, $4, false)",
    )
    .bind(role_id)
    .bind(format!("test-role-{}", Uuid::now_v7()))
    .bind(realm_id)
    .bind(client_id)
    .execute(&ctx.app_state.pool)
    .await
    .expect("Failed to create role");

    // 2. Grant permissions to the role
    for (resource, action) in permissions {
        query(
            "INSERT INTO role_policies (id, role_id, realm_id, resource, action)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(Uuid::now_v7())
        .bind(role_id)
        .bind(realm_id)
        .bind(resource)
        .bind(action)
        .execute(&ctx.app_state.pool)
        .await
        .expect("Failed to insert role policy");
    }

    // 3. Create the API key
    let api_key_id = Uuid::now_v7();
    let api_key_plaintext = ClientApiKeyService::generate_api_key();
    let api_key_hash = ClientApiKeyService::hash_api_key(&api_key_plaintext);

    query(
        "INSERT INTO client_api_keys (id, name, api_key_hash, realm_id, client_app_id, enabled, expires_at, created_at, last_used_at)
         VALUES ($1, $2, $3, $4, NULL, true, NULL, NOW(), NULL)",
    )
    .bind(api_key_id)
    .bind(format!("test-key-{}", Uuid::now_v7()))
    .bind(&api_key_hash)
    .bind(realm_id)
    .execute(&ctx.app_state.pool)
    .await
    .expect("Failed to create API key");

    // 4. Bind role to the API key principal via user_roles
    query(
        "INSERT INTO user_roles (id, user_id, role_id, realm_id, client_id, principal_type, principal_id)
         VALUES ($1, $2, $3, $4, $5, 'api_key', $2::text)",
    )
    .bind(api_key_id)
    .bind(api_key_id)
    .bind(role_id)
    .bind(realm_id)
    .bind(client_id)
    .execute(&ctx.app_state.pool)
    .await
    .expect("Failed to bind role to API key principal");

    // 5. Invalidate permission cache so the new binding takes effect
    let _ = ctx
        .app_state
        .permission_checker
        .invalidate_user_role_cache(realm_id, &api_key_id.to_string())
        .await;

    (
        api_key_plaintext,
        api_key_id.to_string(),
        realm_id.to_string(),
    )
}

/// Ensure the "admin" realm row exists in the database.
/// The template schema only has "default-template-realm", but the ext API
/// hard-codes the admin realm check against the literal "admin".
async fn ensure_admin_realm(ctx: &SchemaTestContext) {
    query("INSERT INTO realm (id, name) VALUES ('admin', 'Admin') ON CONFLICT (id) DO NOTHING")
        .execute(&ctx.app_state.pool)
        .await
        .expect("Failed to ensure admin realm");
}

/// Create a second realm for cross-realm tests.
fn make_second_realm_id() -> String {
    format!("realm-b-{}", Uuid::now_v7())
}

/// Start a test server on an ephemeral port and return (base_url, abort_handle).
async fn start_test_server(ctx: &SchemaTestContext) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{}", addr);

    let router = ctx.create_unified_test_router();
    let handle = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    (base_url, handle)
}

// ---------------------------------------------------------------------------
// Test 1: Create realm success
// ---------------------------------------------------------------------------

// User Story: docs/user-stories/16-sdk-user-stories.md - US-TP-012 Scenario 1
// Covers: API Key in admin realm with realm:create -> create succeeds, returns RealmInfo

#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_sdk_realm_create_success(ctx: &mut SchemaTestContext) {
    // Given: admin realm exists and an API key in that realm with realm:create permission
    ensure_admin_realm(ctx).await;

    let (api_key, _key_id, _realm) =
        setup_api_key_with_permissions(ctx, "admin", &ctx._client_id, &[("realm", "manage")]).await;

    let (base_url, handle) = start_test_server(ctx).await;
    let client = Client::new(base_url, api_key, None);

    // When: calling create_realm with valid input
    let request = CreateRealmSdkRequest {
        name: "test-realm-scenario".to_string(),
        description: Some("Created by scenario test".to_string()),
        admin_user: AdminUserSdkInput {
            email: "realm-admin-scenario@test.com".to_string(),
            password: "password123".to_string(),
        },
    };

    let result = client.create_realm(request).await;

    // Then: realm is created successfully
    if let Err(e) = &result {
        eprintln!("create_realm error: {:?}", e);
    }
    assert!(
        result.is_ok(),
        "create_realm should succeed: {:?}",
        result.err()
    );
    let realm = result.unwrap();
    assert_eq!(realm.name, "test-realm-scenario");
    assert!(!realm.id.is_empty(), "Realm ID should be populated");

    handle.abort();
}

// ---------------------------------------------------------------------------
// Test 2: Create realm forbidden
// ---------------------------------------------------------------------------

// User Story: docs/user-stories/16-sdk-user-stories.md - US-TP-012 Scenario 4
// Covers: (A) non-admin realm key -> 403, (B) admin realm key without realm:create -> 403

#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_sdk_realm_create_forbidden(ctx: &mut SchemaTestContext) {
    ensure_admin_realm(ctx).await;

    // --- Subcase A: API key in non-admin realm cannot create realms ---

    // Given: API key in the default (non-admin) realm, even with realm:create
    let (api_key_non_admin, _, _) = setup_api_key_with_permissions(
        ctx,
        &ctx._realm_id,
        &ctx._client_id,
        &[("realm", "manage")],
    )
    .await;

    let (base_url_a, handle_a) = start_test_server(ctx).await;
    let client_a = Client::new(base_url_a, api_key_non_admin, None);

    let request = CreateRealmSdkRequest {
        name: "forbidden-realm-a".to_string(),
        description: None,
        admin_user: AdminUserSdkInput {
            email: "forbidden-a@test.com".to_string(),
            password: "password123".to_string(),
        },
    };

    let result_a = client_a.create_realm(request).await;

    // Then: non-admin realm key -> Forbidden (403)
    assert!(result_a.is_err(), "Non-admin realm key should be forbidden");
    match result_a.unwrap_err() {
        Error::Forbidden(_) => {}
        other => panic!("Expected Forbidden, got: {:?}", other),
    }

    handle_a.abort();

    // --- Subcase B: API key in admin realm but WITHOUT realm:create ---

    // Given: API key in admin realm with realm:view only (no realm:create)
    let (api_key_no_create, _, _) =
        setup_api_key_with_permissions(ctx, "admin", &ctx._client_id, &[("realm", "view")]).await;

    let (base_url_b, handle_b) = start_test_server(ctx).await;
    let client_b = Client::new(base_url_b, api_key_no_create, None);

    let request = CreateRealmSdkRequest {
        name: "forbidden-realm-b".to_string(),
        description: None,
        admin_user: AdminUserSdkInput {
            email: "forbidden-b@test.com".to_string(),
            password: "password123".to_string(),
        },
    };

    let result_b = client_b.create_realm(request).await;

    // Then: admin realm key without realm:create -> Forbidden (403)
    assert!(
        result_b.is_err(),
        "Admin realm key without realm:create should be forbidden"
    );
    match result_b.unwrap_err() {
        Error::Forbidden(_) => {}
        other => panic!("Expected Forbidden, got: {:?}", other),
    }

    handle_b.abort();
}

// ---------------------------------------------------------------------------
// Test 3: Create realm validation error
// ---------------------------------------------------------------------------

// User Story: docs/user-stories/16-sdk-user-stories.md - US-TP-012 Scenario 4 (validation)
// Covers: Empty/too-short name -> 400

#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_sdk_realm_create_validation_error(ctx: &mut SchemaTestContext) {
    ensure_admin_realm(ctx).await;

    // Given: admin-realm API key with realm:manage
    let (api_key, _, _) =
        setup_api_key_with_permissions(ctx, "admin", &ctx._client_id, &[("realm", "manage")]).await;

    let (base_url, handle) = start_test_server(ctx).await;
    let client = Client::new(base_url, api_key, None);

    // When: calling create_realm with a name that is too short (< 3 chars)
    let request = CreateRealmSdkRequest {
        name: "ab".to_string(), // too short, min is 3
        description: None,
        admin_user: AdminUserSdkInput {
            email: "validation@test.com".to_string(),
            password: "password123".to_string(),
        },
    };

    let result = client.create_realm(request).await;

    // Then: returns an error (400 validation)
    assert!(result.is_err(), "Short name should return validation error");
    match result.unwrap_err() {
        Error::ApiError { status, message } => {
            assert_eq!(
                status, 400,
                "Expected 400 status, got {}: {}",
                status, message
            );
        }
        other => panic!("Expected ApiError with 400, got: {:?}", other),
    }

    handle.abort();
}

// ---------------------------------------------------------------------------
// Test 4: List realms admin
// ---------------------------------------------------------------------------

// User Story: docs/user-stories/16-sdk-user-stories.md - US-TP-012 Scenario 2
// Covers: Admin-realm key with realm:view -> sees all realms

#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_sdk_realm_list_admin(ctx: &mut SchemaTestContext) {
    ensure_admin_realm(ctx).await;

    // Given: admin-realm API key with realm:view
    let (api_key, _, _) =
        setup_api_key_with_permissions(ctx, "admin", &ctx._client_id, &[("realm", "view")]).await;

    let (base_url, handle) = start_test_server(ctx).await;
    let client = Client::new(base_url, api_key, None);

    // When: listing realms
    let result = client.list_realms().await;

    // Then: succeeds and returns all realms (admin + default-template-realm at minimum)
    if let Err(e) = &result {
        eprintln!("list_realms error: {:?}", e);
    }
    assert!(
        result.is_ok(),
        "list_realms should succeed: {:?}",
        result.err()
    );
    let realms = result.unwrap();
    assert!(
        realms.len() >= 2,
        "Admin should see at least 2 realms, got {}",
        realms.len()
    );

    handle.abort();
}

// ---------------------------------------------------------------------------
// Test 5: List realms non-admin own only
// ---------------------------------------------------------------------------

// User Story: docs/user-stories/16-sdk-user-stories.md - US-TP-012 Scenario 2
// Covers: Non-admin key with realm:view -> sees own realm only

#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_sdk_realm_list_non_admin_own_only(ctx: &mut SchemaTestContext) {
    ensure_admin_realm(ctx).await;

    // Given: API key in default (non-admin) realm with realm:view
    let (api_key, _, _) =
        setup_api_key_with_permissions(ctx, &ctx._realm_id, &ctx._client_id, &[("realm", "view")])
            .await;

    let (base_url, handle) = start_test_server(ctx).await;
    let client = Client::new(base_url, api_key, None);

    // When: listing realms
    let result = client.list_realms().await;

    // Then: succeeds but returns only own realm
    if let Err(e) = &result {
        eprintln!("list_realms error: {:?}", e);
    }
    assert!(
        result.is_ok(),
        "list_realms should succeed: {:?}",
        result.err()
    );
    let realms = result.unwrap();
    assert_eq!(
        realms.len(),
        1,
        "Non-admin should see exactly 1 realm (own), got {}",
        realms.len()
    );
    assert_eq!(
        realms[0].id, ctx._realm_id,
        "The visible realm should be the API key's own realm"
    );

    handle.abort();
}

// ---------------------------------------------------------------------------
// Test 6: Realm detail success
// ---------------------------------------------------------------------------

// User Story: docs/user-stories/16-sdk-user-stories.md - US-TP-012 Scenario 3
// Covers: Valid realm:view -> returns realm info

#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_sdk_realm_detail_success(ctx: &mut SchemaTestContext) {
    // Given: API key with realm:view in its own realm
    let (api_key, _, _) =
        setup_api_key_with_permissions(ctx, &ctx._realm_id, &ctx._client_id, &[("realm", "view")])
            .await;

    let (base_url, handle) = start_test_server(ctx).await;
    let client = Client::new(base_url, api_key, None);

    // When: getting realm detail for the API key's own realm
    let result = client.get_realm(&ctx._realm_id).await;

    // Then: succeeds and returns realm info
    if let Err(e) = &result {
        eprintln!("get_realm error: {:?}", e);
    }
    assert!(
        result.is_ok(),
        "get_realm should succeed: {:?}",
        result.err()
    );
    let realm = result.unwrap();
    assert_eq!(realm.id, ctx._realm_id);
    assert!(!realm.name.is_empty(), "Realm name should be populated");

    handle.abort();
}

// ---------------------------------------------------------------------------
// Test 7: Realm detail cross-realm forbidden
// ---------------------------------------------------------------------------

// User Story: docs/user-stories/16-sdk-user-stories.md - US-TP-012 Scenario 4
// Covers: Key in realm A tries to view realm B -> 403

#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_sdk_realm_detail_cross_realm_forbidden(ctx: &mut SchemaTestContext) {
    // Given: realm B exists and API key in default realm with realm:view
    let other_realm_id = make_second_realm_id();
    query("INSERT INTO realm (id, name) VALUES ($1, 'Other Realm')")
        .bind(&other_realm_id)
        .execute(&ctx.app_state.pool)
        .await
        .expect("Failed to create second realm");

    let (api_key, _, _) =
        setup_api_key_with_permissions(ctx, &ctx._realm_id, &ctx._client_id, &[("realm", "view")])
            .await;

    let (base_url, handle) = start_test_server(ctx).await;
    let client = Client::new(base_url, api_key, None);

    // When: trying to view realm B (cross-realm access)
    let result = client.get_realm(&other_realm_id).await;

    // Then: returns 403 Forbidden
    assert!(result.is_err(), "Cross-realm access should be forbidden");
    match result.unwrap_err() {
        Error::Forbidden(_) => {}
        other => panic!("Expected Forbidden, got: {:?}", other),
    }

    handle.abort();
}

// ---------------------------------------------------------------------------
// Test 8: Realm detail not found
// ---------------------------------------------------------------------------

// User Story: docs/user-stories/16-sdk-user-stories.md - US-TP-012 Scenario 5
// Covers: Non-existent realm -> blocked by the realm-boundary check. There is
// no cross-realm super-admin: the membership check runs before any existence
// lookup, and returning 404 here would leak realm existence to foreign-realm
// callers, so a realm outside the caller's own (existent or not) is 403.

#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_sdk_realm_detail_not_found(ctx: &mut SchemaTestContext) {
    ensure_admin_realm(ctx).await;

    // Given: admin-realm API key with realm.manage
    let (api_key, _, _) =
        setup_api_key_with_permissions(ctx, "admin", &ctx._client_id, &[("realm", "manage")]).await;

    let (base_url, handle) = start_test_server(ctx).await;
    let client = Client::new(base_url, api_key, None);

    // When: querying a non-existent realm ID
    let nonexistent_id = Uuid::now_v7().to_string();
    let result = client.get_realm(&nonexistent_id).await;

    // Then: returns 403 Forbidden (cross-realm boundary precedes existence)
    assert!(result.is_err(), "Non-existent realm should return error");
    match result.unwrap_err() {
        Error::Forbidden(_) => {}
        other => panic!("Expected Forbidden, got: {:?}", other),
    }

    handle.abort();
}

// ---------------------------------------------------------------------------
// Test 9: Unauthenticated
// ---------------------------------------------------------------------------

// User Story: docs/user-stories/16-sdk-user-stories.md - US-TP-012 (cross-cutting)
// Covers: No API key -> 401

#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_scenario_sdk_realm_unauthenticated(ctx: &mut SchemaTestContext) {
    // Given: an SDK client with a fake/empty API key
    let (base_url, handle) = start_test_server(ctx).await;
    let client = Client::new(base_url, "invalid-nonexistent-key".to_string(), None);

    // When: calling any realm endpoint
    let result = client.list_realms().await;

    // Then: returns 401 Unauthorized
    assert!(result.is_err(), "Invalid API key should return error");
    match result.unwrap_err() {
        Error::Unauthorized(_) => {}
        other => panic!("Expected Unauthorized, got: {:?}", other),
    }

    handle.abort();
}
