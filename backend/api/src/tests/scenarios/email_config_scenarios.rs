// =============================================================================
// Email Config CRUD & Status API Scenarios
// =============================================================================
//
// Scenario tests for per-realm email configuration, covering:
// - Email status endpoint when unconfigured
// - Email status endpoint with complete Resend config
// - Email status endpoint with complete SMTP config
// - Email status endpoint with partial config (missing fields detection)
// - Batch upsert of email config followed by status verification
//
// User Stories: US-RA-013 (Configure Email Service), US-RA-014 (Send Test Email)
// PRD: docs/prd/core/realm-settings.md Section 2.2.4
// =============================================================================

use crate::tests::helpers::email_config_helpers::*;
use crate::tests::helpers::*;
use crate::tests::schema_test_context::SchemaTestContext as TestContext;
use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use serde_json::json;
use test_context::test_context;
use tower::ServiceExt;

// User Story: docs/user-stories/02-realm-admin-user-stories.md US-RA-013
// Covers: US-RA-013 Scenario 1 pre-condition — no email config means status shows unconfigured
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_email_status_unconfigured_returns_not_configured(ctx: &mut TestContext) {
    // Given: a realm admin with no email configuration
    let app = ctx.create_unified_test_router();
    let (token, user_id) =
        create_admin_session_with_user(ctx, "email-unconfigured@test.com", 1800).await;
    grant_realm_admin_role(ctx, &user_id).await;

    // Ensure clean slate
    delete_email_config_direct(&ctx._app_state.pool, &ctx._realm_id).await;

    // When: requesting email status
    let body = get_email_status(&app, &ctx._realm_id, &token).await;

    // Then: configured is false, provider is null, missing_fields is non-empty
    assert_eq!(body["configured"], false, "should not be configured");
    assert!(
        body["provider"].is_null(),
        "provider should be null when unconfigured"
    );
    assert!(
        body["fromAddress"].is_null(),
        "fromAddress should be null when unconfigured"
    );
    let missing = body["missingFields"]
        .as_array()
        .expect("missingFields should be an array");
    assert!(
        !missing.is_empty(),
        "missingFields should be non-empty when unconfigured"
    );
}

// User Story: docs/user-stories/02-realm-admin-user-stories.md US-RA-013
// Covers: US-RA-013 Scenario 1 — Resend config shows configured=true with provider details
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_email_status_resend_configured_returns_complete(ctx: &mut TestContext) {
    // Given: a realm admin with complete Resend email configuration
    let app = ctx.create_unified_test_router();
    let (token, user_id) = create_admin_session_with_user(ctx, "email-resend@test.com", 1800).await;
    grant_realm_admin_role(ctx, &user_id).await;

    insert_resend_email_config_direct(&ctx._app_state.pool, &ctx._realm_id).await;

    // When: requesting email status
    let body = get_email_status(&app, &ctx._realm_id, &token).await;

    // Then: configured=true, provider="resend", fromAddress set, missingFields empty
    assert_eq!(body["configured"], true, "should be configured with Resend");
    assert_eq!(body["provider"], "resend", "provider should be resend");
    assert_eq!(
        body["fromAddress"], "noreply@example.com",
        "fromAddress should match inserted value"
    );
    let missing = body["missingFields"]
        .as_array()
        .expect("missingFields should be an array");
    assert!(
        missing.is_empty(),
        "missingFields should be empty when fully configured, got: {:?}",
        missing
    );
}

// User Story: docs/user-stories/02-realm-admin-user-stories.md US-RA-013
// Covers: US-RA-013 Scenario 2 — SMTP config shows configured=true with provider details
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_email_status_smtp_configured_returns_complete(ctx: &mut TestContext) {
    // Given: a realm admin with complete SMTP email configuration
    let app = ctx.create_unified_test_router();
    let (token, user_id) = create_admin_session_with_user(ctx, "email-smtp@test.com", 1800).await;
    grant_realm_admin_role(ctx, &user_id).await;

    insert_smtp_email_config_direct(&ctx._app_state.pool, &ctx._realm_id).await;

    // When: requesting email status
    let body = get_email_status(&app, &ctx._realm_id, &token).await;

    // Then: configured=true, provider="smtp", fromAddress set, missingFields empty
    assert_eq!(body["configured"], true, "should be configured with SMTP");
    assert_eq!(body["provider"], "smtp", "provider should be smtp");
    assert_eq!(
        body["fromAddress"], "notify@company.com",
        "fromAddress should match inserted SMTP value"
    );
    let missing = body["missingFields"]
        .as_array()
        .expect("missingFields should be an array");
    assert!(
        missing.is_empty(),
        "missingFields should be empty when SMTP fully configured, got: {:?}",
        missing
    );
}

// User Story: docs/user-stories/02-realm-admin-user-stories.md US-RA-013
// Covers: US-RA-013 Scenario 3 — partial config shows configured=false with specific missing fields
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_email_status_partial_config_returns_missing_fields(ctx: &mut TestContext) {
    // Given: a realm admin with partial SMTP config (missing username and password)
    let app = ctx.create_unified_test_router();
    let (token, user_id) =
        create_admin_session_with_user(ctx, "email-partial@test.com", 1800).await;
    grant_realm_admin_role(ctx, &user_id).await;

    insert_partial_smtp_email_config_direct(&ctx._app_state.pool, &ctx._realm_id).await;

    // When: requesting email status
    let body = get_email_status(&app, &ctx._realm_id, &token).await;

    // Then: configured=false, provider="smtp", missingFields lists username and password
    assert_eq!(
        body["configured"], false,
        "should not be configured with partial SMTP"
    );
    assert_eq!(body["provider"], "smtp", "provider should still be smtp");
    assert_eq!(
        body["fromAddress"], "partial@company.com",
        "fromAddress should reflect inserted value"
    );
    let missing = body["missingFields"]
        .as_array()
        .expect("missingFields should be an array");
    let missing_str: Vec<String> = missing
        .iter()
        .map(|v| v.as_str().unwrap_or("").to_string())
        .collect();

    assert!(
        missing_str.contains(&"smtp_username".to_string()),
        "missingFields should include smtp_username, got: {:?}",
        missing_str
    );
    assert!(
        missing_str.contains(&"smtp_password".to_string()),
        "missingFields should include smtp_password, got: {:?}",
        missing_str
    );
}

// User Story: docs/user-stories/02-realm-admin-user-stories.md US-RA-013
// Covers: US-RA-013 Scenario 1 — save Resend config via batch upsert API, then verify status
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_email_config_save_via_batch_upsert(ctx: &mut TestContext) {
    // Given: a realm admin with no email config
    let app = ctx.create_unified_test_router();
    let (token, user_id) =
        create_admin_session_with_user(ctx, "email-batch-upsert@test.com", 1800).await;
    grant_realm_admin_role(ctx, &user_id).await;

    delete_email_config_direct(&ctx._app_state.pool, &ctx._realm_id).await;

    // When: saving Resend config via batch upsert API
    let batch_payload = json!({
        "configs": [
            {
                "configType": "email",
                "configKey": "provider",
                "configValue": "resend",
                "isSecret": false,
                "enabled": true
            },
            {
                "configType": "email",
                "configKey": "from_address",
                "configValue": "batch-upsert@example.com",
                "isSecret": false,
                "enabled": true
            },
            {
                "configType": "email",
                "configKey": "resend_api_key",
                "configValue": "re_batch_test_key_67890",
                "isSecret": true,
                "enabled": true
            }
        ]
    });

    let req = Request::builder()
        .method("POST")
        .uri(format!(
            "/api/configs/{realmId}/batch",
            realmId = ctx._realm_id
        ))
        .header("content-type", "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::from(batch_payload.to_string()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "batch upsert should return 200"
    );

    // Then: email status shows configured=true with Resend provider
    let body = get_email_status(&app, &ctx._realm_id, &token).await;
    assert_eq!(
        body["configured"], true,
        "should be configured after batch upsert"
    );
    assert_eq!(body["provider"], "resend", "provider should be resend");
    assert_eq!(
        body["fromAddress"], "batch-upsert@example.com",
        "fromAddress should match batch-upserted value"
    );
    let missing = body["missingFields"]
        .as_array()
        .expect("missingFields should be an array");
    assert!(
        missing.is_empty(),
        "missingFields should be empty after complete batch upsert, got: {:?}",
        missing
    );

    // SMTP/Resend writes are "关键配置变更" (audit PRD): every persisted email
    // row (including the credential) must leave a `realm_config.update` audit
    // trail — the non-payment counterpart of `payment_config.update`.
    let audited_keys: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events
         WHERE realm_id = $1 AND action = 'realm_config.update'
           AND details->>'config_type' = 'email'
           AND details->>'config_key' = ANY($2)",
    )
    .bind(&ctx._realm_id)
    .bind(["provider", "from_address", "resend_api_key"])
    .fetch_one(&ctx._app_state.pool)
    .await
    .unwrap();
    assert_eq!(
        audited_keys, 3,
        "each batch-upserted email config row must be audited (incl. the API key)"
    );
}

// =============================================================================
// Feature Switch Validation & Regression Scenarios
// =============================================================================
//
// Scenario tests for email-dependent feature switches and regression coverage:
// - Enabling email verification rejected when no email config
// - Enabling email verification succeeds when email is configured
// - Auto-disable: registration succeeds as active when verification required but no email config
// - Regression: registration without email config succeeds
// - Regression: password reset without email config does not error
//
// User Stories: US-RA-015 (email-dependent feature switch), US-RU-001 (registration)
// =============================================================================

// User Story: docs/user-stories/02-realm-admin-user-stories.md US-RA-015
// Covers: US-RA-015 scenario 1 -- cannot enable email verification without email config
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_enable_email_verification_without_email_config_rejected(
    ctx: &mut TestContext,
) {
    // Given: no email config, registration enabled = true via SQL
    let app = ctx.create_unified_test_router();
    let (token, user_id) =
        create_admin_session_with_user(ctx, "switch-no-email@test.com", 1800).await;
    grant_realm_admin_role(ctx, &user_id).await;

    delete_email_config_direct(&ctx._app_state.pool, &ctx._realm_id).await;

    sqlx::query(
        "INSERT INTO realm_config (realm_id, config_type, config_key, config_value, enabled)
         VALUES ($1, 'registration', 'enabled', 'true', true)
         ON CONFLICT (realm_id, config_type, config_key)
         DO UPDATE SET config_value = EXCLUDED.config_value, enabled = EXCLUDED.enabled",
    )
    .bind(&ctx._realm_id)
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to enable registration");

    // When: batch upsert with require_email_verification = true
    let payload = json!({
        "configs": [{
            "configType": "registration",
            "configKey": "require_email_verification",
            "configValue": "true",
            "isSecret": false,
            "enabled": true
        }]
    });

    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/configs/{}/batch", ctx._realm_id))
        .header("content-type", "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::from(payload.to_string()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();

    // Then: 400 Bad Request with message about missing email configuration
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "should reject enabling email verification without email config"
    );
    let body: serde_json::Value = crate::tests::response_json(resp).await;
    assert!(
        body["message"]
            .as_str()
            .unwrap_or("")
            .contains("Cannot enable email verification without email configuration"),
        "error message should mention missing email configuration, got: {:?}",
        body
    );
}

// User Story: docs/user-stories/02-realm-admin-user-stories.md US-RA-015
// Covers: US-RA-015 scenario 2 -- can enable after email is configured
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_enable_email_verification_with_email_config_succeeds(ctx: &mut TestContext) {
    // Given: complete Resend email config, registration enabled = true
    let app = ctx.create_unified_test_router();
    let (token, user_id) =
        create_admin_session_with_user(ctx, "switch-with-email@test.com", 1800).await;
    grant_realm_admin_role(ctx, &user_id).await;

    insert_resend_email_config_direct(&ctx._app_state.pool, &ctx._realm_id).await;

    sqlx::query(
        "INSERT INTO realm_config (realm_id, config_type, config_key, config_value, enabled)
         VALUES ($1, 'registration', 'enabled', 'true', true)
         ON CONFLICT (realm_id, config_type, config_key)
         DO UPDATE SET config_value = EXCLUDED.config_value, enabled = EXCLUDED.enabled",
    )
    .bind(&ctx._realm_id)
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to enable registration");

    // When: batch upsert with require_email_verification = true
    let payload = json!({
        "configs": [{
            "configType": "registration",
            "configKey": "require_email_verification",
            "configValue": "true",
            "isSecret": false,
            "enabled": true
        }]
    });

    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/configs/{}/batch", ctx._realm_id))
        .header("content-type", "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::from(payload.to_string()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();

    // Then: 200 OK
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "should accept enabling email verification when email is configured"
    );
}

// User Story: docs/user-stories/02-realm-admin-user-stories.md US-RA-015
// Covers: US-RA-015 scenario 3 -- auto-disable when no email config;
//         require_email_verification=true in DB but no email config means user registered as active
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_is_email_verification_required_returns_false_without_config(
    ctx: &mut TestContext,
) {
    // Given: require_email_verification = true via SQL, BUT no email config
    let app = ctx.create_unified_test_router();

    delete_email_config_direct(&ctx._app_state.pool, &ctx._realm_id).await;

    sqlx::query(
        "INSERT INTO realm_config (realm_id, config_type, config_key, config_value, enabled)
         VALUES ($1, 'registration', 'enabled', 'true', true)
         ON CONFLICT (realm_id, config_type, config_key)
         DO UPDATE SET config_value = EXCLUDED.config_value, enabled = EXCLUDED.enabled",
    )
    .bind(&ctx._realm_id)
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to enable registration");

    sqlx::query(
        "INSERT INTO realm_config (realm_id, config_type, config_key, config_value, enabled)
         VALUES ($1, 'registration', 'require_email_verification', 'true', true)
         ON CONFLICT (realm_id, config_type, config_key)
         DO UPDATE SET config_value = EXCLUDED.config_value, enabled = EXCLUDED.enabled",
    )
    .bind(&ctx._realm_id)
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to set require_email_verification=true");

    // When: register with valid email/password (Turnstile skipped in test)
    let email = "auto-disable-verif@test.com";
    let payload = json!({
        "clientId": ctx._client_id,
        "email": email,
        "password": "password123",
        "turnstileToken": "dummy"
    });

    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{}/register", ctx._realm_id))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "3.3.3.3")
        .body(Body::from(payload.to_string()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();

    // Then: 200 OK, user created with status active (1), not pending verification
    assert_eq!(resp.status(), StatusCode::OK, "registration should succeed");

    let body: serde_json::Value = crate::tests::response_json(resp).await;
    assert_eq!(
        body["verificationRequired"], false,
        "verificationRequired should be false when email not configured"
    );

    let status: i16 =
        sqlx::query_scalar("SELECT status FROM account WHERE realm_id = $1 AND email = $2")
            .bind(&ctx._realm_id)
            .bind(email)
            .fetch_one(&ctx._app_state.pool)
            .await
            .expect("Failed to fetch user status");

    assert_eq!(
        status, 1,
        "user should be active (status=1), not pending verification"
    );

    // Cleanup
    sqlx::query("DELETE FROM account WHERE email = $1")
        .bind(email)
        .execute(&ctx._app_state.pool)
        .await
        .unwrap();
}

// User Story: docs/user-stories/03-regular-user-user-stories.md US-RU-001
// Covers: US-RU-001 scenario 1a -- registration without email verification (regression)
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_registration_without_email_config_succeeds(ctx: &mut TestContext) {
    // Given: registration enabled = true, no email config, no require_email_verification
    let app = ctx.create_unified_test_router();

    delete_email_config_direct(&ctx._app_state.pool, &ctx._realm_id).await;

    sqlx::query(
        "INSERT INTO realm_config (realm_id, config_type, config_key, config_value, enabled)
         VALUES ($1, 'registration', 'enabled', 'true', true)
         ON CONFLICT (realm_id, config_type, config_key)
         DO UPDATE SET config_value = EXCLUDED.config_value, enabled = EXCLUDED.enabled",
    )
    .bind(&ctx._realm_id)
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to enable registration");

    // When: register with valid email/password
    let email = "regression-reg@test.com";
    let payload = json!({
        "clientId": ctx._client_id,
        "email": email,
        "password": "password123",
        "turnstileToken": "dummy"
    });

    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{}/register", ctx._realm_id))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "3.3.3.3")
        .body(Body::from(payload.to_string()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();

    // Then: 200 OK, user is active
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "registration should succeed without email config"
    );

    let status: i16 =
        sqlx::query_scalar("SELECT status FROM account WHERE realm_id = $1 AND email = $2")
            .bind(&ctx._realm_id)
            .bind(email)
            .fetch_one(&ctx._app_state.pool)
            .await
            .expect("Failed to fetch user status");

    assert_eq!(status, 1, "user should be active (status=1)");

    // Cleanup
    sqlx::query("DELETE FROM account WHERE email = $1")
        .bind(email)
        .execute(&ctx._app_state.pool)
        .await
        .unwrap();
}

// Regression: reset_password.rs affected by AppState.resend removal
// Covers: best-effort email sending should not error when email not configured
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_password_reset_without_email_config_does_not_error(ctx: &mut TestContext) {
    // Given: a registered user with no email config on the realm
    let app = ctx.create_unified_test_router();

    delete_email_config_direct(&ctx._app_state.pool, &ctx._realm_id).await;

    // Enable registration and create a user
    sqlx::query(
        "INSERT INTO realm_config (realm_id, config_type, config_key, config_value, enabled)
         VALUES ($1, 'registration', 'enabled', 'true', true)
         ON CONFLICT (realm_id, config_type, config_key)
         DO UPDATE SET config_value = EXCLUDED.config_value, enabled = EXCLUDED.enabled",
    )
    .bind(&ctx._realm_id)
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to enable registration");

    let email = "reset-no-email@test.com";
    let register_payload = json!({
        "clientId": ctx._client_id,
        "email": email,
        "password": "password123",
        "turnstileToken": "dummy"
    });

    let register_req = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{}/register", ctx._realm_id))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "3.3.3.3")
        .body(Body::from(register_payload.to_string()))
        .unwrap();

    let register_resp = app.clone().oneshot(register_req).await.unwrap();
    assert_eq!(
        register_resp.status(),
        StatusCode::OK,
        "user registration should succeed"
    );

    // When: POST reset_password/request with the user's email
    let reset_payload = json!({
        "clientId": ctx._client_id,
        "email": email,
        "turnstileToken": "dummy"
    });

    let req = Request::builder()
        .method("POST")
        .uri(format!(
            "/api/auth/{}/reset_password/request",
            ctx._realm_id
        ))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "4.4.4.4")
        .body(Body::from(reset_payload.to_string()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();

    // Then: 200 OK, response body contains "message": "ok"
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "reset password request should succeed even without email config"
    );
    let body: serde_json::Value = crate::tests::response_json(resp).await;
    assert_eq!(body["message"], "ok", "response message should be 'ok'");

    // Cleanup
    sqlx::query("DELETE FROM account WHERE email = $1")
        .bind(email)
        .execute(&ctx._app_state.pool)
        .await
        .unwrap();
}

// =============================================================================
// Email Test API Scenarios
// =============================================================================
//
// Scenario tests for the email test endpoint:
// - Test email rejected when email not configured
// - Test email rejected with invalid recipient
// - Test email returns response when configured (dummy key causes send failure)
// - Test email rate limiting (3 per 60 seconds)
//
// User Stories: US-RA-014 (Send Test Email)
// =============================================================================

// User Story: docs/user-stories/02-realm-admin-user-stories.md US-RA-014
// Covers: US-RA-014 scenario 2 -- email not configured, cannot send test email
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_email_test_unconfigured_returns_400(ctx: &mut TestContext) {
    // Given: no email config, admin session
    let app = ctx.create_unified_test_router();
    let (token, user_id) =
        create_admin_session_with_user(ctx, "email-test-noconfig@test.com", 1800).await;
    grant_realm_admin_role(ctx, &user_id).await;

    delete_email_config_direct(&ctx._app_state.pool, &ctx._realm_id).await;

    // When: POST /api/configs/{realmId}/email/test with valid recipient
    let resp = send_test_email(&app, &ctx._realm_id, &token, "test@example.com").await;

    // Then: 400 Bad Request (email not configured)
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "should return 400 when email is not configured"
    );
}

// User Story: docs/user-stories/02-realm-admin-user-stories.md US-RA-014
// Covers: Email test API validation -- invalid recipient email
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_email_test_invalid_recipient_returns_400(ctx: &mut TestContext) {
    // Given: complete Resend email config via direct SQL
    let app = ctx.create_unified_test_router();
    let (token, user_id) =
        create_admin_session_with_user(ctx, "email-test-invalid@test.com", 1800).await;
    grant_realm_admin_role(ctx, &user_id).await;

    insert_resend_email_config_direct(&ctx._app_state.pool, &ctx._realm_id).await;

    // When: POST /api/configs/{realmId}/email/test with invalid recipient
    let resp = send_test_email(&app, &ctx._realm_id, &token, "not-an-email").await;

    // Then: 400 Bad Request (invalid recipient format)
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "should return 400 for invalid recipient email"
    );
}

// User Story: docs/user-stories/02-realm-admin-user-stories.md US-RA-014
// Covers: US-RA-014 scenario 1 (test email with configured service)
//         and scenario 3 (send failure handling with dummy key)
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_email_test_configured_returns_response(ctx: &mut TestContext) {
    // Given: complete Resend config with dummy API key
    let app = ctx.create_unified_test_router();
    let (token, user_id) =
        create_admin_session_with_user(ctx, "email-test-ok@test.com", 1800).await;
    grant_realm_admin_role(ctx, &user_id).await;

    // Insert config with dummy API key that will cause Resend send failure
    insert_resend_email_config_direct(&ctx._app_state.pool, &ctx._realm_id).await;

    // When: POST /api/configs/{realmId}/email/test with valid recipient
    let resp = send_test_email(&app, &ctx._realm_id, &token, "test@example.com").await;

    // Then: 200 OK, body has success=false and message field present
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "should return 200 when email is configured"
    );
    let body: serde_json::Value = crate::tests::response_json(resp).await;
    assert!(
        body.get("success").is_some(),
        "response should contain 'success' field"
    );
    assert!(
        body.get("message").is_some(),
        "response should contain 'message' field"
    );
    // Dummy key means send will fail, so success should be false
    assert_eq!(
        body["success"], false,
        "success should be false because dummy key cannot authenticate with Resend"
    );
}

// User Story: docs/user-stories/02-realm-admin-user-stories.md US-RA-014
// Covers: Rate limiting on email test endpoint (3 per 60 seconds)
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_email_test_rate_limited(ctx: &mut TestContext) {
    // Given: complete email config
    let app = ctx.create_unified_test_router();
    let (token, user_id) =
        create_admin_session_with_user(ctx, "email-test-ratelimit@test.com", 1800).await;
    grant_realm_admin_role(ctx, &user_id).await;

    insert_resend_email_config_direct(&ctx._app_state.pool, &ctx._realm_id).await;

    // When: send 4 test email requests rapidly
    for i in 0..3 {
        let resp = send_test_email(&app, &ctx._realm_id, &token, "test@example.com").await;
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "request {} should return 200",
            i + 1
        );
    }

    let resp = send_test_email(&app, &ctx._realm_id, &token, "test@example.com").await;

    // Then: 4th returns 429 Too Many Requests
    assert_eq!(
        resp.status(),
        StatusCode::TOO_MANY_REQUESTS,
        "4th request should return 429 (rate limited)"
    );
}
