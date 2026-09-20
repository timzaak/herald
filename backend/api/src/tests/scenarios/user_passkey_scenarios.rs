// =============================================================================
// User Passkey API Scenarios
// =============================================================================

use crate::tests::helpers::auth_helpers::{
    enable_totp_for_session, generate_totp_code, obtain_reauth_token,
};
use crate::tests::helpers::passkey_authenticator::Es256Authenticator;
use crate::tests::helpers::passkey_flow_helpers::{
    RP_ORIGIN, begin_registration, clear_passkey_user_rate_limit, register_one_passkey,
    setup_realm_passkey_config,
};
use crate::tests::schema_test_context::SchemaTestContext as TestContext;
use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use redis::AsyncCommands;
use serde_json::{Value, json};
use test_context::test_context;
use tower::ServiceExt;
use uuid::Uuid;

const PASSWORD: &str = "Password123!";
const PASSKEY_VERIFY_FAILED: &str = "Passkey 验证失败";

type TestAuthenticator = Es256Authenticator;

fn setup_passkey_env() {
    unsafe {
        std::env::set_var("RP_ID", "localhost");
        std::env::set_var("RP_ORIGIN", "https://localhost");
        std::env::set_var("TOTP_SECRET_KEY", "test_key_32_bytes_long_1234567890");
    }
}

fn softtoken() -> TestAuthenticator {
    Es256Authenticator::new()
}

async fn response_body(response: axum::response::Response) -> Value {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body should be readable");
    if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("response body should be JSON")
    }
}

async fn ensure_user_consented(ctx: &TestContext, user_id: &str, realm_id: &str) {
    sqlx::query(
        "INSERT INTO user_agreement_consent (id, user_id, realm_id, agreement_type, consented_version_id)
         SELECT uuidv7(), $1::uuid, $2, agreement_type, id
         FROM legal_agreement_version
         WHERE realm_id IS NULL AND version_no = 1 AND agreement_type IN ('terms_of_service', 'privacy_policy')
         ON CONFLICT (user_id, agreement_type) DO NOTHING",
    )
    .bind(user_id)
    .bind(realm_id)
    .execute(&ctx._app_state.pool)
    .await
    .expect("test user should consent to default agreements");
}

async fn create_test_user(ctx: &TestContext, email: &str, password: &str) -> String {
    let user_uuid = Uuid::now_v7();
    let password_hash = bcrypt::hash(password, bcrypt::DEFAULT_COST).expect("password should hash");

    sqlx::query(
        "INSERT INTO account (id, realm_id, email, password, status)
         VALUES ($1, $2, $3, $4, 1)",
    )
    .bind(user_uuid)
    .bind(&ctx._realm_id)
    .bind(email)
    .bind(&password_hash)
    .execute(&ctx._app_state.pool)
    .await
    .expect("test user should be inserted");

    ensure_user_consented(ctx, &user_uuid.to_string(), &ctx._realm_id).await;

    user_uuid.to_string()
}

async fn create_session(ctx: &TestContext, email: &str, password: &str) -> String {
    let payload = json!({
        "clientId": ctx._client_id,
        "email": email,
        "password": password,
        "turnstileToken": "dummy"
    });
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{}/login", ctx._realm_id))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "8.8.8.8")
        .body(Body::from(payload.to_string()))
        .unwrap();

    let response = ctx.create_unified_test_router().oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let (_response, token) = crate::tests::extract_bearer_token(response).await;
    token.expect("login should return accessToken")
}

async fn setup_realm_totp_config(ctx: &TestContext, enabled: bool, force_enabled: bool) {
    let config_value = json!({ "enabled": enabled, "force_enabled": force_enabled });
    let metadata = json!({ "force_enabled": force_enabled });

    sqlx::query(
        "INSERT INTO realm_config
            (id, realm_id, config_type, config_key, config_value, is_secret, enabled, metadata, created_at, updated_at)
         VALUES ($1, $2, 'totp', 'settings', $3, false, $4, $5::jsonb, NOW(), NOW())
         ON CONFLICT (realm_id, config_type, config_key)
         DO UPDATE SET config_value = EXCLUDED.config_value,
                       enabled = EXCLUDED.enabled,
                       metadata = EXCLUDED.metadata,
                       updated_at = NOW()",
    )
    .bind(Uuid::now_v7())
    .bind(&ctx._realm_id)
    .bind(config_value.to_string())
    .bind(enabled)
    .bind(metadata.to_string())
    .execute(&ctx._app_state.pool)
    .await
    .expect("totp realm config should upsert");
}

async fn delete_passkey(
    ctx: &TestContext,
    session_token: &str,
    password: &str,
    credential_id: &str,
) -> axum::response::Response {
    let reauth_token =
        obtain_reauth_token(ctx, session_token, "remove_authenticator", password).await;
    let req = Request::builder()
        .method("DELETE")
        .uri(format!("/api/user/passkey/credentials/{credential_id}"))
        .header("content-type", "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {session_token}"))
        .body(Body::from(
            json!({ "reauthToken": reauth_token }).to_string(),
        ))
        .unwrap();
    ctx.create_unified_test_router().oneshot(req).await.unwrap()
}

async fn credential_bytes_for_id(ctx: &TestContext, credential_id: &str) -> Vec<u8> {
    sqlx::query_scalar("SELECT credential_id FROM user_passkey_credential WHERE id = $1::uuid")
        .bind(credential_id)
        .fetch_one(&ctx._app_state.pool)
        .await
        .expect("credential bytes should exist")
}

fn add_allow_credential(options: &mut Value, credential_id: &[u8]) {
    let encoded = URL_SAFE_NO_PAD.encode(credential_id);
    options["allowCredentials"] = json!([{
        "type": "public-key",
        "id": encoded,
    }]);
}

async fn begin_first_factor(ctx: &TestContext, realm_id: &str) -> (Value, String) {
    let payload = json!({
        "clientId": ctx._client_id,
        "turnstileToken": "dummy"
    });
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{realm_id}/login/passkey/options"))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "9.9.9.9")
        .body(Body::from(payload.to_string()))
        .unwrap();
    let response = ctx.create_unified_test_router().oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_body(response).await;

    (
        body["options"].clone(),
        body["authToken"]
            .as_str()
            .expect("authToken should be present")
            .to_string(),
    )
}

async fn finish_first_factor(
    ctx: &TestContext,
    realm_id: &str,
    authenticator: &mut TestAuthenticator,
    auth_token: &str,
    options: Value,
) -> (String, Value) {
    let assertion = authenticator.authenticate(&options, RP_ORIGIN);
    let payload = json!({
        "authToken": auth_token,
        "assertion": assertion
    });

    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{realm_id}/login/passkey/verify"))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "9.9.9.9")
        .body(Body::from(payload.to_string()))
        .unwrap();
    let response = ctx.create_unified_test_router().oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let (response, token) = crate::tests::extract_bearer_token(response).await;
    let token = token.expect("passkey login should return accessToken");
    let body = response_body(response).await;

    (token, body)
}

async fn password_login(
    ctx: &TestContext,
    email: &str,
    password: &str,
) -> axum::response::Response {
    let payload = json!({
        "clientId": ctx._client_id,
        "email": email,
        "password": password,
        "turnstileToken": "dummy"
    });
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{}/login", ctx._realm_id))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "10.10.10.10")
        .body(Body::from(payload.to_string()))
        .unwrap();
    ctx.create_unified_test_router().oneshot(req).await.unwrap()
}

async fn begin_second_factor(ctx: &TestContext, temp_token: &str) -> (Value, String) {
    let payload = json!({ "tempToken": temp_token });
    let req = Request::builder()
        .method("POST")
        .uri(format!(
            "/api/auth/{}/login/passkey/2fa/options",
            ctx._realm_id
        ))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "11.11.11.11")
        .body(Body::from(payload.to_string()))
        .unwrap();
    let response = ctx.create_unified_test_router().oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_body(response).await;

    (
        body["options"].clone(),
        body["authToken"]
            .as_str()
            .expect("authToken should be present")
            .to_string(),
    )
}

async fn finish_second_factor(
    ctx: &TestContext,
    authenticator: &mut TestAuthenticator,
    temp_token: &str,
    auth_token: &str,
    options: Value,
) -> (String, Value) {
    let assertion = authenticator.authenticate(&options, RP_ORIGIN);
    let payload = json!({
        "tempToken": temp_token,
        "authToken": auth_token,
        "assertion": assertion
    });
    let req = Request::builder()
        .method("POST")
        .uri(format!(
            "/api/auth/{}/login/passkey/2fa/verify",
            ctx._realm_id
        ))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "11.11.11.11")
        .body(Body::from(payload.to_string()))
        .unwrap();
    let response = ctx.create_unified_test_router().oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let (response, token) = crate::tests::extract_bearer_token(response).await;
    let token = token.expect("2FA passkey login should return accessToken");
    let body = response_body(response).await;

    (token, body)
}

async fn enable_totp_via_http(ctx: &TestContext, session_token: &str) -> String {
    enable_totp_for_session(ctx, session_token, PASSWORD)
        .await
        .0
}

async fn create_other_realm(ctx: &TestContext, realm_id: &str) {
    sqlx::query("INSERT INTO realm (id, name) VALUES ($1, $2)")
        .bind(realm_id)
        .bind("Other Realm")
        .execute(&ctx._app_state.pool)
        .await
        .expect("other realm should insert");

    sqlx::query(
        "INSERT INTO client_app
            (id, realm_id, client_id, name, redirect_uris, enabled, browser_refresh_absolute_ttl_seconds, is_first_party)
         VALUES ($1, $2, $3, 'Other Console', '[]'::jsonb, true, 86400, true)",
    )
    .bind(Uuid::now_v7())
    .bind(realm_id)
    .bind(&ctx._client_id)
    .execute(&ctx._app_state.pool)
    .await
    .expect("other realm client should insert");
}

/// User Story: US-PK-004
/// Covers: passkey design §4.1 registration, §4.3 persistence, §6.1 ceremony tests.
#[test_context(TestContext)]
#[tokio::test]
async fn test_passkey_registration_finish_persists_credential(ctx: &mut TestContext) {
    setup_passkey_env();
    setup_realm_passkey_config(ctx, &ctx._realm_id, true).await;
    let email = "passkey-register@test.com";
    let user_id = create_test_user(ctx, email, PASSWORD).await;
    let session = create_session(ctx, email, PASSWORD).await;
    let mut authenticator = softtoken();

    let credential_id = register_one_passkey(
        ctx,
        &session,
        &user_id,
        PASSWORD,
        Some("Laptop"),
        &mut authenticator,
    )
    .await;

    let row: (i64, bool, bool, Option<String>) = sqlx::query_as(
        "SELECT counter, backup_eligible, backup_state, nickname
         FROM user_passkey_credential WHERE id = $1::uuid",
    )
    .bind(&credential_id)
    .fetch_one(&ctx._app_state.pool)
    .await
    .expect("credential row should exist");
    assert_eq!(row.0, 0, "registration stores an initial counter");
    assert_eq!(row.3.as_deref(), Some("Laptop"));

    clear_passkey_user_rate_limit(ctx, &user_id).await;
    let req = Request::builder()
        .method("GET")
        .uri("/api/user/passkey/credentials")
        .header(header::AUTHORIZATION, format!("Bearer {session}"))
        .body(Body::empty())
        .unwrap();
    let response = ctx.create_unified_test_router().oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_body(response).await;
    assert_eq!(body["credentials"].as_array().unwrap().len(), 1);
    assert_eq!(body["credentials"][0]["credentialId"], credential_id);
    assert!(body["credentials"][0]["backupEligible"].is_boolean());
    assert!(body["credentials"][0]["backupState"].is_boolean());
}

/// User Story: US-PK-005
/// Covers: passkey design §4.1 first-factor login, §4.5 counter/last-used update, §6.1.
#[test_context(TestContext)]
#[tokio::test]
async fn test_passkey_first_factor_login_success_and_counter_updates(ctx: &mut TestContext) {
    setup_passkey_env();
    setup_realm_passkey_config(ctx, &ctx._realm_id, true).await;
    let email = "passkey-first-factor@test.com";
    let user_id = create_test_user(ctx, email, PASSWORD).await;
    let session = create_session(ctx, email, PASSWORD).await;
    let mut authenticator = softtoken();
    let credential_id = register_one_passkey(
        ctx,
        &session,
        &user_id,
        PASSWORD,
        Some("Security Key"),
        &mut authenticator,
    )
    .await;
    let credential_bytes = credential_bytes_for_id(ctx, &credential_id).await;

    let (mut options, auth_token) = begin_first_factor(ctx, &ctx._realm_id).await;
    add_allow_credential(&mut options, &credential_bytes);
    let (session_token, body) = finish_first_factor(
        ctx,
        &ctx._realm_id,
        &mut authenticator,
        &auth_token,
        options,
    )
    .await;

    assert!(!session_token.is_empty());
    assert!(
        body["accessToken"].is_string() && body["refreshToken"].is_string(),
        "first-factor passkey login should issue a browser token family"
    );
    let row: (i64, Option<chrono::DateTime<chrono::Utc>>) = sqlx::query_as(
        "SELECT counter, last_used_at FROM user_passkey_credential WHERE id = $1::uuid",
    )
    .bind(&credential_id)
    .fetch_one(&ctx._app_state.pool)
    .await
    .expect("credential row should exist");
    assert!(row.0 > 0, "successful assertion increments the counter");
    assert!(row.1.is_some(), "successful assertion records last_used_at");
}

/// Covers: users.md "被禁用的用户无法登录" — the passkey entrance must reject a
/// disabled account even when the credential assertion itself is valid. Every
/// other entrance (password, email-OTP, LDAP, OAuth callback) refuses before
/// issuing tokens; without this check a Forbidden/Deleted user could complete
/// passkey login and mint a fresh token family.
#[test_context(TestContext)]
#[tokio::test]
async fn test_passkey_first_factor_login_rejects_disabled_user(ctx: &mut TestContext) {
    setup_passkey_env();
    setup_realm_passkey_config(ctx, &ctx._realm_id, true).await;
    let email = "passkey-disabled@test.com";
    let user_id = create_test_user(ctx, email, PASSWORD).await;
    let session = create_session(ctx, email, PASSWORD).await;
    let mut authenticator = softtoken();
    let credential_id = register_one_passkey(
        ctx,
        &session,
        &user_id,
        PASSWORD,
        Some("Security Key"),
        &mut authenticator,
    )
    .await;
    let credential_bytes = credential_bytes_for_id(ctx, &credential_id).await;

    // Disable the account directly (mirrors user_sessions_scenarios): the
    // disable happened outside this login flow, so no session teardown ran.
    sqlx::query("UPDATE account SET status = 2, updated_at = NOW() WHERE id = $1::uuid")
        .bind(&user_id)
        .execute(&ctx._app_state.pool)
        .await
        .expect("user should be disabled");

    let (mut options, auth_token) = begin_first_factor(ctx, &ctx._realm_id).await;
    add_allow_credential(&mut options, &credential_bytes);
    let assertion = authenticator.authenticate(&options, RP_ORIGIN);
    let payload = json!({
        "authToken": auth_token,
        "assertion": assertion
    });
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{}/login/passkey/verify", ctx._realm_id))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "9.9.9.9")
        .body(Body::from(payload.to_string()))
        .unwrap();
    let response = ctx.create_unified_test_router().oneshot(req).await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "a valid passkey assertion must not complete login for a disabled account"
    );
}

/// User Story: US-PK-006
/// Covers: passkey design §4.1 second-factor login, §4.2.2 temp session reuse, §6.1.
#[test_context(TestContext)]
#[tokio::test]
async fn test_passkey_second_factor_login_success(ctx: &mut TestContext) {
    setup_passkey_env();
    setup_realm_passkey_config(ctx, &ctx._realm_id, true).await;
    let email = "passkey-second-factor@test.com";
    let user_id = create_test_user(ctx, email, PASSWORD).await;
    let session = create_session(ctx, email, PASSWORD).await;
    let mut authenticator = softtoken();
    register_one_passkey(
        ctx,
        &session,
        &user_id,
        PASSWORD,
        Some("Phone"),
        &mut authenticator,
    )
    .await;

    let login_response = password_login(ctx, email, PASSWORD).await;
    assert_eq!(login_response.status(), StatusCode::OK);
    let login_body = response_body(login_response).await;
    assert_eq!(login_body["requiresTotp"], false);
    assert_eq!(login_body["secondFactors"], json!(["passkey"]));
    let temp_token = login_body["tempToken"].as_str().unwrap();

    let (options, auth_token) = begin_second_factor(ctx, temp_token).await;
    let (session_token, body) =
        finish_second_factor(ctx, &mut authenticator, temp_token, &auth_token, options).await;
    assert!(!session_token.is_empty());
    assert!(
        body["accessToken"].is_string() && body["refreshToken"].is_string(),
        "second-factor passkey login should issue a browser token family"
    );

    let mut conn = ctx._app_state.redis_manager.get().await.unwrap();
    let temp_exists: bool = conn
        .exists(format!("totp:temp:{temp_token}"))
        .await
        .expect("temp token existence should be queryable");
    assert!(
        !temp_exists,
        "second-factor success consumes the temp token"
    );
}

/// User Story: US-PK-007
/// Covers: passkey design §4.1 list/rename/delete, §4.2.3 precise success statuses, §6.1.
#[test_context(TestContext)]
#[tokio::test]
async fn test_passkey_list_rename_and_delete(ctx: &mut TestContext) {
    setup_passkey_env();
    setup_realm_passkey_config(ctx, &ctx._realm_id, true).await;
    let email = "passkey-crud@test.com";
    let user_id = create_test_user(ctx, email, PASSWORD).await;
    let session = create_session(ctx, email, PASSWORD).await;
    let mut first = softtoken();
    let mut second = softtoken();
    let first_id =
        register_one_passkey(ctx, &session, &user_id, PASSWORD, Some("First"), &mut first).await;
    let second_id = register_one_passkey(
        ctx,
        &session,
        &user_id,
        PASSWORD,
        Some("Second"),
        &mut second,
    )
    .await;

    clear_passkey_user_rate_limit(ctx, &user_id).await;
    let list_req = Request::builder()
        .method("GET")
        .uri("/api/user/passkey/credentials")
        .header(header::AUTHORIZATION, format!("Bearer {session}"))
        .body(Body::empty())
        .unwrap();
    let list_response = ctx
        .create_unified_test_router()
        .oneshot(list_req)
        .await
        .unwrap();
    assert_eq!(list_response.status(), StatusCode::OK);
    assert_eq!(
        response_body(list_response).await["credentials"]
            .as_array()
            .unwrap()
            .len(),
        2
    );

    clear_passkey_user_rate_limit(ctx, &user_id).await;
    let rename_req = Request::builder()
        .method("PATCH")
        .uri(format!("/api/user/passkey/credentials/{first_id}"))
        .header("content-type", "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {session}"))
        .body(Body::from(json!({ "nickname": "Renamed" }).to_string()))
        .unwrap();
    let rename_response = ctx
        .create_unified_test_router()
        .oneshot(rename_req)
        .await
        .unwrap();
    assert_eq!(rename_response.status(), StatusCode::NO_CONTENT);

    clear_passkey_user_rate_limit(ctx, &user_id).await;
    let delete_response = delete_passkey(ctx, &session, PASSWORD, &second_id).await;
    assert_eq!(delete_response.status(), StatusCode::NO_CONTENT);

    let rows: Vec<(String, Option<String>)> =
        sqlx::query_as("SELECT id::text, nickname FROM user_passkey_credential ORDER BY nickname")
            .fetch_all(&ctx._app_state.pool)
            .await
            .expect("credentials should query");
    assert_eq!(rows, vec![(first_id, Some("Renamed".to_string()))]);
}

/// User Story: US-PK-009
/// Covers: passkey design §6.3 delete-last regression and §5.3 secondFactors derivation.
#[test_context(TestContext)]
#[tokio::test]
async fn test_delete_last_passkey_removes_from_second_factors(ctx: &mut TestContext) {
    setup_passkey_env();
    setup_realm_passkey_config(ctx, &ctx._realm_id, true).await;
    let email = "passkey-delete-last@test.com";
    let user_id = create_test_user(ctx, email, PASSWORD).await;
    let session = create_session(ctx, email, PASSWORD).await;
    let mut authenticator = softtoken();
    let credential_id = register_one_passkey(
        ctx,
        &session,
        &user_id,
        PASSWORD,
        Some("Only"),
        &mut authenticator,
    )
    .await;

    clear_passkey_user_rate_limit(ctx, &user_id).await;
    let delete_response = delete_passkey(ctx, &session, PASSWORD, &credential_id).await;
    assert_eq!(delete_response.status(), StatusCode::NO_CONTENT);

    let login_response = password_login(ctx, email, PASSWORD).await;
    assert_eq!(login_response.status(), StatusCode::OK);
    let (login_response, token) = crate::tests::extract_bearer_token(login_response).await;
    assert!(token.is_some(), "password login should issue accessToken");
    let body = response_body(login_response).await;
    assert!(body.get("requiresTotp").is_none() || body["requiresTotp"] == false);
    assert!(
        body.get("secondFactors").is_none() || body["secondFactors"].as_array().unwrap().is_empty()
    );
    assert!(body.get("tempToken").is_none() || body["tempToken"].is_null());
}

/// User Story: US-PK-001 / US-PK-005
/// Disabling registration must not strand credentials that were registered
/// while Passkey was enabled.
#[test_context(TestContext)]
#[tokio::test]
async fn test_first_factor_options_remain_available_when_realm_is_disabled(ctx: &mut TestContext) {
    setup_passkey_env();
    setup_realm_passkey_config(ctx, &ctx._realm_id, false).await;

    let payload = json!({
        "clientId": ctx._client_id,
        "turnstileToken": "dummy"
    });
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{}/login/passkey/options", ctx._realm_id))
        .header("content-type", "application/json")
        .body(Body::from(payload.to_string()))
        .unwrap();
    let response = ctx.create_unified_test_router().oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

/// Listing passkey credentials must be gated on realm enablement, mirroring
/// registration/begin. When the realm has Passkey disabled, the list endpoint
/// returns 404 rather than a misleading 200 empty list — so the user security
/// page never shows an empty "add passkey" affordance for a feature the realm
/// turned off.
#[test_context(TestContext)]
#[tokio::test]
async fn test_list_credentials_returns_404_when_realm_passkey_disabled(ctx: &mut TestContext) {
    setup_passkey_env();
    setup_realm_passkey_config(ctx, &ctx._realm_id, false).await;
    let email = "passkey-list-disabled-realm@test.com";
    create_test_user(ctx, email, PASSWORD).await;
    let session = create_session(ctx, email, PASSWORD).await;

    let req = Request::builder()
        .method("GET")
        .uri("/api/user/passkey/credentials")
        .header(header::AUTHORIZATION, format!("Bearer {session}"))
        .body(Body::empty())
        .unwrap();
    let response = ctx.create_unified_test_router().oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[test_context(TestContext)]
#[tokio::test]
async fn test_assertion_failure_returns_unified_message_no_internal_cause(ctx: &mut TestContext) {
    setup_passkey_env();
    setup_realm_passkey_config(ctx, &ctx._realm_id, true).await;
    let email = "passkey-failure-message@test.com";
    let user_id = create_test_user(ctx, email, PASSWORD).await;
    let session = create_session(ctx, email, PASSWORD).await;
    let mut authenticator = softtoken();
    register_one_passkey(
        ctx,
        &session,
        &user_id,
        PASSWORD,
        Some("Key"),
        &mut authenticator,
    )
    .await;

    let (_options, auth_token) = begin_first_factor(ctx, &ctx._realm_id).await;
    let invalid_payload = json!({
        "authToken": auth_token,
        "assertion": { "id": "bad", "rawId": "bad", "type": "public-key", "response": {} }
    });
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{}/login/passkey/verify", ctx._realm_id))
        .header("content-type", "application/json")
        .body(Body::from(invalid_payload.to_string()))
        .unwrap();
    let response = ctx.create_unified_test_router().oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let body_text = response_body(response).await.to_string();
    assert!(body_text.contains(PASSKEY_VERIFY_FAILED));
    for leaked in ["attestation", "VerificationFailed", "credential", "counter"] {
        assert!(
            !body_text.contains(leaked),
            "unified failure must not leak internal cause: {leaked}"
        );
    }

    let login_response = password_login(ctx, email, PASSWORD).await;
    let login_body = response_body(login_response).await;
    let temp_token = login_body["tempToken"].as_str().unwrap();
    let (_second_options, second_auth_token) = begin_second_factor(ctx, temp_token).await;
    let invalid_2fa_payload = json!({
        "tempToken": temp_token,
        "authToken": second_auth_token,
        "assertion": { "id": "bad", "rawId": "bad", "type": "public-key", "response": {} }
    });
    let second_req = Request::builder()
        .method("POST")
        .uri(format!(
            "/api/auth/{}/login/passkey/2fa/verify",
            ctx._realm_id
        ))
        .header("content-type", "application/json")
        .body(Body::from(invalid_2fa_payload.to_string()))
        .unwrap();
    let second_response = ctx
        .create_unified_test_router()
        .oneshot(second_req)
        .await
        .unwrap();
    assert_eq!(second_response.status(), StatusCode::UNAUTHORIZED);
    let second_body_text = response_body(second_response).await.to_string();
    assert!(second_body_text.contains(PASSKEY_VERIFY_FAILED));
    for leaked in ["attestation", "VerificationFailed", "credential", "counter"] {
        assert!(
            !second_body_text.contains(leaked),
            "2fa unified failure must not leak internal cause: {leaked}"
        );
    }
}

/// User Story: US-PK-005
/// Covers: passkey design §4.5 cross-realm isolation and §4.2.3 401 for wrong realm credential.
#[test_context(TestContext)]
#[tokio::test]
async fn test_first_factor_cross_realm_credential_isolated(ctx: &mut TestContext) {
    setup_passkey_env();
    setup_realm_passkey_config(ctx, &ctx._realm_id, true).await;
    let other_realm = "passkey-other-realm";
    create_other_realm(ctx, other_realm).await;
    setup_realm_passkey_config(ctx, other_realm, true).await;
    let email = "passkey-cross-realm@test.com";
    let user_id = create_test_user(ctx, email, PASSWORD).await;
    let session = create_session(ctx, email, PASSWORD).await;
    let mut authenticator = softtoken();
    let credential_id = register_one_passkey(
        ctx,
        &session,
        &user_id,
        PASSWORD,
        Some("A Realm Key"),
        &mut authenticator,
    )
    .await;
    let credential_bytes = credential_bytes_for_id(ctx, &credential_id).await;

    let (mut options, auth_token) = begin_first_factor(ctx, other_realm).await;
    add_allow_credential(&mut options, &credential_bytes);
    let assertion = authenticator.authenticate(&options, RP_ORIGIN);
    let payload = json!({
        "authToken": auth_token,
        "assertion": assertion
    });
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{other_realm}/login/passkey/verify"))
        .header("content-type", "application/json")
        .body(Body::from(payload.to_string()))
        .unwrap();
    let response = ctx.create_unified_test_router().oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(
        response_body(response)
            .await
            .to_string()
            .contains(PASSKEY_VERIFY_FAILED)
    );
}

/// User Story: US-PK-004
/// Covers: passkey design §4.2.3 409 on finish duplicate credential and §4.3 unique index.
#[test_context(TestContext)]
#[tokio::test]
async fn test_registration_finish_duplicate_credential_id_conflict(ctx: &mut TestContext) {
    setup_passkey_env();
    setup_realm_passkey_config(ctx, &ctx._realm_id, true).await;
    let email = "passkey-duplicate@test.com";
    let user_id = create_test_user(ctx, email, PASSWORD).await;
    let user_uuid = Uuid::parse_str(&user_id).unwrap();
    let session = create_session(ctx, email, PASSWORD).await;
    let mut authenticator = softtoken();
    let (options, reg_token) =
        begin_registration(ctx, &session, PASSWORD, Some("Duplicate Key")).await;
    clear_passkey_user_rate_limit(ctx, &user_id).await;
    let attestation_json = authenticator.register(&options, RP_ORIGIN);
    let raw_id = attestation_json["id"]
        .as_str()
        .or_else(|| attestation_json["rawId"].as_str())
        .expect("attestation should include credential id");
    let credential_bytes = URL_SAFE_NO_PAD
        .decode(raw_id)
        .expect("credential id should be base64url");

    sqlx::query(
        "INSERT INTO user_passkey_credential
            (id, user_id, realm_id, rp_id, credential_id, credential_public_key, counter, transports,
             backup_eligible, backup_state, user_verified, nickname, created_at, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, 0, '[]'::jsonb, false, false, false, 'existing', NOW(), NOW())",
    )
    .bind(Uuid::now_v7())
    .bind(user_uuid)
    .bind(&ctx._realm_id)
    .bind("localhost")
    .bind(credential_bytes)
    .bind(Vec::<u8>::from("duplicate-public-key"))
    .execute(&ctx._app_state.pool)
    .await
    .expect("duplicate fixture should insert before finish");

    let reauth_token = obtain_reauth_token(ctx, &session, "bind_authenticator", PASSWORD).await;
    let payload = json!({
        "reauthToken": reauth_token,
        "regToken": reg_token,
        "attestation": attestation_json
    });
    let req = Request::builder()
        .method("POST")
        .uri("/api/user/passkey/registration/finish")
        .header("content-type", "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {session}"))
        .body(Body::from(payload.to_string()))
        .unwrap();
    let response = ctx.create_unified_test_router().oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
}

/// User Story: US-PK-008
/// Covers: passkey design §5.3 and §6.3 password+TOTP backward compatibility.
#[test_context(TestContext)]
#[tokio::test]
async fn test_password_totp_login_backward_compat_after_second_factors_field(
    ctx: &mut TestContext,
) {
    setup_passkey_env();
    setup_realm_passkey_config(ctx, &ctx._realm_id, true).await;
    setup_realm_totp_config(ctx, true, false).await;
    let email = "passkey-totp-compat@test.com";
    create_test_user(ctx, email, PASSWORD).await;
    let session = create_session(ctx, email, PASSWORD).await;
    let totp_secret = enable_totp_via_http(ctx, &session).await;

    let login_response = password_login(ctx, email, PASSWORD).await;
    assert_eq!(login_response.status(), StatusCode::OK);
    let login_body = response_body(login_response).await;
    assert_eq!(login_body["requiresTotp"], true);
    assert_eq!(login_body["secondFactors"], json!(["totp"]));
    let temp_token = login_body["tempToken"].as_str().unwrap();

    let code = generate_totp_code(&totp_secret);
    let payload = json!({
        "tempToken": temp_token,
        "code": code
    });
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{}/login/verify-totp", ctx._realm_id))
        .header("content-type", "application/json")
        .body(Body::from(payload.to_string()))
        .unwrap();
    let response = ctx.create_unified_test_router().oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let (_response, token) = crate::tests::extract_bearer_token(response).await;
    assert!(
        token.is_some(),
        "TOTP-only verification should issue accessToken"
    );
}

/// 回归（审计 run-2：passkey-challenge-consumption-not-atomic）：
/// passkey 断言的一次性由 challenge 的消费承载。旧代码 GET→DEL 分离，两个
/// 并发提交同一断言都读到同一份 ceremony 状态、都通过确定性验证、都完成
/// 登录/2FA/reauth 仪式。修复后 finish_authentication 以 GETDEL 原子消费
/// challenge —— 同一断言恰好完成一次仪式，败者 401。
#[test_context(TestContext)]
#[tokio::test]
async fn test_passkey_duplicate_concurrent_assertion_completes_once(ctx: &mut TestContext) {
    setup_passkey_env();
    setup_realm_passkey_config(ctx, &ctx._realm_id, true).await;
    let email = "passkey-concurrent-2fa@test.com";
    let user_id = create_test_user(ctx, email, PASSWORD).await;
    let session = create_session(ctx, email, PASSWORD).await;
    let mut authenticator = softtoken();
    register_one_passkey(
        ctx,
        &session,
        &user_id,
        PASSWORD,
        Some("Concurrent Key"),
        &mut authenticator,
    )
    .await;

    let login_response = password_login(ctx, email, PASSWORD).await;
    assert_eq!(login_response.status(), StatusCode::OK);
    let login_body = response_body(login_response).await;
    assert_eq!(
        login_body["secondFactors"],
        json!(["passkey"]),
        "passkey-only second factor must require the ceremony"
    );
    let temp_token = login_body["tempToken"].as_str().unwrap().to_string();

    let (options, auth_token) = begin_second_factor(ctx, &temp_token).await;
    // ONE assertion, submitted twice concurrently — the replay shape.
    let assertion = authenticator.authenticate(&options, RP_ORIGIN);
    let payload = json!({
        "tempToken": temp_token,
        "authToken": auth_token,
        "assertion": assertion
    })
    .to_string();

    let make_req = || {
        Request::builder()
            .method("POST")
            .uri(format!(
                "/api/auth/{}/login/passkey/2fa/verify",
                ctx._realm_id
            ))
            .header("content-type", "application/json")
            .header("x-forwarded-for", "12.12.12.12")
            .body(Body::from(payload.clone()))
            .unwrap()
    };
    let (resp_a, resp_b) = tokio::join!(
        async {
            ctx.create_unified_test_router()
                .oneshot(make_req())
                .await
                .unwrap()
        },
        async {
            ctx.create_unified_test_router()
                .oneshot(make_req())
                .await
                .unwrap()
        },
    );
    let mut successes = 0;
    let mut rejections = 0;
    for status in [resp_a.status(), resp_b.status()] {
        if status == StatusCode::OK {
            successes += 1;
        } else if status == StatusCode::UNAUTHORIZED {
            rejections += 1;
        } else {
            panic!("unexpected 2FA verify status under concurrency: {status}");
        }
    }
    assert_eq!(
        (successes, rejections),
        (1, 1),
        "one passkey assertion must complete the ceremony exactly once"
    );

    // 顺序重放同样失败：challenge 已被原子消费。
    let replay = ctx
        .create_unified_test_router()
        .oneshot(make_req())
        .await
        .unwrap();
    assert_eq!(replay.status(), StatusCode::UNAUTHORIZED);
}

/// 回归（审计 run-2：passkey-second-factor-probe-fails-open-on-rp-resolution）：
/// 登录的二因素探测曾把任何 RP 解析失败当作 has_passkey=false —— 包括由
/// 请求 Origin 头触发的 bad_request —— 于是对 passkey-2FA-only 账户，攻击者
/// 只需带一个未映射的 Origin 头即可让密码登录跳过第二因素直接签发会话。
/// 修复后：请求侧解析失败改走 realm 级凭据检查（fail closed），伪造 Origin
/// 仍要求完成 2FA 仪式；对照：无 Origin 头与 TOTP-only 用户同样必须完成仪式。
#[test_context(TestContext)]
#[tokio::test]
async fn test_password_login_crafted_origin_cannot_drop_passkey_second_factor(
    ctx: &mut TestContext,
) {
    setup_passkey_env();
    setup_realm_passkey_config(ctx, &ctx._realm_id, true).await;
    let email = "passkey-probe-origin@test.com";
    let user_id = create_test_user(ctx, email, PASSWORD).await;
    let session = create_session(ctx, email, PASSWORD).await;
    let mut authenticator = softtoken();
    register_one_passkey(
        ctx,
        &session,
        &user_id,
        PASSWORD,
        Some("Probe Key"),
        &mut authenticator,
    )
    .await;

    let login_with_origin = |origin: Option<&str>| {
        let payload = json!({
            "clientId": ctx._client_id,
            "email": email,
            "password": PASSWORD,
            "turnstileToken": "dummy"
        });
        let mut builder = Request::builder()
            .method("POST")
            .uri(format!("/api/auth/{}/login", ctx._realm_id))
            .header("content-type", "application/json")
            .header("x-forwarded-for", "13.13.13.13");
        if let Some(origin) = origin {
            builder = builder.header("origin", origin);
        }
        builder.body(Body::from(payload.to_string())).unwrap()
    };

    // Crafted unmapped Origin: the second factor must still be required —
    // the response must NOT carry a browser token family.
    let resp = ctx
        .create_unified_test_router()
        .oneshot(login_with_origin(Some("https://unmapped.example")))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = response_body(resp).await;
    assert_eq!(
        body["secondFactors"],
        json!(["passkey"]),
        "a crafted Origin header must not drop the passkey second factor"
    );
    assert!(
        body["accessToken"].is_null(),
        "no session may be issued while the second-factor ceremony is pending"
    );
    assert!(
        body["tempToken"].as_str().is_some(),
        "the 2FA ceremony must be reachable for the legitimate user"
    );

    // Control: no Origin header — same ceremony requirement.
    let resp = ctx
        .create_unified_test_router()
        .oneshot(login_with_origin(None))
        .await
        .unwrap();
    let body = response_body(resp).await;
    assert_eq!(body["secondFactors"], json!(["passkey"]));
    assert!(body["accessToken"].is_null());

    // Control: a TOTP-only user under the same crafted Origin still faces the
    // ceremony (OTP login must not bypass an existing second factor either).
    let totp_email = "totp-probe-origin@test.com";
    create_test_user(ctx, totp_email, PASSWORD).await;
    setup_realm_totp_config(ctx, true, false).await;
    let totp_session = create_session(ctx, totp_email, PASSWORD).await;
    let _secret = enable_totp_via_http(ctx, &totp_session).await;
    let payload = json!({
        "clientId": ctx._client_id,
        "email": totp_email,
        "password": PASSWORD,
        "turnstileToken": "dummy"
    });
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{}/login", ctx._realm_id))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "13.13.13.13")
        .header("origin", "https://unmapped.example")
        .body(Body::from(payload.to_string()))
        .unwrap();
    let resp = ctx.create_unified_test_router().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = response_body(resp).await;
    assert_eq!(
        body["requiresTotp"], true,
        "a crafted Origin must not drop the TOTP ceremony either"
    );
    assert!(body["accessToken"].is_null());
}
