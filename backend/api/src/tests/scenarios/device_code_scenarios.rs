/// Device Authorization Endpoint Scenario Tests
///
/// Tests for `POST /api/device/{realmId}/authorize` covering US-DC-001 acceptance criteria.
///
/// User Story: Device Authorization Endpoint (US-DC-001)
/// Covers: Acceptance criteria 1-4
#[cfg(test)]
mod tests {
    use crate::tests::helpers::auth_helpers::{
        create_admin_session_with_user, grant_realm_admin_role,
    };
    use crate::tests::helpers::device_code_helpers::{
        create_client_app_with_device_code_grant, delete_device_code_redis, device_authorize,
        device_confirm, device_token_poll, device_verify, set_device_code_status_redis,
    };
    use crate::tests::response_json;
    use crate::tests::schema_test_context::SchemaTestContext;
    use serde_json::{Value, json};
    use test_context::test_context;
    use tower::ServiceExt;

    /// Helper: set up admin session and return the token.
    async fn setup_admin_session(ctx: &mut SchemaTestContext, email: &str) -> String {
        let (admin_token, user_id) = create_admin_session_with_user(ctx, email, 1800).await;
        grant_realm_admin_role(ctx, &user_id).await;
        admin_token
    }

    // User Story: US-DC-001 Device Authorization Endpoint
    // Covers: Acceptance criteria 1 -- successful device authorization returns correct fields

    /// Test: Device authorization success flow
    ///
    /// Given: Client App with enabled=true, device_code_grant_enabled=true
    /// When: POST /api/device/{realmId}/authorize with client_id
    /// Then: 200 OK, response contains device_code, user_code, verification_uri,
    ///       verification_uri_complete, expires_in=900, interval=5
    ///       user_code matches XXXX-XXXX format (only BCDFGHJKMNPQRSTVWXYZ + hyphen)
    ///       verification_uri contains /{realmId}/device
    ///       verification_uri_complete contains /{realmId}/device/{user_code}
    #[test_context(SchemaTestContext)]
    #[tokio::test]
    async fn test_scenario_device_authorization_success(ctx: &mut SchemaTestContext) {
        let realm_id = ctx._realm_id.clone();
        let admin_token = setup_admin_session(ctx, "dc-auth-success@test.com").await;

        // Create client app with device code grant enabled
        let create_response = create_client_app_with_device_code_grant(
            ctx,
            &realm_id,
            &admin_token,
            "dc-test-app",
            "DC Test App",
            true,
            true,
        )
        .await;
        assert_eq!(
            create_response.status(),
            201,
            "Client app creation should succeed"
        );

        // Request device authorization
        let response = device_authorize(ctx, &realm_id, "dc-test-app").await;
        assert_eq!(
            response.status(),
            200,
            "Device authorization should succeed"
        );

        let json: Value = response_json(response).await;

        // Verify all required fields are present
        assert!(
            json["device_code"].is_string(),
            "device_code should be a string"
        );
        assert!(
            json["user_code"].is_string(),
            "user_code should be a string"
        );
        assert!(
            json["verification_uri"].is_string(),
            "verification_uri should be a string"
        );
        assert!(
            json["verification_uri_complete"].is_string(),
            "verification_uri_complete should be a string"
        );

        // Verify expires_in and interval values
        assert_eq!(json["expires_in"], 900, "expires_in should be 900 seconds");
        assert_eq!(json["interval"], 5, "interval should be 5 seconds");

        // Verify user_code format: XXXX-XXXX (4 uppercase consonants, hyphen, 4 uppercase consonants)
        let user_code = json["user_code"].as_str().unwrap();
        let valid_chars: std::collections::HashSet<char> = "BCDFGHJKMNPQRSTVWXYZ".chars().collect();
        assert_eq!(
            user_code.len(),
            9,
            "user_code should be 9 chars (XXXX-XXXX)"
        );
        assert_eq!(
            user_code.chars().nth(4),
            Some('-'),
            "user_code should have hyphen at position 4"
        );
        for (i, c) in user_code.chars().enumerate() {
            if i == 4 {
                assert_eq!(c, '-', "char at position 4 should be hyphen");
            } else {
                assert!(
                    valid_chars.contains(&c),
                    "char '{}' at position {} should be in valid alphabet",
                    c,
                    i
                );
            }
        }

        // Verify verification_uri contains realm path
        let verification_uri = json["verification_uri"].as_str().unwrap();
        assert!(
            verification_uri.contains(&format!("/{}/device", realm_id)),
            "verification_uri should contain /{}/device, got: {}",
            realm_id,
            verification_uri
        );

        // Verify verification_uri_complete contains user_code path
        let verification_uri_complete = json["verification_uri_complete"].as_str().unwrap();
        assert!(
            verification_uri_complete.contains(&format!("/{}/device/{}", realm_id, user_code)),
            "verification_uri_complete should contain /{}/device/{}, got: {}",
            realm_id,
            user_code,
            verification_uri_complete
        );
    }

    // User Story: US-DC-001 Device Authorization Endpoint
    // Covers: Acceptance criteria 2 -- user_code format and uniqueness

    /// Test: Multiple authorization requests produce valid and unique codes
    ///
    /// Given: Client App with device code grant enabled
    /// When: Multiple authorization requests
    /// Then: Every user_code is 9 chars (XXXX-XXXX), only contains valid alphabet chars and hyphen
    ///       Two requests produce different device_code values
    #[test_context(SchemaTestContext)]
    #[tokio::test]
    async fn test_scenario_device_user_code_format(ctx: &mut SchemaTestContext) {
        let realm_id = ctx._realm_id.clone();
        let admin_token = setup_admin_session(ctx, "dc-user-code-format@test.com").await;

        // Create client app
        let create_response = create_client_app_with_device_code_grant(
            ctx,
            &realm_id,
            &admin_token,
            "dc-format-app",
            "DC Format App",
            true,
            true,
        )
        .await;
        assert_eq!(create_response.status(), 201);

        // First request
        let response1 = device_authorize(ctx, &realm_id, "dc-format-app").await;
        assert_eq!(response1.status(), 200);
        let json1: Value = response_json(response1).await;

        // Second request
        let response2 = device_authorize(ctx, &realm_id, "dc-format-app").await;
        assert_eq!(response2.status(), 200);
        let json2: Value = response_json(response2).await;

        // Verify user_code format for both responses
        let valid_chars: std::collections::HashSet<char> = "BCDFGHJKMNPQRSTVWXYZ".chars().collect();

        for (label, json_val) in [("first", &json1), ("second", &json2)] {
            let user_code = json_val["user_code"].as_str().unwrap();
            assert_eq!(
                user_code.len(),
                9,
                "{} user_code should be 9 chars, got {}",
                label,
                user_code.len()
            );
            assert_eq!(
                user_code.chars().nth(4),
                Some('-'),
                "{} user_code should have hyphen at position 4",
                label
            );
            for (i, c) in user_code.chars().enumerate() {
                if i != 4 {
                    assert!(
                        valid_chars.contains(&c),
                        "{} user_code: invalid char '{}' at position {}",
                        label,
                        c,
                        i
                    );
                }
            }
        }

        // Verify device_code values are different
        let device_code1 = json1["device_code"].as_str().unwrap();
        let device_code2 = json2["device_code"].as_str().unwrap();
        assert_ne!(
            device_code1, device_code2,
            "Two authorization requests should produce different device_code values"
        );
    }

    // User Story: US-DC-001 Device Authorization Endpoint
    // Covers: Acceptance criteria 3 -- disabled client app returns 403

    /// Test: Device authorization with disabled client app
    ///
    /// Given: Client App with enabled=false
    /// When: POST authorize with that client_id
    /// Then: 403 Forbidden, error = client_app_disabled
    #[test_context(SchemaTestContext)]
    #[tokio::test]
    async fn test_scenario_device_authorization_client_disabled(ctx: &mut SchemaTestContext) {
        let realm_id = ctx._realm_id.clone();
        let admin_token = setup_admin_session(ctx, "dc-client-disabled@test.com").await;

        // Create client app with enabled=false
        let create_response = create_client_app_with_device_code_grant(
            ctx,
            &realm_id,
            &admin_token,
            "dc-disabled-app",
            "DC Disabled App",
            false, // enabled = false
            true,  // device_code_grant_enabled = true
        )
        .await;
        assert_eq!(create_response.status(), 201);

        // Request device authorization
        let response = device_authorize(ctx, &realm_id, "dc-disabled-app").await;
        assert_eq!(
            response.status(),
            403,
            "Disabled client should return 403 Forbidden"
        );

        let json: Value = response_json(response).await;
        assert_eq!(
            json["error"].as_str(),
            Some("client_app_disabled"),
            "error should be 'client_app_disabled', got: {:?}",
            json["error"]
        );
    }

    // User Story: US-DC-001 Device Authorization Endpoint
    // Covers: Acceptance criteria 4 -- nonexistent client returns 401

    /// Test: Device authorization with invalid (nonexistent) client_id
    ///
    /// Given: No Client App with given client_id
    /// When: POST authorize with nonexistent client_id
    /// Then: 401 Unauthorized, error = invalid_client
    #[test_context(SchemaTestContext)]
    #[tokio::test]
    async fn test_scenario_device_authorization_invalid_client(ctx: &mut SchemaTestContext) {
        let realm_id = ctx._realm_id.clone();

        // Request device authorization with a client_id that does not exist
        let response = device_authorize(ctx, &realm_id, "nonexistent-client-id").await;
        assert_eq!(
            response.status(),
            401,
            "Nonexistent client should return 401 Unauthorized"
        );

        let json: Value = response_json(response).await;
        assert_eq!(
            json["error"].as_str(),
            Some("invalid_client"),
            "error should be 'invalid_client', got: {:?}",
            json["error"]
        );
    }

    /// Create an active account row for the token-issuance path. The token
    /// endpoint resolves the device state's user_id to a real account (the
    /// Session-Token family is bound to a user), so tests must seed a real
    /// user id instead of a synthetic string.
    async fn seed_device_user(ctx: &SchemaTestContext, realm_id: &str) -> String {
        let user_id = uuid::Uuid::now_v7();
        sqlx::query(
            "INSERT INTO account (id, realm_id, email, password, status)
             VALUES ($1, $2, $3, '$2a$12$dummy_password_hash', 1)",
        )
        .bind(user_id)
        .bind(realm_id)
        .bind(format!("dc-user-{}@test.com", user_id))
        .execute(&ctx._app_state.pool)
        .await
        .expect("failed to seed device user");
        user_id.to_string()
    }

    // =========================================================================
    // US-DC-003: Token Polling Endpoint Scenarios
    // =========================================================================

    // User Story: US-DC-003 Token Polling Endpoint
    // Covers: Acceptance criteria 1 -- successful token polling returns JWT

    /// Test: Token polling success flow
    ///
    /// Given: Client App with device code grant enabled; authorization created
    ///        and user has authorized (Redis status = authorized)
    /// When: POST /api/device/{realmId}/token with grant_type and device_code
    /// Then: 200 OK, response has non-empty access_token, token_type="Bearer", expires_in
    #[test_context(SchemaTestContext)]
    #[tokio::test]
    async fn test_scenario_device_token_polling_success(ctx: &mut SchemaTestContext) {
        let realm_id = ctx._realm_id.clone();
        let admin_token = setup_admin_session(ctx, "dc-token-success@test.com").await;

        // Create client app with device code grant enabled
        let create_response = create_client_app_with_device_code_grant(
            ctx,
            &realm_id,
            &admin_token,
            "dc-token-success-app",
            "DC Token Success App",
            true,
            true,
        )
        .await;
        assert_eq!(
            create_response.status(),
            201,
            "Client app creation should succeed"
        );

        // Create device authorization
        let auth_response = device_authorize(ctx, &realm_id, "dc-token-success-app").await;
        assert_eq!(
            auth_response.status(),
            200,
            "Device authorization should succeed"
        );

        let auth_json: Value = response_json(auth_response).await;
        let device_code = auth_json["device_code"].as_str().unwrap();

        // Simulate user authorization: set Redis status to "authorized" with a
        // real account id (the token family is user-bound).
        let device_user = seed_device_user(ctx, &realm_id).await;
        set_device_code_status_redis(ctx, device_code, "authorized", Some(&device_user)).await;

        // Poll for token
        let token_response = device_token_poll(ctx, &realm_id, device_code).await;
        assert_eq!(
            token_response.status(),
            200,
            "Token polling should succeed when authorized"
        );

        let token_json: Value = response_json(token_response).await;

        // Verify access_token is present and non-empty
        assert!(
            token_json["access_token"].is_string(),
            "access_token should be a string"
        );
        assert!(
            !token_json["access_token"].as_str().unwrap().is_empty(),
            "access_token should not be empty"
        );

        // Verify token_type
        assert_eq!(
            token_json["token_type"].as_str(),
            Some("Bearer"),
            "token_type should be 'Bearer'"
        );

        // Verify expires_in is present and positive
        assert!(
            token_json["expires_in"].is_number(),
            "expires_in should be a number"
        );
        assert!(
            token_json["expires_in"].as_i64().unwrap() > 0,
            "expires_in should be positive"
        );
    }

    // User Story: US-DC-003 Token Polling Endpoint
    // Covers: Acceptance criteria 2 -- consumed code returns invalid_request

    /// Test: Token polling with already consumed device code
    ///
    /// Given: Device code was already used to obtain a token (status = consumed)
    /// When: POST token with same device_code
    /// Then: 400 Bad Request, error = invalid_request
    #[test_context(SchemaTestContext)]
    #[tokio::test]
    async fn test_scenario_device_token_polling_consumed_code(ctx: &mut SchemaTestContext) {
        let realm_id = ctx._realm_id.clone();
        let admin_token = setup_admin_session(ctx, "dc-token-consumed@test.com").await;

        // Create client app
        let create_response = create_client_app_with_device_code_grant(
            ctx,
            &realm_id,
            &admin_token,
            "dc-token-consumed-app",
            "DC Token Consumed App",
            true,
            true,
        )
        .await;
        assert_eq!(create_response.status(), 201);

        // Create device authorization
        let auth_response = device_authorize(ctx, &realm_id, "dc-token-consumed-app").await;
        assert_eq!(auth_response.status(), 200);
        let auth_json: Value = response_json(auth_response).await;
        let device_code = auth_json["device_code"].as_str().unwrap();

        // Simulate authorization then consume (first poll succeeds); the
        // user must be a real account (the token family is user-bound).
        let device_user = seed_device_user(ctx, &realm_id).await;
        set_device_code_status_redis(ctx, device_code, "authorized", Some(&device_user)).await;
        let first_poll = device_token_poll(ctx, &realm_id, device_code).await;
        assert_eq!(first_poll.status(), 200, "First token poll should succeed");

        // Second poll with consumed code
        let second_poll = device_token_poll(ctx, &realm_id, device_code).await;
        assert_eq!(second_poll.status(), 400, "Consumed code should return 400");

        let json: Value = response_json(second_poll).await;
        assert_eq!(
            json["error"].as_str(),
            Some("invalid_request"),
            "error should be 'invalid_request' for consumed code, got: {:?}",
            json["error"]
        );
    }

    // User Story: US-DC-003 Token Polling Endpoint
    // Covers: Acceptance criteria 3 -- pending authorization returns authorization_pending

    /// Test: Token polling when user has not yet authorized
    ///
    /// Given: Device authorization created, status is still pending
    /// When: POST token
    /// Then: 400 Bad Request, error = authorization_pending
    #[test_context(SchemaTestContext)]
    #[tokio::test]
    async fn test_scenario_device_token_polling_authorization_pending(ctx: &mut SchemaTestContext) {
        let realm_id = ctx._realm_id.clone();
        let admin_token = setup_admin_session(ctx, "dc-token-pending@test.com").await;

        // Create client app
        let create_response = create_client_app_with_device_code_grant(
            ctx,
            &realm_id,
            &admin_token,
            "dc-token-pending-app",
            "DC Token Pending App",
            true,
            true,
        )
        .await;
        assert_eq!(create_response.status(), 201);

        // Create device authorization (status defaults to pending)
        let auth_response = device_authorize(ctx, &realm_id, "dc-token-pending-app").await;
        assert_eq!(auth_response.status(), 200);
        let auth_json: Value = response_json(auth_response).await;
        let device_code = auth_json["device_code"].as_str().unwrap();

        // Poll while still pending
        let poll_response = device_token_poll(ctx, &realm_id, device_code).await;
        assert_eq!(
            poll_response.status(),
            400,
            "Pending authorization should return 400"
        );

        let json: Value = response_json(poll_response).await;
        assert_eq!(
            json["error"].as_str(),
            Some("authorization_pending"),
            "error should be 'authorization_pending', got: {:?}",
            json["error"]
        );
    }

    // User Story: US-DC-003 Token Polling Endpoint
    // Covers: Acceptance criteria 4 -- polling too fast returns slow_down

    /// Test: Token polling too fast returns slow_down
    ///
    /// Given: Device authorization created
    /// When: POST token twice in rapid succession (within interval)
    /// Then: Second returns 400, error = slow_down
    #[test_context(SchemaTestContext)]
    #[tokio::test]
    async fn test_scenario_device_token_polling_slow_down(ctx: &mut SchemaTestContext) {
        let realm_id = ctx._realm_id.clone();
        let admin_token = setup_admin_session(ctx, "dc-token-slowdown@test.com").await;

        // Create client app
        let create_response = create_client_app_with_device_code_grant(
            ctx,
            &realm_id,
            &admin_token,
            "dc-token-slowdown-app",
            "DC Token Slowdown App",
            true,
            true,
        )
        .await;
        assert_eq!(create_response.status(), 201);

        // Create device authorization
        let auth_response = device_authorize(ctx, &realm_id, "dc-token-slowdown-app").await;
        assert_eq!(auth_response.status(), 200);
        let auth_json: Value = response_json(auth_response).await;
        let device_code = auth_json["device_code"].as_str().unwrap();

        // First poll (sets last_poll_at to now)
        let first_poll = device_token_poll(ctx, &realm_id, device_code).await;
        assert_eq!(
            first_poll.status(),
            400,
            "First poll should return 400 (pending)"
        );
        let first_json: Value = response_json(first_poll).await;
        assert_eq!(
            first_json["error"].as_str(),
            Some("authorization_pending"),
            "First poll should be 'authorization_pending'"
        );

        // Second poll immediately (within interval) should trigger slow_down
        let second_poll = device_token_poll(ctx, &realm_id, device_code).await;
        assert_eq!(
            second_poll.status(),
            400,
            "Fast second poll should return 400"
        );

        let json: Value = response_json(second_poll).await;
        assert_eq!(
            json["error"].as_str(),
            Some("slow_down"),
            "error should be 'slow_down', got: {:?}",
            json["error"]
        );
    }

    // User Story: US-DC-003 Token Polling Endpoint
    // Covers: Acceptance criteria 5 -- expired device code returns expired_token

    /// Test: Token polling with expired device code
    ///
    /// Given: Device authorization created, Redis key deleted (simulating expiry)
    /// When: POST token
    /// Then: 400 Bad Request, error = expired_token
    #[test_context(SchemaTestContext)]
    #[tokio::test]
    async fn test_scenario_device_token_polling_expired(ctx: &mut SchemaTestContext) {
        let realm_id = ctx._realm_id.clone();
        let admin_token = setup_admin_session(ctx, "dc-token-expired@test.com").await;

        // Create client app
        let create_response = create_client_app_with_device_code_grant(
            ctx,
            &realm_id,
            &admin_token,
            "dc-token-expired-app",
            "DC Token Expired App",
            true,
            true,
        )
        .await;
        assert_eq!(create_response.status(), 201);

        // Create device authorization
        let auth_response = device_authorize(ctx, &realm_id, "dc-token-expired-app").await;
        assert_eq!(auth_response.status(), 200);
        let auth_json: Value = response_json(auth_response).await;
        let device_code = auth_json["device_code"].as_str().unwrap();
        let user_code = auth_json["user_code"].as_str().unwrap();

        // Simulate expiry by deleting Redis keys
        delete_device_code_redis(ctx, device_code, user_code).await;

        // Poll after expiry
        let poll_response = device_token_poll(ctx, &realm_id, device_code).await;
        assert_eq!(
            poll_response.status(),
            400,
            "Expired code should return 400"
        );

        let json: Value = response_json(poll_response).await;
        assert_eq!(
            json["error"].as_str(),
            Some("expired_token"),
            "error should be 'expired_token', got: {:?}",
            json["error"]
        );
    }

    // User Story: US-DC-003 Token Polling Endpoint
    // Covers: Acceptance criteria 6 -- denied authorization returns access_denied

    /// Test: Token polling when user denied authorization
    ///
    /// Given: Device authorization created, Redis status set to denied
    /// When: POST token
    /// Then: 403 Forbidden, error = access_denied
    #[test_context(SchemaTestContext)]
    #[tokio::test]
    async fn test_scenario_device_token_polling_access_denied(ctx: &mut SchemaTestContext) {
        let realm_id = ctx._realm_id.clone();
        let admin_token = setup_admin_session(ctx, "dc-token-denied@test.com").await;

        // Create client app
        let create_response = create_client_app_with_device_code_grant(
            ctx,
            &realm_id,
            &admin_token,
            "dc-token-denied-app",
            "DC Token Denied App",
            true,
            true,
        )
        .await;
        assert_eq!(create_response.status(), 201);

        // Create device authorization
        let auth_response = device_authorize(ctx, &realm_id, "dc-token-denied-app").await;
        assert_eq!(auth_response.status(), 200);
        let auth_json: Value = response_json(auth_response).await;
        let device_code = auth_json["device_code"].as_str().unwrap();

        // Simulate user denial
        set_device_code_status_redis(ctx, device_code, "denied", None).await;

        // Poll after denial
        let poll_response = device_token_poll(ctx, &realm_id, device_code).await;
        assert_eq!(
            poll_response.status(),
            403,
            "Denied authorization should return 403"
        );

        let json: Value = response_json(poll_response).await;
        assert_eq!(
            json["error"].as_str(),
            Some("access_denied"),
            "error should be 'access_denied', got: {:?}",
            json["error"]
        );
    }

    // =========================================================================
    // US-DC-005 & US-DC-002: Device Verify & Confirm Endpoint Scenarios
    // =========================================================================

    // User Story: US-DC-005 Device Verify & Confirm
    // Covers: Acceptance criteria -- full E2E verify + confirm + token flow

    /// Test: Complete device verify and confirm flow produces a valid token
    ///
    /// Given: Client App with device_code_grant_enabled=true; authorization created; admin session
    /// When: POST verify with session -> 200, client_app_name matches
    ///       POST confirm approved=true -> 200, status="authorized"
    ///       POST token with device_code -> 200, access_token returned
    /// Then: Full E2E flow succeeds
    #[test_context(SchemaTestContext)]
    #[tokio::test]
    async fn test_scenario_device_verify_and_confirm_flow(ctx: &mut SchemaTestContext) {
        let realm_id = ctx._realm_id.clone();
        let admin_token = setup_admin_session(ctx, "dc-verify-flow@test.com").await;

        // Create client app with device code grant enabled
        let create_response = create_client_app_with_device_code_grant(
            ctx,
            &realm_id,
            &admin_token,
            "dc-verify-flow-app",
            "Test CLI App",
            true,
            true,
        )
        .await;
        assert_eq!(
            create_response.status(),
            201,
            "Client app creation should succeed"
        );

        // Create device authorization
        let auth_response = device_authorize(ctx, &realm_id, "dc-verify-flow-app").await;
        assert_eq!(
            auth_response.status(),
            200,
            "Device authorization should succeed"
        );
        let auth_json: Value = response_json(auth_response).await;
        let device_code = auth_json["device_code"].as_str().unwrap().to_string();
        let user_code = auth_json["user_code"].as_str().unwrap().to_string();

        // Step 1: Verify with admin session
        let verify_response = device_verify(ctx, &realm_id, &user_code, &admin_token).await;
        assert_eq!(verify_response.status(), 200, "Verify should return 200");
        let verify_json: Value = response_json(verify_response).await;
        assert_eq!(
            verify_json["client_app_name"].as_str(),
            Some("Test CLI App"),
            "client_app_name should match, got: {:?}",
            verify_json["client_app_name"]
        );

        // Step 2: Confirm approved=true
        let confirm_response = device_confirm(ctx, &realm_id, &user_code, true, &admin_token).await;
        assert_eq!(confirm_response.status(), 200, "Confirm should return 200");
        let confirm_json: Value = response_json(confirm_response).await;
        assert_eq!(
            confirm_json["status"].as_str(),
            Some("authorized"),
            "status should be 'approved', got: {:?}",
            confirm_json["status"]
        );

        // Step 3: Poll for token
        let token_response = device_token_poll(ctx, &realm_id, &device_code).await;
        assert_eq!(token_response.status(), 200, "Token poll should return 200");
        let token_json: Value = response_json(token_response).await;
        assert!(
            token_json["access_token"].is_string(),
            "access_token should be a string"
        );
        assert!(
            !token_json["access_token"].as_str().unwrap().is_empty(),
            "access_token should not be empty"
        );
    }

    // User Story: US-DC-002 Device Verify Errors
    // Covers: Acceptance criteria -- invalid user_code returns 404

    /// Test: Verify with non-existent user_code returns 404
    ///
    /// Given: User with session, no active device auth
    /// When: POST verify with random user_code
    /// Then: 404 Not Found, error = not_found
    #[test_context(SchemaTestContext)]
    #[tokio::test]
    async fn test_scenario_device_verify_invalid_code(ctx: &mut SchemaTestContext) {
        let realm_id = ctx._realm_id.clone();
        let admin_token = setup_admin_session(ctx, "dc-verify-invalid@test.com").await;

        // Verify with a non-existent user_code
        let response = device_verify(ctx, &realm_id, "BBBB-BBBB", &admin_token).await;
        assert_eq!(
            response.status(),
            404,
            "Invalid user_code should return 404"
        );

        let json: Value = response_json(response).await;
        assert_eq!(
            json["error"].as_str(),
            Some("not_found"),
            "error should be 'not_found', got: {:?}",
            json["error"]
        );
    }

    // User Story: US-DC-002 Device Verify Errors
    // Covers: Acceptance criteria -- second user verifying same code returns 409

    /// Test: Verify same user_code by a different user returns 409 already_used
    ///
    /// Given: Authorization created; user A verifies; user B verifies same code
    /// When: User B POST verify
    /// Then: 409 Conflict, error = already_used
    #[test_context(SchemaTestContext)]
    #[tokio::test]
    async fn test_scenario_device_verify_already_used(ctx: &mut SchemaTestContext) {
        let realm_id = ctx._realm_id.clone();
        let admin_token_a = setup_admin_session(ctx, "dc-verify-user-a@test.com").await;

        // Create client app
        let create_response = create_client_app_with_device_code_grant(
            ctx,
            &realm_id,
            &admin_token_a,
            "dc-verify-used-app",
            "DC Verify Used App",
            true,
            true,
        )
        .await;
        assert_eq!(create_response.status(), 201);

        // Create device authorization
        let auth_response = device_authorize(ctx, &realm_id, "dc-verify-used-app").await;
        assert_eq!(auth_response.status(), 200);
        let auth_json: Value = response_json(auth_response).await;
        let user_code = auth_json["user_code"].as_str().unwrap();

        // User A verifies
        let verify_a = device_verify(ctx, &realm_id, user_code, &admin_token_a).await;
        assert_eq!(verify_a.status(), 200, "User A verify should succeed");

        // User B (different session) tries to verify same code
        let (admin_token_b, _user_id_b) =
            create_admin_session_with_user(ctx, "dc-verify-user-b@test.com", 1800).await;
        let verify_b = device_verify(ctx, &realm_id, user_code, &admin_token_b).await;
        assert_eq!(
            verify_b.status(),
            409,
            "User B verify should return 409 Conflict"
        );

        let json: Value = response_json(verify_b).await;
        assert_eq!(
            json["error"].as_str(),
            Some("already_used"),
            "error should be 'already_used', got: {:?}",
            json["error"]
        );
    }

    // User Story: US-DC-002 Device Confirm Errors
    // Covers: Acceptance criteria -- denied confirmation blocks token

    /// Test: Confirm with approved=false denies the authorization
    ///
    /// Given: Authorization created and verified
    /// When: POST confirm approved=false -> 200, status="denied"
    ///       POST token -> 403 access_denied
    /// Then: Denial blocks token issuance
    #[test_context(SchemaTestContext)]
    #[tokio::test]
    async fn test_scenario_device_confirm_denied(ctx: &mut SchemaTestContext) {
        let realm_id = ctx._realm_id.clone();
        let admin_token = setup_admin_session(ctx, "dc-confirm-denied@test.com").await;

        // Create client app
        let create_response = create_client_app_with_device_code_grant(
            ctx,
            &realm_id,
            &admin_token,
            "dc-confirm-denied-app",
            "DC Confirm Denied App",
            true,
            true,
        )
        .await;
        assert_eq!(create_response.status(), 201);

        // Create device authorization
        let auth_response = device_authorize(ctx, &realm_id, "dc-confirm-denied-app").await;
        assert_eq!(auth_response.status(), 200);
        let auth_json: Value = response_json(auth_response).await;
        let device_code = auth_json["device_code"].as_str().unwrap().to_string();
        let user_code = auth_json["user_code"].as_str().unwrap();

        // Verify first
        let verify_response = device_verify(ctx, &realm_id, user_code, &admin_token).await;
        assert_eq!(verify_response.status(), 200, "Verify should succeed");

        // Confirm denied
        let confirm_response = device_confirm(ctx, &realm_id, user_code, false, &admin_token).await;
        assert_eq!(confirm_response.status(), 200, "Confirm should return 200");
        let confirm_json: Value = response_json(confirm_response).await;
        assert_eq!(
            confirm_json["status"].as_str(),
            Some("denied"),
            "status should be 'denied', got: {:?}",
            confirm_json["status"]
        );

        // Token polling should return access_denied
        let token_response = device_token_poll(ctx, &realm_id, &device_code).await;
        assert_eq!(
            token_response.status(),
            403,
            "Token poll after denial should return 403"
        );

        let json: Value = response_json(token_response).await;
        assert_eq!(
            json["error"].as_str(),
            Some("access_denied"),
            "error should be 'access_denied', got: {:?}",
            json["error"]
        );
    }

    // User Story: US-DC-002 Device Verify Errors
    // Covers: Acceptance criteria -- unauthenticated verify returns 401

    /// Test: Verify without session returns 401
    ///
    /// Given: No session cookie
    /// When: POST verify without an Authorization Bearer token
    /// Then: 401 Unauthorized
    #[test_context(SchemaTestContext)]
    #[tokio::test]
    async fn test_scenario_device_verify_without_session(ctx: &mut SchemaTestContext) {
        let realm_id = ctx._realm_id.clone();

        // Verify without session token (empty string means no cookie header)
        let app = ctx.create_unified_test_router();
        let request = axum::http::Request::builder()
            .method("POST")
            .uri(format!("/api/device/{}/verify", realm_id))
            .header("content-type", "application/json")
            .body(axum::body::Body::from(
                serde_json::json!({ "user_code": "BBBB-BBBB" }).to_string(),
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(
            response.status(),
            401,
            "Verify without session should return 401"
        );
    }

    // User Story: US-DC-002 Device Confirm Errors
    // Covers: Acceptance criteria -- confirm without prior verify returns 400

    /// Test: Confirm without prior verify returns 400
    ///
    /// Given: Authorization created; user has session but has NOT called verify
    /// When: POST confirm
    /// Then: 400 Bad Request, error = invalid_request
    #[test_context(SchemaTestContext)]
    #[tokio::test]
    async fn test_scenario_device_confirm_without_prior_verify(ctx: &mut SchemaTestContext) {
        let realm_id = ctx._realm_id.clone();
        let admin_token = setup_admin_session(ctx, "dc-confirm-no-verify@test.com").await;

        // Create client app
        let create_response = create_client_app_with_device_code_grant(
            ctx,
            &realm_id,
            &admin_token,
            "dc-confirm-nv-app",
            "DC Confirm No Verify App",
            true,
            true,
        )
        .await;
        assert_eq!(create_response.status(), 201);

        // Create device authorization
        let auth_response = device_authorize(ctx, &realm_id, "dc-confirm-nv-app").await;
        assert_eq!(auth_response.status(), 200);
        let auth_json: Value = response_json(auth_response).await;
        let user_code = auth_json["user_code"].as_str().unwrap();

        // Confirm directly without calling verify first
        let confirm_response = device_confirm(ctx, &realm_id, user_code, true, &admin_token).await;
        assert_eq!(
            confirm_response.status(),
            400,
            "Confirm without verify should return 400"
        );

        let json: Value = response_json(confirm_response).await;
        assert_eq!(
            json["error"].as_str(),
            Some("invalid_request"),
            "error should be 'invalid_request', got: {:?}",
            json["error"]
        );
    }

    // =========================================================================
    // US-DC-004: Grant Config Toggle & Regression Scenarios
    // =========================================================================

    // User Story: US-DC-004 Grant Config Toggle
    // Covers: Acceptance criteria -- toggle deviceCodeGrantEnabled changes authorize behavior

    /// Test: Device code grant config toggle
    ///
    /// Step A: Create Client App with default (device_code_grant_enabled=false);
    ///          POST authorize -> 403 unauthorized_client
    /// Step B: Update Client App with deviceCodeGrantEnabled=true;
    ///          POST authorize -> 200
    /// Step C: Update Client App with deviceCodeGrantEnabled=false;
    ///          POST authorize -> 403 unauthorized_client
    #[test_context(SchemaTestContext)]
    #[tokio::test]
    async fn test_scenario_device_code_grant_config_toggle(ctx: &mut SchemaTestContext) {
        let realm_id = ctx._realm_id.clone();
        let admin_token = setup_admin_session(ctx, "dc-grant-toggle@test.com").await;

        // Step A: Create Client App with default (device_code_grant_enabled not set = false)
        let create_response = create_client_app_with_device_code_grant(
            ctx,
            &realm_id,
            &admin_token,
            "dc-grant-toggle-app",
            "DC Grant Toggle App",
            true,
            false, // device_code_grant_enabled = false
        )
        .await;
        assert_eq!(
            create_response.status(),
            201,
            "Client app creation should succeed"
        );

        // Get the app UUID from the create response for updates
        let create_json: Value = response_json(create_response).await;
        let app_id = create_json["id"].as_str().unwrap();

        // Step A: Authorize should fail with 403 unauthorized_client
        let auth_a = device_authorize(ctx, &realm_id, "dc-grant-toggle-app").await;
        assert_eq!(
            auth_a.status(),
            403,
            "Step A: authorize should return 403 when grant disabled"
        );
        let json_a: Value = response_json(auth_a).await;
        assert_eq!(
            json_a["error"].as_str(),
            Some("unauthorized_client"),
            "Step A: error should be 'unauthorized_client', got: {:?}",
            json_a["error"]
        );

        // Step B: Update Client App with deviceCodeGrantEnabled=true
        let app = ctx.create_unified_test_router();
        let update_b = axum::http::Request::builder()
            .method("PUT")
            .uri(format!("/api/client/{}/{}", realm_id, app_id))
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {}", admin_token))
            .body(axum::body::Body::from(
                json!({
                    "name": "DC Grant Toggle App",
                    "description": "Updated for grant toggle",
                    "redirectUris": ["https://example.com/callback"],
                    "enabled": true,
                    "browserRefreshAbsoluteTtlSeconds": 86400,
                    "deviceCodeGrantEnabled": true
                })
                .to_string(),
            ))
            .unwrap();
        let update_b_resp = app.oneshot(update_b).await.unwrap();
        assert_eq!(update_b_resp.status(), 200, "Step B: update should succeed");

        // Step B: Authorize should succeed with 200
        let auth_b = device_authorize(ctx, &realm_id, "dc-grant-toggle-app").await;
        assert_eq!(
            auth_b.status(),
            200,
            "Step B: authorize should return 200 when grant enabled"
        );

        // Step C: Update Client App with deviceCodeGrantEnabled=false
        let app2 = ctx.create_unified_test_router();
        let update_c = axum::http::Request::builder()
            .method("PUT")
            .uri(format!("/api/client/{}/{}", realm_id, app_id))
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {}", admin_token))
            .body(axum::body::Body::from(
                json!({
                    "name": "DC Grant Toggle App",
                    "description": "Updated for grant toggle",
                    "redirectUris": ["https://example.com/callback"],
                    "enabled": true,
                    "browserRefreshAbsoluteTtlSeconds": 86400,
                    "deviceCodeGrantEnabled": false
                })
                .to_string(),
            ))
            .unwrap();
        let update_c_resp = app2.oneshot(update_c).await.unwrap();
        assert_eq!(update_c_resp.status(), 200, "Step C: update should succeed");

        // Step C: Authorize should fail with 403 unauthorized_client
        let auth_c = device_authorize(ctx, &realm_id, "dc-grant-toggle-app").await;
        assert_eq!(
            auth_c.status(),
            403,
            "Step C: authorize should return 403 when grant disabled again"
        );
        let json_c: Value = response_json(auth_c).await;
        assert_eq!(
            json_c["error"].as_str(),
            Some("unauthorized_client"),
            "Step C: error should be 'unauthorized_client', got: {:?}",
            json_c["error"]
        );
    }

    // User Story: US-DC-004 Grant Config Toggle
    // Covers: Acceptance criteria -- default is device_code_grant_enabled=false

    /// Test: Device code grant is disabled by default for new client apps
    ///
    /// Given: Client App created without explicit deviceCodeGrantEnabled
    /// When: POST authorize
    /// Then: 403 unauthorized_client
    #[test_context(SchemaTestContext)]
    #[tokio::test]
    async fn test_scenario_device_code_default_disabled(ctx: &mut SchemaTestContext) {
        let realm_id = ctx._realm_id.clone();
        let admin_token = setup_admin_session(ctx, "dc-default-disabled@test.com").await;

        // Create client app without explicit deviceCodeGrantEnabled (omit the field)
        let app = ctx.create_unified_test_router();
        let create_request = axum::http::Request::builder()
            .method("POST")
            .uri(format!("/api/client/{}", realm_id))
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {}", admin_token))
            .body(axum::body::Body::from(
                json!({
                    "clientId": "dc-default-app",
                    "name": "DC Default App",
                    "description": "App without explicit deviceCodeGrantEnabled",
                    "redirectUris": ["https://example.com/callback"],
                    "enabled": true,
                    "browserRefreshAbsoluteTtlSeconds": 86400
                })
                .to_string(),
            ))
            .unwrap();
        let create_response = app.oneshot(create_request).await.unwrap();
        assert_eq!(
            create_response.status(),
            201,
            "Client app creation should succeed"
        );

        // POST authorize should fail with 403 unauthorized_client
        let auth_response = device_authorize(ctx, &realm_id, "dc-default-app").await;
        assert_eq!(
            auth_response.status(),
            403,
            "Default device_code_grant should be disabled"
        );
        let json: Value = response_json(auth_response).await;
        assert_eq!(
            json["error"].as_str(),
            Some("unauthorized_client"),
            "error should be 'unauthorized_client', got: {:?}",
            json["error"]
        );
    }

    // Regression: Ensure device code route registration does not break existing OAuth routes

    /// Test: Existing OAuth routes remain reachable after device code route registration
    ///
    /// Given: Device code routes are registered in the router
    /// When: GET /api/oauth/{realmId}/google/login
    /// Then: Returns a response (not a 404/405 from router misconfiguration)
    #[test_context(SchemaTestContext)]
    #[tokio::test]
    async fn test_scenario_device_code_oauth_regression(ctx: &mut SchemaTestContext) {
        let realm_id = ctx._realm_id.clone();

        // Call an existing OAuth route to verify it's still reachable
        let app = ctx.create_unified_test_router();
        let request = axum::http::Request::builder()
            .method("GET")
            .uri(format!("/api/oauth/{}/google/login", realm_id))
            .body(axum::body::Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.unwrap();

        // The route exists even without OAuth config. It may return 404 (no provider configured)
        // or a redirect. The key is it does not return 405 (method not allowed) or 500
        // from a router conflict with device code routes.
        let status = response.status();
        assert!(
            status.as_u16() != 405,
            "OAuth login route should not return 405 Method Not Allowed (router conflict), got: {}",
            status
        );
        // We accept 302 (redirect), 404 (no provider configured), or other non-405 statuses
        assert!(
            status.as_u16() != 500 || status.as_u16() == 404 || status.as_u16() == 302,
            "OAuth login route should be reachable, got status: {}",
            status
        );
    }

    // User Story: US-DC-001 Device Authorization Endpoint
    // Covers: Edge case -- missing client_id returns 400 invalid_request

    /// Test: Device authorize with missing client_id returns 400
    ///
    /// Given: No client_id in the request body
    /// When: POST /api/device/{realmId}/authorize with empty form body
    /// Then: 422 Unprocessable Entity (form deserialization fails for missing required field)
    #[test_context(SchemaTestContext)]
    #[tokio::test]
    async fn test_scenario_device_authorize_missing_client_id(ctx: &mut SchemaTestContext) {
        let realm_id = ctx._realm_id.clone();

        // POST authorize with empty body (no client_id)
        let app = ctx.create_unified_test_router();
        let request = axum::http::Request::builder()
            .method("POST")
            .uri(format!("/api/device/{}/authorize", realm_id))
            .header("content-type", "application/x-www-form-urlencoded")
            .body(axum::body::Body::from(""))
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(
            response.status(),
            422,
            "Missing client_id should return 422"
        );
    }

    // User Story: US-DC-003 Token Polling Endpoint
    // Covers: Edge case -- wrong grant_type returns 400 invalid_request

    /// Test: Device token with invalid grant_type returns 400
    ///
    /// Given: Device authorization created with valid client
    /// When: POST token with grant_type=authorization_code instead of device_code grant type
    /// Then: 400 Bad Request, error = invalid_request
    #[test_context(SchemaTestContext)]
    #[tokio::test]
    async fn test_scenario_device_token_invalid_grant_type(ctx: &mut SchemaTestContext) {
        let realm_id = ctx._realm_id.clone();
        let admin_token = setup_admin_session(ctx, "dc-invalid-grant@test.com").await;

        // Create client app with device code grant enabled
        let create_response = create_client_app_with_device_code_grant(
            ctx,
            &realm_id,
            &admin_token,
            "dc-invalid-grant-app",
            "DC Invalid Grant App",
            true,
            true,
        )
        .await;
        assert_eq!(
            create_response.status(),
            201,
            "Client app creation should succeed"
        );

        // Create device authorization to get a device_code
        let auth_response = device_authorize(ctx, &realm_id, "dc-invalid-grant-app").await;
        assert_eq!(
            auth_response.status(),
            200,
            "Device authorization should succeed"
        );
        let auth_json: Value = response_json(auth_response).await;
        let device_code = auth_json["device_code"].as_str().unwrap();

        // POST token with wrong grant_type
        let app = ctx.create_unified_test_router();
        let request = axum::http::Request::builder()
            .method("POST")
            .uri(format!("/api/device/{}/token", realm_id))
            .header("content-type", "application/x-www-form-urlencoded")
            .body(axum::body::Body::from(format!(
                "grant_type=authorization_code&device_code={}",
                device_code
            )))
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), 400, "Wrong grant_type should return 400");

        let json: Value = response_json(response).await;
        assert_eq!(
            json["error"].as_str(),
            Some("invalid_request"),
            "error should be 'invalid_request', got: {:?}",
            json["error"]
        );
    }
}
