//! Custom User UI browser-token scenarios.
//!
//! These tests intentionally use the production router and Bearer headers. They do not
//! construct `Identity` extensions or Redis records directly, so a regression in login,
//! token middleware, scope enforcement, or family revocation remains observable.

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
    response::Response,
};
use herald_core::domain::authentication::{CredentialClass, TargetOperation};
use herald_test_support::{
    SchemaTestContext,
    helpers::{create_admin_session_with_user, grant_realm_admin_role},
};
use serde_json::{Value, json};
use test_context::test_context;
use tower::ServiceExt;
use uuid::Uuid;

const PASSWORD: &str = "CustomUiPassword123!";

struct TestClientApp {
    id: Uuid,
    client_id: String,
}

struct LoginTokens {
    access_token: String,
    refresh_token: String,
}

async fn create_custom_client_app(ctx: &SchemaTestContext, enabled: bool) -> TestClientApp {
    let id = Uuid::now_v7();
    let client_id = format!("custom-ui-{}", &id.simple().to_string()[..12]);
    sqlx::query(
        "INSERT INTO client_app
         (id, realm_id, client_id, name, redirect_uris, allowed_origins,
          email_verify_return_url, password_reset_return_url,
          browser_refresh_absolute_ttl_seconds, is_first_party, enabled)
         VALUES ($1, $2, $3, 'Custom UI test app', $4, $5, $6, $7, 86400, false, $8)",
    )
    .bind(id)
    .bind(&ctx._realm_id)
    .bind(&client_id)
    .bind(json!(["https://partner.example.com/oauth/callback"]))
    .bind(json!(["https://partner.example.com"]))
    .bind("https://partner.example.com/email-verified")
    .bind("https://partner.example.com/password-reset")
    .bind(enabled)
    .execute(&ctx.app_state.pool)
    .await
    .expect("custom Client App fixture must be inserted");
    TestClientApp { id, client_id }
}

async fn create_user(ctx: &SchemaTestContext, email: &str) -> Uuid {
    let id = Uuid::now_v7();
    let password_hash = bcrypt_hash(PASSWORD);
    sqlx::query(
        "INSERT INTO account (id, realm_id, email, password, status)
         VALUES ($1, $2, $3, $4, 1)",
    )
    .bind(id)
    .bind(&ctx._realm_id)
    .bind(email)
    .bind(password_hash)
    .execute(&ctx.app_state.pool)
    .await
    .expect("test user fixture must be inserted");
    id
}

fn bcrypt_hash(password: &str) -> String {
    // `$2b$04$` is a valid low-cost bcrypt hash generated for the fixed test password.
    // Keeping hashing out of the async scenario avoids needing a direct bcrypt dev dependency.
    assert_eq!(password, PASSWORD);
    "$2b$04$Hzhp583AOGgVnC7nKxlVOOCneAhWbGkG09lPPlgbNp5x7uUt8qQHW".to_owned()
}

async fn send(app: Router, method: &str, uri: &str, bearer: Option<&str>, body: Value) -> Response {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-forwarded-for", "127.0.0.1");
    if let Some(token) = bearer {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    app.oneshot(
        builder
            .body(Body::from(body.to_string()))
            .expect("test request must build"),
    )
    .await
    .expect("production router must respond")
}

async fn response_json(response: Response) -> Value {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body must be readable");
    serde_json::from_slice(&bytes).expect("response body must be JSON")
}

fn response_data(value: &Value) -> &Value {
    value.get("data").unwrap_or(value)
}

/// Resolve the current effective `{agreementType, versionId}` pairs for this
/// realm's ToS and Privacy agreements via the public agreements endpoint.
///
/// Mirrors the established `consent_gate_scenarios.rs` pattern: a fresh test
/// user has no recorded consent, so the login consent gate (design §4.1:
/// credentials → TOTP → consent → session) would otherwise return
/// `consentRequired: true` with no tokens. Including these summaries in the
/// login payload's `agreements` field lets the consent gate record re-consent
/// after credentials pass and proceed to token issuance, so `login(...)` yields
/// a real Bearer token for every login-based test.
async fn fetch_current_agreements(ctx: &SchemaTestContext) -> Vec<Value> {
    let response = send(
        ctx.create_unified_test_router(),
        "GET",
        &format!("/api/legal/{}/agreements", ctx._realm_id),
        None,
        Value::Null,
    )
    .await;
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "current legal agreements must resolve so login consent can be satisfied"
    );
    let body = response_json(response).await;
    body["agreements"]
        .as_array()
        .expect("agreements response must contain an array")
        .iter()
        .map(|a| {
            json!({
                "agreementType": a["agreement_type"].as_str().expect("agreement_type must be a string"),
                "versionId": a["version_id"].as_str().expect("version_id must be a string")
            })
        })
        .collect()
}

async fn login(ctx: &SchemaTestContext, app: &TestClientApp, email: &str) -> LoginTokens {
    // Fresh test users have no recorded consent; include the current effective
    // agreements so the consent gate (design §4.1) records re-consent and
    // proceeds to token issuance. See `consent_gate_scenarios.rs` for the
    // established pattern.
    let agreements = fetch_current_agreements(ctx).await;
    let response = send(
        ctx.create_unified_test_router(),
        "POST",
        &format!("/api/auth/{}/login", ctx._realm_id),
        None,
        json!({
            "clientId": app.client_id,
            "email": email,
            "password": PASSWORD,
            "agreements": agreements
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK, "login must succeed");
    assert!(
        !response.headers().contains_key(header::SET_COOKIE),
        "Custom UI login must never set a cookie"
    );
    let body = response_json(response).await;
    assert_eq!(body["tokenType"], "Bearer");
    LoginTokens {
        access_token: body["accessToken"]
            .as_str()
            .expect("accessToken is required")
            .to_owned(),
        refresh_token: body["refreshToken"]
            .as_str()
            .expect("refreshToken is required")
            .to_owned(),
    }
}

async fn refresh(
    ctx: &SchemaTestContext,
    app: &TestClientApp,
    refresh_token: &str,
) -> (Response, Option<LoginTokens>) {
    let response = send(
        ctx.create_unified_test_router(),
        "POST",
        "/api/auth/browser-token/refresh",
        None,
        json!({"clientId": app.id, "refreshToken": refresh_token}),
    )
    .await;
    if response.status() != StatusCode::OK {
        return (response, None);
    }
    let body = response_json(response).await;
    let body = response_data(&body);
    (
        Response::new(Body::empty()),
        Some(LoginTokens {
            access_token: body["accessToken"].as_str().unwrap().to_owned(),
            refresh_token: body["refreshToken"].as_str().unwrap().to_owned(),
        }),
    )
}

async fn status(ctx: &SchemaTestContext, token: Option<&str>) -> Response {
    send(
        ctx.create_unified_test_router(),
        "GET",
        "/api/auth/status",
        token,
        Value::Null,
    )
    .await
}

async fn issue_password_reauth(
    ctx: &SchemaTestContext,
    access_token: &str,
    target_operation: &str,
) -> String {
    issue_password_reauth_with(ctx, access_token, target_operation, PASSWORD).await
}

/// Same as `issue_password_reauth` but lets the caller supply the current
/// password. Needed when an earlier step in the same test changed the
/// password (e.g. a successful `/api/user/change-password` call) — reauth
/// verify checks the live bcrypt hash, so the test must supply the new
/// password or verification returns 401.
async fn issue_password_reauth_with(
    ctx: &SchemaTestContext,
    access_token: &str,
    target_operation: &str,
    current_password: &str,
) -> String {
    let begin = send(
        ctx.create_unified_test_router(),
        "POST",
        "/api/user/reauth",
        Some(access_token),
        json!({"targetOperation": target_operation}),
    )
    .await;
    assert_eq!(begin.status(), StatusCode::OK);
    let begin_body = response_json(begin).await;
    assert!(
        response_data(&begin_body)["availableFactors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|factor| factor == "password"),
        "a password user must advertise the password reauth factor"
    );

    let verify = send(
        ctx.create_unified_test_router(),
        "POST",
        "/api/user/reauth/verify",
        Some(access_token),
        json!({
            "targetOperation": target_operation,
            "factor": "password",
            "password": current_password
        }),
    )
    .await;
    assert_eq!(verify.status(), StatusCode::OK);
    let verify_body = response_json(verify).await;
    response_data(&verify_body)["reauthToken"]
        .as_str()
        .expect("reauth verification must issue a token")
        .to_owned()
}

async fn seed_oauth_code(
    ctx: &SchemaTestContext,
    code: &str,
    user_id: Uuid,
    client_id: &str,
    redirect_uri: &str,
    code_challenge: &str,
) {
    ctx.redis_set_ex(
        &format!("oauth:code:{code}"),
        &json!({
            "code_challenge": code_challenge,
            "client_id": client_id,
            "redirect_uri": redirect_uri,
            "user_id": user_id,
            "realm_id": ctx._realm_id
        })
        .to_string(),
        300,
    )
    .await;
}

async fn seed_mailflow(ctx: &SchemaTestContext, code: &str, client_id: &str, flow_type: &str) {
    ctx.redis_set_ex(
        &format!("mailflow:{code}"),
        &json!({
            "realm_id": ctx._realm_id,
            "client_app_id": client_id,
            "flow_type": flow_type
        })
        .to_string(),
        86_400,
    )
    .await;
}

async fn seed_invoice(ctx: &SchemaTestContext, realm_id: &str, applicant_user_id: Uuid) -> Uuid {
    let invoice_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO invoice (
            id, realm_id, invoice_number, source, account_id, applicant_user_id,
            status, currency, subtotal, discount_amount, tax_amount, shipping_amount, total,
            billing_name, billing_address, billing_email,
            seller_name, seller_address, due_date, created_at, updated_at
         ) VALUES (
            $1, $2, $3, 'user_application', $4, $5,
            'draft', 'USD', 5000, 0, 0, 0, 5000,
            'Other User', 'Other Address', 'other-user@test.com',
            'Seller', 'Seller Address', CURRENT_DATE + INTERVAL '30 days', NOW(), NOW()
         )",
    )
    .bind(invoice_id)
    .bind(realm_id)
    .bind(format!("INV-TEST-{invoice_id}"))
    .bind(Uuid::nil())
    .bind(applicant_user_id)
    .execute(&ctx.app_state.pool)
    .await
    .expect("cross-user invoice fixture must insert");
    invoice_id
}

async fn seed_passkey(ctx: &SchemaTestContext, user_id: Uuid, rp_id: &str, nickname: &str) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO user_passkey_credential
         (id, user_id, realm_id, rp_id, credential_id, credential_public_key, counter,
          transports, backup_eligible, backup_state, user_verified, nickname)
         VALUES ($1, $2, $3, $4, $5, $6, 0, '[\"internal\"]'::jsonb,
                 false, false, true, $7)",
    )
    .bind(id)
    .bind(user_id)
    .bind(&ctx._realm_id)
    .bind(rp_id)
    .bind(id.as_bytes().to_vec())
    .bind(vec![1_u8, 2, 3])
    .bind(nickname)
    .execute(&ctx.app_state.pool)
    .await
    .expect("RP-scoped passkey fixture must insert");
    id
}

async fn enable_passkeys(ctx: &SchemaTestContext) {
    sqlx::query(
        "INSERT INTO realm_config
         (realm_id, config_type, config_key, config_value, is_secret, enabled)
         VALUES ($1, 'passkey', 'settings', $2, false, true)
         ON CONFLICT (realm_id, config_type, config_key)
         DO UPDATE SET config_value = EXCLUDED.config_value, enabled = true",
    )
    .bind(&ctx._realm_id)
    .bind(json!({"enabled": true}).to_string())
    .execute(&ctx.app_state.pool)
    .await
    .expect("passkey realm fixture must be enabled");
}

/// User Story: US-CUI-002 / US-CUI-006
/// Covers: login returns Bearer tokens without Set-Cookie and status resolves identity.
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_custom_user_ui_login_browser_token_success(ctx: &mut SchemaTestContext) {
    let app = create_custom_client_app(ctx, true).await;
    let email = format!("cui-login-{}@test.com", Uuid::now_v7());
    let user_id = create_user(ctx, &email).await;
    let tokens = login(ctx, &app, &email).await;

    let response = status(ctx, Some(&tokens.access_token)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    let data = response_data(&body);
    assert_eq!(data["authenticated"], true);
    assert_eq!(data["userId"], user_id.to_string());
    assert_eq!(data["realmId"], ctx._realm_id);
    assert_eq!(data["credentialClass"], "custom_user_ui");
}

/// User Story: US-CUI-002
/// Covers: FirstParty token exchange is bound to PKCE, redirect URI, Client App, and realm.
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_custom_user_ui_login_first_party_pkce(ctx: &mut SchemaTestContext) {
    // RFC 7636 Appendix B vector.
    let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
    let challenge = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
    let redirect_uri = "http://localhost:8080/callback";
    let email = format!("cui-pkce-{}@test.com", Uuid::now_v7());
    let user_id = create_user(ctx, &email).await;

    for (suffix, client_id, redirect, candidate_verifier) in [
        (
            "verifier",
            "admin-web-console",
            redirect_uri,
            "wrong-verifier",
        ),
        (
            "redirect",
            "admin-web-console",
            "http://localhost:8080/not-callback",
            verifier,
        ),
        ("client", "wrong-client", redirect_uri, verifier),
    ] {
        let code = format!("pkce-{suffix}-{}", Uuid::now_v7());
        seed_oauth_code(
            ctx,
            &code,
            user_id,
            "admin-web-console",
            redirect_uri,
            challenge,
        )
        .await;
        let response = send(
            ctx.create_unified_test_router(),
            "POST",
            &format!("/api/oauth/{}/token", ctx._realm_id),
            None,
            json!({
                "grant_type": "authorization_code",
                "code": code,
                "redirect_uri": redirect,
                "client_id": client_id,
                "code_verifier": candidate_verifier
            }),
        )
        .await;
        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "{suffix} binding mismatch must reject token exchange"
        );
    }

    let code = format!("pkce-valid-{}", Uuid::now_v7());
    seed_oauth_code(
        ctx,
        &code,
        user_id,
        "admin-web-console",
        redirect_uri,
        challenge,
    )
    .await;
    let response = send(
        ctx.create_unified_test_router(),
        "POST",
        &format!("/api/oauth/{}/token", ctx._realm_id),
        None,
        json!({
            "grant_type": "authorization_code",
            "code": code,
            "redirect_uri": redirect_uri,
            "client_id": "admin-web-console",
            "code_verifier": verifier
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["token_type"], "Bearer");
    let access_token = body["access_token"].as_str().unwrap();
    let status_body = response_json(status(ctx, Some(access_token)).await).await;
    assert_eq!(
        response_data(&status_body)["credentialClass"],
        "first_party"
    );
}

/// User Story: US-CUI-002
/// Covers: a refresh token rotates exactly once and reuse is rejected.
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_custom_user_ui_refresh_rotates(ctx: &mut SchemaTestContext) {
    let app = create_custom_client_app(ctx, true).await;
    let email = format!("cui-refresh-{}@test.com", Uuid::now_v7());
    create_user(ctx, &email).await;
    let first = login(ctx, &app, &email).await;
    let (_, second) = refresh(ctx, &app, &first.refresh_token).await;
    let second = second.expect("first refresh must succeed");
    assert_ne!(second.access_token, first.access_token);
    assert_ne!(second.refresh_token, first.refresh_token);

    let (reused, _) = refresh(ctx, &app, &first.refresh_token).await;
    assert_eq!(reused.status(), StatusCode::UNAUTHORIZED);
}

/// User Story: US-CUI-002
/// Covers: refresh-token reuse revokes every access token in the family.
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_custom_user_ui_reuse_revokes_family(ctx: &mut SchemaTestContext) {
    let app = create_custom_client_app(ctx, true).await;
    let email = format!("cui-reuse-{}@test.com", Uuid::now_v7());
    create_user(ctx, &email).await;
    let first = login(ctx, &app, &email).await;
    let (_, second) = refresh(ctx, &app, &first.refresh_token).await;
    let second = second.unwrap();
    let (reused, _) = refresh(ctx, &app, &first.refresh_token).await;
    assert_eq!(reused.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        status(ctx, Some(&first.access_token)).await.status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        status(ctx, Some(&second.access_token)).await.status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        refresh(ctx, &app, &second.refresh_token).await.0.status(),
        StatusCode::UNAUTHORIZED
    );
}

/// User Story: US-CUI-006
/// Covers: logout revokes the caller's access token and its refresh family.
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_custom_user_ui_logout_revokes_family(ctx: &mut SchemaTestContext) {
    let app = create_custom_client_app(ctx, true).await;
    let email = format!("cui-logout-{}@test.com", Uuid::now_v7());
    create_user(ctx, &email).await;
    let tokens = login(ctx, &app, &email).await;
    let logout = send(
        ctx.create_unified_test_router(),
        "POST",
        "/api/auth/logout",
        Some(&tokens.access_token),
        Value::Null,
    )
    .await;
    assert_eq!(logout.status(), StatusCode::OK);
    assert_eq!(
        status(ctx, Some(&tokens.access_token)).await.status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        refresh(ctx, &app, &tokens.refresh_token).await.0.status(),
        StatusCode::UNAUTHORIZED
    );
}

/// User Story: US-CUI-002
/// Covers: status distinguishes CustomUserUi and FirstParty credentials.
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_custom_user_ui_status_returns_identity(ctx: &mut SchemaTestContext) {
    let app = create_custom_client_app(ctx, true).await;
    let email = format!("cui-status-{}@test.com", Uuid::now_v7());
    let user_id = create_user(ctx, &email).await;
    let custom = login(ctx, &app, &email).await;
    let custom_body = response_json(status(ctx, Some(&custom.access_token)).await).await;
    assert_eq!(
        response_data(&custom_body)["credentialClass"],
        "custom_user_ui"
    );

    let (first_party, first_party_user_id) =
        create_admin_session_with_user(ctx, &format!("cui-first-{user_id}@test.com"), 1800).await;
    let first_body = response_json(status(ctx, Some(&first_party)).await).await;
    let first_data = response_data(&first_body);
    assert_eq!(first_data["credentialClass"], "first_party");
    assert_eq!(first_data["userId"], first_party_user_id);
}

/// User Story: US-CUI-002 / US-CUI-005
/// Covers: CustomUserUi permits profile self-service but refuses admin resources.
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_custom_user_ui_scope_upper_bound(ctx: &mut SchemaTestContext) {
    let app = create_custom_client_app(ctx, true).await;
    let email = format!("cui-scope-{}@test.com", Uuid::now_v7());
    create_user(ctx, &email).await;
    let tokens = login(ctx, &app, &email).await;
    let profile = send(
        ctx.create_unified_test_router(),
        "GET",
        "/api/user/profile",
        Some(&tokens.access_token),
        Value::Null,
    )
    .await;
    assert_eq!(profile.status(), StatusCode::OK);
    let clients = send(
        ctx.create_unified_test_router(),
        "GET",
        &format!("/api/client/{}", ctx._realm_id),
        Some(&tokens.access_token),
        Value::Null,
    )
    .await;
    assert_eq!(
        clients.status(),
        StatusCode::FORBIDDEN,
        "self-service token must not gain clients.view"
    );
}

/// User Story: US-CUI-005
/// Covers: FirstParty remains subject to RBAC rather than receiving implicit admin access.
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_custom_user_ui_first_party_admin_rbac(ctx: &mut SchemaTestContext) {
    let (admin_token, admin_id) = create_admin_session_with_user(
        ctx,
        &format!("cui-rbac-admin-{}@test.com", Uuid::now_v7()),
        1800,
    )
    .await;
    grant_realm_admin_role(ctx, &admin_id).await;
    let allowed = send(
        ctx.create_unified_test_router(),
        "GET",
        &format!("/api/client/{}", ctx._realm_id),
        Some(&admin_token),
        Value::Null,
    )
    .await;
    assert_eq!(allowed.status(), StatusCode::OK);

    let (plain_token, _) = create_admin_session_with_user(
        ctx,
        &format!("cui-no-rbac-{}@test.com", Uuid::now_v7()),
        1800,
    )
    .await;
    let denied = send(
        ctx.create_unified_test_router(),
        "GET",
        &format!("/api/client/{}", ctx._realm_id),
        Some(&plain_token),
        Value::Null,
    )
    .await;
    assert_eq!(
        denied.status(),
        StatusCode::FORBIDDEN,
        "FirstParty class must not bypass RBAC"
    );
}

/// User Story: US-CUI-007
/// Covers: a user cannot read an invoice whose applicant is another user.
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_custom_user_ui_cross_user_access_denied(ctx: &mut SchemaTestContext) {
    let app = create_custom_client_app(ctx, true).await;
    let user_a_email = format!("cui-owner-a-{}@test.com", Uuid::now_v7());
    create_user(ctx, &user_a_email).await;
    let user_b = create_user(ctx, &format!("cui-owner-b-{}@test.com", Uuid::now_v7())).await;
    let invoice_b = seed_invoice(ctx, &ctx._realm_id, user_b).await;
    let token_a = login(ctx, &app, &user_a_email).await;
    let response = send(
        ctx.create_unified_test_router(),
        "GET",
        &format!("/api/user/bill/invoices/{invoice_b}"),
        Some(&token_a.access_token),
        Value::Null,
    )
    .await;
    assert!(
        matches!(
            response.status(),
            StatusCode::FORBIDDEN | StatusCode::NOT_FOUND
        ),
        "cross-user invoice lookup must not disclose the invoice"
    );
}

/// User Story: US-CUI-004
/// Covers: resources in another realm are invisible to the token-derived realm query.
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_custom_user_ui_cross_realm_access_denied(ctx: &mut SchemaTestContext) {
    let app = create_custom_client_app(ctx, true).await;
    let email = format!("cui-realm-a-{}@test.com", Uuid::now_v7());
    create_user(ctx, &email).await;
    let tokens = login(ctx, &app, &email).await;

    let realm_b = format!("realm-b-{}", Uuid::now_v7());
    sqlx::query("INSERT INTO realm (id, name) VALUES ($1, 'Cross Realm B')")
        .bind(&realm_b)
        .execute(&ctx.app_state.pool)
        .await
        .unwrap();
    let user_b = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO account (id, realm_id, email, password, status)
         VALUES ($1, $2, $3, $4, 1)",
    )
    .bind(user_b)
    .bind(&realm_b)
    .bind(format!("cui-realm-b-{user_b}@test.com"))
    .bind(bcrypt_hash(PASSWORD))
    .execute(&ctx.app_state.pool)
    .await
    .unwrap();
    let realm_b_invoice = seed_invoice(ctx, &realm_b, user_b).await;
    let response = send(
        ctx.create_unified_test_router(),
        "GET",
        &format!("/api/user/bill/invoices/{realm_b_invoice}"),
        Some(&tokens.access_token),
        Value::Null,
    )
    .await;
    assert_eq!(
        response.status(),
        StatusCode::NOT_FOUND,
        "token realm must scope the invoice query before ownership checks"
    );
}

/// User Story: US-CUI-004
/// Covers: an absent Bearer header is 401 and an X-Auth cookie is not a fallback.
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_custom_user_ui_missing_authorization_rejected(ctx: &mut SchemaTestContext) {
    let missing = status(ctx, None).await;
    assert_eq!(missing.status(), StatusCode::UNAUTHORIZED);

    let request = Request::builder()
        .method("GET")
        .uri("/api/auth/status")
        .header(header::COOKIE, "X-Auth=legacy-cookie-must-not-work")
        .body(Body::empty())
        .unwrap();
    let cookie_only = ctx
        .create_unified_test_router()
        .oneshot(request)
        .await
        .unwrap();
    assert_eq!(cookie_only.status(), StatusCode::UNAUTHORIZED);
}

/// User Story: US-CUI-004
/// Covers: a registered exact origin receives CORS headers without credential cookies.
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_custom_user_ui_cors_allows_registered_origin(ctx: &mut SchemaTestContext) {
    let app = create_custom_client_app(ctx, true).await;
    let email = format!("cui-cors-allowed-{}@test.com", Uuid::now_v7());
    create_user(ctx, &email).await;
    let tokens = login(ctx, &app, &email).await;

    let preflight = Request::builder()
        .method("OPTIONS")
        .uri("/api/user/profile")
        .header(header::ORIGIN, "https://partner.example.com")
        .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
        .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "authorization")
        .body(Body::empty())
        .unwrap();
    let preflight = ctx
        .create_cors_test_router("https://console.example.com")
        .oneshot(preflight)
        .await
        .unwrap();
    assert_eq!(
        preflight.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN],
        "https://partner.example.com"
    );
    assert!(
        !preflight
            .headers()
            .contains_key(header::ACCESS_CONTROL_ALLOW_CREDENTIALS),
        "Bearer CORS must not enable credential cookies"
    );

    let actual = Request::builder()
        .method("GET")
        .uri("/api/user/profile")
        .header(header::ORIGIN, "https://partner.example.com")
        .header(
            header::AUTHORIZATION,
            format!("Bearer {}", tokens.access_token),
        )
        .body(Body::empty())
        .unwrap();
    let actual = ctx
        .create_cors_test_router("https://console.example.com")
        .oneshot(actual)
        .await
        .unwrap();
    assert_eq!(actual.status(), StatusCode::OK);
    assert_eq!(
        actual.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN],
        "https://partner.example.com"
    );
}

/// User Story: US-CUI-004
/// Covers: an unregistered origin receives no CORS grant while Bearer auth remains independent.
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_custom_user_ui_cors_blocks_unregistered_origin(ctx: &mut SchemaTestContext) {
    let app = create_custom_client_app(ctx, true).await;
    let email = format!("cui-cors-blocked-{}@test.com", Uuid::now_v7());
    create_user(ctx, &email).await;
    let tokens = login(ctx, &app, &email).await;
    let request = Request::builder()
        .method("GET")
        .uri("/api/user/profile")
        .header(header::ORIGIN, "https://evil.example.com")
        .header(
            header::AUTHORIZATION,
            format!("Bearer {}", tokens.access_token),
        )
        .body(Body::empty())
        .unwrap();
    let response = ctx
        .create_cors_test_router("https://console.example.com")
        .oneshot(request)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        !response
            .headers()
            .contains_key(header::ACCESS_CONTROL_ALLOW_ORIGIN),
        "unregistered origins must not receive a browser CORS grant"
    );
}

/// User Story: US-CUI-004
/// Covers: profile nickname can be read and updated through a CustomUserUi token.
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_custom_user_ui_profile_read_write(ctx: &mut SchemaTestContext) {
    let app = create_custom_client_app(ctx, true).await;
    let email = format!("cui-profile-{}@test.com", Uuid::now_v7());
    create_user(ctx, &email).await;
    let tokens = login(ctx, &app, &email).await;
    let update = send(
        ctx.create_unified_test_router(),
        "PUT",
        "/api/user/profile",
        Some(&tokens.access_token),
        json!({"nickname": "Partner User"}),
    )
    .await;
    assert_eq!(update.status(), StatusCode::OK);
    let read = send(
        ctx.create_unified_test_router(),
        "GET",
        "/api/user/profile",
        Some(&tokens.access_token),
        Value::Null,
    )
    .await;
    assert_eq!(read.status(), StatusCode::OK);
    let body = response_json(read).await;
    assert_eq!(response_data(&body)["nickname"], "Partner User");
}

/// User Story: US-CUI-005
/// Covers: password change requires an operation-bound, single-use reauth token.
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_custom_user_ui_change_password_requires_reauth(ctx: &mut SchemaTestContext) {
    // `/api/user/reauth` advertises the passkey factor by probing the user's
    // passkey credentials via `resolve_passkey_rp`, which reads the global
    // RP_ID/RP_ORIGIN env vars first (see login.rs:284). Set them so a
    // password-only user can still reach the reauth flow.
    unsafe {
        std::env::set_var("RP_ID", "localhost");
        std::env::set_var("RP_ORIGIN", "http://localhost:3000");
    }
    let app = create_custom_client_app(ctx, true).await;
    let email = format!("cui-change-password-{}@test.com", Uuid::now_v7());
    create_user(ctx, &email).await;
    let tokens = login(ctx, &app, &email).await;
    let missing = send(
        ctx.create_unified_test_router(),
        "POST",
        "/api/user/change-password",
        Some(&tokens.access_token),
        json!({"reauthToken": "", "newPass": "ChangedPassword123!"}),
    )
    .await;
    assert_eq!(missing.status(), StatusCode::UNAUTHORIZED);

    let reauth = issue_password_reauth(ctx, &tokens.access_token, "change_password").await;
    let payload = json!({"reauthToken": reauth, "newPass": "ChangedPassword123!"});
    let changed = send(
        ctx.create_unified_test_router(),
        "POST",
        "/api/user/change-password",
        Some(&tokens.access_token),
        payload.clone(),
    )
    .await;
    assert_eq!(changed.status(), StatusCode::OK);
    let reused = send(
        ctx.create_unified_test_router(),
        "POST",
        "/api/user/change-password",
        Some(&tokens.access_token),
        payload,
    )
    .await;
    assert_eq!(
        reused.status(),
        StatusCode::CONFLICT,
        "reauth tickets are single-use"
    );
}

/// User Story: US-CUI-005
/// Covers: account deletion requires delete_account reauth and revokes the access token.
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_custom_user_ui_delete_account_requires_reauth(ctx: &mut SchemaTestContext) {
    // See `test_custom_user_ui_change_password_requires_reauth` for why the
    // global passkey RP env vars must be set before any /api/user/reauth call.
    unsafe {
        std::env::set_var("RP_ID", "localhost");
        std::env::set_var("RP_ORIGIN", "http://localhost:3000");
    }
    let app = create_custom_client_app(ctx, true).await;
    let email = format!("cui-delete-{}@test.com", Uuid::now_v7());
    create_user(ctx, &email).await;
    let tokens = login(ctx, &app, &email).await;
    let missing = send(
        ctx.create_unified_test_router(),
        "DELETE",
        "/api/user",
        Some(&tokens.access_token),
        json!({"reauthToken": "invalid"}),
    )
    .await;
    assert_eq!(missing.status(), StatusCode::UNAUTHORIZED);

    let reauth = issue_password_reauth(ctx, &tokens.access_token, "delete_account").await;
    let deleted = send(
        ctx.create_unified_test_router(),
        "DELETE",
        "/api/user",
        Some(&tokens.access_token),
        json!({"reauthToken": reauth}),
    )
    .await;
    assert_eq!(deleted.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        status(ctx, Some(&tokens.access_token)).await.status(),
        StatusCode::UNAUTHORIZED
    );
}

/// User Story: US-CUI-005
/// Covers: expired, consumed, and target-mismatched reauth tickets remain distinct.
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_custom_user_ui_reauth_expired_consumed_mismatch(ctx: &mut SchemaTestContext) {
    // See `test_custom_user_ui_change_password_requires_reauth` for why the
    // global passkey RP env vars must be set before any /api/user/reauth call.
    unsafe {
        std::env::set_var("RP_ID", "localhost");
        std::env::set_var("RP_ORIGIN", "http://localhost:3000");
    }
    let app = create_custom_client_app(ctx, true).await;
    let email = format!("cui-reauth-mismatch-{}@test.com", Uuid::now_v7());
    let user_id = create_user(ctx, &email).await;
    let tokens = login(ctx, &app, &email).await;

    let expired = ctx
        .issue_expired_reauth(
            app.id,
            &user_id.to_string(),
            TargetOperation::ChangePassword,
        )
        .await;
    let expired_response = send(
        ctx.create_unified_test_router(),
        "POST",
        "/api/user/change-password",
        Some(&tokens.access_token),
        json!({"reauthToken": expired, "newPass": "ExpiredMustNotApply123!"}),
    )
    .await;
    assert_eq!(expired_response.status(), StatusCode::UNAUTHORIZED);

    let consumed = issue_password_reauth(ctx, &tokens.access_token, "change_password").await;
    let consumed_payload = json!({"reauthToken": consumed, "newPass": "ConsumedPassword123!"});
    let first_use = send(
        ctx.create_unified_test_router(),
        "POST",
        "/api/user/change-password",
        Some(&tokens.access_token),
        consumed_payload.clone(),
    )
    .await;
    assert_eq!(first_use.status(), StatusCode::OK);
    let second_use = send(
        ctx.create_unified_test_router(),
        "POST",
        "/api/user/change-password",
        Some(&tokens.access_token),
        consumed_payload,
    )
    .await;
    assert_eq!(second_use.status(), StatusCode::CONFLICT);

    // `first_use` rotated the password to `ConsumedPassword123!`, so the
    // final reauth ticket for the target-mismatch assertion must verify
    // against that new password, not the original `PASSWORD`.
    let reauth = issue_password_reauth_with(
        ctx,
        &tokens.access_token,
        "change_password",
        "ConsumedPassword123!",
    )
    .await;
    let mismatch = send(
        ctx.create_unified_test_router(),
        "DELETE",
        "/api/user",
        Some(&tokens.access_token),
        json!({"reauthToken": reauth}),
    )
    .await;
    assert_eq!(mismatch.status(), StatusCode::CONFLICT);
}

/// User Story: US-CUI-001
/// Covers: verification redirects only to the Client App's registered return URL or realm fallback.
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_custom_user_ui_verify_email_redirect(ctx: &mut SchemaTestContext) {
    let app = create_custom_client_app(ctx, true).await;
    sqlx::query(
        "INSERT INTO realm_config (realm_id, config_type, config_key, config_value, enabled)
         VALUES ($1, 'registration', 'enabled', 'true', true)
         ON CONFLICT (realm_id, config_type, config_key)
         DO UPDATE SET config_value = 'true', enabled = true",
    )
    .bind(&ctx._realm_id)
    .execute(&ctx.app_state.pool)
    .await
    .unwrap();
    let email = format!("cui-verify-{}@test.com", Uuid::now_v7());
    let register = send(
        ctx.create_unified_test_router(),
        "POST",
        &format!("/api/auth/{}/register", ctx._realm_id),
        None,
        json!({"clientId": app.client_id, "email": email, "password": PASSWORD}),
    )
    .await;
    assert_eq!(register.status(), StatusCode::OK);

    let code = format!("verify-{}", Uuid::now_v7());
    sqlx::query(
        "INSERT INTO email_verification_code (realm_id, email, type, verification_code)
         VALUES ($1, $2, 'register', $3)",
    )
    .bind(&ctx._realm_id)
    .bind(&email)
    .bind(&code)
    .execute(&ctx.app_state.pool)
    .await
    .unwrap();
    seed_mailflow(ctx, &code, &app.client_id, "verify_email").await;
    let confirmed = send(
        ctx.create_unified_test_router(),
        "GET",
        &format!("/api/auth/{}/verify_email/confirm/{code}", ctx._realm_id),
        None,
        Value::Null,
    )
    .await;
    assert_eq!(confirmed.status(), StatusCode::FOUND);
    assert_eq!(
        confirmed.headers()[header::LOCATION],
        "https://partner.example.com/email-verified"
    );

    sqlx::query("UPDATE client_app SET email_verify_return_url = NULL WHERE id = $1")
        .bind(app.id)
        .execute(&ctx.app_state.pool)
        .await
        .unwrap();
    let fallback_code = format!("verify-fallback-{}", Uuid::now_v7());
    sqlx::query(
        "INSERT INTO email_verification_code (realm_id, email, type, verification_code)
         VALUES ($1, $2, 'register', $3)",
    )
    .bind(&ctx._realm_id)
    .bind(&email)
    .bind(&fallback_code)
    .execute(&ctx.app_state.pool)
    .await
    .unwrap();
    seed_mailflow(ctx, &fallback_code, &app.client_id, "verify_email").await;
    let fallback = send(
        ctx.create_unified_test_router(),
        "GET",
        &format!(
            "/api/auth/{}/verify_email/confirm/{fallback_code}",
            ctx._realm_id
        ),
        None,
        Value::Null,
    )
    .await;
    assert_eq!(fallback.status(), StatusCode::FOUND);
    let location = fallback.headers()[header::LOCATION].to_str().unwrap();
    // The fallback uses the existing `realm_public_url` behavior (design §5.6,
    // backward compatible): for a realm without a custom domain it yields
    // `{public_base_url}/{realm_id}/`. It must never reflect a request-supplied URL.
    assert_eq!(
        location,
        format!("http://localhost:8080/{}/", ctx._realm_id)
    );
    assert!(!location.contains("evil.example.com"));
}

/// User Story: US-CUI-003
/// Covers: password reset state binds the registered Client App return URL.
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_custom_user_ui_reset_password_redirect(ctx: &mut SchemaTestContext) {
    let app = create_custom_client_app(ctx, true).await;
    let email = format!("cui-reset-{}@test.com", Uuid::now_v7());
    create_user(ctx, &email).await;
    let requested = send(
        ctx.create_unified_test_router(),
        "POST",
        &format!("/api/auth/{}/reset_password/request", ctx._realm_id),
        None,
        json!({"clientId": app.client_id, "email": email}),
    )
    .await;
    assert_eq!(requested.status(), StatusCode::OK);
    let code: String = sqlx::query_scalar(
        "SELECT verification_code FROM email_verification_code
         WHERE email = $1 AND type = 'reset_password' ORDER BY id DESC LIMIT 1",
    )
    .bind(&email)
    .fetch_one(&ctx.app_state.pool)
    .await
    .expect("reset request must persist its code");
    let confirmed = send(
        ctx.create_unified_test_router(),
        "POST",
        &format!("/api/auth/{}/reset_password/confirm/{code}", ctx._realm_id),
        None,
        json!({"newPass": "ResetPassword456!"}),
    )
    .await;
    assert_eq!(confirmed.status(), StatusCode::FOUND);
    assert_eq!(
        confirmed.headers()[header::LOCATION],
        "https://partner.example.com/password-reset"
    );
}

/// User Story: US-CUI-005
/// Covers: passkey list/delete operations are scoped to the request's resolved RP.
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_custom_user_ui_passkey_rp_isolation(ctx: &mut SchemaTestContext) {
    // Existing Passkey scenario tests use the same process-level test RP setup.
    unsafe {
        std::env::set_var("RP_ID", "localhost");
        std::env::set_var("RP_ORIGIN", "http://localhost:3000");
    }
    enable_passkeys(ctx).await;
    let app = create_custom_client_app(ctx, true).await;
    let email = format!("cui-passkey-isolation-{}@test.com", Uuid::now_v7());
    let user_id = create_user(ctx, &email).await;
    let tokens = login(ctx, &app, &email).await;
    let credential_a = seed_passkey(ctx, user_id, "localhost", "Herald RP").await;
    let credential_b = seed_passkey(ctx, user_id, "partner.example.com", "Partner RP").await;

    let list_for = async |origin: &str| {
        let request = Request::builder()
            .method("GET")
            .uri("/api/user/passkey/credentials")
            .header(header::ORIGIN, origin)
            .header(
                header::AUTHORIZATION,
                format!("Bearer {}", tokens.access_token),
            )
            .body(Body::empty())
            .unwrap();
        ctx.create_unified_test_router()
            .oneshot(request)
            .await
            .unwrap()
    };
    let herald_list = list_for("http://localhost:3000").await;
    assert_eq!(herald_list.status(), StatusCode::OK);
    let herald_body = response_json(herald_list).await;
    let herald_credentials = response_data(&herald_body)["credentials"]
        .as_array()
        .unwrap();
    assert_eq!(herald_credentials.len(), 1);
    assert_eq!(
        herald_credentials[0]["credentialId"],
        credential_a.to_string()
    );

    let partner_list = list_for("https://partner.example.com").await;
    assert_eq!(partner_list.status(), StatusCode::OK);
    let partner_body = response_json(partner_list).await;
    let partner_credentials = response_data(&partner_body)["credentials"]
        .as_array()
        .unwrap();
    assert_eq!(partner_credentials.len(), 1);
    assert_eq!(
        partner_credentials[0]["credentialId"],
        credential_b.to_string()
    );

    let remove_reauth =
        issue_password_reauth(ctx, &tokens.access_token, "remove_authenticator").await;
    let delete_request = Request::builder()
        .method("DELETE")
        .uri(format!("/api/user/passkey/credentials/{credential_b}"))
        .header(header::ORIGIN, "http://localhost:3000")
        .header(
            header::AUTHORIZATION,
            format!("Bearer {}", tokens.access_token),
        )
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({"reauthToken": remove_reauth}).to_string(),
        ))
        .unwrap();
    let denied = ctx
        .create_unified_test_router()
        .oneshot(delete_request)
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::NOT_FOUND);
    let still_exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM user_passkey_credential WHERE id = $1)")
            .bind(credential_b)
            .fetch_one(&ctx.app_state.pool)
            .await
            .unwrap();
    assert!(
        still_exists,
        "wrong-RP delete must not remove the credential"
    );
}

/// User Story: US-CUI-005
/// Covers: registration begin derives RP ID from the enabled Client App origin.
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_custom_user_ui_passkey_rp_from_client_app_origin(ctx: &mut SchemaTestContext) {
    unsafe {
        std::env::set_var("RP_ID", "localhost");
        std::env::set_var("RP_ORIGIN", "http://localhost:3000");
    }
    enable_passkeys(ctx).await;
    let app = create_custom_client_app(ctx, true).await;
    let email = format!("cui-passkey-origin-{}@test.com", Uuid::now_v7());
    create_user(ctx, &email).await;
    let tokens = login(ctx, &app, &email).await;
    let bind_reauth = issue_password_reauth(ctx, &tokens.access_token, "bind_authenticator").await;
    let request = Request::builder()
        .method("POST")
        .uri("/api/user/passkey/registration/begin")
        .header(header::ORIGIN, "https://partner.example.com")
        .header(
            header::AUTHORIZATION,
            format!("Bearer {}", tokens.access_token),
        )
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({"reauthToken": bind_reauth, "nickname": "Partner Key"}).to_string(),
        ))
        .unwrap();
    let response = ctx
        .create_unified_test_router()
        .oneshot(request)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    let data = response_data(&body);
    assert_eq!(data["options"]["rp"]["id"], "partner.example.com");
    assert!(data["regToken"].is_string());
}

/// User Story: US-CUI-002
/// Covers: a disabled Client App rejects direct browser-token login.
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_custom_user_ui_disabled_client_app_blocks_auth_flows(ctx: &mut SchemaTestContext) {
    let disabled = create_custom_client_app(ctx, false).await;
    let email = format!("cui-disabled-{}@test.com", Uuid::now_v7());
    create_user(ctx, &email).await;
    let response = send(
        ctx.create_unified_test_router(),
        "POST",
        &format!("/api/auth/{}/login", ctx._realm_id),
        None,
        json!({"clientId": disabled.client_id, "email": email, "password": PASSWORD}),
    )
    .await;
    assert!(matches!(
        response.status(),
        StatusCode::BAD_REQUEST | StatusCode::FORBIDDEN
    ));

    sqlx::query(
        "INSERT INTO realm_config (realm_id, config_type, config_key, config_value, enabled)
         VALUES ($1, 'registration', 'enabled', 'true', true)
         ON CONFLICT (realm_id, config_type, config_key)
         DO UPDATE SET config_value = 'true', enabled = true",
    )
    .bind(&ctx._realm_id)
    .execute(&ctx.app_state.pool)
    .await
    .unwrap();
    let disabled_register = send(
        ctx.create_unified_test_router(),
        "POST",
        &format!("/api/auth/{}/register", ctx._realm_id),
        None,
        json!({
            "clientId": disabled.client_id,
            "email": format!("disabled-register-{}@test.com", Uuid::now_v7()),
            "password": PASSWORD
        }),
    )
    .await;
    assert!(matches!(
        disabled_register.status(),
        StatusCode::BAD_REQUEST | StatusCode::FORBIDDEN
    ));
    let disabled_reset = send(
        ctx.create_unified_test_router(),
        "POST",
        &format!("/api/auth/{}/reset_password/request", ctx._realm_id),
        None,
        json!({"clientId": disabled.client_id, "email": email}),
    )
    .await;
    assert!(matches!(
        disabled_reset.status(),
        StatusCode::BAD_REQUEST | StatusCode::FORBIDDEN
    ));

    let enabled = create_custom_client_app(ctx, true).await;
    let enabled_email = format!("enabled-login-{}@test.com", Uuid::now_v7());
    create_user(ctx, &enabled_email).await;
    let enabled_login = send(
        ctx.create_unified_test_router(),
        "POST",
        &format!("/api/auth/{}/login", ctx._realm_id),
        None,
        json!({"clientId": enabled.client_id, "email": enabled_email, "password": PASSWORD}),
    )
    .await;
    assert_eq!(enabled_login.status(), StatusCode::OK);
}

/// User Story: US-CUI-002
/// Covers: disabling a Client App revokes its already-issued token families.
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_custom_user_ui_client_app_disable_revokes_family(ctx: &mut SchemaTestContext) {
    let app = create_custom_client_app(ctx, true).await;
    let email = format!("cui-disable-family-{}@test.com", Uuid::now_v7());
    create_user(ctx, &email).await;
    let tokens = login(ctx, &app, &email).await;
    let (admin_token, admin_user_id) = create_admin_session_with_user(
        ctx,
        &format!("cui-disable-admin-{}@test.com", Uuid::now_v7()),
        1800,
    )
    .await;
    grant_realm_admin_role(ctx, &admin_user_id).await;
    let disabled = send(
        ctx.create_unified_test_router(),
        "PUT",
        &format!("/api/client/{}/{}", ctx._realm_id, app.id),
        Some(&admin_token),
        json!({"enabled": false}),
    )
    .await;
    assert_eq!(disabled.status(), StatusCode::OK);
    assert_eq!(
        status(ctx, Some(&tokens.access_token)).await.status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        refresh(ctx, &app, &tokens.refresh_token).await.0.status(),
        StatusCode::UNAUTHORIZED
    );
}

/// Public `GET /api/auth/{realmId}/passkey/status` reflects the realm's passkey
/// enablement flag. The login page gates the passkey entry on this signal
/// *before* firing the begin-options probe, so the contract it protects is:
/// a realm that never configured passkey reads `{ enabled: false }` (not 404),
/// and a realm with `passkey`/`settings`/`enabled:true` reads `true`.
/// Anonymous — no Bearer required.
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_passkey_status_reflects_realm_config(ctx: &mut SchemaTestContext) {
    // Default: no passkey config row → disabled (opt-in per realm), NOT a 404.
    let off = send(
        ctx.create_unified_test_router(),
        "GET",
        &format!("/api/auth/{}/passkey/status", ctx._realm_id),
        None,
        Value::Null,
    )
    .await;
    assert_eq!(off.status(), StatusCode::OK);
    assert_eq!(response_json(off).await["enabled"], false);

    // Enable passkey for the realm → status flips to true.
    enable_passkeys(ctx).await;
    let on = send(
        ctx.create_unified_test_router(),
        "GET",
        &format!("/api/auth/{}/passkey/status", ctx._realm_id),
        None,
        Value::Null,
    )
    .await;
    assert_eq!(on.status(), StatusCode::OK);
    assert_eq!(response_json(on).await["enabled"], true);
}

// CredentialClass is imported deliberately so this file fails to compile if the public
// credential-class contract disappears even though JSON assertions cover its wire values.
const _: CredentialClass = CredentialClass::CustomUserUi;
